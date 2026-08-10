//! Anthropic Messages request encoding, response parsing, and client.

use std::time::Instant;

use nh_routes::ThinkingDialect;
use zeroize::Zeroizing;

use super::http::{
    client, is_request_timeout, provider_error, read_body_capped, send_error,
    MAX_PROVIDER_BODY_BYTES,
};
use super::retry::{
    is_retryable, parse_retry_after, run_with_retry, system_jitter, AttemptOutcome, AttemptResult,
    RetryPolicy,
};
use super::usage_debug::UsageDebug;
use super::{
    ChatClient, ChatMessage, ChatRequest, ChatResponse, ContentPart, RetryStats, ToolCallReq, Usage,
};

/// Blocking client for `POST {base_url}/v1/messages`.
pub struct AnthropicMessagesClient {
    pub base_url: String,
    api_key: Zeroizing<String>,
    max_tokens: u64,
    dialect: ThinkingDialect,
    http: reqwest::blocking::Client,
    usage_debug: Option<UsageDebug>,
}

impl AnthropicMessagesClient {
    pub(crate) fn new(
        base_url: String,
        api_key: Zeroizing<String>,
        max_tokens: u64,
        dialect: ThinkingDialect,
        route_id: &str,
    ) -> anyhow::Result<Self> {
        let usage_debug = UsageDebug::from_env(route_id, "anthropic", api_key.as_str());
        Ok(Self {
            base_url,
            api_key,
            max_tokens,
            dialect,
            http: client()?,
            usage_debug,
        })
    }
}

impl ChatClient for AnthropicMessagesClient {
    fn complete(&self, request: &ChatRequest) -> anyhow::Result<ChatResponse> {
        let url = endpoint(&self.base_url);
        let request_body = build_body(request, self.max_tokens, self.dialect);
        let mut output = run_with_retry(
            RetryPolicy::DEFAULT,
            &std::thread::sleep,
            &system_jitter,
            |_| {
                let started = Instant::now();
                let response = match self
                    .http
                    .post(&url)
                    .header("x-api-key", self.api_key.as_str())
                    .header("anthropic-version", "2023-06-01")
                    .json(&request_body)
                    .send()
                {
                    Ok(response) => response,
                    Err(error) => {
                        return AttemptResult::Failure {
                            outcome: AttemptOutcome::TransportFailure {
                                timed_out: is_request_timeout(&error),
                            },
                            retry_after: None,
                            detail: send_error(&url, &error).to_string(),
                            usage: None,
                            elapsed: started.elapsed(),
                        };
                    }
                };
                let status = response.status();
                let retry_after = response
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|value| value.to_str().ok())
                    .and_then(parse_retry_after);
                let body = match read_body_capped(response, MAX_PROVIDER_BODY_BYTES) {
                    Ok(body) => body,
                    Err(error) => {
                        return AttemptResult::Failure {
                            outcome: AttemptOutcome::HttpStatus(status.as_u16()),
                            retry_after,
                            detail: format!(
                                "could not read provider HTTP {} response: {error}",
                                status.as_u16()
                            ),
                            usage: None,
                            elapsed: started.elapsed(),
                        };
                    }
                };
                if let Some(debug) = &self.usage_debug {
                    debug.emit(&body);
                }
                if !status.is_success() {
                    let outcome = AttemptOutcome::HttpStatus(status.as_u16());
                    return AttemptResult::Failure {
                        outcome,
                        retry_after,
                        detail: provider_error(status, &body, self.api_key.as_str()).to_string(),
                        usage: if is_retryable(outcome) {
                            extract_usage(&body)
                        } else {
                            None
                        },
                        elapsed: started.elapsed(),
                    };
                }
                match parse_response(&body) {
                    Ok(response) => AttemptResult::Success(response),
                    Err(error) => AttemptResult::Failure {
                        outcome: AttemptOutcome::HttpStatus(status.as_u16()),
                        retry_after: None,
                        detail: error.to_string(),
                        usage: extract_usage(&body),
                        elapsed: started.elapsed(),
                    },
                }
            },
        )
        .map_err(anyhow::Error::new)?;
        let success_usage = output.value.usage.take();
        let combined_usage = output.combine_success_usage(success_usage);
        let mut response = output.value;
        response.retries = output.stats;
        response.usage = combined_usage;
        Ok(response)
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
            // User and unexpected roles degrade to user blocks; declared parts keep their order.
            _ => {
                let blocks: Vec<serde_json::Value> = match &message.parts {
                    Some(parts) if !parts.is_empty() => parts
                        .iter()
                        .map(|part| match part {
                            ContentPart::Text { text } => {
                                serde_json::json!({ "type": "text", "text": text })
                            }
                            ContentPart::ImageB64 { media_type, data } => serde_json::json!({
                                "type": "image",
                                "source": {
                                    "type": "base64",
                                    "media_type": media_type,
                                    "data": data,
                                }
                            }),
                        })
                        .collect(),
                    _ => vec![serde_json::json!({
                        "type": "text",
                        "text": message.content.clone().unwrap_or_default()
                    })],
                };
                for block in blocks {
                    push_user_block(&mut messages, previous_was_user, block);
                    previous_was_user = true;
                }
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
    input_tokens: Option<u64>,
    #[serde(default)]
    output_tokens: Option<u64>,
    #[serde(default)]
    cache_read_input_tokens: Option<u64>,
}

fn usage_from_wire(usage: AnthropicUsage) -> Usage {
    let evidence = super::usage_evidence(
        usage.input_tokens,
        usage.output_tokens,
        usage.cache_read_input_tokens,
    );
    Usage {
        prompt_tokens: usage.input_tokens.unwrap_or(0),
        completion_tokens: usage.output_tokens.unwrap_or(0),
        cached_tokens: usage.cache_read_input_tokens,
        evidence,
    }
}

pub(super) fn extract_usage(body: &str) -> Option<Usage> {
    serde_json::from_str::<AnthropicResponse>(body)
        .ok()?
        .usage
        .map(usage_from_wire)
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
            parts: None,
            tool_calls: (!calls.is_empty()).then_some(calls),
            tool_call_id: None,
            reasoning_content: None,
        },
        finish_reason: super::FinishReason::from_wire(wire.stop_reason, "tool_use"),
        usage: wire.usage.map(usage_from_wire),
        retries: RetryStats::default(),
    })
}
