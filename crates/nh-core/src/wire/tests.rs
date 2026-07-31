use super::anthropic::{
    build_body as build_anthropic_body, endpoint as anthropic_endpoint,
    extract_usage as extract_anthropic_usage, parse_response as parse_anthropic_response,
};
use super::http::{scrub_snippet, send_error_line, CONNECT_TIMEOUT, REQUEST_TIMEOUT};
use super::openai::{
    build_body, endpoint, extract_usage as extract_openai_usage, parse_response, OpenAiPolicy,
};
use super::retry::combine_usage;
use super::usage_debug::{UsageDebug, DEBUG_USAGE_ENV};
use super::*;
use std::ffi::OsStr;
use std::time::Duration;

#[test]
fn cache_hit_percentage_is_optional_and_rejects_inconsistent_usage() {
    assert_eq!(cache_hit_pct(0, Some(10)), None);
    assert_eq!(cache_hit_pct(20, None), None);
    assert_eq!(cache_hit_pct(20, Some(0)), Some(0.0));
    assert_eq!(cache_hit_pct(20, Some(5)), Some(25.0));
    assert_eq!(cache_hit_pct(10, Some(20)), None);
    assert_eq!(cache_hit_pct(20, Some(21)), None);
    assert_eq!(cache_hit_pct(20, Some(20)), Some(100.0));
}

fn msg(role: &str, content: Option<&str>) -> ChatMessage {
    ChatMessage {
        role: role.into(),
        content: content.map(str::to_string),
        parts: None,
        tool_calls: None,
        tool_call_id: None,
        reasoning_content: None,
    }
}

fn tool_call(id: &str, name: &str, arguments: &str) -> ToolCallReq {
    ToolCallReq {
        id: id.into(),
        name: name.into(),
        arguments: arguments.into(),
    }
}

fn req(messages: Vec<ChatMessage>) -> ChatRequest {
    ChatRequest {
        model: "mock-model".into(),
        messages,
        tools: vec![],
        thinking: ThinkingEffort::None,
    }
}

fn policy(dialect: ThinkingDialect, preserve_reasoning: bool, quirk: bool) -> OpenAiPolicy {
    OpenAiPolicy {
        dialect,
        preserve_reasoning,
        preserve_when_thinking: false,
        empty_reasoning_on_tool_replay: quirk,
        max_out: None,
    }
}

#[test]
fn anthropic_body_roles_alternate_after_compaction() {
    // Reproduces the post-L7-compaction shape on the Anthropic wire: the
    // elision note (a SECOND system message inserted at history[1]) degrades
    // to a user block and lands immediately before the first retained user
    // turn — the Anthropic Messages API rejects two consecutive user roles.
    let request = req(vec![
        msg("system", Some("sealed constitution")),
        msg(
            "system",
            Some("[nosis] earlier context compacted: 3 messages, ~900 tokens elided."),
        ),
        msg("user", Some("retained question")),
        msg("assistant", Some("retained answer")),
    ]);
    let body = build_anthropic_body(&request, 1024, ThinkingDialect::None);
    let roles: Vec<String> = body["messages"]
        .as_array()
        .expect("messages array")
        .iter()
        .map(|m| m["role"].as_str().unwrap_or_default().to_string())
        .collect();
    assert_eq!(
        roles.first().map(String::as_str),
        Some("user"),
        "first message must be user: {roles:?}"
    );
    for pair in roles.windows(2) {
        assert_ne!(
            pair[0], pair[1],
            "consecutive same-role messages break the Anthropic wire: {roles:?}"
        );
    }
}

