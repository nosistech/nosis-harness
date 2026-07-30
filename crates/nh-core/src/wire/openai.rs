//! OpenAI-compatible request encoding, response parsing, and client.

use nh_routes::ThinkingDialect;
use zeroize::Zeroizing;

use super::http::{client, provider_error, read_body_capped, send_error, MAX_PROVIDER_BODY_BYTES};
use super::{
    ChatClient, ChatMessage, ChatRequest, ChatResponse, ThinkingEffort, ToolCallReq, Usage,
};

pub(super) const DEFAULT_MAX_TOKENS: u64 = 65_536;

/// Per-route OpenAI-wire policy, captured once by the credentialed factory.
#[derive(Debug, Clone, Copy)]
pub(super) struct OpenAiPolicy {
    pub(super) dialect: ThinkingDialect,
    pub(super) preserve_reasoning: bool,
    pub(super) preserve_when_thinking: bool,
    pub(super) empty_reasoning_on_tool_replay: bool,
    pub(super) max_out: Option<u64>,
}

impl Default for OpenAiPolicy {
    fn default() -> Self {
        Self {
            dialect: ThinkingDialect::None,
            preserve_reasoning: false,
            preserve_when_thinking: false,
            empty_reasoning_on_tool_replay: false,
            max_out: None,
        }
    }
}

/// Blocking reqwest client against `{base_url}/chat/completions` (no streaming).
/// The API key is held zeroized, injected per call, and never logged.
pub struct OpenAiCompatClient {
    pub base_url: String,
    api_key: Zeroizing<String>,
    http: reqwest::blocking::Client,
    pub(super) policy: OpenAiPolicy,
}

impl OpenAiCompatClient {
    pub(crate) fn new(base_url: String, api_key: Zeroizing<String>) -> anyhow::Result<Self> {
        Ok(Self {
            base_url,
            api_key,
            http: client()?,
            policy: OpenAiPolicy::default(),
        })
    }
}

impl ChatClient for OpenAiCompatClient {
    fn complete(&self, request: &ChatRequest) -> anyhow::Result<ChatResponse> {
        let url = endpoint(&self.base_url);
        let response = self
            .http
            .post(&url)
            .bearer_auth(self.api_key.as_str())
            .json(&build_body(request, self.policy))
            .send()
            .map_err(|error| send_error(&url, &error))?;
        let status = response.status();
        let body = read_body_capped(response, MAX_PROVIDER_BODY_BYTES)?;
        if !status.is_success() {
            return Err(provider_error(status, &body, self.api_key.as_str()));
        }
        parse_response(&body)
    }
}

pub(super) fn endpoint(base_url: &str) -> String {
    format!("{}/chat/completions", base_url.trim_end_matches('/'))
}

/// Build the OpenAI-wire request body. Tool calls and tools use the nested
/// `{"type":"function","function":{...}}` shape the wire requires.
pub(super) fn build_body(request: &ChatRequest, policy: OpenAiPolicy) -> serde_json::Value {
    let thinking_active = thinking_is_active(policy.dialect, request.thinking);
    let messages: Vec<serde_json::Value> = request
        .messages
        .iter()
        .map(|message| {
            let mut encoded = serde_json::json!({ "role": message.role });
            if let Some(content) = &message.content {
                encoded["content"] = serde_json::Value::String(content.clone());
            }
            if let Some(calls) = &message.tool_calls {
                encoded["tool_calls"] = calls
                    .iter()
                    .map(|call| {
                        serde_json::json!({
                            "id": call.id,
                            "type": "function",
                            "function": {
                                "name": call.name,
                                "arguments": call.arguments,
                            }
                        })
                    })
                    .collect();
            }
            if let Some(id) = &message.tool_call_id {
                encoded["tool_call_id"] = serde_json::Value::String(id.clone());
            }
            if let Some(reasoning) = reasoning_to_send(message, policy, thinking_active) {
                encoded["reasoning_content"] = serde_json::Value::String(reasoning.to_owned());
            }
            encoded
        })
        .collect();
    let mut body = serde_json::json!({
        "model": request.model,
        "messages": messages,
        "max_tokens": policy.max_out.unwrap_or(DEFAULT_MAX_TOKENS),
    });
    // [VERIFY-LIVE §7] Provider-specific output_config effort mapping remains live-pending.
    if !request.tools.is_empty() {
        body["tools"] = request
            .tools
            .iter()
            .map(|tool| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.parameters,
                    }
                })
            })
            .collect();
    }
    apply_thinking(&mut body, policy, request.thinking);
    body
}

/// The single reasoning-replay policy boundary:
/// - preserving routes replay stored assistant reasoning;
/// - other routes omit it;
/// - the DeepSeek tool-only quirk inserts an empty string when required.
fn reasoning_to_send(
    message: &ChatMessage,
    policy: OpenAiPolicy,
    thinking_active: bool,
) -> Option<&str> {
    if message.role != "assistant" {
        return None;
    }
    if policy.preserve_reasoning || (policy.preserve_when_thinking && thinking_active) {
        if let Some(reasoning) = message.reasoning_content.as_deref() {
            return Some(reasoning);
        }
    }
    let tool_only = message
        .tool_calls
        .as_ref()
        .is_some_and(|calls| !calls.is_empty())
        && message.content.as_deref().is_none_or(str::is_empty);
    if policy.empty_reasoning_on_tool_replay && tool_only {
        return Some("");
    }
    None
}

