//! Chat wire clients (M1): OpenAI-compatible + Anthropic Messages.
//! The crate-private factory captures per-route policy after the public
//! credential boundary authorizes and materializes the route's secret.
use nh_routes::{ThinkingDialect, ThinkingPosture, Wire};
use std::time::Duration;
use zeroize::Zeroizing;

/// Non-streaming completions from thinking routes legitimately run for
/// minutes; reqwest's blocking default would abort every request at 30 s.
/// Generous total cap instead - a dead host still fails fast on connect.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(600);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_MAX_TOKENS: u64 = 65_536;
const MAX_PROVIDER_BODY_BYTES: usize = 8 * 1024 * 1024;

/// One HTTP client config for both wire clients: explicit timeouts (never
/// the hidden 30 s blocking default) and no redirect following - reqwest
/// forwards custom headers like `x-api-key` across cross-host redirects,
/// so a redirecting endpoint must surface as a friendly HTTP error, not
/// silently mail the key to whoever controls the Location header.
/// Panics only where `Client::new` would (TLS backend unavailable).
fn http_client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .connect_timeout(CONNECT_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("HTTP client")
}

/// Read a blocking response body under a hard byte ceiling. Rejects an
/// oversized `Content-Length` up front, then streams under a `take(MAX+1)`
/// guard so a missing/lying Content-Length still cannot exhaust memory.
/// Providers send UTF-8 JSON; lossy decoding keeps error snippets safe.
fn read_body_capped(resp: reqwest::blocking::Response, max: usize) -> anyhow::Result<String> {
    use std::io::Read;
    if let Some(len) = resp.content_length() {
        if len > max as u64 {
            anyhow::bail!("provider response too large: {len} bytes exceeds cap {max}");
        }
    }
    let mut buf = Vec::new();
    resp.take(max as u64 + 1).read_to_end(&mut buf)?;
    if buf.len() > max {
        anyhow::bail!("provider response exceeded cap of {max} bytes");
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Friendly send-failure line: a slow provider is not an unreachable one.
fn send_error(url: &str, e: &reqwest::Error) -> anyhow::Error {
    anyhow::anyhow!(
        "{}",
        send_error_line(url, e.is_timeout() && !e.is_connect(), &e.to_string())
    )
}

fn send_error_line(url: &str, timed_out: bool, detail: &str) -> String {
    if timed_out {
        format!(
            "provider at {url} did not answer within {}s - retry, or switch to another route",
            REQUEST_TIMEOUT.as_secs()
        )
    } else {
        format!("could not reach provider at {url}: {detail}")
    }
}

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
    /// route policy (`preserve_reasoning` / deepseek tool-replay quirk).
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

/// Requested thinking effort. Clients map it to the route's dialect in one
/// function each; `None` means "no extra thinking requested".
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
/// capability. No profile can disable an always-thinking route or enable a
/// route with no thinking toggle.
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
            ThinkingDialect::GlmHm => match effort {
                ThinkingEffort::High | ThinkingEffort::Max => effort,
                ThinkingEffort::None | ThinkingEffort::Low => ThinkingEffort::High,
            },
            ThinkingDialect::None => ThinkingEffort::None,
        };
    }

    match posture {
        ThinkingPosture::Floor | ThinkingPosture::Default => match dialect {
            ThinkingDialect::AlwaysThinking | ThinkingDialect::GlmHm => ThinkingEffort::High,
            ThinkingDialect::DeepseekNhm | ThinkingDialect::KimiToggle | ThinkingDialect::None => {
                ThinkingEffort::None
            }
        },
        ThinkingPosture::Ceiling => match dialect {
            ThinkingDialect::DeepseekNhm
            | ThinkingDialect::KimiToggle
            | ThinkingDialect::AlwaysThinking
            | ThinkingDialect::GlmHm => ThinkingEffort::High,
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

/// Provider abstraction - tests inject a mock, production uses the
/// `credential` module.
pub trait ChatClient: Send + Sync {
    fn complete(&self, req: &ChatRequest) -> anyhow::Result<ChatResponse>;
}

/// Builds the right wire client after the credential module has authorized
/// and materialized the route-scoped secret.
pub(crate) fn make_client(
    route: &nh_routes::ResolvedRoute,
    api_key: Zeroizing<String>,
    max_out: Option<u64>,
) -> Box<dyn ChatClient> {
    match route.wire() {
        nh_routes::Wire::OpenAi => {
            let mut client = OpenAiCompatClient::new(route.base_url().to_owned(), api_key);
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
        nh_routes::Wire::AnthropicMessages => Box::new(AnthropicMessagesClient::new(
            route.base_url().to_owned(),
            api_key,
            max_out.unwrap_or(DEFAULT_MAX_TOKENS),
        )),
    }
}

/// Per-route OpenAI-wire policy, captured once by `make_client`.
#[derive(Debug, Clone, Copy)]
struct OpenAiPolicy {
    dialect: ThinkingDialect,
    preserve_reasoning: bool,
    preserve_when_thinking: bool,
    empty_reasoning_on_tool_replay: bool,
    max_out: Option<u64>,
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
/// API key held zeroized, injected per-call, never logged.
pub struct OpenAiCompatClient {
    pub base_url: String,
    api_key: Zeroizing<String>,
    http: reqwest::blocking::Client,
    policy: OpenAiPolicy,
}

impl OpenAiCompatClient {
    pub(crate) fn new(base_url: String, api_key: Zeroizing<String>) -> Self {
        Self {
            base_url,
            api_key,
            http: http_client(),
            policy: OpenAiPolicy::default(),
        }
    }
}

impl ChatClient for OpenAiCompatClient {
    fn complete(&self, req: &ChatRequest) -> anyhow::Result<ChatResponse> {
        let url = endpoint(&self.base_url);
        let resp = self
            .http
            .post(&url)
            .bearer_auth(self.api_key.as_str())
            .json(&build_body(req, self.policy))
            .send()
            .map_err(|e| send_error(&url, &e))?;
        let status = resp.status();
        let body = read_body_capped(resp, MAX_PROVIDER_BODY_BYTES)?;
        if !status.is_success() {
            return Err(provider_http_error(status, &body, self.api_key.as_str()));
        }
        parse_response(&body)
    }
}

/// Shared HTTP-error UX for both wire clients: status hint that says what to
/// do next, plus a scrubbed one-line body snippet.
fn provider_http_error(status: reqwest::StatusCode, body: &str, key: &str) -> anyhow::Error {
    let hint = match status.as_u16() {
        401 | 403 => " - key rejected; run `nh key add <provider>`",
        429 => " - rate limited; retry later",
        _ => "",
    };
    anyhow::anyhow!(
        "provider returned HTTP {}{}: {}",
        status.as_u16(),
        hint,
        scrub_snippet(body, key)
    )
}

fn endpoint(base_url: &str) -> String {
    format!("{}/chat/completions", base_url.trim_end_matches('/'))
}

/// Build the OpenAI-wire request body. Tool calls and tools use the nested
/// `{"type":"function","function":{...}}` shape the wire requires.
fn build_body(req: &ChatRequest, policy: OpenAiPolicy) -> serde_json::Value {
    let thinking_active = thinking_is_active(policy.dialect, req.thinking);
    let messages: Vec<serde_json::Value> = req
        .messages
        .iter()
        .map(|m| {
            let mut obj = serde_json::json!({ "role": m.role });
            if let Some(c) = &m.content {
                obj["content"] = serde_json::Value::String(c.clone());
            }
            if let Some(calls) = &m.tool_calls {
                obj["tool_calls"] = calls
                    .iter()
                    .map(|c| {
                        serde_json::json!({
                            "id": c.id,
                            "type": "function",
                            "function": { "name": c.name, "arguments": c.arguments }
                        })
                    })
                    .collect();
            }
            if let Some(id) = &m.tool_call_id {
                obj["tool_call_id"] = serde_json::Value::String(id.clone());
            }
            if let Some(r) = reasoning_to_send(m, policy, thinking_active) {
                obj["reasoning_content"] = serde_json::Value::String(r.to_string());
            }
            obj
        })
        .collect();
    let mut body = serde_json::json!({
        "model": req.model,
        "messages": messages,
        "max_tokens": policy.max_out.unwrap_or(DEFAULT_MAX_TOKENS),
    });
    // [VERIFY-LIVE §7] Provider-specific output_config effort mapping remains live-pending.
    if !req.tools.is_empty() {
        body["tools"] = req
            .tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters
                    }
                })
            })
            .collect();
    }
    apply_thinking(&mut body, policy.dialect, req.thinking);
    body
}

