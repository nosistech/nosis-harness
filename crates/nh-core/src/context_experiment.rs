//! Run-only extractive context experiment.
//!
//! This wrapper never summarizes or invents state. It preserves every system
//! and user message, removes only validated complete assistant/tool groups,
//! and retains the removed messages through the run's observation capability.

use crate::agent::{effective_context, estimate_message_tokens, estimate_request_tokens};
use crate::efficiency::{ContextEconomicsMeasurement, ContextMeasurement, EfficiencyRecorder};
use crate::wire::{ChatClient, ChatMessage, ChatRequest, ChatResponse, ContentPart};
use nh_routes::PriceQuote;
use nh_tools::ObservationRetainer;
use serde::Serialize;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::Mutex;

const KEEP_RECENT_GROUPS: usize = 2;
const MAX_CONTEXT_ARCHIVES: u32 = 8;
const ASSUMED_FUTURE_REQUESTS: u32 = 2;
const RETRIEVAL_OUTPUT_TOKEN_ALLOWANCE: u64 = 512;
const SOFT_PRESSURE_PERCENT: u64 = 50;
const HARD_PRESSURE_PERCENT: u64 = 70;
const ECONOMICS_LIMITATION: &str =
    "heuristic only: assumes two future requests and one full archive retrieval with a 512-output-token allowance; no saving is guaranteed";

/// Inputs fixed at the start of one explicitly enabled `nh run` experiment.
pub struct ExtractiveContextConfig {
    pub context_limit: u64,
    pub max_turns: u32,
    pub quote: Option<PriceQuote>,
    pub archive: ObservationRetainer,
    pub measurement: Option<EfficiencyRecorder>,
    pub cancel: Arc<AtomicBool>,
}

/// Wrap a provider client without changing the baseline agent loop or receipts.
pub fn wrap_extractive_context(
    inner: Box<dyn ChatClient>,
    config: ExtractiveContextConfig,
) -> Box<dyn ChatClient> {
    Box::new(ExtractiveContextClient {
        inner,
        effective_context_limit: effective_context(config.context_limit),
        max_turns: config.max_turns,
        quote: config.quote,
        archive: config.archive,
        measurement: config.measurement,
        cancel: config.cancel,
        next_request: AtomicU64::new(0),
        state: Mutex::new(ContextState::default()),
    })
}

struct ExtractiveContextClient {
    inner: Box<dyn ChatClient>,
    effective_context_limit: u64,
    max_turns: u32,
    quote: Option<PriceQuote>,
    archive: ObservationRetainer,
    measurement: Option<EfficiencyRecorder>,
    cancel: Arc<AtomicBool>,
    next_request: AtomicU64,
    state: Mutex<ContextState>,
}

#[derive(Default)]
struct ContextState {
    preceding_cached_tokens: Option<u64>,
    archive_count: u32,
    active: Option<ActiveArchive>,
}

struct ActiveArchive {
    history_prefix: Vec<Vec<u8>>,
    removed_indices: Vec<usize>,
    handle: String,
    messages: u64,
    estimated_tokens: u64,
}

#[derive(Clone, Copy)]
struct PlanFacts {
    request_seq: u64,
    original_tokens: u64,
    system_messages: u64,
    user_messages: u64,
    history_rewrite: bool,
}

impl ActiveArchive {
    fn matches(&self, messages: &[ChatMessage]) -> bool {
        messages.len() >= self.history_prefix.len()
            && messages
                .iter()
                .take(self.history_prefix.len())
                .map(message_bytes)
                .eq(self.history_prefix.iter().cloned())
    }
}

impl ChatClient for ExtractiveContextClient {
    fn complete(&self, request: &ChatRequest) -> anyhow::Result<ChatResponse> {
        let request_seq = self
            .next_request
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        let (sent, measurement) = match self.state.lock() {
            Ok(mut state) => self.prepare_request(request, request_seq, &mut state),
            Err(_) => (
                request.clone(),
                self.state_unavailable_measurement(request, request_seq),
            ),
        };
        if let Some(recorder) = &self.measurement {
            recorder.record_context(measurement);
        }

        let result = self.inner.complete(&sent);
        if let Ok(mut state) = self.state.lock() {
            state.preceding_cached_tokens = result
                .as_ref()
                .ok()
                .filter(|response| response.retries.retries == 0)
                .and_then(|response| response.usage.as_ref())
                .filter(|usage| usage.evidence.is_measured())
                .and_then(|usage| usage.cached_tokens);
        }
        result
    }
}

