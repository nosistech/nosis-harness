//! nh-core - agent turn loop, wire client, receipts.
//! Every turn writes a scrubbed JSONL receipt to .nosis/receipts.jsonl (append-only).

pub mod wire {
    //! OpenAI-compatible chat wire (M0). Anthropic Messages wire lands in M1.
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
    }

    /// OpenAI shape: `arguments` is a raw JSON string.
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    pub struct ToolCallReq {
        pub id: String,
        pub name: String,
        pub arguments: String,
    }

    #[derive(Debug, Clone)]
    pub struct ChatRequest {
        pub model: String,
        pub messages: Vec<ChatMessage>,
        pub tools: Vec<nh_tools::ToolSpec>,
    }

    #[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
    pub struct Usage {
        pub prompt_tokens: u64,
        pub completion_tokens: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub cached_tokens: Option<u64>,
    }

    #[derive(Debug, Clone)]
    pub struct ChatResponse {
        pub message: ChatMessage,
        pub finish_reason: String,
        pub usage: Option<Usage>,
    }

    /// Provider abstraction - tests inject a mock, production uses OpenAiCompatClient.
    pub trait ChatClient: Send + Sync {
        fn complete(&self, req: &ChatRequest) -> anyhow::Result<ChatResponse>;
    }

    /// Blocking reqwest client against `{base_url}/chat/completions` (M0: no streaming).
    /// API key held zeroized, injected per-call, never logged.
    pub struct OpenAiCompatClient {
        pub base_url: String,
        api_key: Zeroizing<String>,
        http: reqwest::blocking::Client,
    }

    impl OpenAiCompatClient {
        pub fn new(base_url: String, api_key: Zeroizing<String>) -> Self {
            Self { base_url, api_key, http: reqwest::blocking::Client::new() }
        }
    }

    impl ChatClient for OpenAiCompatClient {
        fn complete(&self, req: &ChatRequest) -> anyhow::Result<ChatResponse> {
            let url = endpoint(&self.base_url);
            let resp = self
                .http
                .post(&url)
                .bearer_auth(self.api_key.as_str())
                .json(&build_body(req))
                .send()
                .map_err(|e| anyhow::anyhow!("could not reach provider at {url}: {e}"))?;
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            if !status.is_success() {
                let hint = match status.as_u16() {
                    401 | 403 => " - key rejected; run `nh key add <provider>`",
                    429 => " - rate limited; retry later",
                    _ => "",
                };
                anyhow::bail!(
                    "provider returned HTTP {}{}: {}",
                    status.as_u16(),
                    hint,
                    scrub_snippet(&body, self.api_key.as_str())
                );
            }
            parse_response(&body)
        }
    }

    fn endpoint(base_url: &str) -> String {
        format!("{}/chat/completions", base_url.trim_end_matches('/'))
    }

    /// Build the OpenAI-wire request body. Tool calls and tools use the nested
    /// `{"type":"function","function":{...}}` shape the wire requires.
    fn build_body(req: &ChatRequest) -> serde_json::Value {
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
        body
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
            },
            finish_reason: choice.finish_reason.unwrap_or_default(),
            usage: wire.usage.map(|u| Usage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
                cached_tokens: u.prompt_tokens_details.and_then(|d| d.cached_tokens),
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
        fn endpoint_trims_trailing_slash() {
            assert_eq!(endpoint("https://api.example.com/"), "https://api.example.com/chat/completions");
            assert_eq!(endpoint("https://api.example.com"), "https://api.example.com/chat/completions");
        }

        #[test]
        fn body_nests_tools_and_tool_calls() {
            let req = ChatRequest {
                model: "mock-model".into(),
                messages: vec![
                    ChatMessage {
                        role: "assistant".into(),
                        content: None,
                        tool_calls: Some(vec![ToolCallReq {
                            id: "c1".into(),
                            name: "read_file".into(),
                            arguments: r#"{"path":"a.txt"}"#.into(),
                        }]),
                        tool_call_id: None,
                    },
                    ChatMessage {
                        role: "tool".into(),
                        content: Some("data".into()),
                        tool_calls: None,
                        tool_call_id: Some("c1".into()),
                    },
                ],
                tools: vec![nh_tools::ToolSpec {
                    name: "read_file".into(),
                    description: "read a file".into(),
                    parameters: serde_json::json!({"type": "object"}),
                }],
            };
            let body = build_body(&req);
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
    use crate::wire::{ChatClient, ChatMessage, ChatRequest, ToolCallReq, Usage};
    use nh_tools::{Tool, ToolCtx};

    pub struct AgentLoop {
        pub client: Box<dyn ChatClient>,
        pub tools: Vec<Box<dyn Tool>>,
        pub ctx: ToolCtx,
        pub receipts: ReceiptWriter,
        pub model_id: String,
        pub max_turns: u32,
        /// Progress callback: invoked with one short line per tool call
        /// ("turn 2: edit_file src/lib.rs"). Core stays print-free - nh-cli
        /// wires this to its own printer.
        #[allow(clippy::type_complexity)]
        pub on_event: Option<Box<dyn Fn(&str) + Send>>,
    }

    impl AgentLoop {
        /// Runs one task to completion. Always writes exactly one receipt, even on error.
        /// Returns the final assistant text. UX: progress surfaces via `on_event` -
        /// one short line per tool call (name + key arg), never a wall of JSON.
        pub fn run(&mut self, task: &str) -> anyhow::Result<(String, Receipt)> {
            let specs: Vec<nh_tools::ToolSpec> = self.tools.iter().map(|t| t.spec()).collect();
            let tool_names: Vec<&str> = specs.iter().map(|s| s.name.as_str()).collect();
            let system = format!(
                "You are a coding agent. Complete the user's task using the available \
                 tools, then reply with a short final answer. Available tools: {}.",
                tool_names.join(", ")
            );
            let mut messages = vec![plain_msg("system", system), plain_msg("user", task.to_string())];

            let mut turns: u32 = 0;
            let mut tool_calls: u32 = 0;
            let mut usage_total = Usage::default();
            let mut saw_usage = false;

            while turns < self.max_turns {
                turns += 1;
                let req = ChatRequest {
                    model: self.model_id.clone(),
                    messages: messages.clone(),
                    tools: specs.clone(),
                };
                let resp = match self.client.complete(&req) {
                    Ok(r) => r,
                    Err(e) => {
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
                if let Some(u) = &resp.usage {
                    saw_usage = true;
                    usage_total.prompt_tokens += u.prompt_tokens;
                    usage_total.completion_tokens += u.completion_tokens;
                    if let Some(c) = u.cached_tokens {
                        *usage_total.cached_tokens.get_or_insert(0) += c;
                    }
                }
                messages.push(resp.message.clone());
                let calls = resp.message.tool_calls.clone().unwrap_or_default();
                if calls.is_empty() {
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
                    messages.push(ChatMessage {
                        role: "tool".into(),
                        content: Some(result),
                        tool_calls: None,
                        tool_call_id: Some(call.id.clone()),
                    });
                }
            }

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
                return format!("unknown tool '{}' - available tools: {}", call.name, names.join(", "));
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
        ChatMessage { role: role.into(), content: Some(content), tool_calls: None, tool_call_id: None }
    }

    /// "turn 2: edit_file src/lib.rs" - name plus the key argument, kept short.
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
