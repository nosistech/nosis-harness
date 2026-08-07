//! The turn loop: send task → model responds with tool calls → execute (gated) →
//! feed results back → repeat until final answer or max_turns (then Outcome::Timeout).

mod context;
mod tool_repair;

pub use context::{effective_context, PrefixSeal};

use crate::receipt::{
    CompactionStats, FailureClass, Outcome, Receipt, ReceiptKind, ReceiptWriter, RepairStats,
};
use crate::wire::{
    ChatClient, ChatMessage, ChatRequest, ContentPart, FinishReason, RetryExhausted, RetryStats,
    ThinkingEffort, ToolCallReq, Usage,
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
    ToolUse,
    Truncated,
    Context,
    Filtered,
    Interrupted,
    Missing,
    Unknown,
}

/// Provider-call failure paired with the real receipt already attempted by
/// core. Presentation layers may project this receipt; they must never invent
/// a replacement.
#[derive(Debug)]
pub struct AgentRunError {
    receipt: Receipt,
    source: anyhow::Error,
}

impl AgentRunError {
    pub fn receipt(&self) -> &Receipt {
        &self.receipt
    }
}

impl std::fmt::Display for AgentRunError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.source, formatter)
    }
}

impl std::error::Error for AgentRunError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}

struct ToolRun {
    output: String,
    repair_attempted: bool,
    repair_notes: Vec<String>,
    audit: Vec<ToolAudit>,
}

fn push_message(
    history: &mut Vec<ChatMessage>,
    appended: &mut Option<&mut Vec<ChatMessage>>,
    message: ChatMessage,
) {
    if let Some(journal) = appended.as_deref_mut() {
        journal.push(message.clone());
    }
    history.push(message);
}

struct ReceiptFields {
    turns: u32,
    tool_calls: u32,
    duration_ms: u64,
    outcome: Outcome,
    failure_class: Option<FailureClass>,
    usage: Option<Usage>,
    repairs: RepairStats,
    retries: RetryStats,
    compaction: CompactionStats,
}

fn classify_finish_reason(reason: &FinishReason) -> FinishKind {
    match reason {
        FinishReason::Stop => FinishKind::Normal,
        FinishReason::ToolUse => FinishKind::ToolUse,
        FinishReason::Truncated => FinishKind::Truncated,
        FinishReason::ContextWindow => FinishKind::Context,
        FinishReason::Filtered => FinishKind::Filtered,
        FinishReason::Interrupted => FinishKind::Interrupted,
        FinishReason::Missing => FinishKind::Missing,
        FinishReason::Unknown(_) => FinishKind::Unknown,
    }
}

fn add_usage_checked(total: &mut Usage, usage: &Usage) -> bool {
    total.checked_add_assign(usage)
}

fn observe_usage_checked(
    total: &mut Option<Usage>,
    saw_unreported: &mut bool,
    usage: Option<&Usage>,
) -> bool {
    let Some(usage) = usage else {
        *saw_unreported = true;
        if let Some(total) = total {
            total.mark_unreported_component();
        }
        return true;
    };

    let mut usage = usage.clone();
    if *saw_unreported {
        usage.mark_unreported_component();
    }
    match total {
        Some(total) => add_usage_checked(total, &usage),
        None => {
            *total = Some(usage);
            true
        }
    }
}

fn add_retry_stats(total: &mut RetryStats, retries: RetryStats) {
    total.retries = total.retries.saturating_add(retries.retries);
    total.rate_limited = total.rate_limited.saturating_add(retries.rate_limited);
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

/// Lossless, route-agnostic compaction facts carried over the existing text
/// progress callback. Frontends parse this record and own user-facing wording
/// and pricing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompactionEvent {
    pub context_percent: u64,
    pub messages_elided: u64,
    pub estimated_tokens_elided: u64,
    pub preceding_cached_tokens: Option<u64>,
    pub occurred_at_unix_seconds: Option<i64>,
}