impl ExtractiveContextClient {
    fn prepare_request(
        &self,
        request: &ChatRequest,
        request_seq: u64,
        state: &mut ContextState,
    ) -> (ChatRequest, ContextMeasurement) {
        let original_tokens = estimate_request_tokens(&request.messages, &request.tools, true);
        let system_messages = count_role(&request.messages, "system");
        let user_messages = count_role(&request.messages, "user");
        let history_rewrite = state
            .active
            .as_ref()
            .is_some_and(|active| !active.matches(&request.messages));
        if history_rewrite {
            state.active = None;
        }
        let facts = PlanFacts {
            request_seq,
            original_tokens,
            system_messages,
            user_messages,
            history_rewrite,
        };

        let current = state
            .active
            .as_ref()
            .map_or_else(|| request.clone(), |active| apply_archive(request, active));
        let current_tokens = estimate_request_tokens(&current.messages, &current.tools, true);
        let candidate = archive_candidate(&request.messages);
        let candidate_adds_messages = candidate.as_ref().is_some_and(|candidate| {
            state
                .active
                .as_ref()
                .is_none_or(|active| active.removed_indices != candidate.removed_indices)
        });
        let pressure = context_percent(current_tokens, self.effective_context_limit);
        let hard_pressure = pressure >= HARD_PRESSURE_PERCENT;
        let assumed_future_requests = remaining_request_assumption(self.max_turns, request_seq);
        let mut economics = economics_measurement(
            self.quote.as_ref(),
            state.preceding_cached_tokens,
            assumed_future_requests,
        );

        if pressure < SOFT_PRESSURE_PERCENT {
            let decision = if state.active.is_some() {
                "archive_reused"
            } else {
                "unchanged"
            };
            let reason = if history_rewrite {
                "history_rewrite"
            } else {
                "below_soft_pressure"
            };
            return self.finish_plan(current, decision, reason, state, facts, economics);
        }

        let Some(candidate) = candidate.filter(|_| candidate_adds_messages) else {
            let decision = if state.active.is_some() {
                "archive_reused"
            } else {
                "unchanged"
            };
            let reason = if state.active.is_some() {
                "no_additional_completed_groups"
            } else {
                "no_completed_groups"
            };
            return self.finish_plan(current, decision, reason, state, facts, economics);
        };

        if state.archive_count >= MAX_CONTEXT_ARCHIVES {
            let decision = if state.active.is_some() {
                "archive_reused"
            } else {
                "unchanged"
            };
            return self.finish_plan(current, decision, "archive_limit", state, facts, economics);
        }

        let prospective = active_archive(request, &candidate, placeholder_handle());
        let prospective_request = apply_archive(request, &prospective);
        let prospective_tokens = estimate_request_tokens(
            &prospective_request.messages,
            &prospective_request.tools,
            true,
        );
        if prospective_tokens >= current_tokens {
            let decision = if state.active.is_some() {
                "archive_reused"
            } else {
                "deferred"
            };
            return self.finish_plan(
                current,
                decision,
                "no_token_reduction",
                state,
                facts,
                economics,
            );
        }
        let incremental_tokens_removed = current_tokens.saturating_sub(prospective_tokens);
        let favorable = evaluate_costs(
            self.quote.as_ref(),
            state.preceding_cached_tokens,
            assumed_future_requests,
            incremental_tokens_removed,
            candidate.estimated_tokens,
            prospective_tokens,
            &mut economics,
        );
        let economic_reason = favorable
            .as_ref()
            .err()
            .copied()
            .unwrap_or("estimated_cost_favorable");
        if !hard_pressure && favorable.is_err() {
            let decision = if state.active.is_some() {
                "archive_reused"
            } else {
                "deferred"
            };
            return self.finish_plan(current, decision, economic_reason, state, facts, economics);
        }

        let archive_json = match archive_payload(request, &candidate, &self.archive) {
            Ok(archive) => archive,
            Err(_) => {
                let (decision, reason) = archive_failure_labels(state.active.is_some());
                return self.finish_plan(current, decision, reason, state, facts, economics);
            }
        };
        if self.cancel.load(Ordering::Acquire) {
            let decision = if state.active.is_some() {
                "archive_reused"
            } else {
                "unchanged"
            };
            return self.finish_plan(
                current,
                decision,
                "cancelled_before_archive",
                state,
                facts,
                economics,
            );
        }
        let retained = match self.archive.retain(&archive_json) {
            Ok(retained) => retained,
            Err(_) => {
                let (decision, reason) = archive_failure_labels(state.active.is_some());
                return self.finish_plan(current, decision, reason, state, facts, economics);
            }
        };
        state.archive_count = state.archive_count.saturating_add(1);
        state.active = Some(active_archive(
            request,
            &candidate,
            retained.handle().to_owned(),
        ));
        let sent = apply_archive(request, state.active.as_ref().expect("archive installed"));
        self.finish_plan(
            sent,
            "archive_created",
            if hard_pressure {
                "hard_context_pressure"
            } else {
                "estimated_cost_favorable"
            },
            state,
            facts,
            economics,
        )
    }

    fn finish_plan(
        &self,
        sent: ChatRequest,
        decision: &'static str,
        reason: &'static str,
        state: &ContextState,
        facts: PlanFacts,
        economics: ContextEconomicsMeasurement,
    ) -> (ChatRequest, ContextMeasurement) {
        let sent_tokens = estimate_request_tokens(&sent.messages, &sent.tools, true);
        let (messages_archived, removed_tokens) = state
            .active
            .as_ref()
            .map_or((0, 0), |active| (active.messages, active.estimated_tokens));
        (
            sent,
            ContextMeasurement {
                request_seq: facts.request_seq,
                decision,
                reason,
                original_estimated_tokens: facts.original_tokens,
                sent_estimated_tokens: sent_tokens,
                removed_estimated_tokens: removed_tokens,
                effective_context_limit: self.effective_context_limit,
                original_context_percent: context_percent(
                    facts.original_tokens,
                    self.effective_context_limit,
                ),
                sent_context_percent: context_percent(sent_tokens, self.effective_context_limit),
                archive_count: state.archive_count,
                messages_archived,
                system_messages_preserved: facts.system_messages,
                user_messages_preserved: facts.user_messages,
                history_rewrite_detected: facts.history_rewrite,
                economics,
            },
        )
    }