#[test]
fn anthropic_body_merges_user_role_blocks_in_order() {
    let request = req(vec![
        msg("system", Some("sealed constitution")),
        msg("system", Some("elision note")),
        msg("user", Some("retained question")),
        ChatMessage {
            tool_call_id: Some("c1".into()),
            ..msg("tool", Some("tool output"))
        },
        msg("user", Some("follow-up")),
    ]);
    let body = build_anthropic_body(&request, 1024, ThinkingDialect::None);
    let messages = body["messages"].as_array().expect("messages array");

    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["role"], "user");
    let blocks = messages[0]["content"].as_array().expect("content array");
    assert_eq!(blocks.len(), 4);
    assert_eq!(
        blocks[0],
        serde_json::json!({"type": "text", "text": "elision note"})
    );
    assert_eq!(
        blocks[1],
        serde_json::json!({"type": "text", "text": "retained question"})
    );
    assert_eq!(
        blocks[2],
        serde_json::json!({
            "type": "tool_result",
            "tool_use_id": "c1",
            "content": "tool output",
        })
    );
    assert_eq!(
        blocks[3],
        serde_json::json!({"type": "text", "text": "follow-up"})
    );
}

#[test]
fn endpoint_trims_trailing_slash() {
    assert_eq!(
        endpoint("https://api.example.com/"),
        "https://api.example.com/chat/completions"
    );
    assert_eq!(
        endpoint("https://api.example.com"),
        "https://api.example.com/chat/completions"
    );
}

#[test]
fn timeout_error_says_what_happened_and_what_to_do() {
    let line = send_error_line(
        "https://api.example.com/chat/completions",
        true,
        "op timed out",
    );
    assert_eq!(
        line,
        "provider at https://api.example.com/chat/completions did not answer within 600s \
         — retry, or switch to another route"
    );
    // Non-timeout failures keep the reachability wording and the detail.
    let line = send_error_line(
        "https://api.example.com/chat/completions",
        false,
        "dns error",
    );
    assert!(
        line.starts_with("could not reach provider at "),
        "got: {line}"
    );
    assert!(line.ends_with("dns error"), "got: {line}");
}

#[test]
fn request_timeout_outlives_slow_thinking_turns() {
    // Guard against the hidden 30 s blocking-client default sneaking back:
    // thinking routes (kimi/glm at High) routinely exceed 30 s per turn.
    assert!(
        REQUEST_TIMEOUT >= Duration::from_secs(300),
        "got: {REQUEST_TIMEOUT:?}"
    );
    assert!(
        CONNECT_TIMEOUT <= Duration::from_secs(30),
        "dead hosts must fail fast"
    );
}