/// The ONE place reasoning replay policy lives (CONTRACTS_M1.md §2.4):
/// 1. `preserve_reasoning` routes send stored reasoning on assistant history
///    (Kimi K2.7*/MiMo - stripping it degrades the model).
/// 2. Everyone else never serializes it.
/// 3. Deepseek quirk: assistant replay turns carrying ONLY tool_calls get
///    `reasoning_content: ""` (empty string, not null) even under rule 2;
///    a stored value under rule 1 wins over the empty string.
fn reasoning_to_send(m: &ChatMessage, policy: OpenAiPolicy, thinking_active: bool) -> Option<&str> {
    if m.role != "assistant" {
        return None;
    }
    if policy.preserve_reasoning || (policy.preserve_when_thinking && thinking_active) {
        if let Some(r) = m.reasoning_content.as_deref() {
            return Some(r);
        }
    }
    let tool_only = m.tool_calls.as_ref().is_some_and(|c| !c.is_empty())
        && m.content.as_deref().is_none_or(str::is_empty);
    if policy.empty_reasoning_on_tool_replay && tool_only {
        return Some("");
    }
    None
}

fn thinking_is_active(dialect: ThinkingDialect, effort: ThinkingEffort) -> bool {
    match dialect {
        ThinkingDialect::DeepseekNhm | ThinkingDialect::GlmHm => {
            matches!(effort, ThinkingEffort::High | ThinkingEffort::Max)
        }
        ThinkingDialect::KimiToggle => effort != ThinkingEffort::None,
        ThinkingDialect::AlwaysThinking => true,
        ThinkingDialect::None => false,
    }
}