impl CompactionEvent {
    pub const fn new(
        context_percent: u64,
        messages_elided: u64,
        estimated_tokens_elided: u64,
        preceding_cached_tokens: Option<u64>,
    ) -> Self {
        Self {
            context_percent,
            messages_elided,
            estimated_tokens_elided,
            preceding_cached_tokens,
            occurred_at_unix_seconds: None,
        }
    }

    pub const fn new_at(
        context_percent: u64,
        messages_elided: u64,
        estimated_tokens_elided: u64,
        preceding_cached_tokens: Option<u64>,
        unix_seconds: i64,
    ) -> Self {
        Self {
            context_percent,
            messages_elided,
            estimated_tokens_elided,
            preceding_cached_tokens,
            occurred_at_unix_seconds: Some(unix_seconds),
        }
    }

    /// Parse the exact natural-language fact record emitted by the turn loop.
    pub fn parse(line: &str) -> Option<Self> {
        let line = line.strip_prefix("context ~")?;
        let (context_percent, line) = line.split_once("% - compacted ")?;
        let (messages_elided, line) = line.split_once(" earlier messages · ~")?;
        let (estimated_tokens_elided, line) =
            line.split_once(" tokens elided · preceding cache ")?;
        let (cache, unix_time) = line.split_once(" · Unix time ")?;
        let preceding_cached_tokens = if cache == "unavailable" {
            None
        } else {
            Some(cache.strip_suffix(" tokens measured")?.parse().ok()?)
        };
        let occurred_at_unix_seconds = if unix_time == "unavailable" {
            None
        } else {
            Some(unix_time.strip_suffix(" measured")?.parse().ok()?)
        };

        Some(Self {
            context_percent: context_percent.parse().ok()?,
            messages_elided: messages_elided.parse().ok()?,
            estimated_tokens_elided: estimated_tokens_elided.parse().ok()?,
            preceding_cached_tokens,
            occurred_at_unix_seconds,
        })
    }
}

