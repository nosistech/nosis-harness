//! Chat wire clients and their common request/response contract.
//!
//! Provider-specific encoding and parsing live in `openai` and `anthropic`.
//! The crate-private factory captures immutable route policy only after the
//! credential boundary authorizes and materializes the route secret.

mod anthropic;
mod http;
mod openai;

pub use anthropic::AnthropicMessagesClient;
pub use openai::OpenAiCompatClient;

use nh_routes::{ThinkingDialect, ThinkingPosture, Wire};
use openai::{OpenAiPolicy, DEFAULT_MAX_TOKENS};
use zeroize::Zeroizing;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallReq>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Model reasoning captured from responses; sent back in history only per
    /// route policy (`preserve_reasoning` or the tool-replay quirk).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
}

/// OpenAI shape: `arguments` is a raw JSON string.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolCallReq {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// Requested thinking effort. Clients map it to the route's dialect;
/// `None` means no extra thinking was requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThinkingEffort {
    #[default]
    None,
    Low,
    High,
    Max,
}

/// Stable user-facing vocabulary for effective thinking effort.
pub fn effort_label(effort: ThinkingEffort) -> &'static str {
    match effort {
        ThinkingEffort::None => "none",
        ThinkingEffort::Low => "low",
        ThinkingEffort::High => "high",
        ThinkingEffort::Max => "max",
    }
}

/// Resolve a user override or profile posture within immutable route
/// capability. A profile cannot disable an always-thinking route or enable a
/// route without a thinking toggle.
pub fn resolve_effort(
    explicit: Option<ThinkingEffort>,
    posture: ThinkingPosture,
    dialect: ThinkingDialect,
    wire: Wire,
) -> ThinkingEffort {
    if wire == Wire::AnthropicMessages {
        return ThinkingEffort::None;
    }
    if let Some(effort) = explicit {
        return match dialect {
            ThinkingDialect::DeepseekNhm => match effort {
                ThinkingEffort::Low => ThinkingEffort::None,
                _ => effort,
            },
            ThinkingDialect::KimiToggle => effort,
            ThinkingDialect::AlwaysThinking => ThinkingEffort::High,
            ThinkingDialect::AlwaysThinkingEffort => match effort {
                ThinkingEffort::None | ThinkingEffort::Low => ThinkingEffort::Low,
                ThinkingEffort::High | ThinkingEffort::Max => effort,
            },
            ThinkingDialect::GlmHm => match effort {
                ThinkingEffort::High | ThinkingEffort::Max => effort,
                ThinkingEffort::Low => ThinkingEffort::High,
                ThinkingEffort::None => ThinkingEffort::None,
            },
            ThinkingDialect::None => ThinkingEffort::None,
        };
    }

    match posture {
        ThinkingPosture::Floor => match dialect {
            ThinkingDialect::AlwaysThinking => ThinkingEffort::High,
            ThinkingDialect::AlwaysThinkingEffort => ThinkingEffort::Low,
            ThinkingDialect::DeepseekNhm
            | ThinkingDialect::KimiToggle
            | ThinkingDialect::GlmHm
            | ThinkingDialect::None => ThinkingEffort::None,
        },
        ThinkingPosture::Default => match dialect {
            ThinkingDialect::AlwaysThinking
            | ThinkingDialect::AlwaysThinkingEffort
            | ThinkingDialect::GlmHm => ThinkingEffort::High,
            ThinkingDialect::DeepseekNhm | ThinkingDialect::KimiToggle | ThinkingDialect::None => {
                ThinkingEffort::None
            }
        },
        ThinkingPosture::Ceiling => match dialect {
            ThinkingDialect::DeepseekNhm
            | ThinkingDialect::KimiToggle
            | ThinkingDialect::AlwaysThinking
            | ThinkingDialect::GlmHm => ThinkingEffort::High,
            ThinkingDialect::AlwaysThinkingEffort => ThinkingEffort::Max,
            ThinkingDialect::None => ThinkingEffort::None,
        },
    }
}

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<nh_tools::ToolSpec>,
    pub thinking: ThinkingEffort,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_tokens: Option<u64>,
}

/// Session cache-hit percentage from cumulative usage.
///
/// Returns `None` when there are no prompt tokens to divide by or cached
/// tokens cannot honestly be treated as a subset of prompt tokens.
pub fn cache_hit_pct(prompt_tokens: u64, cached_tokens: u64) -> Option<f64> {
    if prompt_tokens == 0 || cached_tokens > prompt_tokens {
        return None;
    }
    Some(100.0 * cached_tokens as f64 / prompt_tokens as f64)
}

#[derive(Debug, Clone)]
pub struct ChatResponse {
    pub message: ChatMessage,
    pub finish_reason: String,
    pub usage: Option<Usage>,
}

/// Provider abstraction. Tests inject a mock; production constructs a client
/// only through the credential module.
pub trait ChatClient: Send + Sync {
    fn complete(&self, request: &ChatRequest) -> anyhow::Result<ChatResponse>;
}

/// Build the correct wire client after the credential module has authorized
/// and materialized the route-scoped secret.
pub(crate) fn make_client(
    route: &nh_routes::ResolvedRoute,
    api_key: Zeroizing<String>,
    max_out: Option<u64>,
) -> anyhow::Result<Box<dyn ChatClient>> {
    let client: Box<dyn ChatClient> = match route.wire() {
        Wire::OpenAi => {
            let mut client = OpenAiCompatClient::new(route.base_url().to_owned(), api_key)?;
            client.policy = OpenAiPolicy {
                dialect: route.thinking_dialect(),
                preserve_reasoning: route.preserve_reasoning(),
                preserve_when_thinking: route.preserve_when_thinking(),
                empty_reasoning_on_tool_replay: route
                    .has_quirk("empty-reasoning-content-on-tool-replay"),
                max_out,
            };
            Box::new(client)
        }
        Wire::AnthropicMessages => Box::new(AnthropicMessagesClient::new(
            route.base_url().to_owned(),
            api_key,
            max_out.unwrap_or(DEFAULT_MAX_TOKENS),
            route.thinking_dialect(),
        )?),
    };
    Ok(client)
}

#[cfg(test)]
mod tests;