fn thinking_is_active(dialect: ThinkingDialect, effort: ThinkingEffort) -> bool {
    match dialect {
        ThinkingDialect::DeepseekNhm => {
            matches!(effort, ThinkingEffort::High | ThinkingEffort::Max)
        }
        ThinkingDialect::GlmHm | ThinkingDialect::KimiToggle => effort != ThinkingEffort::None,
        ThinkingDialect::AlwaysThinking | ThinkingDialect::AlwaysThinkingEffort => true,
        ThinkingDialect::None => false,
    }
}

/// The single `(dialect, effort)` to OpenAI-wire parameter mapping.
fn apply_thinking(body: &mut serde_json::Value, policy: OpenAiPolicy, effort: ThinkingEffort) {
    match policy.dialect {
        ThinkingDialect::DeepseekNhm => match effort {
            ThinkingEffort::None | ThinkingEffort::Low => {
                // [VERIFY-LIVE §7] DeepSeek explicit non-thinking wire shape.
                body["thinking"] = serde_json::json!({ "type": "disabled" });
            }
            ThinkingEffort::High => {
                body["reasoning_effort"] = serde_json::Value::String("high".into());
            }
            ThinkingEffort::Max => {
                body["reasoning_effort"] = serde_json::Value::String("max".into());
            }
        },
        ThinkingDialect::KimiToggle => {
            // [VERIFY-LIVE §7] Kimi K2.6 documented thinking toggle shape.
            let kind = if effort == ThinkingEffort::None {
                "disabled"
            } else {
                "enabled"
            };
            body["thinking"] = serde_json::json!({ "type": kind });
            if effort != ThinkingEffort::None && policy.preserve_when_thinking {
                body["thinking"]["keep"] = serde_json::Value::String("all".into());
            }
        }
        ThinkingDialect::AlwaysThinkingEffort => {
            let effort = match effort {
                ThinkingEffort::None | ThinkingEffort::Low => "low",
                ThinkingEffort::High => "high",
                ThinkingEffort::Max => "max",
            };
            body["reasoning_effort"] = serde_json::Value::String(effort.into());
        }
        ThinkingDialect::GlmHm => match effort {
            ThinkingEffort::None => {
                body["thinking"] = serde_json::json!({ "type": "disabled" });
            }
            ThinkingEffort::Low | ThinkingEffort::High => {
                body["reasoning_effort"] = serde_json::Value::String("high".into());
            }
            ThinkingEffort::Max => {
                body["reasoning_effort"] = serde_json::Value::String("max".into());
            }
        },
        // Fixed-effort always-thinking routes and routes without a toggle emit nothing.
        ThinkingDialect::AlwaysThinking | ThinkingDialect::None => {}
    }
}

#[derive(serde::Deserialize)]
struct WireResponse {
    #[serde(default)]
    choices: Vec<WireChoice>,
    #[serde(default)]
    usage: Option<WireUsage>,
}

#[derive(serde::Deserialize)]
struct WireChoice {
    message: WireMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(serde::Deserialize)]
struct WireMessage {
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<WireToolCall>>,
    #[serde(default, alias = "reasoning")]
    reasoning_content: Option<String>,
}

#[derive(serde::Deserialize)]
struct WireToolCall {
    id: String,
    function: WireFunction,
}

#[derive(serde::Deserialize)]
struct WireFunction {
    name: String,
    arguments: String,
}

#[derive(serde::Deserialize)]
struct WireUsage {
    #[serde(default)]
    prompt_tokens: u64,
    #[serde(default)]
    completion_tokens: u64,
    #[serde(default)]
    prompt_tokens_details: Option<WirePromptDetails>,
    #[serde(default)]
    prompt_cache_hit_tokens: Option<u64>,
}

#[derive(serde::Deserialize)]
struct WirePromptDetails {
    #[serde(default)]
    cached_tokens: Option<u64>,
}

pub(super) fn parse_response(body: &str) -> anyhow::Result<ChatResponse> {
    let wire: WireResponse = serde_json::from_str(body)
        .map_err(|error| anyhow::anyhow!("could not parse provider response: {error}"))?;
    let choice = wire
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("provider response had no choices"))?;
    let tool_calls = choice.message.tool_calls.map(|calls| {
        calls
            .into_iter()
            .map(|call| ToolCallReq {
                id: call.id,
                name: call.function.name,
                arguments: call.function.arguments,
            })
            .collect()
    });
    Ok(ChatResponse {
        message: ChatMessage {
            role: choice.message.role.unwrap_or_else(|| "assistant".into()),
            content: choice.message.content,
            tool_calls,
            tool_call_id: None,
            reasoning_content: choice.message.reasoning_content,
        },
        finish_reason: choice.finish_reason.unwrap_or_default(),
        usage: wire.usage.map(|usage| {
            let cached_tokens = usage
                .prompt_tokens_details
                .and_then(|details| details.cached_tokens)
                .or(usage.prompt_cache_hit_tokens);
            Usage {
                prompt_tokens: usage.prompt_tokens,
                completion_tokens: usage.completion_tokens,
                cached_tokens,
            }
        }),
    })
}
