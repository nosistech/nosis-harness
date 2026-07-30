//! The turn loop: send task → model responds with tool calls → execute (gated) →
//! feed results back → repeat until final answer or max_turns (then Outcome::Timeout).

mod context;

pub use context::{effective_context, PrefixSeal};

use crate::receipt::{FailureClass, Outcome, Receipt, ReceiptWriter};
use crate::wire::{ChatClient, ChatMessage, ChatRequest, ThinkingEffort, ToolCallReq, Usage};
use context::{
    compact_history, compaction_input_tokens, context_percentage, estimate_request_tokens,
    plain_msg, COMPACT_AT,
};
use nh_tools::{Tool, ToolCtx};

pub const MAX_TASK_BYTES: usize = 64 * 1024;

/// Validate user-controlled task text before it can enter history, a receipt,
/// or a paid provider request.
pub fn validate_task(task: &str) -> anyhow::Result<()> {
    if task.trim().is_empty() {
        anyhow::bail!("task is empty - add a task description");
    }
    if task.len() > MAX_TASK_BYTES {
        anyhow::bail!("task is too large - maximum is {MAX_TASK_BYTES} bytes");
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FinishKind {
    Normal,
    Truncated,
    Context,
    Filtered,
    Interrupted,
    Unknown,
}

fn classify_finish_reason(raw: &str) -> FinishKind {
    match raw.trim() {
        "" | "stop" | "end_turn" | "stop_sequence" => FinishKind::Normal,
        "length" | "max_tokens" | "model_length" => FinishKind::Truncated,
        "model_context_window_exceeded" => FinishKind::Context,
        "content_filter" | "sensitive" => FinishKind::Filtered,
        "network_error" | "insufficient_system_resource" => FinishKind::Interrupted,
        _ => FinishKind::Unknown,
    }
}

fn add_usage_checked(total: &mut Usage, usage: &Usage) -> bool {
    let Some(prompt_tokens) = total.prompt_tokens.checked_add(usage.prompt_tokens) else {
        return false;
    };
    let Some(completion_tokens) = total.completion_tokens.checked_add(usage.completion_tokens)
    else {
        return false;
    };
    let cached_tokens = match usage.cached_tokens {
        Some(cached) => {
            let Some(total_cached) = total.cached_tokens.unwrap_or(0).checked_add(cached) else {
                return false;
            };
            Some(total_cached)
        }
        None => total.cached_tokens,
    };

    total.prompt_tokens = prompt_tokens;
    total.completion_tokens = completion_tokens;
    total.cached_tokens = cached_tokens;
    true
}

/// Build the byte-stable identity prefix required at every model-facing
/// surface. Keeping this beside the turn loop prevents callers from
/// accidentally constructing a session with only the repository law.
pub const TOOL_RESULT_STATE_RULE: &str =
    "Treat tool results as authoritative evidence about process, server, file, and system state. \
     Never assert state that a tool result contradicts. Report every timeout, killed process, and \
     non-zero exit as a failure; never claim that such a command succeeded, is running, or \
     continued in the background.";

pub fn identity_constitution(law_constitution: &str, route_id: &str, provider: &str) -> String {
    format!(
        "You are nosis, an autonomous coding harness. You are running on the model route '{route_id}' via {provider}. If asked what model or assistant you are, answer 'nosis on {route_id}'; never claim to be Claude, GPT, or any other assistant.\n\n{TOOL_RESULT_STATE_RULE}\n\n{law_constitution}"
    )
}

/// Optional progress sink used by terminal frontends.
pub type ProgressCallback = Box<dyn Fn(&str) + Send>;

pub struct AgentLoop {
    pub client: Box<dyn ChatClient>,
    pub tools: Vec<Box<dyn Tool>>,
    pub ctx: ToolCtx,
    pub receipts: ReceiptWriter,
    pub model_id: String,
    pub max_turns: u32,
    /// Thinking effort sent with every request; the client maps it to the
    /// route's dialect (the loop stays policy-free). Default: no thinking.
    pub thinking: ThinkingEffort,
    /// Effective execution profile recorded on every receipt. `None`
    /// preserves the pre-profile JSONL shape.
    pub profile: Option<String>,
    /// Byte-stable system prefix. `None` preserves the M0/M1 default.
    pub constitution: Option<String>,
    /// Effective context window. `None` disables compaction.
    pub context_limit: Option<u64>,
    /// Progress callback: invoked with one short line per tool call
    /// ("turn 2: edit_file src/lib.rs"). Core stays print-free - nh-cli
    /// wires this to its own printer.
    pub on_event: Option<ProgressCallback>,
}

impl AgentLoop {
    /// Runs one accepted task to completion. Always attempts one receipt after
    /// input validation, even on provider or execution error.
    /// Returns the final assistant text. UX: progress surfaces via `on_event` -
    /// one short line per tool call (name + key arg), never a wall of JSON.
    pub fn run(&mut self, task: &str) -> anyhow::Result<(String, Receipt)> {
        let mut history = Vec::new();
        self.run_with_history(&mut history, task)
    }

    /// Same turn loop over a caller-owned session history (`nh chat`).
    /// Empty history gets the system message first; the user task and ALL
    /// produced messages (assistant + tool) are appended, so `history` holds
    /// the full session on return - even on the timeout path. Exactly one
    /// receipt write is attempted per accepted call, same semantics as `run`.
    /// Invalid input is rejected before history or receipt mutation.
    pub fn run_with_history(
        &mut self,
        history: &mut Vec<ChatMessage>,
        task: &str,
    ) -> anyhow::Result<(String, Receipt)> {
        validate_task(task)?;
        let specs: Vec<nh_tools::ToolSpec> = self.tools.iter().map(|t| t.spec()).collect();
        if history.is_empty() {
            let system = self.constitution.clone().unwrap_or_else(|| {
                let tool_names: Vec<&str> = specs.iter().map(|s| s.name.as_str()).collect();
                format!(
                    "You are a coding agent. Complete the user's task using the available \
                     tools, then reply with a short final answer. Available tools: {}.",
                    tool_names.join(", ")
                )
            });
            history.push(plain_msg("system", system));
        }
        history.push(plain_msg("user", task.to_string()));

        let prefix_seal = PrefixSeal::new(&history[..1]);
        let mut prefix_drift_reported = false;

        let mut turns: u32 = 0;
        let mut tool_calls: u32 = 0;
        let mut usage_total = Usage::default();
        let mut saw_usage = false;
        let mut usage_overflowed = false;
        let mut latest_prompt_tokens = None;

        while turns < self.max_turns {
            self.report_prefix_drift(&prefix_seal, history, &mut prefix_drift_reported);

            if let Some(limit) = self.context_limit {
                let working_limit = effective_context(limit);
                let estimated = estimate_request_tokens(history, &specs, true);
                let input_tokens = compaction_input_tokens(latest_prompt_tokens, estimated);
                if input_tokens as f64 >= COMPACT_AT * working_limit as f64 {
                    if let Some(compaction) = compact_history(history, working_limit) {
                        if !compaction.prefix_held {
                            self.emit("cache break detected: retained prefix drift");
                        }
                        let pct = context_percentage(input_tokens, working_limit);
                        self.emit(&format!(
                            "context {pct}% - compacted {} earlier messages",
                            compaction.messages
                        ));
                    }
                }
            }

            self.report_prefix_drift(&prefix_seal, history, &mut prefix_drift_reported);

            turns += 1;
            let req = ChatRequest {
                model: self.model_id.clone(),
                messages: history.clone(),
                tools: specs.clone(),
                thinking: self.thinking,
            };
            let resp = match self.client.complete(&req) {
                Ok(r) => r,
                Err(e) => {
                    self.report_prefix_drift(&prefix_seal, history, &mut prefix_drift_reported);
                    let mut receipt = self.make_receipt(
                        task,
                        turns,
                        tool_calls,
                        Outcome::Fail,
                        Some(FailureClass::Verification),
                        (!usage_overflowed && saw_usage).then(|| usage_total.clone()),
                    );
                    self.append_receipt(&mut receipt);
                    return Err(e);
                }
            };
            latest_prompt_tokens = resp.usage.as_ref().map(|u| u.prompt_tokens);
            if let Some(u) = &resp.usage {
                saw_usage = true;
                if !usage_overflowed && !add_usage_checked(&mut usage_total, u) {
                    usage_overflowed = true;
                }
            }
            history.push(resp.message.clone());
            let calls = resp.message.tool_calls.clone().unwrap_or_default();
            if calls.is_empty() {
                self.report_prefix_drift(&prefix_seal, history, &mut prefix_drift_reported);
                let text = resp.message.content.clone().unwrap_or_default();
                let finish_reason = resp.finish_reason.trim();
                let (outcome, failure_class) = match classify_finish_reason(finish_reason) {
                    FinishKind::Normal => (Outcome::Pass, None),
                    FinishKind::Truncated => (Outcome::Partial, Some(FailureClass::Constraint)),
                    FinishKind::Context => (Outcome::Partial, Some(FailureClass::Context)),
                    FinishKind::Filtered => (Outcome::Fail, Some(FailureClass::Filtered)),
                    FinishKind::Interrupted => (Outcome::Partial, Some(FailureClass::Constraint)),
                    FinishKind::Unknown => {
                        self.emit(&format!(
                            "unrecognized finish reason '{finish_reason}' - treated as partial"
                        ));
                        (Outcome::Partial, Some(FailureClass::Constraint))
                    }
                };
                let mut receipt = self.make_receipt(
                    task,
                    turns,
                    tool_calls,
                    outcome,
                    failure_class,
                    (!usage_overflowed && saw_usage).then(|| usage_total.clone()),
                );
                self.append_receipt(&mut receipt);
                return Ok((text, receipt));
            }
            for call in &calls {
                tool_calls += 1;
                self.emit(&progress_line(turns, call));
                let result = self.run_tool(call);
                history.push(ChatMessage {
                    role: "tool".into(),
                    content: Some(result),
                    tool_calls: None,
                    tool_call_id: Some(call.id.clone()),
                    reasoning_content: None,
                });
            }

            self.report_prefix_drift(&prefix_seal, history, &mut prefix_drift_reported);
        }

        self.report_prefix_drift(&prefix_seal, history, &mut prefix_drift_reported);

        let mut receipt = self.make_receipt(
            task,
            turns,
            tool_calls,
            Outcome::Timeout,
            Some(FailureClass::Constraint),
            (!usage_overflowed && saw_usage).then(|| usage_total.clone()),
        );
        self.append_receipt(&mut receipt);
        Ok((
            format!(
                "stopped after {} turns without a final answer",
                self.max_turns
            ),
            receipt,
        ))
    }

    fn report_prefix_drift(&self, seal: &PrefixSeal, history: &[ChatMessage], reported: &mut bool) {
        let held = seal.check(history);
        if !held && !*reported {
            self.emit("cache break detected: sealed prefix drift");
            *reported = true;
        }
        debug_assert!(held, "sealed message prefix drifted");
    }

    /// Execute one tool call; every failure becomes a message the model can act on.
    fn run_tool(&self, call: &ToolCallReq) -> String {
        let Some(tool) = self.tools.iter().find(|t| t.spec().name == call.name) else {
            let names: Vec<String> = self.tools.iter().map(|t| t.spec().name).collect();
            return format!(
                "unknown tool '{}' - available tools: {}",
                call.name,
                names.join(", ")
            );
        };
        let args: serde_json::Value = match serde_json::from_str(&call.arguments) {
            Ok(v) => v,
            Err(e) => return format!("invalid arguments JSON for '{}': {e}", call.name),
        };
        match tool.execute(args, &self.ctx) {
            Ok(out) => out,
            Err(e) => format!("tool '{}' failed: {e}", call.name),
        }
    }

    fn emit(&self, line: &str) {
        if let Some(f) = &self.on_event {
            f(line);
        }
    }

    fn append_receipt(&self, receipt: &mut Receipt) {
        if let Err(error) = self.receipts.append(receipt) {
            self.emit(&format!(
                "receipt not written - outcome marked unreceipted: {error}"
            ));
            if receipt.outcome == Outcome::Pass {
                receipt.outcome = Outcome::Fail;
            }
            receipt.failure_class = Some(FailureClass::Unreceipted);
        }
    }

    fn make_receipt(
        &self,
        task: &str,
        turns: u32,
        tool_calls: u32,
        outcome: Outcome,
        failure_class: Option<FailureClass>,
        usage: Option<Usage>,
    ) -> Receipt {
        Receipt {
            ts_utc: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            model_id: self.model_id.clone(),
            task: task.to_string(),
            turns,
            tool_calls,
            outcome,
            failure_class,
            usage,
            effective_profile: self.profile.clone(),
        }
    }
}

/// "turn 2: edit_file src/lib.rs" - name plus the key argument, kept short.
fn progress_line(turn: u32, call: &ToolCallReq) -> String {
    let mut line = format!("turn {turn}: {}", call.name);
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&call.arguments) {
        for key in ["path", "command"] {
            if let Some(s) = v.get(key).and_then(|x| x.as_str()) {
                let short: String = s.chars().take(60).collect();
                line.push(' ');
                line.push_str(&short);
                break;
            }
        }
    }
    line
}

#[cfg(test)]
mod tests;