/// The ONE place (dialect, effort) → OpenAI-wire params lives (CONTRACTS_M1.md §2.3).
fn apply_thinking(body: &mut serde_json::Value, dialect: ThinkingDialect, effort: ThinkingEffort) {
    match dialect {
        ThinkingDialect::DeepseekNhm => {
            match effort {
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
            }
        }
        ThinkingDialect::KimiToggle => {
            // [VERIFY-LIVE §7] Kimi K2.6 documented thinking toggle shape.
            let kind = if effort == ThinkingEffort::None {
                "disabled"
            } else {
                "enabled"
            };
            body["thinking"] = serde_json::json!({ "type": kind });
        }
        // Kimi K2.7 has no non-thinking mode - never send a toggle.
        ThinkingDialect::AlwaysThinking => {}
        // GLM thinking is High/Max server-side; mapping verified live in M2.
        ThinkingDialect::GlmHm => {}
        ThinkingDialect::None => {}
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
    #[serde(default)]
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

fn parse_response(body: &str) -> anyhow::Result<ChatResponse> {
    let wire: WireResponse = serde_json::from_str(body)
        .map_err(|e| anyhow::anyhow!("could not parse provider response: {e}"))?;
    let choice = wire
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("provider response had no choices"))?;
    let tool_calls = choice.message.tool_calls.map(|calls| {
        calls
            .into_iter()
            .map(|c| ToolCallReq {
                id: c.id,
                name: c.function.name,
                arguments: c.function.arguments,
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
        usage: wire.usage.map(|u| {
            let cached_tokens = u
                .prompt_tokens_details
                .and_then(|details| details.cached_tokens)
                .or(u.prompt_cache_hit_tokens);
            Usage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
                cached_tokens,
            }
        }),
    })
}

/// Blocking client for the Anthropic Messages wire (`POST {base_url}/v1/messages`),
/// e.g. DeepSeek's deepclaude-proven `https://api.deepseek.com/anthropic` path.
/// `max_tokens` is REQUIRED on this wire and always sent. M1 sends no thinking
/// toggle and never serializes `reasoning_content` here (thinking blocks are M2).
pub struct AnthropicMessagesClient {
    pub base_url: String,
    api_key: Zeroizing<String>,
    max_tokens: u64,
    http: reqwest::blocking::Client,
}

impl AnthropicMessagesClient {
    pub(crate) fn new(base_url: String, api_key: Zeroizing<String>, max_tokens: u64) -> Self {
        Self {
            base_url,
            api_key,
            max_tokens,
            http: http_client(),
        }
    }
}

impl ChatClient for AnthropicMessagesClient {
    fn complete(&self, req: &ChatRequest) -> anyhow::Result<ChatResponse> {
        let url = anthropic_endpoint(&self.base_url);
        let resp = self
            .http
            .post(&url)
            .header("x-api-key", self.api_key.as_str())
            .header("anthropic-version", "2023-06-01")
            .json(&build_anthropic_body(req, self.max_tokens))
            .send()
            .map_err(|e| send_error(&url, &e))?;
        let status = resp.status();
        let body = read_body_capped(resp, MAX_PROVIDER_BODY_BYTES)?;
        if !status.is_success() {
            return Err(provider_http_error(status, &body, self.api_key.as_str()));
        }
        parse_anthropic_response(&body)
    }
}

fn anthropic_endpoint(base_url: &str) -> String {
    format!("{}/v1/messages", base_url.trim_end_matches('/'))
}

/// Map `ChatRequest` onto the Anthropic Messages shape: first system message
/// becomes the top-level `system` string; assistant tool calls become
/// `tool_use` blocks; tool results become `tool_result` blocks inside user
/// messages, with consecutive tool messages merged into ONE user message
/// (roles must alternate on this wire).
fn build_anthropic_body(req: &ChatRequest, max_tokens: u64) -> serde_json::Value {
    let mut system: Option<String> = None;
    let mut messages: Vec<serde_json::Value> = Vec::new();
    let mut prev_was_user = false;
    for m in &req.messages {
        match m.role.as_str() {
            "system" if system.is_none() => {
                system = Some(m.content.clone().unwrap_or_default());
            }
            "tool" => {
                let block = serde_json::json!({
                    "type": "tool_result",
                    "tool_use_id": m.tool_call_id.clone().unwrap_or_default(),
                    "content": m.content.clone().unwrap_or_default(),
                });
                push_user_block(&mut messages, prev_was_user, block);
                prev_was_user = true;
            }
            "assistant" => {
                let mut blocks: Vec<serde_json::Value> = Vec::new();
                if let Some(text) = m.content.as_deref().filter(|t| !t.is_empty()) {
                    blocks.push(serde_json::json!({ "type": "text", "text": text }));
                }
                for call in m.tool_calls.iter().flatten() {
                    let input: serde_json::Value = serde_json::from_str(&call.arguments)
                        .unwrap_or_else(|_| serde_json::json!({}));
                    blocks.push(serde_json::json!({
                        "type": "tool_use", "id": call.id, "name": call.name, "input": input,
                    }));
                }
                messages.push(serde_json::json!({ "role": "assistant", "content": blocks }));
                prev_was_user = false;
            }
            // "user" - and any unexpected role degrades to user text, never dropped.
            _ => {
                let text = m.content.clone().unwrap_or_default();
                let block = serde_json::json!({ "type": "text", "text": text });
                push_user_block(&mut messages, prev_was_user, block);
                prev_was_user = true;
            }
        }
    }
    let mut body = serde_json::json!({
        "model": req.model,
        "max_tokens": max_tokens,
        "messages": messages,
    });
    if let Some(s) = system {
        body["system"] = serde_json::Value::String(s);
    }
    if !req.tools.is_empty() {
        body["tools"] = req
            .tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.parameters,
                })
            })
            .collect();
    }
    body
}