    fn state_unavailable_measurement(
        &self,
        request: &ChatRequest,
        request_seq: u64,
    ) -> ContextMeasurement {
        let tokens = estimate_request_tokens(&request.messages, &request.tools, true);
        ContextMeasurement {
            request_seq,
            decision: "unchanged",
            reason: "state_unavailable",
            original_estimated_tokens: tokens,
            sent_estimated_tokens: tokens,
            removed_estimated_tokens: 0,
            effective_context_limit: self.effective_context_limit,
            original_context_percent: context_percent(tokens, self.effective_context_limit),
            sent_context_percent: context_percent(tokens, self.effective_context_limit),
            archive_count: 0,
            messages_archived: 0,
            system_messages_preserved: count_role(&request.messages, "system"),
            user_messages_preserved: count_role(&request.messages, "user"),
            history_rewrite_detected: false,
            economics: economics_measurement(self.quote.as_ref(), None, 0),
        }
    }
}

struct ArchiveCandidate {
    removed_indices: Vec<usize>,
    estimated_tokens: u64,
}

fn archive_candidate(messages: &[ChatMessage]) -> Option<ArchiveCandidate> {
    let groups = complete_tool_groups(messages);
    if groups.len() <= KEEP_RECENT_GROUPS {
        return None;
    }
    let removed_indices = groups[..groups.len() - KEEP_RECENT_GROUPS]
        .iter()
        .flat_map(|group| group.clone())
        .collect::<Vec<_>>();
    let removed = removed_indices
        .iter()
        .map(|index| messages[*index].clone())
        .collect::<Vec<_>>();
    Some(ArchiveCandidate {
        removed_indices,
        estimated_tokens: estimate_message_tokens(&removed, true),
    })
}

fn complete_tool_groups(messages: &[ChatMessage]) -> Vec<std::ops::Range<usize>> {
    let mut groups = Vec::new();
    let mut index = 0;
    while index < messages.len() {
        let message = &messages[index];
        let Some(calls) = message
            .tool_calls
            .as_ref()
            .filter(|calls| message.role == "assistant" && !calls.is_empty())
        else {
            index += 1;
            continue;
        };
        let ids = calls
            .iter()
            .map(|call| call.id.as_str())
            .collect::<HashSet<_>>();
        let end = index.saturating_add(1).saturating_add(calls.len());
        if ids.len() != calls.len() || end > messages.len() {
            index += 1;
            continue;
        }
        let responses = &messages[index + 1..end];
        let response_ids = responses
            .iter()
            .filter(|response| response.role == "tool")
            .filter_map(|response| response.tool_call_id.as_deref())
            .collect::<HashSet<_>>();
        let complete = responses.len() == calls.len()
            && message.tool_call_id.is_none()
            && responses
                .iter()
                .all(|response| response.role == "tool" && response.tool_calls.is_none())
            && response_ids == ids
            && messages.get(end).is_none_or(|next| next.role != "tool");
        if complete {
            groups.push(index..end);
            index = end;
        } else {
            index += 1;
        }
    }
    groups
}

fn active_archive(
    request: &ChatRequest,
    candidate: &ArchiveCandidate,
    handle: String,
) -> ActiveArchive {
    ActiveArchive {
        history_prefix: request.messages.iter().map(message_bytes).collect(),
        removed_indices: candidate.removed_indices.clone(),
        handle,
        messages: u64::try_from(candidate.removed_indices.len()).unwrap_or(u64::MAX),
        estimated_tokens: candidate.estimated_tokens,
    }
}

fn apply_archive(request: &ChatRequest, active: &ActiveArchive) -> ChatRequest {
    let removed = active
        .removed_indices
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    let mut messages = request
        .messages
        .iter()
        .enumerate()
        .filter(|(index, _)| !removed.contains(index))
        .map(|(_, message)| message.clone())
        .collect::<Vec<_>>();
    let notice_at = messages
        .iter()
        .take_while(|message| message.role == "system")
        .count();
    messages.insert(
        notice_at,
        ChatMessage {
            role: "system".into(),
            content: Some(format!(
                "[nosis] Extractive context archive: {} older completed assistant/tool messages (~{} tokens) are retained under handle={}. Every original system and user message and the latest {} complete tool exchanges remain verbatim. Use read_observation with this handle and a bounded character range if older evidence is needed.",
                active.messages, active.estimated_tokens, active.handle, KEEP_RECENT_GROUPS
            )),
            parts: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
        },
    );
    ChatRequest {
        model: request.model.clone(),
        messages,
        tools: request.tools.clone(),
        thinking: request.thinking,
    }
}

#[derive(Serialize)]
struct ArchivePayload {
    schema_version: u32,
    record_type: &'static str,
    limitation: &'static str,
    messages: Vec<ArchivedMessage>,
}

#[derive(Serialize)]
struct ArchivedMessage {
    original_index: usize,
    message: ChatMessage,
}

