//! Wire-client tests against a local one-shot mock HTTP server (loopback only,
//! no live calls): `make_client` picks the client per wire, headers and paths
//! are exactly per contract, route policy is captured at construction.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;

use nh_core::wire::{make_client, ChatMessage, ChatRequest, ThinkingEffort, ToolCallReq};
use nh_routes::{ResolvedRoute, RouteClass, ThinkingDialect, Wire};
use zeroize::Zeroizing;

/// Obviously fake test secret (never a real key shape in use).
const FAKE_SECRET: &str = "sk-test-00000000";

struct Captured {
    path: String,
    /// Header names lowercased.
    headers: HashMap<String, String>,
    body: serde_json::Value,
}

/// Accepts ONE connection, captures the request, answers with canned JSON.
fn one_shot_server(status: u16, response_body: String) -> (String, mpsc::Receiver<Captured>) {
    one_shot_server_with(status, String::new(), response_body)
}

/// Like `one_shot_server`, with extra response headers ("name: value\r\n"…).
fn one_shot_server_with(
    status: u16,
    extra_headers: String,
    response_body: String,
) -> (String, mpsc::Receiver<Captured>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf: Vec<u8> = Vec::new();
        let mut tmp = [0u8; 4096];
        let header_end = loop {
            let n = stream.read(&mut tmp).unwrap();
            buf.extend_from_slice(&tmp[..n]);
            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break pos + 4;
            }
        };
        let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
        let mut lines = head.lines();
        let path = lines
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .unwrap_or_default()
            .to_string();
        let mut headers = HashMap::new();
        let mut content_length = 0usize;
        for line in lines {
            if let Some((name, value)) = line.split_once(':') {
                let name = name.trim().to_ascii_lowercase();
                let value = value.trim().to_string();
                if name == "content-length" {
                    content_length = value.parse().unwrap_or(0);
                }
                headers.insert(name, value);
            }
        }
        while buf.len() < header_end + content_length {
            let n = stream.read(&mut tmp).unwrap();
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n]);
        }
        let body = serde_json::from_slice(&buf[header_end..header_end + content_length])
            .unwrap_or(serde_json::Value::Null);
        tx.send(Captured {
            path,
            headers,
            body,
        })
        .ok();
        let resp = format!(
            "HTTP/1.1 {status} X\r\n{extra_headers}content-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{response_body}",
            response_body.len()
        );
        stream.write_all(resp.as_bytes()).ok();
    });
    (format!("http://{addr}"), rx)
}

