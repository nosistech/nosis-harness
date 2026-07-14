//! nh-core — agent turn loop, wire client, receipts.
//! Every turn writes a scrubbed JSONL receipt to .nosis/receipts.jsonl (append-only).

pub mod wire {
    //! Chat wire clients (M1): OpenAI-compatible + Anthropic Messages.
    //! `make_client` picks the client from the route's wire and captures per-route
    //! policy (thinking dialect, reasoning persistence, quirks) at construction.
    use nh_routes::ThinkingDialect;
    use std::time::Duration;
    use zeroize::Zeroizing;

    /// Non-streaming completions from thinking routes legitimately run for
    /// minutes; reqwest's blocking default would abort every request at 30 s.
    /// Generous total cap instead — a dead host still fails fast on connect.
    const REQUEST_TIMEOUT: Duration = Duration::from_secs(600);
    const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

    /// One HTTP client config for both wire clients: explicit timeouts (never
    /// the hidden 30 s blocking default) and no redirect following — reqwest
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

    /// Friendly send-failure line: a slow provider is not an unreachable one.
    fn send_error(url: &str, e: &reqwest::Error) -> anyhow::Error {
        anyhow::anyhow!("{}", send_error_line(url, e.is_timeout() && !e.is_connect(), &e.to_string()))
    }