#[test]
fn body_nests_tools_and_tool_calls() {
    let mut request = req(vec![
        ChatMessage {
            tool_calls: Some(vec![tool_call("c1", "read_file", r#"{"path":"a.txt"}"#)]),
            ..msg("assistant", None)
        },
        ChatMessage {
            tool_call_id: Some("c1".into()),
            ..msg("tool", Some("data"))
        },
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
    assert_eq!(
        body["messages"][0]["tool_calls"][0]["function"]["name"],
        "read_file"
    );
    assert_eq!(
        body["messages"][0]["tool_calls"][0]["function"]["arguments"],
        r#"{"path":"a.txt"}"#
    );
    assert_eq!(body["messages"][1]["tool_call_id"], "c1");
    assert!(body["messages"][1].get("tool_calls").is_none());
}

#[test]
fn parts_free_request_bytes_remain_identical() {
    let request = req(vec![msg("user", Some("hello"))]);
    let bytes = serde_json::to_string(&build_body(&request, OpenAiPolicy::default())).unwrap();

    assert_eq!(
        bytes,
        r#"{"max_tokens":65536,"messages":[{"content":"hello","role":"user"}],"model":"mock-model"}"#
    );
    assert_eq!(
        serde_json::to_string(&request.messages[0]).unwrap(),
        r#"{"role":"user","content":"hello"}"#
    );
}

#[test]
fn image_parts_emit_exact_openai_data_uri_shape_and_keep_tools() {
    let mut request = req(vec![ChatMessage {
        role: "user".into(),
        content: None,
        parts: Some(vec![
            ContentPart::Text {
                text: "what is shown?".into(),
            },
            ContentPart::ImageB64 {
                media_type: "image/png".into(),
                data: "Zm9v".into(),
            },
        ]),
        tool_calls: None,
        tool_call_id: None,
        reasoning_content: None,
    }]);
    request.tools = vec![nh_tools::ToolSpec {
        name: "read_file".into(),
        description: "read a file".into(),
        parameters: serde_json::json!({"type": "object"}),
    }];

    let body = build_body(&request, OpenAiPolicy::default());

    assert_eq!(
        body["messages"][0]["content"],
        serde_json::json!([
            {"type": "text", "text": "what is shown?"},
            {
                "type": "image_url",
                "image_url": {"url": "data:image/png;base64,Zm9v"}
            }
        ])
    );
    assert_eq!(body["tools"][0]["function"]["name"], "read_file");
}

#[test]
fn deepseek_dialect_maps_every_effort_tier() {
    for effort in [ThinkingEffort::None, ThinkingEffort::Low] {
        let mut request = req(vec![msg("user", Some("hi"))]);
        request.thinking = effort;
        let body = build_body(&request, policy(ThinkingDialect::DeepseekNhm, false, false));
        assert_eq!(body["thinking"]["type"], "disabled", "effort {effort:?}");
        assert!(body.get("reasoning_effort").is_none());
    }

    for (effort, expected) in [(ThinkingEffort::High, "high"), (ThinkingEffort::Max, "max")] {
        let mut request = req(vec![msg("user", Some("hi"))]);
        request.thinking = effort;
        let body = build_body(&request, policy(ThinkingDialect::DeepseekNhm, false, false));
        assert_eq!(body["reasoning_effort"], expected, "effort {effort:?}");
    }
}

#[test]
fn deepseek_replays_reasoning_only_while_thinking_is_active() {
    let mut conditional = policy(ThinkingDialect::DeepseekNhm, false, false);
    conditional.preserve_when_thinking = true;
    let history = vec![ChatMessage {
        reasoning_content: Some("required chain".into()),
        tool_calls: Some(vec![tool_call("c1", "read_file", "{}")]),
        ..msg("assistant", None)
    }];

    let mut request = req(history.clone());
    request.thinking = ThinkingEffort::High;
    let active = build_body(&request, conditional);
    assert_eq!(active["messages"][0]["reasoning_content"], "required chain");

    request.messages = history;
    request.thinking = ThinkingEffort::None;
    let inactive = build_body(&request, conditional);
    assert!(inactive["messages"][0].get("reasoning_content").is_none());
}

#[test]
fn kimi_toggle_replays_reasoning_only_while_thinking_is_active() {
    let mut conditional = policy(ThinkingDialect::KimiToggle, false, false);
    conditional.preserve_when_thinking = true;
    conditional.max_out = Some(123_456);
    let history = vec![ChatMessage {
        reasoning_content: Some("required chain".into()),
        tool_calls: Some(vec![tool_call("c1", "read_file", "{}")]),
        ..msg("assistant", None)
    }];

    let mut request = req(history.clone());
    request.thinking = ThinkingEffort::High;
    let active = build_body(&request, conditional);
    assert_eq!(active["thinking"]["type"], "enabled");
    assert_eq!(active["thinking"]["keep"], "all");
    assert_eq!(active["messages"][0]["reasoning_content"], "required chain");
    assert_eq!(active["max_tokens"], 123_456);

    request.messages = history;
    request.thinking = ThinkingEffort::None;
    let inactive = build_body(&request, conditional);
    assert_eq!(inactive["thinking"]["type"], "disabled");
    assert!(inactive["thinking"].get("keep").is_none());
    assert!(inactive["messages"][0].get("reasoning_content").is_none());
}

#[test]
fn kimi_toggle_without_conditional_replay_emits_only_the_toggle_shape() {
    let mut request = req(vec![msg("user", Some("hi"))]);
    let mimo_policy = policy(ThinkingDialect::KimiToggle, true, false);

    request.thinking = ThinkingEffort::None;
    assert_eq!(
        build_body(&request, mimo_policy)["thinking"],
        serde_json::json!({ "type": "disabled" })
    );

    request.thinking = ThinkingEffort::High;
    assert_eq!(
        build_body(&request, mimo_policy)["thinking"],
        serde_json::json!({ "type": "enabled" })
    );
}

#[test]
fn fixed_or_absent_thinking_dialects_send_no_control() {
    for dialect in [ThinkingDialect::AlwaysThinking, ThinkingDialect::None] {
        let mut request = req(vec![msg("user", Some("hi"))]);
        request.thinking = ThinkingEffort::Max;
        let body = build_body(&request, policy(dialect, false, false));
        assert!(
            body.get("reasoning_effort").is_none(),
            "dialect {dialect:?} must not send effort"
        );
        assert!(body.get("thinking").is_none(), "dialect {dialect:?}");
    }
}

#[test]
fn glm_dialect_disables_thinking_or_sends_normalized_effort() {
    for (effort, expected) in [
        (ThinkingEffort::Low, "high"),
        (ThinkingEffort::High, "high"),
        (ThinkingEffort::Max, "max"),
    ] {
        let mut request = req(vec![msg("user", Some("hi"))]);
        request.thinking = effort;
        let body = build_body(&request, policy(ThinkingDialect::GlmHm, false, false));
        assert_eq!(body["reasoning_effort"], expected, "effort {effort:?}");
        assert!(body.get("thinking").is_none());
    }

    let request = req(vec![msg("user", Some("hi"))]);
    let body = build_body(&request, policy(ThinkingDialect::GlmHm, false, false));
    assert_eq!(body["thinking"]["type"], "disabled");
    assert!(body.get("reasoning_effort").is_none());
}

#[test]
fn always_thinking_effort_dialect_sends_every_legal_tier() {
    for (effort, expected) in [
        (ThinkingEffort::None, "low"),
        (ThinkingEffort::Low, "low"),
        (ThinkingEffort::High, "high"),
        (ThinkingEffort::Max, "max"),
    ] {
        let mut request = req(vec![msg("user", Some("hi"))]);
        request.thinking = effort;
        let body = build_body(
            &request,
            policy(ThinkingDialect::AlwaysThinkingEffort, true, false),
        );
        assert_eq!(body["reasoning_effort"], expected, "effort {effort:?}");
        assert!(body.get("thinking").is_none());
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
    let body = build_body(
        &request,
        policy(ThinkingDialect::AlwaysThinking, true, false),
    );
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
    let request = req(vec![ChatMessage {
        tool_calls: calls.clone(),
        ..msg("assistant", None)
    }]);
    let body = build_body(&request, quirked);
    assert_eq!(body["messages"][0]["reasoning_content"], "");

    // Empty-string content still counts as tool-only.
    let request = req(vec![ChatMessage {
        tool_calls: calls.clone(),
        ..msg("assistant", Some(""))
    }]);
    let body = build_body(&request, quirked);
    assert_eq!(body["messages"][0]["reasoning_content"], "");

    // Assistant turns WITH text do not get it.
    let request = req(vec![ChatMessage {
        tool_calls: calls.clone(),
        ..msg("assistant", Some("look"))
    }]);
    let body = build_body(&request, quirked);
    assert!(body["messages"][0].get("reasoning_content").is_none());

    // Plain text turns and non-assistant roles do not get it.
    let request = req(vec![
        msg("assistant", Some("done")),
        msg("user", Some("hi")),
    ]);
    let body = build_body(&request, quirked);
    assert!(body["messages"][0].get("reasoning_content").is_none());
    assert!(body["messages"][1].get("reasoning_content").is_none());

    // Non-quirked routes never get it, even on tool-only replay.
    let request = req(vec![ChatMessage {
        tool_calls: calls,
        ..msg("assistant", None)
    }]);
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
fn parses_both_openai_and_ollama_reasoning_field_names() {
    for field in ["reasoning_content", "reasoning"] {
        let body = format!(
            r#"{{"choices":[{{"message":{{"role":"assistant","content":"hi",
                "{field}":"thought hard"}},"finish_reason":"stop"}}]}}"#
        );
        let response = parse_response(&body).unwrap();
        assert_eq!(
            response.message.reasoning_content.as_deref(),
            Some("thought hard"),
            "{field}"
        );
    }
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
    let body = build_anthropic_body(&request, 8192, ThinkingDialect::None);
    assert_eq!(body["model"], "mock-model");
    assert_eq!(body["max_tokens"], 8192);
    assert_eq!(body["system"], "be brief");
    let messages = body["messages"].as_array().unwrap();
    assert_eq!(
        messages.len(),
        2,
        "system message must not appear in messages"
    );
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[0]["content"][0]["type"], "text");
    assert_eq!(messages[0]["content"][0]["text"], "hi");
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[1]["content"][0]["text"], "hello");
    assert!(body.get("tools").is_none());
    // Generic Anthropic routes do not inherit provider-specific controls.
    let raw = body.to_string();
    assert!(!raw.contains("reasoning_effort") && !raw.contains("thinking"));
}