fn archive_payload(
    request: &ChatRequest,
    candidate: &ArchiveCandidate,
    retainer: &ObservationRetainer,
) -> serde_json::Result<String> {
    let messages = candidate
        .removed_indices
        .iter()
        .map(|index| ArchivedMessage {
            original_index: *index,
            message: scrub_message_for_archive(&request.messages[*index], retainer),
        })
        .collect();
    serde_json::to_string_pretty(&ArchivePayload {
        schema_version: 1,
        record_type: "extractive_context_archive",
        limitation: "scrubbed extractive history; no semantic summary or encryption claim",
        messages,
    })
}

fn scrub_message_for_archive(message: &ChatMessage, retainer: &ObservationRetainer) -> ChatMessage {
    ChatMessage {
        role: retainer.scrub_field(&message.role),
        content: message
            .content
            .as_deref()
            .map(|content| retainer.scrub_field(content)),
        parts: message.parts.as_ref().map(|parts| {
            parts
                .iter()
                .map(|part| match part {
                    ContentPart::Text { text } => ContentPart::Text {
                        text: retainer.scrub_field(text),
                    },
                    ContentPart::ImageB64 { media_type, data } => ContentPart::ImageB64 {
                        media_type: retainer.scrub_field(media_type),
                        data: if data.is_empty() {
                            String::new()
                        } else {
                            "[REDACTED ARCHIVED IMAGE DATA]".into()
                        },
                    },
                })
                .collect()
        }),
        tool_calls: message.tool_calls.as_ref().map(|calls| {
            calls
                .iter()
                .map(|call| crate::wire::ToolCallReq {
                    id: retainer.scrub_field(&call.id),
                    name: retainer.scrub_field(&call.name),
                    arguments: scrub_tool_arguments(&call.arguments, retainer),
                })
                .collect()
        }),
        tool_call_id: message
            .tool_call_id
            .as_deref()
            .map(|id| retainer.scrub_field(id)),
        reasoning_content: message
            .reasoning_content
            .as_deref()
            .map(|reasoning| retainer.scrub_field(reasoning)),
    }
}

fn scrub_tool_arguments(arguments: &str, retainer: &ObservationRetainer) -> String {
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(arguments) else {
        return "[REDACTED ARCHIVED TOOL ARGUMENTS]".into();
    };
    scrub_json_value(&mut value, retainer);
    serde_json::to_string(&value).unwrap_or_else(|_| "[REDACTED ARCHIVED TOOL ARGUMENTS]".into())
}

fn scrub_json_value(value: &mut serde_json::Value, retainer: &ObservationRetainer) {
    match value {
        serde_json::Value::String(text) => *text = retainer.scrub_field(text),
        serde_json::Value::Array(values) => {
            for value in values {
                scrub_json_value(value, retainer);
            }
        }
        serde_json::Value::Object(values) => {
            let old = std::mem::take(values);
            for (key, mut value) in old {
                scrub_json_value(&mut value, retainer);
                values.insert(retainer.scrub_field(&key), value);
            }
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {}
    }
}

fn economics_measurement(
    quote: Option<&PriceQuote>,
    preceding_cached_tokens: Option<u64>,
    assumed_future_requests: u32,
) -> ContextEconomicsMeasurement {
    ContextEconomicsMeasurement {
        quote_available: quote.is_some(),
        currency: quote.map(|quote| quote.currency.as_str().to_owned()),
        cache_hit_per_million: quote.map(|quote| quote.cache_hit),
        cache_miss_per_million: quote.map(|quote| quote.cache_miss),
        output_per_million: quote.map(|quote| quote.output),
        preceding_cached_tokens,
        assumed_future_requests,
        retrieval_output_token_allowance: RETRIEVAL_OUTPUT_TOKEN_ALLOWANCE,
        estimated_savings: None,
        estimated_penalty: None,
        limitation: ECONOMICS_LIMITATION,
    }
}

fn evaluate_costs(
    quote: Option<&PriceQuote>,
    preceding_cached_tokens: Option<u64>,
    assumed_future_requests: u32,
    incremental_tokens_removed: u64,
    archive_retrieval_tokens: u64,
    retained_tokens: u64,
    measurement: &mut ContextEconomicsMeasurement,
) -> Result<(), &'static str> {
    let Some(quote) = quote else {
        return Err("quote_unavailable");
    };
    let Some(cached_tokens) = preceding_cached_tokens else {
        return Err("cache_evidence_unavailable");
    };
    if assumed_future_requests < ASSUMED_FUTURE_REQUESTS {
        return Err("remaining_turns_uncertain");
    }
    let saving_rate = quote.cache_hit.min(quote.cache_miss);
    let retrieval_rate = quote.cache_hit.max(quote.cache_miss);
    let rebuild_tokens = cached_tokens.min(retained_tokens);
    let rebuild_rate = (quote.cache_miss - quote.cache_hit).max(0.0);
    let savings =
        incremental_tokens_removed as f64 * saving_rate * f64::from(assumed_future_requests)
            / 1_000_000.0;
    let penalty = (archive_retrieval_tokens as f64 * retrieval_rate
        + rebuild_tokens as f64 * rebuild_rate
        + RETRIEVAL_OUTPUT_TOKEN_ALLOWANCE as f64 * quote.output)
        / 1_000_000.0;
    if !savings.is_finite() || !penalty.is_finite() {
        return Err("estimated_cost_not_favorable");
    }
    measurement.estimated_savings = Some(savings);
    measurement.estimated_penalty = Some(penalty);
    (savings > penalty)
        .then_some(())
        .ok_or("estimated_cost_not_favorable")
}