impl std::fmt::Display for CompactionEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "context ~{}% - compacted {} earlier messages · ~{} tokens elided · preceding cache ",
            self.context_percent, self.messages_elided, self.estimated_tokens_elided
        )?;
        if let Some(cached_tokens) = self.preceding_cached_tokens {
            write!(f, "{cached_tokens} tokens measured")
        } else {
            f.write_str("unavailable")
        }?;
        f.write_str(" · Unix time ")?;
        if let Some(unix_seconds) = self.occurred_at_unix_seconds {
            write!(f, "{unix_seconds} measured")
        } else {
            f.write_str("unavailable")
        }
    }
}

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
        let mut latest_cached_tokens = None;
        self.run_with_history_inner(history, task, None, None, &mut latest_cached_tokens)
    }

    /// Run against a compactable working copy while keeping the caller's
    /// transcript append-only. Session ledgers use this entry point so every
    /// newly produced message can be persisted as an exact suffix delta.
    pub fn run_with_persistent_history(
        &mut self,
        history: &mut Vec<ChatMessage>,
        task: &str,
    ) -> anyhow::Result<(String, Receipt)> {
        let mut latest_cached_tokens = None;
        self.run_with_persistent_history_inner(history, task, None, &mut latest_cached_tokens)
    }

    /// Persistent-history form with cache evidence from the immediately
    /// preceding provider response. `preceding_cached_tokens` is in/out:
    /// callers supply the prior response's exact field, and receive the final
    /// response's exact field only while it still describes the caller's
    /// prefix. Output is `None` after an unreported value, provider error or
    /// retry, or working-copy compaction because caller history stays
    /// append-only and will rebuild a different prefix.
    pub fn run_with_persistent_history_and_cache(
        &mut self,
        history: &mut Vec<ChatMessage>,
        task: &str,
        preceding_cached_tokens: &mut Option<u64>,
    ) -> anyhow::Result<(String, Receipt)> {
        self.run_with_persistent_history_inner(history, task, None, preceding_cached_tokens)
    }

    /// Add image or future content parts to the next user task. The existing
    /// history entry point remains the unchanged text-only path.
    pub fn run_with_history_and_parts(
        &mut self,
        history: &mut Vec<ChatMessage>,
        task: &str,
        parts: Vec<ContentPart>,
    ) -> anyhow::Result<(String, Receipt)> {
        let mut latest_cached_tokens = None;
        self.run_with_history_inner(history, task, Some(parts), None, &mut latest_cached_tokens)
    }

    /// Multimodal form of [`Self::run_with_persistent_history`].
    pub fn run_with_persistent_history_and_parts(
        &mut self,
        history: &mut Vec<ChatMessage>,
        task: &str,
        parts: Vec<ContentPart>,
    ) -> anyhow::Result<(String, Receipt)> {
        let mut latest_cached_tokens = None;
        self.run_with_persistent_history_inner(
            history,
            task,
            Some(parts),
            &mut latest_cached_tokens,
        )
    }

    /// Multimodal persistent-history form with the same in/out cache evidence
    /// semantics as [`Self::run_with_persistent_history_and_cache`].
    pub fn run_with_persistent_history_and_parts_and_cache(
        &mut self,
        history: &mut Vec<ChatMessage>,
        task: &str,
        parts: Vec<ContentPart>,
        preceding_cached_tokens: &mut Option<u64>,
    ) -> anyhow::Result<(String, Receipt)> {
        self.run_with_persistent_history_inner(history, task, Some(parts), preceding_cached_tokens)
    }

    fn run_with_persistent_history_inner(
        &mut self,
        history: &mut Vec<ChatMessage>,
        task: &str,
        parts: Option<Vec<ContentPart>>,
        latest_cached_tokens: &mut Option<u64>,
    ) -> anyhow::Result<(String, Receipt)> {
        let mut working = history.clone();
        let mut appended = Vec::new();
        let result = self.run_with_history_inner(
            &mut working,
            task,
            parts,
            Some(&mut appended),
            latest_cached_tokens,
        );
        let compacted = result
            .as_ref()
            .is_ok_and(|(_, receipt)| !receipt.compaction.is_empty());
        history.extend(appended);
        if compacted {
            *latest_cached_tokens = None;
        }
        result
    }

    fn run_with_history_inner(
        &mut self,
        history: &mut Vec<ChatMessage>,
        task: &str,
        parts: Option<Vec<ContentPart>>,
        mut appended: Option<&mut Vec<ChatMessage>>,
        latest_cached_tokens: &mut Option<u64>,
    ) -> anyhow::Result<(String, Receipt)> {
        let turn_started = std::time::Instant::now();
        validate_task(task)?;
        let image_count = parts.as_ref().map_or(0, |parts| {
            parts
                .iter()
                .filter(|part| matches!(part, ContentPart::ImageB64 { .. }))
                .count()
        });
        if image_count > nh_tools::MAX_IMAGES_PER_MESSAGE {
            anyhow::bail!(
                "a message can attach at most {} images - split them across messages",
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
            push_message(history, &mut appended, plain_msg("system", system));
        }
        if let Some(parts) = parts {
            let mut message_parts = Vec::with_capacity(parts.len().saturating_add(1));
            message_parts.push(ContentPart::Text {
                text: task.to_owned(),
            });
            message_parts.extend(parts);
            push_message(
                history,
                &mut appended,
                ChatMessage {
                    role: "user".into(),
                    content: None,
                    parts: Some(message_parts),
                    tool_calls: None,
                    tool_call_id: None,
                    reasoning_content: None,
                },
            );
        } else {
            push_message(history, &mut appended, plain_msg("user", task.to_string()));
        }

        let prefix_seal = PrefixSeal::new(&history[..1]);
        let mut prefix_drift_reported = false;

        let mut turns: u32 = 0;
        let mut tool_calls: u32 = 0;
        let mut repairs = RepairStats::default();
        let mut usage_total: Option<Usage> = None;
        let mut saw_unreported_usage = false;
        let mut usage_overflowed = false;
        let mut latest_prompt_tokens = None;
        let mut retry_total = RetryStats::default();
        let mut compaction_total = CompactionStats::default();

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
                        let messages_elided = compaction.messages as u64;
                        let occurred_at_unix_seconds = chrono::Utc::now().timestamp();
                        let event = CompactionEvent::new_at(
                            pct,
                            messages_elided,
                            compaction.estimated_tokens_elided,
                            *latest_cached_tokens,
                            occurred_at_unix_seconds,
                        );
                        compaction_total.record_at(
                            messages_elided,
                            compaction.estimated_tokens_elided,
                            *latest_cached_tokens,
                            occurred_at_unix_seconds,
                        );
                        self.emit(&event.to_string());
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
                    *latest_cached_tokens = None;
                    let failure_usage = if let Some(exhausted) = e.downcast_ref::<RetryExhausted>()
                    {
                        add_retry_stats(&mut retry_total, exhausted.stats);
                        exhausted.usage.as_ref()
                    } else {
                        None
                    };
                    if !usage_overflowed
                        && !observe_usage_checked(
                            &mut usage_total,
                            &mut saw_unreported_usage,
                            failure_usage,
                        )
                    {
                        usage_overflowed = true;
                    }
                    self.report_prefix_drift(&prefix_seal, history, &mut prefix_drift_reported);
                    let mut receipt = self.make_receipt(
                        task,
                        ReceiptFields {
                            turns,
                            tool_calls,
                            duration_ms: u64::try_from(turn_started.elapsed().as_millis())
                                .unwrap_or(u64::MAX),
                            outcome: Outcome::Fail,
                            failure_class: Some(FailureClass::Verification),
                            usage: if usage_overflowed {
                                None
                            } else {
                                usage_total.clone()
                            },
                            repairs,
                            retries: retry_total,
                            compaction: compaction_total,
                        },
                    );
                    self.append_receipt(&mut receipt);
                    return Err(anyhow::Error::new(AgentRunError { receipt, source: e }));
                }
            };
            add_retry_stats(&mut retry_total, resp.retries);
            if resp.retries.retries > 0 {
                self.emit(&format!(
                    "turn {turns}: {} attempts, {} rate-limited",
                    resp.retries.retries.saturating_add(1),
                    resp.retries.rate_limited
                ));
            }
            latest_prompt_tokens = if resp.retries.retries == 0 {
                resp.usage
                    .as_ref()
                    .filter(|usage| usage.evidence.is_measured())
                    .map(|usage| usage.prompt_tokens)
            } else {
                None
            };
            // Wire clients fold any usage salvaged from failed retry attempts
            // into the successful response. That aggregate remains correct for
            // metered totals, but it is not an exact final-call cache measure.
            *latest_cached_tokens = if resp.retries.retries == 0 {
                resp.usage
                    .as_ref()
                    .filter(|usage| usage.evidence.is_measured())
                    .and_then(|usage| usage.cached_tokens)
            } else {
                None
            };
            if !usage_overflowed
                && !observe_usage_checked(
                    &mut usage_total,
                    &mut saw_unreported_usage,
                    resp.usage.as_ref(),
                )
            {
                usage_overflowed = true;
            }
            push_message(history, &mut appended, resp.message.clone());
            let calls = resp.message.tool_calls.clone().unwrap_or_default();
            let finish_kind = classify_finish_reason(&resp.finish_reason);
            let tool_use_confirmed = matches!(finish_kind, FinishKind::ToolUse);
            if calls.is_empty() || !tool_use_confirmed {
                self.report_prefix_drift(&prefix_seal, history, &mut prefix_drift_reported);
                let text = resp.message.content.clone().unwrap_or_default();
                if !calls.is_empty() {
                    self.emit(
                        "finish reason did not confirm tool use - tool calls were not executed",
                    );
                }
                let (outcome, failure_class) = match finish_kind {
                    FinishKind::Normal if calls.is_empty() => (Outcome::Pass, None),
                    FinishKind::Normal | FinishKind::ToolUse => {
                        if calls.is_empty() {
                            self.emit(
                                "tool-use finish reason without tool calls - treated as partial",
                            );
                        }
                        (Outcome::Partial, Some(FailureClass::Constraint))
                    }
                    FinishKind::Truncated => (Outcome::Partial, Some(FailureClass::Constraint)),
                    FinishKind::Context => (Outcome::Partial, Some(FailureClass::Context)),
                    FinishKind::Filtered => (Outcome::Fail, Some(FailureClass::Filtered)),
                    FinishKind::Interrupted => (Outcome::Partial, Some(FailureClass::Constraint)),
                    FinishKind::Missing => {
                        self.emit("finish reason missing - treated as partial");
                        (Outcome::Partial, Some(FailureClass::Constraint))
                    }
                    FinishKind::Unknown => {
                        self.emit("unrecognized finish reason - treated as partial");
                        (Outcome::Partial, Some(FailureClass::Constraint))
                    }
                };
                let mut receipt = self.make_receipt(
                    task,
                    ReceiptFields {
                        turns,
                        tool_calls,
                        duration_ms: u64::try_from(turn_started.elapsed().as_millis())
                            .unwrap_or(u64::MAX),
                        outcome,
                        failure_class,
                        usage: if usage_overflowed {
                            None
                        } else {
                            usage_total.clone()
                        },
                        repairs,
                        retries: retry_total,
                        compaction: compaction_total,
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
                    self.emit(&format!("turn {turns}: repaired tool call - {note}"));
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
                push_message(
                    history,
                    &mut appended,
                    ChatMessage {
                        role: "tool".into(),
                        content: Some(result.output),
                        parts: None,
                        tool_calls: None,
                        tool_call_id: Some(call.id.clone()),
                        reasoning_content: None,
                    },
                );
            }

            self.report_prefix_drift(&prefix_seal, history, &mut prefix_drift_reported);
        }

        self.report_prefix_drift(&prefix_seal, history, &mut prefix_drift_reported);

        let mut receipt = self.make_receipt(
            task,
            ReceiptFields {
                turns,
                tool_calls,
                duration_ms: u64::try_from(turn_started.elapsed().as_millis()).unwrap_or(u64::MAX),
                outcome: Outcome::Timeout,
                failure_class: Some(FailureClass::Constraint),
                usage: if usage_overflowed {
                    None
                } else {
                    usage_total.clone()
                },
                repairs,
                retries: retry_total,
                compaction: compaction_total,
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
                    "unknown tool '{}' - available tools: {}",
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
        if self.ctx.cancel.load(std::sync::atomic::Ordering::Acquire) {
            receipt.kind = ReceiptKind::CancelledTurn;
        }
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

    fn make_receipt(&self, task: &str, fields: ReceiptFields) -> Receipt {
        let ReceiptFields {
            turns,
            tool_calls,
            duration_ms,
            outcome,
            failure_class,
            usage,
            repairs,
            retries,
            compaction,
        } = fields;
        let cache_hit_pct = usage
            .as_ref()
            .filter(|usage| usage.evidence.is_measured())
            .and_then(|usage| crate::wire::cache_hit_pct(usage.prompt_tokens, usage.cached_tokens));
        Receipt {
            kind: ReceiptKind::Task,
            ts_utc: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            model_id: self.model_id.clone(),
            task: task.to_string(),
            turns,
            tool_calls,
            duration_ms: Some(duration_ms),
            outcome,
            failure_class,
            usage,
            cache_hit_pct,
            repairs,
            retries,
            compaction: Box::new(compaction),
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

/// "turn 2: edit_file src/lib.rs" - name plus the key argument, kept short.
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