    fn send_error_line(url: &str, timed_out: bool, detail: &str) -> String {
        if timed_out {
            format!(
                "provider at {url} did not answer within {}s — retry, or switch to another route",
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
    /// Returns `None` when there are no prompt tokens to divide by.
    pub fn cache_hit_pct(prompt_tokens: u64, cached_tokens: u64) -> Option<f64> {
        if prompt_tokens == 0 {
            return None;
        }
        Some((100.0 * cached_tokens as f64 / prompt_tokens as f64).clamp(0.0, 100.0))
    }

    #[derive(Debug, Clone)]
    pub struct ChatResponse {
        pub message: ChatMessage,
        pub finish_reason: String,
        pub usage: Option<Usage>,
    }

    /// Provider abstraction — tests inject a mock, production uses `make_client`.
    pub trait ChatClient: Send + Sync {
        fn complete(&self, req: &ChatRequest) -> anyhow::Result<ChatResponse>;
    }

    /// Builds the right client for a resolved route. Total over `Wire` — never fails.
    /// Captures per-route wire policy here so the agent loop stays policy-free.
    /// Callers must not pass `RouteClass::Delegate` routes (check `route.class` first).
    pub fn make_client(
        route: &nh_routes::ResolvedRoute,
        api_key: Zeroizing<String>,
    ) -> Box<dyn ChatClient> {
        match route.wire {
            nh_routes::Wire::OpenAi => {
                let mut client = OpenAiCompatClient::new(route.base_url.clone(), api_key);
                client.policy = OpenAiPolicy {
                    dialect: route.thinking_dialect,
                    preserve_reasoning: route.preserve_reasoning,
                    empty_reasoning_on_tool_replay: route
                        .has_quirk("empty-reasoning-content-on-tool-replay"),
                };
                Box::new(client)
            }
            nh_routes::Wire::AnthropicMessages => Box::new(AnthropicMessagesClient::new(
                route.base_url.clone(),
                api_key,
                route.max_out.unwrap_or(8192).min(8192),
            )),
        }
    }

    /// Per-route OpenAI-wire policy, captured once by `make_client`.
    #[derive(Debug, Clone, Copy)]
    struct OpenAiPolicy {
        dialect: ThinkingDialect,
        preserve_reasoning: bool,
        empty_reasoning_on_tool_replay: bool,
    }

    impl Default for OpenAiPolicy {
        fn default() -> Self {
            Self {
                dialect: ThinkingDialect::None,
                preserve_reasoning: false,
                empty_reasoning_on_tool_replay: false,
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
        pub fn new(base_url: String, api_key: Zeroizing<String>) -> Self {
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
            let body = resp.text().unwrap_or_default();
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
            401 | 403 => " — key rejected; run `nh key add <provider>`",
            429 => " — rate limited; retry later",
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
                if let Some(r) = reasoning_to_send(m, policy) {
                    obj["reasoning_content"] = serde_json::Value::String(r.to_string());
                }
                obj
            })
            .collect();
        let mut body = serde_json::json!({ "model": req.model, "messages": messages });
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
    ///    (Kimi K2.7*/MiMo — stripping it degrades the model).
    /// 2. Everyone else never serializes it.
    /// 3. Deepseek quirk: assistant replay turns carrying ONLY tool_calls get
    ///    `reasoning_content: ""` (empty string, not null) even under rule 2;
    ///    a stored value under rule 1 wins over the empty string.
    fn reasoning_to_send(m: &ChatMessage, policy: OpenAiPolicy) -> Option<&str> {
        if m.role != "assistant" {
            return None;
        }
        if policy.preserve_reasoning {
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

    /// The ONE place (dialect, effort) → OpenAI-wire params lives (CONTRACTS_M1.md §2.3).
    fn apply_thinking(
        body: &mut serde_json::Value,
        dialect: ThinkingDialect,
        effort: ThinkingEffort,
    ) {
        match dialect {
            ThinkingDialect::DeepseekNhm => {
                // verify at live test: param name "reasoning_effort" and its values
                // are unconfirmed (CONTRACTS_M1.md §6 verify-live ledger).
                let value = match effort {
                    // DeepSeek has no low tier — Low maps down to "none".
                    ThinkingEffort::None | ThinkingEffort::Low => "none",
                    ThinkingEffort::High => "high",
                    ThinkingEffort::Max => "max",
                };
                body["reasoning_effort"] = serde_json::Value::String(value.into());
            }
            // Kimi K2.7 has no non-thinking mode — never send a toggle.
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
                .map(|c| ToolCallReq { id: c.id, name: c.function.name, arguments: c.function.arguments })
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
            usage: wire.usage.map(|u| Usage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
                cached_tokens: u.prompt_tokens_details.and_then(|d| d.cached_tokens),
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
        pub fn new(base_url: String, api_key: Zeroizing<String>, max_tokens: u64) -> Self {
            Self { base_url, api_key, max_tokens, http: http_client() }
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
            let body = resp.text().unwrap_or_default();
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
        let mut prev_was_tool = false;
        for m in &req.messages {
            match m.role.as_str() {
                "system" if system.is_none() => {
                    system = Some(m.content.clone().unwrap_or_default());
                    prev_was_tool = false;
                }
                "tool" => {
                    let block = serde_json::json!({
                        "type": "tool_result",
                        "tool_use_id": m.tool_call_id.clone().unwrap_or_default(),
                        "content": m.content.clone().unwrap_or_default(),
                    });
                    match messages.last_mut() {
                        Some(last) if prev_was_tool => {
                            last["content"].as_array_mut().expect("tool_result array").push(block);
                        }
                        _ => messages.push(serde_json::json!({ "role": "user", "content": [block] })),
                    }
                    prev_was_tool = true;
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
                    prev_was_tool = false;
                }
                // "user" — and any unexpected role degrades to user text, never dropped.
                _ => {
                    let text = m.content.clone().unwrap_or_default();
                    messages.push(serde_json::json!({
                        "role": "user", "content": [{ "type": "text", "text": text }],
                    }));
                    prev_was_tool = false;
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
                "tool_use" => calls.push(ToolCallReq {
                    id: block.id.unwrap_or_default(),
                    name: block.name.unwrap_or_default(),
                    arguments: serde_json::to_string(
                        &block.input.unwrap_or_else(|| serde_json::json!({})),
                    )
                    .unwrap_or_else(|_| "{}".into()),
                }),
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
        let s = if key.is_empty() { body.to_string() } else { body.replace(key, "[REDACTED]") };
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
    mod tests {
        use super::*;

        #[test]
        fn cache_hit_percentage_is_optional_and_clamped() {
            assert_eq!(cache_hit_pct(0, 10), None);
            assert_eq!(cache_hit_pct(20, 5), Some(25.0));
            assert_eq!(cache_hit_pct(10, 20), Some(100.0));
        }

        fn msg(role: &str, content: Option<&str>) -> ChatMessage {
            ChatMessage {
                role: role.into(),
                content: content.map(str::to_string),
                tool_calls: None,
                tool_call_id: None,
                reasoning_content: None,
            }
        }

        fn tool_call(id: &str, name: &str, arguments: &str) -> ToolCallReq {
            ToolCallReq { id: id.into(), name: name.into(), arguments: arguments.into() }
        }

        fn req(messages: Vec<ChatMessage>) -> ChatRequest {
            ChatRequest {
                model: "mock-model".into(),
                messages,
                tools: vec![],
                thinking: ThinkingEffort::None,
            }
        }

        fn policy(
            dialect: ThinkingDialect,
            preserve_reasoning: bool,
            quirk: bool,
        ) -> OpenAiPolicy {
            OpenAiPolicy { dialect, preserve_reasoning, empty_reasoning_on_tool_replay: quirk }
        }

        #[test]
        fn endpoint_trims_trailing_slash() {
            assert_eq!(endpoint("https://api.example.com/"), "https://api.example.com/chat/completions");
            assert_eq!(endpoint("https://api.example.com"), "https://api.example.com/chat/completions");
        }

        #[test]
        fn timeout_error_says_what_happened_and_what_to_do() {
            let line = send_error_line("https://api.example.com/chat/completions", true, "op timed out");
            assert_eq!(
                line,
                "provider at https://api.example.com/chat/completions did not answer within 600s \
                 — retry, or switch to another route"
            );
            // Non-timeout failures keep the reachability wording and the detail.
            let line = send_error_line("https://api.example.com/chat/completions", false, "dns error");
            assert!(line.starts_with("could not reach provider at "), "got: {line}");
            assert!(line.ends_with("dns error"), "got: {line}");
        }

        #[test]
        fn request_timeout_outlives_slow_thinking_turns() {
            // Guard against the hidden 30 s blocking-client default sneaking back:
            // thinking routes (kimi/glm at High) routinely exceed 30 s per turn.
            assert!(REQUEST_TIMEOUT >= Duration::from_secs(300), "got: {REQUEST_TIMEOUT:?}");
            assert!(CONNECT_TIMEOUT <= Duration::from_secs(30), "dead hosts must fail fast");
        }

        #[test]
        fn body_nests_tools_and_tool_calls() {
            let mut request = req(vec![
                ChatMessage {
                    tool_calls: Some(vec![tool_call("c1", "read_file", r#"{"path":"a.txt"}"#)]),
                    ..msg("assistant", None)
                },
                ChatMessage { tool_call_id: Some("c1".into()), ..msg("tool", Some("data")) },
            ]);
            request.tools = vec![nh_tools::ToolSpec {
                name: "read_file".into(),
                description: "read a file".into(),
                parameters: serde_json::json!({"type": "object"}),
            }];
            let body = build_body(&request, OpenAiPolicy::default());
            assert_eq!(body["model"], "mock-model");
            assert_eq!(body["tools"][0]["type"], "function");
            assert_eq!(body["tools"][0]["function"]["name"], "read_file");
            assert_eq!(body["messages"][0]["tool_calls"][0]["type"], "function");
            assert_eq!(body["messages"][0]["tool_calls"][0]["function"]["name"], "read_file");
            assert_eq!(body["messages"][0]["tool_calls"][0]["function"]["arguments"], r#"{"path":"a.txt"}"#);
            assert_eq!(body["messages"][1]["tool_call_id"], "c1");
            assert!(body["messages"][1].get("tool_calls").is_none());
        }

        #[test]
        fn deepseek_dialect_maps_every_effort_tier() {
            // verify at live test: values pinned in CONTRACTS_M1.md §2.3.
            for (effort, expected) in [
                (ThinkingEffort::None, "none"),
                (ThinkingEffort::Low, "none"), // DeepSeek has no low tier
                (ThinkingEffort::High, "high"),
                (ThinkingEffort::Max, "max"),
            ] {
                let mut request = req(vec![msg("user", Some("hi"))]);
                request.thinking = effort;
                let body = build_body(&request, policy(ThinkingDialect::DeepseekNhm, false, false));
                assert_eq!(body["reasoning_effort"], expected, "effort {effort:?}");
            }
        }

        #[test]
        fn non_deepseek_dialects_send_no_thinking_toggle() {
            for dialect in
                [ThinkingDialect::AlwaysThinking, ThinkingDialect::GlmHm, ThinkingDialect::None]
            {
                let mut request = req(vec![msg("user", Some("hi"))]);
                request.thinking = ThinkingEffort::Max;
                let body = build_body(&request, policy(dialect, false, false));
                assert!(
                    body.get("reasoning_effort").is_none(),
                    "dialect {dialect:?} must not send a toggle"
                );
            }
        }

        #[test]
        fn preserve_reasoning_keeps_assistant_reasoning_in_history() {
            // Kimi-style route: stripping reasoning degrades the model (plan A.10.5).
            let request = req(vec![
                msg("user", Some("hi")),
                ChatMessage {
                    reasoning_content: Some("chain".into()),
                    ..msg("assistant", Some("answer"))
                },
            ]);
            let body = build_body(&request, policy(ThinkingDialect::AlwaysThinking, true, false));
            assert!(body["messages"][0].get("reasoning_content").is_none());
            assert_eq!(body["messages"][1]["reasoning_content"], "chain");
        }

        #[test]
        fn non_preserving_routes_strip_reasoning_from_history() {
            let request = req(vec![ChatMessage {
                reasoning_content: Some("chain".into()),
                ..msg("assistant", Some("answer"))
            }]);
            let body = build_body(&request, policy(ThinkingDialect::None, false, false));
            assert!(body["messages"][0].get("reasoning_content").is_none());
        }

        #[test]
        fn quirk_inserts_empty_reasoning_only_on_tool_only_replay_turns() {
            let quirked = policy(ThinkingDialect::DeepseekNhm, false, true);
            let calls = Some(vec![tool_call("c1", "read_file", "{}")]);

            // Tool-only replay (content None) → empty string, not null.
            let request = req(vec![ChatMessage { tool_calls: calls.clone(), ..msg("assistant", None) }]);
            let body = build_body(&request, quirked);
            assert_eq!(body["messages"][0]["reasoning_content"], "");

            // Empty-string content still counts as tool-only.
            let request = req(vec![ChatMessage { tool_calls: calls.clone(), ..msg("assistant", Some("")) }]);
            let body = build_body(&request, quirked);
            assert_eq!(body["messages"][0]["reasoning_content"], "");

            // Assistant turns WITH text do not get it.
            let request = req(vec![ChatMessage { tool_calls: calls.clone(), ..msg("assistant", Some("look")) }]);
            let body = build_body(&request, quirked);
            assert!(body["messages"][0].get("reasoning_content").is_none());

            // Plain text turns and non-assistant roles do not get it.
            let request = req(vec![msg("assistant", Some("done")), msg("user", Some("hi"))]);
            let body = build_body(&request, quirked);
            assert!(body["messages"][0].get("reasoning_content").is_none());
            assert!(body["messages"][1].get("reasoning_content").is_none());

            // Non-quirked routes never get it, even on tool-only replay.
            let request = req(vec![ChatMessage { tool_calls: calls, ..msg("assistant", None) }]);
            let body = build_body(&request, policy(ThinkingDialect::DeepseekNhm, false, false));
            assert!(body["messages"][0].get("reasoning_content").is_none());
        }

        #[test]
        fn stored_reasoning_wins_over_quirk_empty_string() {
            let request = req(vec![ChatMessage {
                tool_calls: Some(vec![tool_call("c1", "read_file", "{}")]),
                reasoning_content: Some("kept".into()),
                ..msg("assistant", None)
            }]);
            let body = build_body(&request, policy(ThinkingDialect::DeepseekNhm, true, true));
            assert_eq!(body["messages"][0]["reasoning_content"], "kept");
        }

        #[test]
        fn parses_reasoning_content_from_response() {
            let body = r#"{"choices":[{"message":{"role":"assistant","content":"hi",
                "reasoning_content":"thought hard"},"finish_reason":"stop"}]}"#;
            let resp = parse_response(body).unwrap();
            assert_eq!(resp.message.reasoning_content.as_deref(), Some("thought hard"));
        }

        #[test]
        fn anthropic_endpoint_trims_trailing_slash() {
            assert_eq!(
                anthropic_endpoint("https://api.deepseek.com/anthropic/"),
                "https://api.deepseek.com/anthropic/v1/messages"
            );
        }

        #[test]
        fn anthropic_body_lifts_system_and_wraps_text() {
            let mut request = req(vec![
                msg("system", Some("be brief")),
                msg("user", Some("hi")),
                msg("assistant", Some("hello")),
            ]);
            request.thinking = ThinkingEffort::Max;
            let body = build_anthropic_body(&request, 8192);
            assert_eq!(body["model"], "mock-model");
            assert_eq!(body["max_tokens"], 8192);
            assert_eq!(body["system"], "be brief");
            let messages = body["messages"].as_array().unwrap();
            assert_eq!(messages.len(), 2, "system message must not appear in messages");
            assert_eq!(messages[0]["role"], "user");
            assert_eq!(messages[0]["content"][0]["type"], "text");
            assert_eq!(messages[0]["content"][0]["text"], "hi");
            assert_eq!(messages[1]["role"], "assistant");
            assert_eq!(messages[1]["content"][0]["text"], "hello");
            assert!(body.get("tools").is_none());
            // M1: no thinking toggle on this wire, whatever the requested effort.
            let raw = body.to_string();
            assert!(!raw.contains("reasoning_effort") && !raw.contains("thinking"));
        }

        #[test]
        fn anthropic_body_maps_tool_use_and_merges_tool_results() {
            let mut request = req(vec![
                msg("user", Some("fix it")),
                ChatMessage {
                    tool_calls: Some(vec![
                        tool_call("c1", "read_file", r#"{"path":"a.txt"}"#),
                        tool_call("c2", "exec_shell", "{not json"),
                    ]),
                    ..msg("assistant", Some("let me look"))
                },
                ChatMessage { tool_call_id: Some("c1".into()), ..msg("tool", Some("data1")) },
                ChatMessage { tool_call_id: Some("c2".into()), ..msg("tool", Some("data2")) },
                msg("user", Some("thanks")),
            ]);
            request.tools = vec![nh_tools::ToolSpec {
                name: "read_file".into(),
                description: "read a file".into(),
                parameters: serde_json::json!({"type": "object"}),
            }];
            let body = build_anthropic_body(&request, 4096);

            let assistant = &body["messages"][1];
            assert_eq!(assistant["content"][0]["type"], "text");
            assert_eq!(assistant["content"][0]["text"], "let me look");
            assert_eq!(assistant["content"][1]["type"], "tool_use");
            assert_eq!(assistant["content"][1]["id"], "c1");
            assert_eq!(assistant["content"][1]["input"]["path"], "a.txt");
            // Unparseable arguments degrade to an empty object.
            assert_eq!(assistant["content"][2]["input"], serde_json::json!({}));

            // Two consecutive tool messages → ONE user message, two tool_result blocks.
            let messages = body["messages"].as_array().unwrap();
            assert_eq!(messages.len(), 4);
            let results = &messages[2];
            assert_eq!(results["role"], "user");
            assert_eq!(results["content"].as_array().unwrap().len(), 2);
            assert_eq!(results["content"][0]["type"], "tool_result");
            assert_eq!(results["content"][0]["tool_use_id"], "c1");
            assert_eq!(results["content"][0]["content"], "data1");
            assert_eq!(results["content"][1]["tool_use_id"], "c2");
            assert_eq!(messages[3]["content"][0]["text"], "thanks");

            assert_eq!(body["tools"][0]["name"], "read_file");
            assert_eq!(body["tools"][0]["input_schema"]["type"], "object");
        }

        #[test]
        fn anthropic_body_never_serializes_reasoning_content() {
            let request = req(vec![ChatMessage {
                reasoning_content: Some("chain".into()),
                tool_calls: Some(vec![tool_call("c1", "read_file", "{}")]),
                ..msg("assistant", None)
            }]);
            let raw = build_anthropic_body(&request, 8192).to_string();
            assert!(!raw.contains("reasoning_content") && !raw.contains("chain"));
        }

        #[test]
        fn anthropic_response_round_trips_text_tool_use_and_usage() {
            let body = r#"{
                "content": [
                    {"type": "text", "text": "checking "},
                    {"type": "tool_use", "id": "t1", "name": "read_file", "input": {"path": "a.txt"}},
                    {"type": "text", "text": "now"}
                ],
                "stop_reason": "tool_use",
                "usage": {"input_tokens": 11, "output_tokens": 7, "cache_read_input_tokens": 3}
            }"#;
            let resp = parse_anthropic_response(body).unwrap();
            assert_eq!(resp.message.content.as_deref(), Some("checking now"));
            let calls = resp.message.tool_calls.unwrap();
            assert_eq!(calls[0].id, "t1");
            assert_eq!(calls[0].name, "read_file");
            let args: serde_json::Value = serde_json::from_str(&calls[0].arguments).unwrap();
            assert_eq!(args["path"], "a.txt");
            assert_eq!(resp.finish_reason, "tool_use");
            let usage = resp.usage.unwrap();
            assert_eq!(usage.prompt_tokens, 11);
            assert_eq!(usage.completion_tokens, 7);
            assert_eq!(usage.cached_tokens, Some(3));
        }

        #[test]
        fn anthropic_response_without_text_blocks_has_no_content() {
            let body = r#"{
                "content": [{"type": "tool_use", "id": "t1", "name": "read_file", "input": {}}],
                "stop_reason": "tool_use",
                "usage": {"input_tokens": 1, "output_tokens": 1}
            }"#;
            let resp = parse_anthropic_response(body).unwrap();
            assert!(resp.message.content.is_none());
            assert_eq!(resp.usage.unwrap().cached_tokens, None);
        }

        #[test]
        fn parses_message_finish_reason_and_usage() {
            let body = r#"{
                "choices": [{
                    "message": {
                        "role": "assistant",
                        "content": null,
                        "tool_calls": [{"id": "c1", "type": "function",
                            "function": {"name": "edit_file", "arguments": "{}"}}]
                    },
                    "finish_reason": "tool_calls"
                }],
                "usage": {
                    "prompt_tokens": 12,
                    "completion_tokens": 7,
                    "prompt_tokens_details": {"cached_tokens": 4}
                }
            }"#;
            let resp = parse_response(body).unwrap();
            assert_eq!(resp.finish_reason, "tool_calls");
            let calls = resp.message.tool_calls.unwrap();
            assert_eq!(calls[0].name, "edit_file");
            let usage = resp.usage.unwrap();
            assert_eq!(usage.prompt_tokens, 12);
            assert_eq!(usage.completion_tokens, 7);
            assert_eq!(usage.cached_tokens, Some(4));
        }

        #[test]
        fn parses_plain_content_without_usage() {
            let body = r#"{"choices":[{"message":{"role":"assistant","content":"done"}}]}"#;
            let resp = parse_response(body).unwrap();
            assert_eq!(resp.message.content.as_deref(), Some("done"));
            assert_eq!(resp.finish_reason, "");
            assert!(resp.usage.is_none());
        }

        #[test]
        fn no_choices_is_a_concise_error() {
            let err = parse_response(r#"{"choices":[]}"#).unwrap_err();
            assert!(err.to_string().contains("no choices"));
        }

        #[test]
        fn snippet_redacts_key_and_truncates() {
            let key = "sk-test-0000";
            let body = format!("error: bad key {key} was rejected\nline2");
            let s = scrub_snippet(&body, key);
            assert!(!s.contains(key));
            assert!(s.contains("[REDACTED]"));
            assert!(!s.contains('\n'));
            let long = "x".repeat(500);
            assert!(scrub_snippet(&long, "").chars().count() <= 201);
        }
    }
}

pub mod receipt {
    //! Typed receipts (plan §2): why runs fail, not just that they failed.

    use anyhow::Context as _;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    #[serde(rename_all = "lowercase")]
    pub enum Outcome {
        Pass,
        Fail,
        Partial,
        Skip,
        Timeout,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    #[serde(rename_all = "lowercase")]
    pub enum FailureClass {
        Context,
        Constraint,
        Verification,
        Planning,
    }

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    pub struct Receipt {
        pub ts_utc: String,
        pub model_id: String,
        pub task: String,
        pub turns: u32,
        pub tool_calls: u32,
        pub outcome: Outcome,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub failure_class: Option<FailureClass>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub usage: Option<super::wire::Usage>,
    }

    /// Appends scrubbed JSONL lines to .nosis/receipts.jsonl (creates dir if missing).
    pub struct ReceiptWriter {
        pub path: std::path::PathBuf,
        pub scrubber: nh_vault::Scrubber,
    }

    impl ReceiptWriter {
        pub fn append(&self, receipt: &Receipt) -> anyhow::Result<()> {
            use std::io::Write as _;
            if let Some(parent) = self.path.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("could not create {}", parent.display()))?;
            }
            let line = serde_json::to_string(receipt).context("could not serialize receipt")?;
            let line = self.scrubber.scrub(&line);
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
                .with_context(|| format!("could not open {}", self.path.display()))?;
            writeln!(file, "{line}")
                .with_context(|| format!("could not write {}", self.path.display()))?;
            Ok(())
        }
    }
}

pub mod agent {
    //! The turn loop: send task → model responds with tool calls → execute (gated) →
    //! feed results back → repeat until final answer or max_turns (then Outcome::Timeout).

    use crate::receipt::{FailureClass, Outcome, Receipt, ReceiptWriter};
    use crate::wire::{ChatClient, ChatMessage, ChatRequest, ThinkingEffort, ToolCallReq, Usage};
    use nh_tools::{Tool, ToolCtx};

    const COMPACT_AT: f64 = 0.70;
    const COMPACT_TARGET: f64 = 0.50;
    const KEEP_RECENT: usize = 2;
    const MESSAGE_OVERHEAD_TOKENS: u64 = 4;

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
        /// Byte-stable system prefix. `None` preserves the M0/M1 default.
        pub constitution: Option<String>,
        /// Effective context window. `None` disables compaction.
        pub context_limit: Option<u64>,
        /// Progress callback: invoked with one short line per tool call
        /// ("turn 2: edit_file src/lib.rs"). Core stays print-free — nh-cli
        /// wires this to its own printer.
        #[allow(clippy::type_complexity)]
        pub on_event: Option<Box<dyn Fn(&str) + Send>>,
    }

    impl AgentLoop {
        /// Runs one task to completion. Always writes exactly one receipt, even on error.
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
        /// receipt per call, same semantics as `run`.
        pub fn run_with_history(
            &mut self,
            history: &mut Vec<ChatMessage>,
            task: &str,
        ) -> anyhow::Result<(String, Receipt)> {
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

            #[cfg(debug_assertions)]
            let prefix_bytes = message_bytes(&history[0]);

            let mut turns: u32 = 0;
            let mut tool_calls: u32 = 0;
            let mut usage_total = Usage::default();
            let mut saw_usage = false;
            let mut latest_prompt_tokens = None;

            while turns < self.max_turns {
                #[cfg(debug_assertions)]
                debug_assert_eq!(message_bytes(&history[0]), prefix_bytes);

                if let Some(limit) = self.context_limit {
                    let input_tokens =
                        latest_prompt_tokens.unwrap_or_else(|| estimate_tokens(history));
                    if input_tokens as f64 >= COMPACT_AT * limit as f64 {
                        if let Some(compaction) = compact_history(history, limit) {
                            let pct = context_percentage(input_tokens, limit);
                            self.emit(&format!(
                                "context {pct}% — compacted {} earlier messages",
                                compaction.messages
                            ));
                        }
                    }
                }

                #[cfg(debug_assertions)]
                debug_assert_eq!(message_bytes(&history[0]), prefix_bytes);

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
                        #[cfg(debug_assertions)]
                        debug_assert_eq!(message_bytes(&history[0]), prefix_bytes);
                        let receipt = self.make_receipt(
                            task,
                            turns,
                            tool_calls,
                            Outcome::Fail,
                            Some(FailureClass::Verification),
                            saw_usage.then(|| usage_total.clone()),
                        );
                        self.receipts.append(&receipt)?;
                        return Err(e);
                    }
                };
                latest_prompt_tokens = resp.usage.as_ref().map(|u| u.prompt_tokens);
                if let Some(u) = &resp.usage {
                    saw_usage = true;
                    usage_total.prompt_tokens += u.prompt_tokens;
                    usage_total.completion_tokens += u.completion_tokens;
                    if let Some(c) = u.cached_tokens {
                        *usage_total.cached_tokens.get_or_insert(0) += c;
                    }
                }
                history.push(resp.message.clone());
                let calls = resp.message.tool_calls.clone().unwrap_or_default();
                if calls.is_empty() {
                    #[cfg(debug_assertions)]
                    debug_assert_eq!(message_bytes(&history[0]), prefix_bytes);
                    let text = resp.message.content.clone().unwrap_or_default();
                    let receipt = self.make_receipt(
                        task,
                        turns,
                        tool_calls,
                        Outcome::Pass,
                        None,
                        saw_usage.then(|| usage_total.clone()),
                    );
                    self.receipts.append(&receipt)?;
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

                #[cfg(debug_assertions)]
                debug_assert_eq!(message_bytes(&history[0]), prefix_bytes);
            }

            #[cfg(debug_assertions)]
            debug_assert_eq!(message_bytes(&history[0]), prefix_bytes);

            let receipt = self.make_receipt(
                task,
                turns,
                tool_calls,
                Outcome::Timeout,
                Some(FailureClass::Constraint),
                saw_usage.then(|| usage_total.clone()),
            );
            self.receipts.append(&receipt)?;
            Ok((
                format!("stopped after {} turns without a final answer", self.max_turns),
                receipt,
            ))
        }

        /// Execute one tool call; every failure becomes a message the model can act on.
        fn run_tool(&self, call: &ToolCallReq) -> String {
            let Some(tool) = self.tools.iter().find(|t| t.spec().name == call.name) else {
                let names: Vec<String> = self.tools.iter().map(|t| t.spec().name).collect();
                return format!("unknown tool '{}' — available tools: {}", call.name, names.join(", "));
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
            }
        }
    }

    fn plain_msg(role: &str, content: String) -> ChatMessage {
        ChatMessage {
            role: role.into(),
            content: Some(content),
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
        }
    }

    #[derive(Debug, Clone, Copy)]
    struct Compaction {
        messages: usize,
    }

    /// Deterministic fallback estimate used when a provider omits usage.
    fn estimate_tokens(messages: &[ChatMessage]) -> u64 {
        messages
            .iter()
            .map(|message| {
                let content_bytes = message.content.as_ref().map_or(0, String::len);
                let tool_call_bytes = message.tool_calls.as_ref().map_or(0, |calls| {
                    serde_json::to_vec(calls).map_or(0, |serialized| serialized.len())
                });
                let bytes = (content_bytes as u64).saturating_add(tool_call_bytes as u64);
                bytes.div_ceil(4).saturating_add(MESSAGE_OVERHEAD_TOKENS)
            })
            .sum()
    }

    /// Drop the smallest earlier prefix that brings the retained history under
    /// target. The last two user turns win over the target when both cannot fit.
    fn compact_history(history: &mut Vec<ChatMessage>, limit: u64) -> Option<Compaction> {
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
            .find(|index| {
                prefix_tokens.saturating_add(estimate_tokens(&history[*index..])) <= target
            })
            .unwrap_or(required_start);
        if start <= 1 {
            return None;
        }

        let messages = start - 1;
        let tokens = estimate_tokens(&history[1..start]);
        history.drain(1..start);
        let original = history[1].content.take().unwrap_or_default();
        history[1].content = Some(format!(
            "[nosis] earlier context compacted: {messages} messages, ~{tokens} tokens elided.\n\n{original}"
        ));

        Some(Compaction { messages })
    }

    fn context_percentage(input_tokens: u64, limit: u64) -> u64 {
        if limit == 0 {
            100
        } else {
            (100.0 * input_tokens as f64 / limit as f64).round() as u64
        }
    }

    #[cfg(debug_assertions)]
    fn message_bytes(message: &ChatMessage) -> Vec<u8> {
        serde_json::to_vec(message).expect("chat messages serialize")
    }

    /// "turn 2: edit_file src/lib.rs" — name plus the key argument, kept short.
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
    mod tests {
        use super::*;

        #[test]
        fn progress_line_shows_key_arg() {
            let call = ToolCallReq {
                id: "c1".into(),
                name: "edit_file".into(),
                arguments: r#"{"path":"src/lib.rs","old_string":"a","new_string":"b"}"#.into(),
            };
            assert_eq!(progress_line(2, &call), "turn 2: edit_file src/lib.rs");
        }

        #[test]
        fn progress_line_survives_bad_json() {
            let call = ToolCallReq { id: "c1".into(), name: "read_file".into(), arguments: "{oops".into() };
            assert_eq!(progress_line(1, &call), "turn 1: read_file");
        }
    }
}