fn remaining_request_assumption(max_turns: u32, request_seq: u64) -> u32 {
    let used = u32::try_from(request_seq).unwrap_or(u32::MAX);
    max_turns.saturating_sub(used).min(ASSUMED_FUTURE_REQUESTS)
}

fn archive_failure_labels(had_active: bool) -> (&'static str, &'static str) {
    if had_active {
        ("archive_reused", "archive_upgrade_failed")
    } else {
        ("unchanged", "archive_failed")
    }
}

fn placeholder_handle() -> String {
    format!("obs_{}", "0".repeat(32))
}

fn count_role(messages: &[ChatMessage], role: &str) -> u64 {
    u64::try_from(
        messages
            .iter()
            .filter(|message| message.role == role)
            .count(),
    )
    .unwrap_or(u64::MAX)
}

fn context_percent(tokens: u64, limit: u64) -> u64 {
    if limit == 0 {
        100
    } else {
        u64::try_from(
            (u128::from(tokens)
                .saturating_mul(100)
                .saturating_add(u128::from(limit / 2)))
                / u128::from(limit),
        )
        .unwrap_or(u64::MAX)
    }
}

fn message_bytes(message: &ChatMessage) -> Vec<u8> {
    serde_json::to_vec(message).expect("chat messages serialize")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::{
        ChatResponse, FinishReason, RetryStats, ThinkingEffort, ToolCallReq, Usage, UsageEvidence,
    };
    use nh_routes::{Currency, PriceConfidence};
    use nh_tools::{
        read_only_tools_with_features, Guard, ObservationSession, ToolCtx, ToolFeatures,
    };
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    struct CaptureClient {
        seen: Arc<Mutex<Vec<ChatRequest>>>,
    }

    impl ChatClient for CaptureClient {
        fn complete(&self, request: &ChatRequest) -> anyhow::Result<ChatResponse> {
            self.seen.lock().unwrap().push(request.clone());
            Ok(ChatResponse {
                message: plain("assistant", "done"),
                finish_reason: FinishReason::Stop,
                usage: Some(Usage {
                    prompt_tokens: estimate_request_tokens(&request.messages, &request.tools, true),
                    completion_tokens: 1,
                    cached_tokens: Some(0),
                    evidence: UsageEvidence::Measured,
                }),
                retries: RetryStats::default(),
            })
        }
    }

    fn plain(role: &str, content: &str) -> ChatMessage {
        ChatMessage {
            role: role.into(),
            content: Some(content.into()),
            parts: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
        }
    }

    fn group(id: &str, fill: char) -> [ChatMessage; 2] {
        [
            ChatMessage {
                role: "assistant".into(),
                content: None,
                parts: None,
                tool_calls: Some(vec![ToolCallReq {
                    id: id.into(),
                    name: "read_file".into(),
                    arguments: format!(r#"{{"path":"{id}.txt"}}"#),
                }]),
                tool_call_id: None,
                reasoning_content: Some(fill.to_string().repeat(1_200)),
            },
            ChatMessage {
                role: "tool".into(),
                content: Some(fill.to_string().repeat(4_000)),
                parts: None,
                tool_calls: None,
                tool_call_id: Some(id.into()),
                reasoning_content: None,
            },
        ]
    }

    fn request(groups: usize, include_second_user: bool) -> ChatRequest {
        let mut messages = vec![
            plain("system", "original system authority"),
            plain("system", "second system constraint"),
            plain("user", "original user constraint"),
        ];
        for index in 0..groups {
            messages.extend(group(
                &format!("call-{index}"),
                char::from(b'a' + index as u8),
            ));
            if include_second_user && index == 1 {
                messages.push(plain("user", "latest user constraint"));
            }
        }
        ChatRequest {
            model: "fixture".into(),
            messages,
            tools: Vec::new(),
            thinking: ThinkingEffort::Low,
        }
    }

    fn quote(hit: f64, miss: f64) -> PriceQuote {
        PriceQuote {
            cache_hit: hit,
            cache_miss: miss,
            output: 1.0,
            currency: Currency::Usd,
            peak: false,
            confidence: PriceConfidence::Confirmed,
        }
    }

    fn observation_fixture(
        root: &std::path::Path,
    ) -> (ToolCtx, ObservationSession, ObservationRetainer) {
        observation_fixture_with_scrubber(root, nh_vault::Scrubber::new(Vec::new()))
    }

    fn observation_fixture_with_scrubber(
        root: &std::path::Path,
        scrubber: nh_vault::Scrubber,
    ) -> (ToolCtx, ObservationSession, ObservationRetainer) {
        let runtime = root.join("observations");
        std::fs::create_dir(&runtime).unwrap();
        let ctx = ToolCtx::new(
            root.to_path_buf(),
            Box::new(|_| false),
            Box::new(|_| Guard::Allow),
            scrubber,
        );
        let session = ObservationSession::create(&runtime, &ctx).unwrap();
        let ctx = ctx.with_observation_session(session.clone()).unwrap();
        let retainer = ctx.observation_retainer().unwrap();
        (ctx, session, retainer)
    }

    #[test]
    fn hard_pressure_archives_complete_groups_and_preserves_all_authority() {
        let root = tempfile::tempdir().unwrap();
        let (ctx, session, archive) = observation_fixture(root.path());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let client = wrap_extractive_context(
            Box::new(CaptureClient {
                seen: Arc::clone(&seen),
            }),
            ExtractiveContextConfig {
                context_limit: 8_000,
                max_turns: 10,
                quote: None,
                archive,
                measurement: None,
                cancel: Arc::new(AtomicBool::new(false)),
            },
        );
        let original = request(5, true);

        client.complete(&original).unwrap();

        let sent = &seen.lock().unwrap()[0];
        for expected in original
            .messages
            .iter()
            .filter(|message| matches!(message.role.as_str(), "system" | "user"))
        {
            assert!(sent
                .messages
                .iter()
                .any(|actual| message_bytes(actual) == message_bytes(expected)));
        }
        let retained_assistants = sent
            .messages
            .iter()
            .filter(|message| message.role == "assistant")
            .collect::<Vec<_>>();
        assert_eq!(retained_assistants.len(), KEEP_RECENT_GROUPS);
        assert!(retained_assistants
            .iter()
            .all(|message| message.reasoning_content.is_some()));
        assert_complete_tool_pairs(&sent.messages);

        let notice =
            sent.messages
                .iter()
                .find_map(|message| {
                    message.content.as_deref().filter(|content| {
                        content.starts_with("[nosis] Extractive context archive:")
                    })
                })
                .unwrap();
        let handle = notice
            .split("handle=")
            .nth(1)
            .unwrap()
            .split('.')
            .next()
            .unwrap();
        let tools = read_only_tools_with_features(ToolFeatures {
            ranged_reads: false,
            observation_session: Some(session.clone()),
        });
        let reader = tools
            .iter()
            .find(|tool| tool.spec().name == "read_observation")
            .unwrap();
        let recovered = reader
            .execute(
                json!({"handle": handle, "char_offset": 0, "char_count": 6000}),
                &ctx,
            )
            .unwrap();
        assert!(recovered.contains("call-0"));
        assert!(recovered.contains("extractive_context_archive"));
        session.cleanup().unwrap();
    }

    #[test]
    fn archive_failure_sends_original_history_without_loss() {
        let root = tempfile::tempdir().unwrap();
        let (_ctx, session, archive) = observation_fixture(root.path());
        session.cleanup().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let client = wrap_extractive_context(
            Box::new(CaptureClient {
                seen: Arc::clone(&seen),
            }),
            ExtractiveContextConfig {
                context_limit: 8_000,
                max_turns: 10,
                quote: None,
                archive,
                measurement: None,
                cancel: Arc::new(AtomicBool::new(false)),
            },
        );
        let original = request(5, false);

        client.complete(&original).unwrap();

        assert_eq!(
            request_bytes(&seen.lock().unwrap()[0]),
            request_bytes(&original)
        );
    }

    #[test]
    fn archive_scrubs_fields_before_json_escaping() {
        let secret = "active \"quoted\\path\rcontrol\" secret";
        let root = tempfile::tempdir().unwrap();
        let (ctx, session, archive) = observation_fixture_with_scrubber(
            root.path(),
            nh_vault::Scrubber::new(vec![secret.into()]),
        );
        let mut original = request(5, false);
        let first_assistant = original
            .messages
            .iter_mut()
            .find(|message| message.role == "assistant")
            .unwrap();
        first_assistant.content = Some(secret.into());
        first_assistant.parts = Some(vec![
            ContentPart::Text {
                text: secret.into(),
            },
            ContentPart::ImageB64 {
                media_type: "image/png".into(),
                data: "encoded-secret-payload".into(),
            },
        ]);
        first_assistant.reasoning_content = Some(secret.into());
        let first_call = &mut first_assistant.tool_calls.as_mut().unwrap()[0];
        first_call.name = secret.into();
        first_call.arguments = serde_json::to_string(&json!({
            "nested": {"secret": secret},
            (secret): "key is secret too"
        }))
        .unwrap();
        let first_tool = original
            .messages
            .iter_mut()
            .find(|message| message.role == "tool")
            .unwrap();
        first_tool.content = Some(secret.into());

        let candidate = archive_candidate(&original.messages).unwrap();
        let archive_json = archive_payload(&original, &candidate, &archive).unwrap();
        let retained = archive.retain(&archive_json).unwrap();
        let handle = retained.handle();
        let tools = read_only_tools_with_features(ToolFeatures {
            ranged_reads: false,
            observation_session: Some(session.clone()),
        });
        let reader = tools
            .iter()
            .find(|tool| tool.spec().name == "read_observation")
            .unwrap();
        let mut json_text = String::new();
        let mut offset = 0_u64;
        while offset < retained.chars() {
            let recovered = reader
                .execute(
                    json!({"handle": handle, "char_offset": offset, "char_count": 6000}),
                    &ctx,
                )
                .unwrap();
            json_text.push_str(recovered.split_once('\n').unwrap().1);
            offset = offset.saturating_add(6_000).min(retained.chars());
        }
        let decoded: serde_json::Value = serde_json::from_str(&json_text).unwrap();
        assert_no_string_contains(&decoded, secret);
        assert!(json_text.contains("[REDACTED]"));
        assert!(json_text.contains("[REDACTED ARCHIVED IMAGE DATA]"));
        session.cleanup().unwrap();
    }

    #[test]
    fn archive_is_reused_until_compacted_history_reaches_pressure_again() {
        let root = tempfile::tempdir().unwrap();
        let (_ctx, session, archive) = observation_fixture(root.path());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let client = wrap_extractive_context(
            Box::new(CaptureClient {
                seen: Arc::clone(&seen),
            }),
            ExtractiveContextConfig {
                context_limit: 8_000,
                max_turns: 20,
                quote: None,
                archive,
                measurement: None,
                cancel: Arc::new(AtomicBool::new(false)),
            },
        );

        client.complete(&request(5, false)).unwrap();
        client.complete(&request(6, false)).unwrap();
        client.complete(&request(8, false)).unwrap();

        let seen = seen.lock().unwrap();
        let first = archive_handle(&seen[0]);
        let second = archive_handle(&seen[1]);
        let third = archive_handle(&seen[2]);
        assert_eq!(first, second, "stable archive should be reused");
        assert_ne!(second, third, "pressure should create one upgraded archive");
        session.cleanup().unwrap();
    }

    #[test]
    fn rewritten_history_never_reuses_stale_archive_indices() {
        let root = tempfile::tempdir().unwrap();
        let (_ctx, session, archive) = observation_fixture(root.path());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let client = wrap_extractive_context(
            Box::new(CaptureClient {
                seen: Arc::clone(&seen),
            }),
            ExtractiveContextConfig {
                context_limit: 8_000,
                max_turns: 20,
                quote: None,
                archive,
                measurement: None,
                cancel: Arc::new(AtomicBool::new(false)),
            },
        );
        client.complete(&request(5, false)).unwrap();
        let mut rewritten = request(5, false);
        rewritten.messages[0].content = Some("rewritten system authority".into());
        client.complete(&rewritten).unwrap();

        let seen = seen.lock().unwrap();
        assert_ne!(archive_handle(&seen[0]), archive_handle(&seen[1]));
        assert!(seen[1].messages.iter().any(|message| {
            message.role == "system"
                && message.content.as_deref() == Some("rewritten system authority")
        }));
        session.cleanup().unwrap();
    }

    #[test]
    fn cancellation_before_capture_keeps_full_history_and_writes_no_archive() {
        let root = tempfile::tempdir().unwrap();
        let (_ctx, session, archive) = observation_fixture(root.path());
        let cancel = Arc::new(AtomicBool::new(true));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let client = wrap_extractive_context(
            Box::new(CaptureClient {
                seen: Arc::clone(&seen),
            }),
            ExtractiveContextConfig {
                context_limit: 8_000,
                max_turns: 10,
                quote: None,
                archive,
                measurement: None,
                cancel,
            },
        );
        let original = request(5, false);

        client.complete(&original).unwrap();

        assert_eq!(
            request_bytes(&seen.lock().unwrap()[0]),
            request_bytes(&original)
        );
        let retained_files = std::fs::read_dir(root.path().join("observations"))
            .unwrap()
            .map(|entry| std::fs::read_dir(entry.unwrap().path()).unwrap().count())
            .sum::<usize>();
        assert_eq!(retained_files, 0);
        session.cleanup().unwrap();
    }

    #[test]
    fn malformed_tool_groups_are_never_removed() {
        let mut messages = request(3, false).messages;
        messages.push(ChatMessage {
            role: "assistant".into(),
            content: None,
            parts: None,
            tool_calls: Some(vec![ToolCallReq {
                id: "unpaired".into(),
                name: "read_file".into(),
                arguments: "{}".into(),
            }]),
            tool_call_id: None,
            reasoning_content: Some("must remain".into()),
        });
        let candidate = archive_candidate(&messages).unwrap();
        assert!(!candidate
            .removed_indices
            .iter()
            .any(|index| messages[*index].reasoning_content.as_deref() == Some("must remain")));
    }

    #[test]
    fn multi_call_groups_accept_complete_out_of_order_results_and_reject_incomplete_ones() {
        let assistant = ChatMessage {
            role: "assistant".into(),
            content: None,
            parts: None,
            tool_calls: Some(vec![
                ToolCallReq {
                    id: "first".into(),
                    name: "read_file".into(),
                    arguments: "{}".into(),
                },
                ToolCallReq {
                    id: "second".into(),
                    name: "read_file".into(),
                    arguments: "{}".into(),
                },
            ]),
            tool_call_id: None,
            reasoning_content: Some("kept as one group".into()),
        };
        let second_result = ChatMessage {
            tool_call_id: Some("second".into()),
            ..plain("tool", "second result")
        };
        let first_result = ChatMessage {
            tool_call_id: Some("first".into()),
            ..plain("tool", "first result")
        };

        let complete = vec![
            assistant.clone(),
            second_result.clone(),
            first_result,
            plain("user", "continue"),
        ];
        assert_eq!(complete_tool_groups(&complete), vec![0..3]);

        let incomplete = vec![assistant, second_result, plain("user", "continue")];
        assert!(complete_tool_groups(&incomplete).is_empty());
    }

    #[test]
    fn hard_pressure_does_not_archive_when_notice_would_grow_the_request() {
        let root = tempfile::tempdir().unwrap();
        let (_ctx, session, archive) = observation_fixture(root.path());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let client = wrap_extractive_context(
            Box::new(CaptureClient {
                seen: Arc::clone(&seen),
            }),
            ExtractiveContextConfig {
                context_limit: 1,
                max_turns: 10,
                quote: None,
                archive,
                measurement: None,
                cancel: Arc::new(AtomicBool::new(false)),
            },
        );
        let mut tiny = request(3, false);
        for message in &mut tiny.messages {
            if message.role == "assistant" {
                message.reasoning_content = None;
            }
            if message.role == "tool" {
                message.content = Some(String::new());
            }
        }

        client.complete(&tiny).unwrap();

        assert_eq!(
            request_bytes(&seen.lock().unwrap()[0]),
            request_bytes(&tiny)
        );
        session.cleanup().unwrap();
    }

    #[test]
    fn economics_defers_unknown_and_cheap_cache_but_pressure_is_separate() {
        let mut unknown = economics_measurement(None, None, 2);
        assert_eq!(
            evaluate_costs(None, None, 2, 10_000, 10_000, 5_000, &mut unknown),
            Err("quote_unavailable")
        );
        let cheap = quote(0.01, 2.0);
        let mut measured = economics_measurement(Some(&cheap), Some(4_000), 2);
        assert_eq!(
            evaluate_costs(
                Some(&cheap),
                Some(4_000),
                2,
                10_000,
                10_000,
                5_000,
                &mut measured,
            ),
            Err("estimated_cost_not_favorable")
        );
        assert!(measured.estimated_savings.is_some());
        assert!(measured.estimated_penalty.is_some());
        let equal = quote(1.0, 1.0);
        let mut favorable = economics_measurement(Some(&equal), Some(4_000), 2);
        assert_eq!(
            evaluate_costs(
                Some(&equal),
                Some(4_000),
                2,
                10_000,
                10_000,
                5_000,
                &mut favorable,
            ),
            Ok(())
        );
        assert_eq!(
            favorable.retrieval_output_token_allowance,
            RETRIEVAL_OUTPUT_TOKEN_ALLOWANCE
        );
        assert!(context_percent(7_000, 10_000) >= HARD_PRESSURE_PERCENT);
    }

    #[test]
    fn favorable_equal_price_archives_under_soft_pressure_after_cache_evidence() {
        let root = tempfile::tempdir().unwrap();
        let (_ctx, session, archive) = observation_fixture(root.path());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let client = wrap_extractive_context(
            Box::new(CaptureClient {
                seen: Arc::clone(&seen),
            }),
            ExtractiveContextConfig {
                context_limit: 11_000,
                max_turns: 10,
                quote: Some(quote(1.0, 1.0)),
                archive,
                measurement: None,
                cancel: Arc::new(AtomicBool::new(false)),
            },
        );

        client.complete(&request(1, false)).unwrap();
        client.complete(&request(5, false)).unwrap();

        let seen = seen.lock().unwrap();
        assert!(
            context_percent(
                estimate_request_tokens(&request(5, false).messages, &[], true),
                11_000
            ) < HARD_PRESSURE_PERCENT
        );
        assert!(seen[1].messages.iter().any(|message| {
            message
                .content
                .as_deref()
                .is_some_and(|content| content.starts_with("[nosis] Extractive context archive:"))
        }));
        assert_eq!(
            seen[1]
                .messages
                .iter()
                .filter(|message| message.role == "assistant")
                .count(),
            KEEP_RECENT_GROUPS
        );
        drop(seen);
        session.cleanup().unwrap();
    }

    fn archive_handle(request: &ChatRequest) -> String {
        request
            .messages
            .iter()
            .filter_map(|message| message.content.as_deref())
            .find(|content| content.starts_with("[nosis] Extractive context archive:"))
            .and_then(|notice| notice.split("handle=").nth(1))
            .and_then(|tail| tail.split('.').next())
            .unwrap()
            .to_owned()
    }

    fn assert_complete_tool_pairs(messages: &[ChatMessage]) {
        for message in messages.iter().filter(|message| {
            message.role == "assistant"
                && message.tool_calls.as_ref().is_some_and(|c| !c.is_empty())
        }) {
            for call in message.tool_calls.as_deref().unwrap() {
                assert!(messages.iter().any(|candidate| {
                    candidate.role == "tool"
                        && candidate.tool_call_id.as_deref() == Some(call.id.as_str())
                }));
            }
        }
        for message in messages.iter().filter(|message| message.role == "tool") {
            assert!(messages.iter().any(|candidate| {
                candidate.role == "assistant"
                    && candidate.tool_calls.as_ref().is_some_and(|calls| {
                        calls
                            .iter()
                            .any(|call| message.tool_call_id.as_deref() == Some(call.id.as_str()))
                    })
            }));
        }
    }

    fn request_bytes(request: &ChatRequest) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "model": &request.model,
            "messages": &request.messages,
            "tools": &request.tools,
            "thinking": format!("{:?}", request.thinking),
        }))
        .unwrap()
    }

    fn assert_no_string_contains(value: &serde_json::Value, forbidden: &str) {
        match value {
            serde_json::Value::String(value) => assert!(!value.contains(forbidden)),
            serde_json::Value::Array(values) => {
                for value in values {
                    assert_no_string_contains(value, forbidden);
                }
            }
            serde_json::Value::Object(values) => {
                for value in values.values() {
                    assert_no_string_contains(value, forbidden);
                }
            }
            _ => {}
        }
    }
}