fn route(
    base_url: &str,
    wire: Wire,
    dialect: ThinkingDialect,
    preserve_reasoning: bool,
    quirks: &[&str],
    max_out: Option<u64>,
) -> ResolvedRoute {
    ResolvedRoute {
        id: "mock-route".into(),
        provider: "mock".into(),
        model_id: "mock-model".into(),
        base_url: base_url.into(),
        wire,
        vault_entry: "mock".into(),
        class: RouteClass::Api,
        modality: vec!["text".into()],
        context: Some(128_000),
        max_out,
        thinking_dialect: dialect,
        preserve_reasoning,
        preserve_when_thinking: false,
        quirks: quirks.iter().map(|s| s.to_string()).collect(),
        price: None,
    }
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

fn req(messages: Vec<ChatMessage>, thinking: ThinkingEffort) -> ChatRequest {
    ChatRequest {
        model: "mock-model".into(),
        messages,
        tools: vec![],
        thinking,
    }
}

const OPENAI_OK: &str = r#"{"choices":[{"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}],"usage":{"prompt_tokens":2,"completion_tokens":1}}"#;
const ANTHROPIC_OK: &str = r#"{"content":[{"type":"text","text":"hello"}],"stop_reason":"end_turn","usage":{"input_tokens":9,"output_tokens":4,"cache_read_input_tokens":2}}"#;

#[test]
fn factory_openai_wire_posts_chat_completions_with_route_policy() {
    let (url, rx) = one_shot_server(200, OPENAI_OK.into());
    let r = route(
        &url,
        Wire::OpenAi,
        ThinkingDialect::DeepseekNhm,
        false,
        &["empty-reasoning-content-on-tool-replay"],
        Some(384_000),
    );
    let client = make_client(&r, Zeroizing::new(FAKE_SECRET.into()));

    let request = req(
        vec![
            msg("user", Some("go")),
            ChatMessage {
                tool_calls: Some(vec![ToolCallReq {
                    id: "c1".into(),
                    name: "read_file".into(),
                    arguments: "{}".into(),
                }]),
                ..msg("assistant", None)
            },
            ChatMessage {
                tool_call_id: Some("c1".into()),
                ..msg("tool", Some("data"))
            },
        ],
        ThinkingEffort::None,
    );
    let resp = client.complete(&request).unwrap();
    assert_eq!(resp.message.content.as_deref(), Some("ok"));

    let captured = rx.recv().unwrap();
    assert_eq!(captured.path, "/chat/completions");
    assert_eq!(
        captured.headers["authorization"],
        format!("Bearer {FAKE_SECRET}")
    );
    // Route policy captured by the factory: explicit disable + deepseek quirk.
    assert_eq!(captured.body["thinking"]["type"], "disabled");
    assert!(captured.body.get("reasoning_effort").is_none());
    assert_eq!(captured.body["messages"][1]["reasoning_content"], "");
    assert_eq!(captured.body["max_tokens"], 384_000);
}

#[test]
fn deepseek_none_and_low_send_explicit_disable_with_route_cap() {
    for effort in [ThinkingEffort::None, ThinkingEffort::Low] {
        let (url, rx) = one_shot_server(200, OPENAI_OK.into());
        let r = route(
            &url,
            Wire::OpenAi,
            ThinkingDialect::DeepseekNhm,
            false,
            &[],
            Some(384_000),
        );
        let client = make_client(&r, Zeroizing::new(FAKE_SECRET.into()));
        client
            .complete(&req(vec![msg("user", Some("hi"))], effort))
            .unwrap();

        let body = rx.recv().unwrap().body;
        assert_eq!(body["thinking"]["type"], "disabled", "effort {effort:?}");
        assert!(body.get("reasoning_effort").is_none());
        assert_eq!(body["max_tokens"], 384_000);
    }
}

#[test]
fn factory_kimi_toggle_replays_reasoning_only_while_thinking_is_active() {
    for (effort, toggle, replays) in [
        (ThinkingEffort::High, "enabled", true),
        (ThinkingEffort::None, "disabled", false),
    ] {
        let (url, rx) = one_shot_server(200, OPENAI_OK.into());
        let mut r = route(
            &url,
            Wire::OpenAi,
            ThinkingDialect::KimiToggle,
            false,
            &[],
            Some(131_072),
        );
        r.preserve_when_thinking = true;
        let client = make_client(&r, Zeroizing::new(FAKE_SECRET.into()));
        let request = req(
            vec![ChatMessage {
                reasoning_content: Some("required chain".into()),
                tool_calls: Some(vec![ToolCallReq {
                    id: "c1".into(),
                    name: "read_file".into(),
                    arguments: "{}".into(),
                }]),
                ..msg("assistant", None)
            }],
            effort,
        );
        client.complete(&request).unwrap();

        let body = rx.recv().unwrap().body;
        assert_eq!(body["thinking"]["type"], toggle);
        assert_eq!(body["max_tokens"], 131_072);
        assert_eq!(
            body["messages"][0].get("reasoning_content").is_some(),
            replays
        );
    }
}

#[test]
fn factory_anthropic_wire_posts_v1_messages_with_required_headers() {
    let (url, rx) = one_shot_server(200, ANTHROPIC_OK.into());
    let r = route(
        &url,
        Wire::AnthropicMessages,
        ThinkingDialect::None,
        false,
        &[],
        Some(384_000),
    );
    let client = make_client(&r, Zeroizing::new(FAKE_SECRET.into()));

    let request = req(
        vec![msg("system", Some("be brief")), msg("user", Some("hi"))],
        ThinkingEffort::Max,
    );
    let resp = client.complete(&request).unwrap();
    assert_eq!(resp.message.content.as_deref(), Some("hello"));
    assert_eq!(resp.finish_reason, "end_turn");
    let usage = resp.usage.unwrap();
    assert_eq!(usage.prompt_tokens, 9);
    assert_eq!(usage.completion_tokens, 4);
    assert_eq!(usage.cached_tokens, Some(2));

    let captured = rx.recv().unwrap();
    assert_eq!(captured.path, "/v1/messages");
    assert_eq!(captured.headers["x-api-key"], FAKE_SECRET);
    assert_eq!(captured.headers["anthropic-version"], "2023-06-01");
    assert!(
        !captured.headers.contains_key("authorization"),
        "no bearer auth on this wire"
    );
    assert_eq!(captured.body["max_tokens"], 384_000);
    assert_eq!(captured.body["system"], "be brief");
    assert_eq!(captured.body["messages"][0]["content"][0]["text"], "hi");
}

#[test]
fn anthropic_max_tokens_follows_route_max_out_below_cap() {
    let (url, rx) = one_shot_server(200, ANTHROPIC_OK.into());
    let r = route(
        &url,
        Wire::AnthropicMessages,
        ThinkingDialect::None,
        false,
        &[],
        Some(4096),
    );
    let client = make_client(&r, Zeroizing::new(FAKE_SECRET.into()));
    client
        .complete(&req(vec![msg("user", Some("hi"))], ThinkingEffort::None))
        .unwrap();
    assert_eq!(rx.recv().unwrap().body["max_tokens"], 4096);
}

#[test]
fn anthropic_route_cap_is_not_artificially_clamped() {
    let (url, rx) = one_shot_server(200, ANTHROPIC_OK.into());
    let r = route(
        &url,
        Wire::AnthropicMessages,
        ThinkingDialect::None,
        false,
        &[],
        Some(384_000),
    );
    let client = make_client(&r, Zeroizing::new(FAKE_SECRET.into()));
    client
        .complete(&req(vec![msg("user", Some("hi"))], ThinkingEffort::None))
        .unwrap();
    assert_eq!(rx.recv().unwrap().body["max_tokens"], 384_000);
}

#[test]
fn cross_host_redirects_are_refused_never_followed() {
    // Regression (adversarial review): reqwest forwards custom headers like
    // x-api-key across cross-host redirects, so following one would hand the
    // key to whoever controls the Location header. Both wire clients must
    // surface the redirect as an HTTP error and never contact the target.
    let (attacker_url, attacker_rx) = one_shot_server(200, ANTHROPIC_OK.into());
    for wire in [Wire::AnthropicMessages, Wire::OpenAi] {
        let (url, _rx) = one_shot_server_with(
            307,
            format!("location: {attacker_url}/v1/messages\r\n"),
            String::new(),
        );
        let r = route(&url, wire, ThinkingDialect::None, false, &[], None);
        let client = make_client(&r, Zeroizing::new(FAKE_SECRET.into()));
        let err = client
            .complete(&req(vec![msg("user", Some("hi"))], ThinkingEffort::None))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("HTTP 307"),
            "redirect surfaces as an HTTP error: {err}"
        );
    }
    // complete() already returned for both wires — if a redirect had been
    // followed, the attacker's capture would be in the channel by now.
    assert!(
        attacker_rx.try_recv().is_err(),
        "cross-host redirect was followed — key leaked"
    );
}

#[test]
fn anthropic_error_is_one_friendly_scrubbed_line() {
    let (url, _rx) = one_shot_server(
        401,
        format!(r#"{{"error":{{"message":"bad key {FAKE_SECRET} rejected"}}}}"#),
    );
    let r = route(
        &url,
        Wire::AnthropicMessages,
        ThinkingDialect::None,
        false,
        &[],
        None,
    );
    let client = make_client(&r, Zeroizing::new(FAKE_SECRET.into()));
    let err = client
        .complete(&req(vec![msg("user", Some("hi"))], ThinkingEffort::None))
        .unwrap_err()
        .to_string();
    assert!(err.contains("HTTP 401"));
    assert!(err.contains("nh key add"), "401 must say what to do next");
    assert!(err.contains("[REDACTED]"));
    assert!(!err.contains(FAKE_SECRET), "key literal must never appear");
    assert!(!err.contains('\n'), "one line, no debug dumps");
}
