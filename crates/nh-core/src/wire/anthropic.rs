//! Anthropic Messages request encoding, response parsing, and client.

use nh_routes::ThinkingDialect;
use zeroize::Zeroizing;

use super::http::{client, provider_error, read_body_capped, send_error, MAX_PROVIDER_BODY_BYTES};
use super::{ChatClient, ChatMessage, ChatRequest, ChatResponse, ToolCallReq, Usage};

/// Blocking client for `POST {base_url}/v1/messages`.
pub struct AnthropicMessagesClient {
    pub base_url: String,
    api_key: Zeroizing<String>,
    max_tokens: u64,
    dialect: ThinkingDialect,
    http: reqwest::blocking::Client,
}

impl AnthropicMessagesClient {
    pub(crate) fn new(
        base_url: String,
        api_key: Zeroizing<String>,
        max_tokens: u64,
        dialect: ThinkingDialect,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            base_url,
            api_key,
            max_tokens,
            dialect,
            http: client()?,
        })
    }
}

impl ChatClient for AnthropicMessagesClient {
    fn complete(&self, request: &ChatRequest) -> anyhow::Result<ChatResponse> {
        let url = endpoint(&self.base_url);
        let response = self
            .http
            .post(&url)
            .header("x-api-key", self.api_key.as_str())
            .header("anthropic-version", "2023-06-01")
            .json(&build_body(request, self.max_tokens, self.dialect))
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
    format!("{}/v1/messages", base_url.trim_end_matches('/'))
}

/// Map the common request onto Anthropic Messages. The first system message
/// becomes top-level `system`; assistant tool calls become `tool_use`; and
/// consecutive tool results merge into one user message so roles alternate.
pub(super) fn build_body(
    request: &ChatRequest,
    max_tokens: u64,
    dialect: ThinkingDialect,
) -> serde_json::Value {
    let mut system: Option<String> = None;
    let mut messages: Vec<serde_json::Value> = Vec::new();
    let mut previous_was_user = false;
    for message in &request.messages {
        match message.role.as_str() {
            "system" if system.is_none() => {
                system = Some(message.content.clone().unwrap_or_default());
            }
            "tool" => {
                let block = serde_json::json!({
                    "type": "tool_result",
                    "tool_use_id": message.tool_call_id.clone().unwrap_or_default(),
                    "content": message.content.clone().unwrap_or_default(),
                });
                push_user_block(&mut messages, previous_was_user, block);
                previous_was_user = true;
            }
            "assistant" => {
                let mut blocks: Vec<serde_json::Value> = Vec::new();
                if let Some(text) = message.content.as_deref().filter(|text| !text.is_empty()) {
                    blocks.push(serde_json::json!({ "type": "text", "text": text }));
                }
                for call in message.tool_calls.iter().flatten() {
                    let input: serde_json::Value = serde_json::from_str(&call.arguments)
                        .unwrap_or_else(|_| serde_json::json!({}));
                    blocks.push(serde_json::json!({
                        "type": "tool_use",
                        "id": call.id,
                        "name": call.name,
                        "input": input,
                    }));
                }
                messages.push(serde_json::json!({
                    "role": "assistant",
                    "content": blocks,
                }));
                previous_was_user = false;
            }
            // User and unexpected roles degrade to user text; content is never dropped.
            _ => {
                let text = message.content.clone().unwrap_or_default();
                let block = serde_json::json!({ "type": "text", "text": text });
                push_user_block(&mut messages, previous_was_user, block);
                previous_was_user = true;
            }
        }
    }

    let mut body = serde_json::json!({
        "model": request.model,
        "max_tokens": max_tokens,
        "messages": messages,
    });
    if let Some(system) = system {
        body["system"] = serde_json::Value::String(system);
    }
    if !request.tools.is_empty() {
        body["tools"] = request
            .tools
            .iter()
            .map(|tool| {
                serde_json::json!({
                    "name": tool.name,
                    "description": tool.description,
                    "input_schema": tool.parameters,
                })
            })
            .collect();
    }
    if dialect == ThinkingDialect::DeepseekNhm {
        body["thinking"] = serde_json::json!({ "type": "disabled" });
    }
    body
}

fn push_user_block(
    messages: &mut Vec<serde_json::Value>,
    previous_was_user: bool,
    block: serde_json::Value,
) {
    match messages.last_mut() {
        Some(last) if previous_was_user => match last["content"].as_array_mut() {
            Some(content) => content.push(block),
            None => {
                messages.push(serde_json::json!({ "role": "user", "content": [block] }));
            }
        },
        _ => messages.push(serde_json::json!({ "role": "user", "content": [block] })),
    }
}

#[derive(serde::Deserialize)]
struct AnthropicResponse {
    #[serde(default)]
    content: Vec<AnthropicBlock>,
    #[serde(default)]
    stop_reason: Option<String>,
    #[serde(default)]
    usage: Option<AnthropicUsage>,
}

/// Unknown content block types are ignored so a new optional block cannot
/// corrupt text or tool-use parsing.
#[derive(serde::Deserialize)]
struct AnthropicBlock {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    input: Option<serde_json::Value>,
}

#[derive(serde::Deserialize)]
struct AnthropicUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: Option<u64>,
}

pub(super) fn parse_response(body: &str) -> anyhow::Result<ChatResponse> {
    let wire: AnthropicResponse = serde_json::from_str(body)
        .map_err(|error| anyhow::anyhow!("could not parse provider response: {error}"))?;
    let mut text = String::new();
    let mut saw_text = false;
    let mut calls: Vec<ToolCallReq> = Vec::new();
    for block in wire.content {
        match block.kind.as_str() {
            "text" => {
                saw_text = true;
                text.push_str(block.text.as_deref().unwrap_or_default());
            }
            "tool_use" => {
                let id = block.id.filter(|id| !id.is_empty());
                let name = block.name.filter(|name| !name.is_empty());
                let (Some(id), Some(name)) = (id, name) else {
                    return Err(anyhow::anyhow!(
                        "provider tool_use block missing id or name"
                    ));
                };
                calls.push(ToolCallReq {
                    id,
                    name,
                    arguments: serde_json::to_string(
                        &block.input.unwrap_or_else(|| serde_json::json!({})),
                    )
                    .unwrap_or_else(|_| "{}".into()),
                });
            }
            _ => {}
        }
    }
    Ok(ChatResponse {
        message: ChatMessage {
            role: "assistant".into(),
            content: saw_text.then_some(text),
            tool_calls: (!calls.is_empty()).then_some(calls),
            tool_call_id: None,
            reasoning_content: None,
        },
        finish_reason: wire.stop_reason.unwrap_or_default(),
        usage: wire.usage.map(|usage| Usage {
            prompt_tokens: usage.input_tokens,
            completion_tokens: usage.output_tokens,
            cached_tokens: usage.cache_read_input_tokens,
        }),
    })
}
