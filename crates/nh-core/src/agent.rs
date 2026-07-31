//! The turn loop: send task → model responds with tool calls → execute (gated) →
//! feed results back → repeat until final answer or max_turns (then Outcome::Timeout).

mod context;
mod tool_repair;

pub use context::{effective_context, PrefixSeal};

use crate::receipt::{FailureClass, Outcome, Receipt, ReceiptWriter, RepairStats};
use crate::wire::{
    ChatClient, ChatMessage, ChatRequest, ContentPart, ThinkingEffort, ToolCallReq, Usage,
};
use context::{
    compact_history, compaction_input_tokens, context_percentage, estimate_request_tokens,
    plain_msg, COMPACT_AT,
};
use nh_tools::{EditMatchTier, Tool, ToolAudit, ToolCtx};

pub const MAX_TASK_BYTES: usize = 64 * 1024;

/// Validate user-controlled task text before it can enter history, a receipt,
/// or a paid provider request.
pub fn validate_task(task: &str) -> anyhow::Result<()> {
    if task.trim().is_empty() {
        anyhow::bail!("task is empty — add a task description");
    }
    if task.len() > MAX_TASK_BYTES {
        anyhow::bail!("task is too large — maximum is {MAX_TASK_BYTES} bytes");
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

struct ToolRun {
    output: String,
    repair_attempted: bool,
    repair_notes: Vec<String>,
    audit: Vec<ToolAudit>,
}

struct ReceiptFields {
    turns: u32,
    tool_calls: u32,
    outcome: Outcome,
    failure_class: Option<FailureClass>,
    usage: Option<Usage>,
    repairs: RepairStats,
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
    let cached_tokens = match (total.cached_tokens, usage.cached_tokens) {
        (Some(total_cached), Some(cached)) => {
            let Some(total_cached) = total_cached.checked_add(cached) else {
                return false;
            };
            Some(total_cached)
        }
        _ => None,
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
    /// ("turn 2: edit_file src/lib.rs"). Core stays print-free — nh-cli
    /// wires this to its own printer.
    pub on_event: Option<ProgressCallback>,
}

impl AgentLoop {
    /// Runs one accepted task to completion. Always attempts one receipt after
    /// input validation, even on provider or execution error.
    /// Returns the final assistant text. UX: progress surfaces via `on_event` —
    /// one short line per tool call (name + key arg), never a wall of JSON.
    pub fn run(&mut self, task: &str) -> anyhow::Result<(String, Receipt)> {
        let mut history = Vec::new();
        self.run_with_history(&mut history, task)
    }

    /// Same turn loop over a caller-owned session history (`nh chat`).
    /// Empty history gets the system message first; the user task and ALL
    /// produced messages (assistant + tool) are appended, so `history` holds
    /// the full session on return — even on the timeout path. Exactly one
    /// receipt write is attempted per accepted call, same semantics as `run`.
    /// Invalid input is rejected before history or receipt mutation.
    pub fn run_with_history(
        &mut self,
        history: &mut Vec<ChatMessage>,
        task: &str,
    ) -> anyhow::Result<(String, Receipt)> {
        self.run_with_history_inner(history, task, None)
    }

    /// Add image or future content parts to the next user task. The existing
    /// history entry point remains the unchanged text-only path.
    pub fn run_with_history_and_parts(
        &mut self,
        history: &mut Vec<ChatMessage>,
        task: &str,
        parts: Vec<ContentPart>,
    ) -> anyhow::Result<(String, Receipt)> {
        self.run_with_history_inner(history, task, Some(parts))
    }

    fn run_with_history_inner(
        &mut self,
        history: &mut Vec<ChatMessage>,
        task: &str,
        parts: Option<Vec<ContentPart>>,
    ) -> anyhow::Result<(String, Receipt)> {
        validate_task(task)?;
        let image_count = parts.as_ref().map_or(0, |parts| {
            parts
                .iter()
                .filter(|part| matches!(part, ContentPart::ImageB64 { .. }))
                .count()
        });
        if image_count > nh_tools::MAX_IMAGES_PER_MESSAGE {
            anyhow::bail!(
                "a message can attach at most {} images — split them across messages",
                nh_tools::MAX_IMAGES_PER_MESSAGE
            );
        }
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
        if let Some(parts) = parts {
            let mut message_parts = Vec::with_capacity(parts.len().saturating_add(1));
            message_parts.push(ContentPart::Text {
                text: task.to_owned(),
            });
            message_parts.extend(parts);
            history.push(ChatMessage {
                role: "user".into(),
                content: None,
                parts: Some(message_parts),
                tool_calls: None,
                tool_call_id: None,
                reasoning_content: None,
            });
        } else {
            history.push(plain_msg("user", task.to_string()));
        }

        let prefix_seal = PrefixSeal::new(&history[..1]);
        let mut prefix_drift_reported = false;

        let mut turns: u32 = 0;
        let mut tool_calls: u32 = 0;
        let mut repairs = RepairStats::default();
        let mut usage_total = Usage {
            cached_tokens: Some(0),
            ..Usage::default()
        };
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
                            "context {pct}% — compacted {} earlier messages",
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
                        ReceiptFields {
                            turns,
                            tool_calls,
                            outcome: Outcome::Fail,
                            failure_class: Some(FailureClass::Verification),
                            usage: (!usage_overflowed && saw_usage).then(|| usage_total.clone()),
                            repairs,
                        },
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
                            "unrecognized finish reason '{finish_reason}' — treated as partial"
                        ));
                        (Outcome::Partial, Some(FailureClass::Constraint))
                    }
                };
                let mut receipt = self.make_receipt(
                    task,
                    ReceiptFields {
                        turns,
                        tool_calls,
                        outcome,
                        failure_class,
                        usage: (!usage_overflowed && saw_usage).then(|| usage_total.clone()),
                        repairs,
                    },
                );
                self.append_receipt(&mut receipt);
                return Ok((text, receipt));
            }
            for call in &calls {
                tool_calls += 1;
                self.emit(&progress_line(turns, call));
                let result = self.run_tool(call);
                if result.repair_attempted {
                    repairs.tool_call_repair_attempts =
                        repairs.tool_call_repair_attempts.saturating_add(1);
                }
                for note in &result.repair_notes {
                    self.emit(&format!("turn {turns}: repaired tool call — {note}"));
                }
                for audit in &result.audit {
                    match audit {
                        ToolAudit::EditMatch(EditMatchTier::WhitespaceNormalized) => {
                            repairs.edit_whitespace_matches =
                                repairs.edit_whitespace_matches.saturating_add(1);
                            self.emit(&format!(
                                "turn {turns}: edit_file used whitespace-normalized match"
                            ));
                        }
                        ToolAudit::EditMatch(EditMatchTier::IndentationFlexible) => {
                            repairs.edit_indentation_matches =
                                repairs.edit_indentation_matches.saturating_add(1);
                            self.emit(&format!(
                                "turn {turns}: edit_file used indentation-flexible match"
                            ));
                        }
                    }
                }
                history.push(ChatMessage {
                    role: "tool".into(),
                    content: Some(result.output),
                    parts: None,
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
            ReceiptFields {
                turns,
                tool_calls,
                outcome: Outcome::Timeout,
                failure_class: Some(FailureClass::Constraint),
                usage: (!usage_overflowed && saw_usage).then(|| usage_total.clone()),
                repairs,
            },
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
    fn run_tool(&self, call: &ToolCallReq) -> ToolRun {
        let (args, mut repair_notes) = match tool_repair::parse_arguments(&call.arguments) {
            Ok(parsed) => (parsed.value, parsed.notes),
            Err(failure) => {
                return finish_tool_run(
                    format!(
                        "invalid arguments JSON for '{}': {}",
                        call.name, failure.error
                    ),
                    failure.notes,
                    Vec::new(),
                );
            }
        };

        let exact = self.tools.iter().find(|tool| tool.spec().name == call.name);
        let (tool, tool_name) = if let Some(tool) = exact {
            (Some(tool), call.name.as_str())
        } else if let Some(alias) = tool_repair::canonical_tool_name(&call.name) {
            repair_notes.push(format!("mapped tool name '{}' to '{alias}'", call.name));
            (
                self.tools.iter().find(|tool| tool.spec().name == alias),
                alias,
            )
        } else {
            (None, call.name.as_str())
        };

        let Some(tool) = tool else {
            let names: Vec<String> = self.tools.iter().map(|tool| tool.spec().name).collect();
            return finish_tool_run(
                format!(
                    "unknown tool '{}' — available tools: {}",
                    call.name,
                    names.join(", ")
                ),
                repair_notes,
                Vec::new(),
            );
        };

        match tool.execute_with_audit(args, &self.ctx) {
            Ok(execution) => finish_tool_run(execution.output, repair_notes, execution.audit),
            Err(error) => finish_tool_run(
                format!("tool '{tool_name}' failed: {error}"),
                repair_notes,
                Vec::new(),
            ),
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
                "receipt not written — outcome marked unreceipted: {error}"
            ));
            if receipt.outcome == Outcome::Pass {
                receipt.outcome = Outcome::Fail;
            }
            receipt.failure_class = Some(FailureClass::Unreceipted);
        }
    }

    fn make_receipt(&self, task: &str, fields: ReceiptFields) -> Receipt {
        let ReceiptFields {
            turns,
            tool_calls,
            outcome,
            failure_class,
            usage,
            repairs,
        } = fields;
        let cache_hit_pct = usage
            .as_ref()
            .and_then(|usage| crate::wire::cache_hit_pct(usage.prompt_tokens, usage.cached_tokens));
        Receipt {
            ts_utc: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            model_id: self.model_id.clone(),
            task: task.to_string(),
            turns,
            tool_calls,
            outcome,
            failure_class,
            usage,
            cache_hit_pct,
            repairs,
            effective_profile: self.profile.clone(),
        }
    }
}

fn finish_tool_run(output: String, repair_notes: Vec<String>, audit: Vec<ToolAudit>) -> ToolRun {
    let repair_attempted = !repair_notes.is_empty();
    let output = if repair_attempted {
        format!("[tool-call repair: {}]\n{output}", repair_notes.join("; "))
    } else {
        output
    };
    ToolRun {
        output,
        repair_attempted,
        repair_notes,
        audit,
    }
}

/// "turn 2: edit_file src/lib.rs" — name plus the key argument, kept short.
fn progress_line(turn: u32, call: &ToolCallReq) -> String {
    let mut line = format!("turn {turn}: {}", call.name);
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&call.arguments) {
        for key in ["path", "command", "pattern"] {
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