#[test]
fn deepseek_anthropic_body_explicitly_disables_default_thinking() {
    let request = req(vec![msg("user", Some("hi"))]);
    let body = build_anthropic_body(&request, 8192, ThinkingDialect::DeepseekNhm);
    assert_eq!(body["thinking"]["type"], "disabled");
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
        ChatMessage {
            tool_call_id: Some("c1".into()),
            ..msg("tool", Some("data1"))
        },
        ChatMessage {
            tool_call_id: Some("c2".into()),
            ..msg("tool", Some("data2"))
        },
        msg("user", Some("thanks")),
    ]);
    request.tools = vec![nh_tools::ToolSpec {
        name: "read_file".into(),
        description: "read a file".into(),
        parameters: serde_json::json!({"type": "object"}),
    }];
    let body = build_anthropic_body(&request, 4096, ThinkingDialect::None);

    let assistant = &body["messages"][1];
    assert_eq!(assistant["content"][0]["type"], "text");
    assert_eq!(assistant["content"][0]["text"], "let me look");
    assert_eq!(assistant["content"][1]["type"], "tool_use");
    assert_eq!(assistant["content"][1]["id"], "c1");
    assert_eq!(assistant["content"][1]["input"]["path"], "a.txt");
    // Unparseable arguments degrade to an empty object.
    assert_eq!(assistant["content"][2]["input"], serde_json::json!({}));

    // Tool results and the following user turn → ONE user message, preserving block order.
    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 3);
    let results = &messages[2];
    assert_eq!(results["role"], "user");
    assert_eq!(results["content"].as_array().unwrap().len(), 3);
    assert_eq!(results["content"][0]["type"], "tool_result");
    assert_eq!(results["content"][0]["tool_use_id"], "c1");
    assert_eq!(results["content"][0]["content"], "data1");
    assert_eq!(results["content"][1]["tool_use_id"], "c2");
    assert_eq!(results["content"][2]["text"], "thanks");

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
    let raw = build_anthropic_body(&request, 8192, ThinkingDialect::None).to_string();
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
fn anthropic_tool_use_requires_nonempty_id_and_name() {
    for body in [
        r#"{"content":[{"type":"tool_use","name":"read_file","input":{}}]}"#,
        r#"{"content":[{"type":"tool_use","id":"t1","input":{}}]}"#,
        r#"{"content":[{"type":"tool_use","id":"","name":"read_file","input":{}}]}"#,
        r#"{"content":[{"type":"tool_use","id":"t1","name":"","input":{}}]}"#,
    ] {
        let error = parse_anthropic_response(body).unwrap_err().to_string();
        assert!(
            error.contains("tool_use block missing id or name"),
            "got: {error}"
        );
    }

    let response = parse_anthropic_response(
        r#"{"content":[{"type":"tool_use","id":"t1","name":"read_file","input":{}}]}"#,
    )
    .unwrap();
    let call = &response.message.tool_calls.unwrap()[0];
    assert_eq!(call.id, "t1");
    assert_eq!(call.name, "read_file");
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
fn parses_top_level_cache_hit_fallback_without_overriding_nested_value() {
    let fallback = r#"{
        "choices": [{"message": {"role": "assistant", "content": "ok"}}],
        "usage": {
            "prompt_tokens": 20,
            "completion_tokens": 2,
            "prompt_cache_hit_tokens": 7,
            "prompt_cache_miss_tokens": 13
        }
    }"#;
    assert_eq!(
        parse_response(fallback)
            .unwrap()
            .usage
            .unwrap()
            .cached_tokens,
        Some(7)
    );

    let nested = r#"{
        "choices": [{"message": {"role": "assistant", "content": "ok"}}],
        "usage": {
            "prompt_tokens": 20,
            "completion_tokens": 2,
            "prompt_tokens_details": {"cached_tokens": 5},
            "prompt_cache_hit_tokens": 7
        }
    }"#;
    assert_eq!(
        parse_response(nested).unwrap().usage.unwrap().cached_tokens,
        Some(5)
    );
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
fn wire_specific_error_usage_is_salvaged_without_estimation() {
    let openai = extract_openai_usage(
        r#"{"error":{"message":"busy"},"usage":{"prompt_tokens":11,"completion_tokens":2,"prompt_tokens_details":{"cached_tokens":3}}}"#,
    )
    .unwrap();
    assert_eq!(openai.prompt_tokens, 11);
    assert_eq!(openai.completion_tokens, 2);
    assert_eq!(openai.cached_tokens, Some(3));

    let anthropic = extract_anthropic_usage(
        r#"{"type":"error","error":{"message":"busy"},"usage":{"input_tokens":7,"output_tokens":4,"cache_read_input_tokens":2}}"#,
    )
    .unwrap();
    assert_eq!(anthropic.prompt_tokens, 7);
    assert_eq!(anthropic.completion_tokens, 4);
    assert_eq!(anthropic.cached_tokens, Some(2));

    assert!(extract_openai_usage(r#"{"error":{"message":"busy"}}"#).is_none());
    assert!(extract_anthropic_usage("not json").is_none());
}

#[test]
fn salvaged_and_success_usage_sum_while_absence_contributes_zero() {
    let salvaged = Usage {
        prompt_tokens: 11,
        completion_tokens: 2,
        cached_tokens: Some(3),
    };
    let success = Usage {
        prompt_tokens: 7,
        completion_tokens: 4,
        cached_tokens: Some(2),
    };
    let combined = combine_usage(Some(salvaged), Some(success)).unwrap();
    assert_eq!(combined.prompt_tokens, 18);
    assert_eq!(combined.completion_tokens, 6);
    assert_eq!(combined.cached_tokens, Some(5));

    let observed = combine_usage(
        None,
        Some(Usage {
            prompt_tokens: 7,
            completion_tokens: 4,
            cached_tokens: None,
        }),
    )
    .unwrap();
    assert_eq!(observed.prompt_tokens, 7);
    assert_eq!(observed.completion_tokens, 4);
    assert_eq!(observed.cached_tokens, None);
    assert!(combine_usage(None, None).is_none());
}

#[test]
fn raw_usage_debug_preserves_unknown_fields_without_changing_metering() {
    let raw_usage = r#"{ "prompt_tokens": 12,  "completion_tokens": 7, "cached_tokens": 4, "future_counter": {"opaque": true} }"#;
    let body = format!(
        r#"{{"choices":[{{"message":{{"role":"assistant","content":"do not dump this"}}}}],"usage":{raw_usage}}}"#
    );
    let before = parse_response(&body).unwrap().usage.unwrap();
    assert_eq!(before.cached_tokens, None);

    let debug = UsageDebug::from_test_setting(
        Some(OsStr::new("1")),
        "kimi-probe",
        "openai",
        "fixture-sensitive-value",
    )
    .unwrap();
    let rendered = debug.render_for_test(1, &body);

    assert_eq!(
        rendered,
        format!("[{DEBUG_USAGE_ENV} route=kimi-probe wire=openai request=1] {raw_usage}")
    );
    assert!(!rendered.contains("do not dump this"));

    let after = parse_response(&body).unwrap().usage.unwrap();
    assert_eq!(after.prompt_tokens, before.prompt_tokens);
    assert_eq!(after.completion_tokens, before.completion_tokens);
    assert_eq!(after.cached_tokens, before.cached_tokens);
}

#[test]
fn raw_usage_debug_reports_absence_explicitly() {
    let debug = UsageDebug::from_test_setting(
        Some(OsStr::new("1")),
        "anthropic-probe",
        "anthropic",
        "fixture-sensitive-value",
    )
    .unwrap();
    let body = r#"{"content":[{"type":"text","text":"answer only"}]}"#;

    assert_eq!(
        debug.render_for_test(3, body),
        format!("[{DEBUG_USAGE_ENV} route=anthropic-probe wire=anthropic request=3] usage absent")
    );
}

#[test]
fn raw_usage_debug_scrubs_the_complete_line() {
    let secret = "fixture-sensitive-value";
    let debug =
        UsageDebug::from_test_setting(Some(OsStr::new("1")), "scrub-probe", "openai", secret)
            .unwrap();
    let body = format!(r#"{{"usage":{{"provider_note":"{secret}","future_tokens":9}}}}"#);
    let rendered = debug.render_for_test(1, &body);

    assert!(!rendered.contains(secret));
    assert!(rendered.contains(r#""provider_note":"[REDACTED]""#));
    assert!(rendered.contains(r#""future_tokens":9"#));
}

#[test]
fn raw_usage_debug_unset_leaves_both_wire_parsers_unchanged() {
    assert!(UsageDebug::from_test_setting(None, "openai-probe", "openai", "unused").is_none());
    assert!(UsageDebug::from_test_setting(
        Some(OsStr::new("0")),
        "anthropic-probe",
        "anthropic",
        "unused",
    )
    .is_none());

    let openai_body = r#"{"choices":[{"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}],"usage":{"prompt_tokens":2,"completion_tokens":1}}"#;
    let openai_before = parse_response(openai_body).unwrap();
    let openai_after = parse_response(openai_body).unwrap();
    assert_eq!(openai_after.message.content, openai_before.message.content);
    assert_eq!(openai_after.finish_reason, openai_before.finish_reason);
    assert_eq!(
        openai_after.usage.unwrap().prompt_tokens,
        openai_before.usage.unwrap().prompt_tokens
    );

    let anthropic_body = r#"{"content":[{"type":"text","text":"ok"}],"stop_reason":"end_turn","usage":{"input_tokens":9,"output_tokens":4}}"#;
    let anthropic_before = parse_anthropic_response(anthropic_body).unwrap();
    let anthropic_after = parse_anthropic_response(anthropic_body).unwrap();
    assert_eq!(
        anthropic_after.message.content,
        anthropic_before.message.content
    );
    assert_eq!(
        anthropic_after.finish_reason,
        anthropic_before.finish_reason
    );
    assert_eq!(
        anthropic_after.usage.unwrap().prompt_tokens,
        anthropic_before.usage.unwrap().prompt_tokens
    );
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

#[test]
fn resolve_effort_covers_every_posture_dialect_cell() {
    use ThinkingDialect::{
        AlwaysThinking, AlwaysThinkingEffort, DeepseekNhm, GlmHm, KimiToggle, None as NoToggle,
    };
    use ThinkingEffort::{High, Low, Max, None as NoEffort};
    use ThinkingPosture::{Ceiling, Default, Floor};

    let cases = [
        (Floor, DeepseekNhm, NoEffort),
        (Floor, KimiToggle, NoEffort),
        (Floor, AlwaysThinking, High),
        (Floor, AlwaysThinkingEffort, Low),
        (Floor, GlmHm, NoEffort),
        (Floor, NoToggle, NoEffort),
        (Default, DeepseekNhm, NoEffort),
        (Default, KimiToggle, NoEffort),
        (Default, AlwaysThinking, High),
        (Default, AlwaysThinkingEffort, High),
        (Default, GlmHm, High),
        (Default, NoToggle, NoEffort),
        (Ceiling, DeepseekNhm, High),
        (Ceiling, KimiToggle, High),
        (Ceiling, AlwaysThinking, High),
        (Ceiling, AlwaysThinkingEffort, Max),
        (Ceiling, GlmHm, High),
        (Ceiling, NoToggle, NoEffort),
    ];

    for (posture, dialect, expected) in cases {
        assert_eq!(
            resolve_effort(None, posture, dialect, Wire::OpenAi),
            expected,
            "{posture:?} × {dialect:?}"
        );
    }
}

#[test]
fn explicit_effort_wins_but_stays_route_legal() {
    assert_eq!(
        resolve_effort(
            Some(ThinkingEffort::Max),
            ThinkingPosture::Floor,
            ThinkingDialect::DeepseekNhm,
            Wire::OpenAi,
        ),
        ThinkingEffort::Max
    );
    assert_eq!(
        resolve_effort(
            Some(ThinkingEffort::None),
            ThinkingPosture::Ceiling,
            ThinkingDialect::AlwaysThinking,
            Wire::OpenAi,
        ),
        ThinkingEffort::High
    );
    assert_eq!(
        resolve_effort(
            Some(ThinkingEffort::Max),
            ThinkingPosture::Ceiling,
            ThinkingDialect::None,
            Wire::OpenAi,
        ),
        ThinkingEffort::None
    );
    assert_eq!(
        resolve_effort(
            Some(ThinkingEffort::None),
            ThinkingPosture::Default,
            ThinkingDialect::AlwaysThinkingEffort,
            Wire::OpenAi,
        ),
        ThinkingEffort::Low
    );
    assert_eq!(
        resolve_effort(
            Some(ThinkingEffort::Low),
            ThinkingPosture::Default,
            ThinkingDialect::GlmHm,
            Wire::OpenAi,
        ),
        ThinkingEffort::High
    );
    assert_eq!(
        resolve_effort(
            Some(ThinkingEffort::None),
            ThinkingPosture::Default,
            ThinkingDialect::GlmHm,
            Wire::OpenAi,
        ),
        ThinkingEffort::None
    );
}

#[test]
fn deepseek_explicit_low_resolves_to_the_disabled_wire_tier() {
    let resolved = resolve_effort(
        Some(ThinkingEffort::Low),
        ThinkingPosture::Ceiling,
        ThinkingDialect::DeepseekNhm,
        Wire::OpenAi,
    );
    assert_eq!(resolved, ThinkingEffort::None);

    let mut request = req(vec![msg("user", Some("hi"))]);
    request.thinking = resolved;
    let body = build_body(&request, policy(ThinkingDialect::DeepseekNhm, false, false));
    assert_eq!(body["thinking"]["type"], "disabled");
    assert!(body.get("reasoning_effort").is_none());
}

#[test]
fn anthropic_wire_resolves_to_provider_default_while_openai_keeps_high() {
    let explicit = Some(ThinkingEffort::High);
    assert_eq!(
        resolve_effort(
            explicit,
            ThinkingPosture::Ceiling,
            ThinkingDialect::DeepseekNhm,
            Wire::AnthropicMessages,
        ),
        ThinkingEffort::None
    );
    assert_eq!(
        resolve_effort(
            explicit,
            ThinkingPosture::Ceiling,
            ThinkingDialect::DeepseekNhm,
            Wire::OpenAi,
        ),
        ThinkingEffort::High
    );
}
