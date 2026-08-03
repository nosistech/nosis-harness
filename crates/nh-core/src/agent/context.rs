//! Cache-safe context accounting and compaction.

use crate::wire::{ChatMessage, ContentPart};

pub(super) const COMPACT_AT: f64 = 0.70;
const COMPACT_TARGET: f64 = 0.50;
const KEEP_RECENT: usize = 2;
const MESSAGE_OVERHEAD_TOKENS: u64 = 4;
const EFFECTIVE_CONTEXT_CAP: u64 = 256_000;
/// Coarse per-image allowance used only to trigger context compaction. It is
/// never billed or displayed as measured usage; providers report measured
/// image cost inside `usage.prompt_tokens`.
pub(super) const IMAGE_ESTIMATE_TOKENS: u64 = 32;

/// Context-rot guard: very large advertised windows use a smaller working
/// window so compaction starts while the retained context is still useful.
pub fn effective_context(route_window: u64) -> u64 {
    route_window.min(EFFECTIVE_CONTEXT_CAP)
}

/// Byte seal for a stable message prefix. `check` is active in every build.
#[derive(Debug, Clone)]
pub struct PrefixSeal {
    messages: Vec<Vec<u8>>,
}

impl PrefixSeal {
    pub fn new(prefix: &[ChatMessage]) -> Self {
        Self {
            messages: prefix.iter().map(message_bytes).collect(),
        }
    }

    pub fn check(&self, messages: &[ChatMessage]) -> bool {
        self.check_at(messages, 0)
    }

    pub(super) fn check_at(&self, messages: &[ChatMessage], start: usize) -> bool {
        messages
            .get(start..start.saturating_add(self.messages.len()))
            .is_some_and(|candidate| {
                candidate
                    .iter()
                    .map(message_bytes)
                    .eq(self.messages.iter().cloned())
            })
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Compaction {
    pub(super) messages: usize,
    pub(super) estimated_tokens_elided: u64,
    pub(super) prefix_held: bool,
}

pub(super) fn plain_msg(role: &str, content: String) -> ChatMessage {
    ChatMessage {
        role: role.into(),
        content: Some(content),
        parts: None,
        tool_calls: None,
        tool_call_id: None,
        reasoning_content: None,
    }
}

/// Message-only estimate used by compaction. Stored reasoning is counted
/// conservatively; request builders can use the policy-aware sibling below.
pub(super) fn estimate_tokens(messages: &[ChatMessage]) -> u64 {
    estimate_message_tokens(messages, true)
}

fn estimate_message_tokens(messages: &[ChatMessage], preserve_reasoning: bool) -> u64 {
    messages
        .iter()
        .map(|message| {
            let (content_bytes, image_count) = message.parts.as_ref().map_or_else(
                || (message.content.as_ref().map_or(0, String::len), 0_u64),
                |parts| {
                    parts
                        .iter()
                        .fold((0_usize, 0_u64), |totals, part| match part {
                            ContentPart::Text { text } => {
                                (totals.0.saturating_add(text.len()), totals.1)
                            }
                            ContentPart::ImageB64 { .. } => (totals.0, totals.1.saturating_add(1)),
                        })
                },
            );
            let tool_call_bytes = message.tool_calls.as_ref().map_or(0, |calls| {
                serde_json::to_vec(calls).map_or(0, |serialized| serialized.len())
            });
            let reasoning_bytes = if preserve_reasoning {
                message.reasoning_content.as_ref().map_or(0, String::len)
            } else {
                0
            };
            let bytes = (content_bytes as u64)
                .saturating_add(tool_call_bytes as u64)
                .saturating_add(reasoning_bytes as u64);
            bytes
                .div_ceil(4)
                .saturating_add(MESSAGE_OVERHEAD_TOKENS)
                .saturating_add(image_count.saturating_mul(IMAGE_ESTIMATE_TOKENS))
        })
        .sum()
}

/// Policy-aware request estimate including the serialized tool-spec array.
pub(super) fn estimate_request_tokens(
    messages: &[ChatMessage],
    tools: &[nh_tools::ToolSpec],
    preserve_reasoning: bool,
) -> u64 {
    let tool_bytes = serde_json::to_vec(tools).map_or(0, |serialized| serialized.len()) as u64;
    estimate_message_tokens(messages, preserve_reasoning).saturating_add(tool_bytes.div_ceil(4))
}

pub(super) fn compaction_input_tokens(latest_prompt_tokens: Option<u64>, estimated: u64) -> u64 {
    latest_prompt_tokens.map_or(estimated, |prompt_tokens| prompt_tokens.max(estimated))
}

/// Drop the smallest earlier prefix that brings the retained history under
/// target. The last two user turns win over the target when both cannot fit.
pub(super) fn compact_history(history: &mut Vec<ChatMessage>, limit: u64) -> Option<Compaction> {
    let user_indices: Vec<usize> = history
        .iter()
        .enumerate()
        .skip(1)
        .filter_map(|(index, message)| (message.role == "user").then_some(index))
        .collect();
    if user_indices.len() <= KEEP_RECENT {
        return None;
    }

    let required_position = user_indices.len() - KEEP_RECENT;
    let required_start = user_indices[required_position];
    let target = (COMPACT_TARGET * limit as f64) as u64;
    let prefix_tokens = estimate_tokens(&history[..1]);
    let start = user_indices[..=required_position]
        .iter()
        .copied()
        .filter(|index| *index > 1)
        .find(|index| prefix_tokens.saturating_add(estimate_tokens(&history[*index..])) <= target)
        .unwrap_or(required_start);
    if start <= 1 {
        return None;
    }

    let messages = start - 1;
    let tokens = estimate_tokens(&history[1..start]);
    let retained_seal = PrefixSeal::new(&history[start..]);
    history.drain(1..start);
    history.insert(
        1,
        plain_msg(
            "system",
            format!(
                "[nosis] earlier context compacted: {messages} messages, ~{tokens} tokens elided."
            ),
        ),
    );
    let prefix_held = retained_seal.check_at(history, 2);
    debug_assert!(prefix_held, "compaction changed a retained real message");

    Some(Compaction {
        messages,
        estimated_tokens_elided: tokens,
        prefix_held,
    })
}

pub(super) fn context_percentage(input_tokens: u64, limit: u64) -> u64 {
    if limit == 0 {
        100
    } else {
        (100.0 * input_tokens as f64 / limit as f64).round() as u64
    }
}

pub(super) fn message_bytes(message: &ChatMessage) -> Vec<u8> {
    serde_json::to_vec(message).expect("chat messages serialize")
}