fn push_user_block(
    messages: &mut Vec<serde_json::Value>,
    prev_was_user: bool,
    block: serde_json::Value,
) {
    match messages.last_mut() {
        Some(last) if prev_was_user => last["content"]
            .as_array_mut()
            .expect("user content array")
            .push(block),
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
/// One content block; unknown `type`s (e.g. M2 thinking blocks) are ignored.
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

fn parse_anthropic_response(body: &str) -> anyhow::Result<ChatResponse> {
    let wire: AnthropicResponse = serde_json::from_str(body)
        .map_err(|e| anyhow::anyhow!("could not parse provider response: {e}"))?;
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
        usage: wire.usage.map(|u| Usage {
            prompt_tokens: u.input_tokens,
            completion_tokens: u.output_tokens,
            cached_tokens: u.cache_read_input_tokens,
        }),
    })
}

/// One-line, truncated body snippet with the API key literal redacted.
fn scrub_snippet(body: &str, key: &str) -> String {
    let s = if key.is_empty() {
        body.to_string()
    } else {
        body.replace(key, "[REDACTED]")
    };
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.is_empty() {
        return "(empty body)".into();
    }
    if s.chars().count() > 200 {
        let mut t: String = s.chars().take(200).collect();
        t.push('…');
        t
    } else {
        s
    }
}

#[cfg(test)]
mod tests;
