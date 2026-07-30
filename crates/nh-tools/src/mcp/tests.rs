use super::adapter::{args_one_line, McpToolAdapter};
use super::client::{
    lint_headers, ToolEntry, MAX_MCP_BODY_BYTES, MAX_TOOLS, SPEC_DEFAULT, SPEC_FALLBACK,
};
use super::*;
use crate::{Tool, ToolCtx, ToolSpec};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ---- tiny hand-rolled HTTP mock (std only) ----

struct Recorded {
    method: String,
    path: String,
    head: String,
    body: Value,
    raw: String,
}

struct MockServer {
    url: String,
    recorded: Arc<Mutex<Vec<Recorded>>>,
}

fn start_mock<F>(respond: F) -> MockServer
where
    F: Fn(&Recorded) -> (u16, String) + Send + Sync + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
    let addr = listener.local_addr().expect("mock server addr");
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&recorded);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let Some(request) = read_request(&mut stream) else {
                continue;
            };
            let (status, body) = respond(&request);
            seen.lock().unwrap().push(request);
            let reply = format!(
                "HTTP/1.1 {status} OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(reply.as_bytes());
        }
    });
    MockServer {
        url: format!("http://{addr}/mcp"),
        recorded,
    }
}

fn read_request(stream: &mut TcpStream) -> Option<Recorded> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos;
        }
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let content_length = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if name.trim().eq_ignore_ascii_case("content-length") {
                value.trim().parse::<usize>().ok()
            } else {
                None
            }
        })
        .unwrap_or(0);
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < content_length {
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    let mut parts = head.split_whitespace();
    let method = parts.next()?.to_string();
    let path = parts.next()?.to_string();
    let raw = String::from_utf8_lossy(&body).to_string();
    Some(Recorded {
        method,
        path,
        head,
        body: serde_json::from_slice(&body).unwrap_or(Value::Null),
        raw,
    })
}

fn rpc_result(request: &Recorded, result: Value) -> (u16, String) {
    (
        200,
        json!({ "jsonrpc": "2.0", "id": request.body["id"], "result": result }).to_string(),
    )
}

fn mock_tools_result(ttl_ms: Option<u64>) -> Value {
    let mut result = json!({ "tools": [
        { "name": "peek", "description": "Look at the page.",
          "inputSchema": { "type": "object", "properties": { "sel": { "type": "string" } } },
          "annotations": { "readOnlyHint": true } },
        { "name": "shout", "description": "Post a comment.\nSecond line here.",
          "inputSchema": { "type": "object", "properties": { "text": { "type": "string" } } } }
    ] });
    if let Some(ttl) = ttl_ms {
        result["_meta"] = json!({ "ttlMs": ttl });
    }
    result
}

/// Serves tools/list, tools/call (browser handle passthrough), server/discover;
/// GETs 404 so discover exercises the fallback.
fn full_responder(request: &Recorded) -> (u16, String) {
    if request.method == "GET" {
        return (404, "{}".to_string());
    }
    match request.body["method"].as_str().unwrap_or("") {
        "tools/list" => rpc_result(request, mock_tools_result(Some(60_000))),
        "tools/call" => match request.body["params"]["name"].as_str().unwrap_or("") {
            "browser_open" => rpc_result(
                request,
                json!({ "content": [{ "type": "text", "text": "browser_id: b-42" }] }),
            ),
            "browser_click" => {
                // The mock asserts receipt of the handle as an ordinary argument.
                if request.body["params"]["arguments"]["browser_id"] == json!("b-42") {
                    rpc_result(
                        request,
                        json!({ "content": [{ "type": "text", "text": "clicked" }] }),
                    )
                } else {
                    rpc_result(
                        request,
                        json!({ "isError": true,
                                "content": [{ "type": "text", "text": "no browser_id handle received" }] }),
                    )
                }
            }
            "peek" => rpc_result(
                request,
                json!({ "content": [{ "type": "text", "text": "peeked" }] }),
            ),
            "shout" => rpc_result(
                request,
                json!({ "content": [{ "type": "text", "text": "posted" }] }),
            ),
            _ => rpc_result(
                request,
                json!({ "isError": true,
                        "content": [{ "type": "text", "text": "unknown tool" }] }),
            ),
        },
        "server/discover" => rpc_result(
            request,
            json!({ "name": "mock-server", "spec": "2026-07-28" }),
        ),
        _ => (
            200,
            json!({ "jsonrpc": "2.0", "id": request.body["id"],
                    "error": { "code": -32601, "message": "method not found" } })
            .to_string(),
        ),
    }
}

fn config(url: &str, trust: McpTrust) -> McpServerConfig {
    McpServerConfig {
        name: "mock".into(),
        url: url.into(),
        spec: SPEC_DEFAULT.into(),
        auth: McpAuth::None,
        scopes: vec![],
        default_mode: None,
        trust,
    }
}

fn mcp_client(config: McpServerConfig) -> McpClient {
    McpClient::new(config).expect("test HTTP clients initialize")
}

fn approving_ctx(answer: bool) -> (ToolCtx, Arc<Mutex<Vec<String>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = Arc::clone(&seen);
    let ctx = ToolCtx::new(
        PathBuf::from("."),
        Box::new(move |description| {
            record.lock().unwrap().push(description.to_string());
            answer
        }),
    );
    (ctx, seen)
}

fn refused_url() -> String {
    // Bind then drop so the port refuses connections.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    drop(listener);
    url
}

fn count_method(mock: &MockServer, method: &str) -> usize {
    mock.recorded
        .lock()
        .unwrap()
        .iter()
        .filter(|r| r.body["method"] == json!(method))
        .count()
}

fn authorization_bearer(request: &Recorded) -> Option<&str> {
    request.head.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if name.trim().eq_ignore_ascii_case("authorization") {
            value.trim().strip_prefix("Bearer ")
        } else {
            None
        }
    })
}

// ---- §3.1 config parsing ----

#[test]
fn config_parses_example_and_applies_defaults() {
    let toml_str = r#"
[servers.playwright]
url = "http://localhost:8931/mcp"
spec = "2026-07-28"
auth = "apikey"
vault_entry = "playwright"
scopes = ["browse"]
default_mode = "snapshot"
trust = "auto"

[servers.minimal]
url = "http://localhost:9000/mcp"
"#;
    let configs = load_mcp_config(toml_str).unwrap();
    assert_eq!(configs.len(), 2);
    let minimal = &configs[0]; // sorted by name
    assert_eq!(minimal.name, "minimal");
    assert_eq!(minimal.spec, SPEC_DEFAULT);
    assert_eq!(minimal.auth, McpAuth::None);
    assert!(minimal.scopes.is_empty());
    assert_eq!(minimal.default_mode, None);
    assert_eq!(minimal.trust, McpTrust::Ask);
    let playwright = &configs[1];
    assert_eq!(playwright.name, "playwright");
    assert_eq!(playwright.url, "http://localhost:8931/mcp");
    assert_eq!(
        playwright.auth,
        McpAuth::ApiKey {
            vault_entry: "playwright".into()
        }
    );
    assert_eq!(playwright.scopes, ["browse"]);
    assert_eq!(playwright.default_mode.as_deref(), Some("snapshot"));
    assert_eq!(playwright.trust, McpTrust::Auto);
}

#[test]
fn config_ignores_unknown_keys() {
    let toml_str = r#"
future_top_level = true
[servers.a]
url = "http://localhost:1/mcp"
future_knob = "whatever"
"#;
    let configs = load_mcp_config(toml_str).unwrap();
    assert_eq!(configs.len(), 1);
    assert_eq!(configs[0].name, "a");
}

#[test]
fn config_oauth2_and_fallback_spec_parse_fine() {
    let toml_str = r#"
[servers.korvin]
url = "http://localhost:2/mcp"
spec = "2025-11-25"
auth = "oauth2"
token_url = "https://auth.example.test/token"
client_id = "korvin-client"
vault_entry = "korvin-oauth"
"#;
    let configs = load_mcp_config(toml_str).unwrap();
    assert_eq!(
        configs[0].auth,
        McpAuth::OAuth2 {
            token_url: "https://auth.example.test/token".into(),
            client_id: "korvin-client".into(),
            vault_entry: "korvin-oauth".into(),
        }
    );
    assert_eq!(configs[0].spec, SPEC_FALLBACK);
}

#[test]
fn config_unknown_trust_names_valid_values() {
    let err = load_mcp_config("[servers.a]\nurl = \"http://x/mcp\"\ntrust = \"yolo\"")
        .unwrap_err()
        .to_string();
    assert!(err.contains("unknown trust \"yolo\""), "got: {err}");
    for valid in ["auto", "ask", "block"] {
        assert!(err.contains(valid), "missing {valid} in: {err}");
    }
}

#[test]
fn config_unknown_auth_names_valid_values() {
    let err = load_mcp_config("[servers.a]\nurl = \"http://x/mcp\"\nauth = \"basic\"")
        .unwrap_err()
        .to_string();
    assert!(err.contains("unknown auth \"basic\""), "got: {err}");
    for valid in ["none", "apikey", "oauth2"] {
        assert!(err.contains(valid), "missing {valid} in: {err}");
    }
}

#[test]
fn config_unknown_spec_names_valid_values() {
    let err = load_mcp_config("[servers.a]\nurl = \"http://x/mcp\"\nspec = \"2024-01-01\"")
        .unwrap_err()
        .to_string();
    assert!(err.contains("unknown spec \"2024-01-01\""), "got: {err}");
    assert!(err.contains(SPEC_DEFAULT), "got: {err}");
    assert!(err.contains(SPEC_FALLBACK), "got: {err}");
}

#[test]
fn config_apikey_without_vault_entry_is_actionable() {
    let err = load_mcp_config("[servers.gh]\nurl = \"http://x/mcp\"\nauth = \"apikey\"")
        .unwrap_err()
        .to_string();
    assert!(err.contains("needs vault_entry"), "got: {err}");
    assert!(err.contains("nh key add gh"), "got: {err}");
}

#[test]
fn config_oauth2_without_required_fields_is_actionable() {
    let err = load_mcp_config("[servers.korvin]\nurl = \"http://x/mcp\"\nauth = \"oauth2\"")
        .unwrap_err()
        .to_string();
    for required in ["token_url", "client_id", "vault_entry"] {
        assert!(err.contains(required), "missing {required} in: {err}");
    }
    assert!(err.contains("nh key add korvin-refresh"), "got: {err}");
    assert!(err.contains("nh key add korvin-secret"), "got: {err}");
    assert!(!err.contains('\n'), "must be one line, got: {err}");
}

#[test]
fn config_missing_url_is_actionable() {
    let err = load_mcp_config("[servers.a]\ntrust = \"ask\"")
        .unwrap_err()
        .to_string();
    assert!(err.contains("mcp server \"a\": missing url"), "got: {err}");
}

#[test]
fn config_bad_toml_is_one_friendly_line() {
    let err = load_mcp_config("not = = toml").unwrap_err().to_string();
    assert!(
        err.starts_with("could not parse .nosis/mcp.toml"),
        "got: {err}"
    );
    assert!(!err.contains('\n'), "must be one line, got: {err}");
}

// ---- §3.3 statelessness (M1 exit criterion) ----

#[test]
fn stateless_invariant_no_session_no_initialize_meta_on_every_request() {
    let mock = start_mock(full_responder);
    let client = mcp_client(config(&mock.url, McpTrust::Ask));

    client.list_tools().unwrap();
    let opened = client.call_tool("browser_open", json!({})).unwrap();
    assert!(opened.contains("b-42"));
    client
        .call_tool("browser_click", json!({ "browser_id": "b-42" }))
        .unwrap();
    client.discover().unwrap();

    let recorded = mock.recorded.lock().unwrap();
    assert!(
        recorded.len() >= 5,
        "expected list + 2 calls + GET + fallback POST"
    );
    for request in recorded.iter() {
        assert!(
            !request.head.to_ascii_lowercase().contains("mcp-session-id"),
            "session header on the wire: {}",
            request.head
        );
        if request.method == "POST" {
            assert_ne!(
                request.body["method"],
                json!("initialize"),
                "initialize handshake must never be sent"
            );
            assert_eq!(request.body["jsonrpc"], json!("2.0"));
            let meta = &request.body["params"]["_meta"];
            assert_eq!(meta["protocolVersion"], json!("2026-07-28"));
            assert_eq!(meta["clientInfo"]["name"], json!("nosis-harness"));
            assert_eq!(
                meta["clientInfo"]["version"],
                json!(env!("CARGO_PKG_VERSION"))
            );
            assert_eq!(meta["capabilities"], json!({}));
        }
    }
}

#[test]
fn fallback_spec_is_echoed_in_meta() {
    let mock = start_mock(|req| rpc_result(req, mock_tools_result(None)));
    let mut cfg = config(&mock.url, McpTrust::Ask);
    cfg.spec = SPEC_FALLBACK.into();
    mcp_client(cfg).list_tools().unwrap();
    let recorded = mock.recorded.lock().unwrap();
    assert_eq!(
        recorded[0].body["params"]["_meta"]["protocolVersion"],
        json!(SPEC_FALLBACK)
    );
}

#[test]
fn handle_passes_back_as_ordinary_argument() {
    let mock = start_mock(full_responder);
    let client = mcp_client(config(&mock.url, McpTrust::Ask));
    let opened = client.call_tool("browser_open", json!({})).unwrap();
    assert_eq!(opened, "browser_id: b-42");
    // The model reads the handle and passes it back as a plain argument -
    // no session plumbing anywhere. The mock rejects the call without it.
    let clicked = client
        .call_tool("browser_click", json!({ "browser_id": "b-42" }))
        .unwrap();
    assert_eq!(clicked, "clicked");
    let recorded = mock.recorded.lock().unwrap();
    assert_eq!(
        recorded[1].body["params"]["arguments"]["browser_id"],
        json!("b-42")
    );
}

// ---- §3.2 tools/list caching ----

#[test]
fn tools_list_second_call_within_ttl_hits_cache() {
    let mock = start_mock(|req| rpc_result(req, mock_tools_result(Some(60_000))));
    let client = mcp_client(config(&mock.url, McpTrust::Ask));
    let first = client.list_tools().unwrap();
    let second = client.list_tools().unwrap();
    assert_eq!(first.len(), 2);
    assert_eq!(second.len(), 2);
    assert_eq!(first[0].name, "peek");
    assert_eq!(
        count_method(&mock, "tools/list"),
        1,
        "second call within ttlMs must hit the cache"
    );
}

#[test]
fn tools_list_ttl_zero_never_caches() {
    let mock = start_mock(|req| rpc_result(req, mock_tools_result(Some(0))));
    let client = mcp_client(config(&mock.url, McpTrust::Ask));
    client.list_tools().unwrap();
    client.list_tools().unwrap();
    assert_eq!(count_method(&mock, "tools/list"), 2);
}

#[test]
fn tools_list_absent_ttl_defaults_to_60s_cache() {
    let mock = start_mock(|req| rpc_result(req, mock_tools_result(None)));
    let client = mcp_client(config(&mock.url, McpTrust::Ask));
    client.list_tools().unwrap();
    client.list_tools().unwrap();
    assert_eq!(count_method(&mock, "tools/list"), 1);
}

#[test]
fn tools_list_extreme_ttl_is_clamped_and_cached() {
    let mock = start_mock(|req| rpc_result(req, mock_tools_result(Some(u64::MAX))));
    let client = mcp_client(config(&mock.url, McpTrust::Ask));

    let first = client.list_tools().unwrap();
    let second = client.list_tools().unwrap();

    assert_eq!(first.len(), 2);
    assert_eq!(second.len(), 2);
    assert_eq!(
        count_method(&mock, "tools/list"),
        1,
        "clamped ttlMs must still cache the tools"
    );
}

#[test]
fn oversized_mcp_response_is_rejected() {
    let mock = start_mock(|_| (200, "x".repeat(MAX_MCP_BODY_BYTES + 1)));
    let client = mcp_client(config(&mock.url, McpTrust::Ask));

    let error = client.list_tools().unwrap_err().to_string();

    assert!(error.contains("mcp response too large"), "{error}");
}

#[test]
fn tools_list_is_truncated_to_the_tool_count_cap() {
    let mock = start_mock(|request| {
        let tools = (0..MAX_TOOLS + 1)
            .map(|index| {
                json!({
                    "name": format!("tool-{index}"),
                    "description": "fixture",
                    "inputSchema": { "type": "object" }
                })
            })
            .collect::<Vec<_>>();
        rpc_result(request, json!({ "tools": tools, "_meta": { "ttlMs": 0 } }))
    });
    let client = mcp_client(config(&mock.url, McpTrust::Ask));

    let tools = client.list_tools().unwrap();

    assert_eq!(tools.len(), MAX_TOOLS);
    assert_eq!(tools.first().map(|tool| tool.name.as_str()), Some("tool-0"));
    assert_eq!(
        tools.last().map(|tool| tool.name.as_str()),
        Some("tool-511")
    );
}

#[test]
fn tools_list_cache_expires_after_ttl() {
    let mock = start_mock(|req| rpc_result(req, mock_tools_result(Some(1))));
    let client = mcp_client(config(&mock.url, McpTrust::Ask));
    client.list_tools().unwrap();
    std::thread::sleep(Duration::from_millis(25));
    client.list_tools().unwrap();
    assert_eq!(count_method(&mock, "tools/list"), 2);
}

// ---- §3.2 call_tool result mapping ----

#[test]
fn call_tool_joins_text_and_marks_non_text_blocks() {
    let mock = start_mock(|req| {
        rpc_result(
            req,
            json!({ "content": [
                { "type": "text", "text": "a" },
                { "type": "image", "data": "…" },
                { "type": "text", "text": "b" }
            ] }),
        )
    });
    let client = mcp_client(config(&mock.url, McpTrust::Ask));
    assert_eq!(
        client.call_tool("peek", json!({})).unwrap(),
        "a\n[image block]\nb"
    );
}

#[test]
fn call_tool_is_error_becomes_one_line_err() {
    let mock = start_mock(|req| {
        rpc_result(
            req,
            json!({ "isError": true,
                    "content": [{ "type": "text", "text": "boom\nhappened" }] }),
        )
    });
    let client = mcp_client(config(&mock.url, McpTrust::Ask));
    let err = client.call_tool("peek", json!({})).unwrap_err().to_string();
    assert_eq!(err, "boom happened");
}

#[test]
fn jsonrpc_error_is_a_friendly_one_liner() {
    let mock = start_mock(|req| {
        (
            200,
            json!({ "jsonrpc": "2.0", "id": req.body["id"],
                    "error": { "code": -32000, "message": "kaboom" } })
            .to_string(),
        )
    });
    let client = mcp_client(config(&mock.url, McpTrust::Ask));
    let err = client.list_tools().unwrap_err().to_string();
    assert_eq!(err, "server error: kaboom");
}

// ---- §3.2 discovery ----

#[test]
fn discover_reads_well_known_business_card() {
    let mock = start_mock(|req| {
        if req.method == "GET" && req.path.ends_with("/.well-known/mcp.json") {
            (200, json!({ "name": "mock-server" }).to_string())
        } else {
            (500, "{}".to_string())
        }
    });
    let client = mcp_client(config(&mock.url, McpTrust::Ask));
    let card = client.discover().unwrap();
    assert_eq!(card["name"], json!("mock-server"));
    let recorded = mock.recorded.lock().unwrap();
    assert_eq!(recorded.len(), 1, "well-known hit must need no POST");
    assert_eq!(recorded[0].method, "GET");
    assert_eq!(recorded[0].path, "/mcp/.well-known/mcp.json");
}

#[test]
fn discover_falls_back_to_server_discover_post() {
    let mock = start_mock(full_responder); // GETs 404
    let client = mcp_client(config(&mock.url, McpTrust::Ask));
    let card = client.discover().unwrap();
    assert_eq!(card["name"], json!("mock-server"));
    let recorded = mock.recorded.lock().unwrap();
    assert_eq!(recorded.len(), 2);
    assert_eq!(recorded[0].method, "GET");
    assert_eq!(recorded[1].body["method"], json!("server/discover"));
}

#[test]
fn discover_unreachable_is_one_friendly_error() {
    let client = mcp_client(config(&refused_url(), McpTrust::Ask));
    let err = client.discover().unwrap_err().to_string();
    assert!(
        err.contains("unreachable - check the url in .nosis/mcp.toml"),
        "got: {err}"
    );
}

// ---- §3.4 auth ----

#[test]
fn apikey_bearer_comes_from_vault_env_fallback() {
    std::env::set_var("NH_MCP_BEARER_TEST_KEY", "sk-test-0000-mcp");
    let mock = start_mock(|req| rpc_result(req, mock_tools_result(None)));
    let mut cfg = config(&mock.url, McpTrust::Ask);
    cfg.auth = McpAuth::ApiKey {
        vault_entry: "mcp-bearer-test".into(),
    };
    mcp_client(cfg).list_tools().unwrap();
    std::env::remove_var("NH_MCP_BEARER_TEST_KEY");
    let recorded = mock.recorded.lock().unwrap();
    assert!(
        recorded[0]
            .head
            .to_ascii_lowercase()
            .contains("authorization: bearer sk-test-0000-mcp"),
        "missing bearer in: {}",
        recorded[0].head
    );
}

#[test]
fn auth_none_sends_no_authorization_header() {
    let mock = start_mock(|req| rpc_result(req, mock_tools_result(None)));
    mcp_client(config(&mock.url, McpTrust::Ask))
        .list_tools()
        .unwrap();
    let recorded = mock.recorded.lock().unwrap();
    assert!(
        !recorded[0]
            .head
            .to_ascii_lowercase()
            .contains("authorization:"),
        "unexpected auth header in: {}",
        recorded[0].head
    );
}

#[test]
fn apikey_missing_key_is_actionable() {
    std::env::remove_var("NH_MCP_MISSING_TEST_KEY");
    let mut cfg = config("http://127.0.0.1:1/mcp", McpTrust::Ask);
    cfg.auth = McpAuth::ApiKey {
        vault_entry: "mcp-missing-test".into(),
    };
    let err = mcp_client(cfg).list_tools().unwrap_err().to_string();
    assert!(err.contains("nh key add mcp-missing-test"), "got: {err}");
}

#[test]
fn oauth_refresh_refuses_redirects_without_replaying_the_form() {
    const REFRESH_ENV: &str = "NH_OAUTH_REDIRECT_TEST_REFRESH_KEY";
    const SECRET_ENV: &str = "NH_OAUTH_REDIRECT_TEST_SECRET_KEY";
    std::env::set_var(REFRESH_ENV, "opaque-refresh-value");
    std::env::set_var(SECRET_ENV, "opaque-client-value");

    let sink = TcpListener::bind("127.0.0.1:0").unwrap();
    sink.set_nonblocking(true).unwrap();
    let sink_url = format!("http://{}/capture", sink.local_addr().unwrap());
    let redirect = TcpListener::bind("127.0.0.1:0").unwrap();
    let token_url = format!("http://{}/token", redirect.local_addr().unwrap());
    let redirect_thread = std::thread::spawn(move || {
        let (mut stream, _) = redirect.accept().unwrap();
        let request = read_request(&mut stream).expect("oauth token request");
        assert!(request.raw.contains("opaque-refresh-value"));
        assert!(request.raw.contains("opaque-client-value"));
        let reply = format!(
            "HTTP/1.1 307 Temporary Redirect\r\nlocation: {sink_url}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
        );
        stream.write_all(reply.as_bytes()).unwrap();
    });

    let mut cfg = config("http://127.0.0.1:1/mcp", McpTrust::Ask);
    cfg.auth = McpAuth::OAuth2 {
        token_url,
        client_id: "redirect-test-client".into(),
        vault_entry: "oauth-redirect-test".into(),
    };
    let error = mcp_client(cfg).list_tools().unwrap_err().to_string();
    redirect_thread.join().unwrap();

    assert!(
        matches!(sink.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
        "the OAuth form was replayed to the redirect target"
    );
    assert!(error.contains("oauth refresh failed"), "got: {error}");

    std::env::remove_var(REFRESH_ENV);
    std::env::remove_var(SECRET_ENV);
}

#[test]
fn oauth2_refreshes_on_absence_expiry_and_one_401_retry() {
    const REFRESH_ENV: &str = "NH_MOCK_OAUTH_REFRESH_KEY";
    const SECRET_ENV: &str = "NH_MOCK_OAUTH_SECRET_KEY";
    std::env::set_var(REFRESH_ENV, "refresh-token-fake");
    std::env::set_var(SECRET_ENV, "csk-secret-fake");

    let mint_count = Arc::new(AtomicU64::new(0));
    let valid_token = Arc::new(Mutex::new(String::new()));
    let token_count = Arc::clone(&mint_count);
    let token_valid = Arc::clone(&valid_token);
    let token_server = start_mock(move |_| {
        let number = token_count.fetch_add(1, Ordering::SeqCst) + 1;
        let access = format!("fresh-access-{number}");
        *token_valid.lock().unwrap() = access.clone();
        (
            200,
            json!({
                "access_token": access,
                "expires_in": 3_600,
                "token_type": "Bearer"
            })
            .to_string(),
        )
    });

    let mcp_valid = Arc::clone(&valid_token);
    let mcp_server = start_mock(move |request| {
        let expected = mcp_valid.lock().unwrap();
        if authorization_bearer(request) == Some(expected.as_str()) {
            rpc_result(
                request,
                json!({ "content": [{ "type": "text", "text": "ok" }] }),
            )
        } else {
            (401, "{}".to_string())
        }
    });

    let mut cfg = config(&mcp_server.url, McpTrust::Ask);
    cfg.auth = McpAuth::OAuth2 {
        token_url: token_server.url.clone(),
        client_id: "cid-test".into(),
        vault_entry: "mock-oauth".into(),
    };
    cfg.scopes = vec!["mcp".into()];
    let client = mcp_client(cfg);

    assert_eq!(client.call_tool("peek", json!({})).unwrap(), "ok");
    assert_eq!(mint_count.load(Ordering::SeqCst), 1);

    client.expire_oauth_for_test().unwrap();
    assert_eq!(client.call_tool("peek", json!({})).unwrap(), "ok");
    assert_eq!(mint_count.load(Ordering::SeqCst), 2);

    *valid_token.lock().unwrap() = "server-invalidated".into();
    assert_eq!(client.call_tool("peek", json!({})).unwrap(), "ok");
    assert_eq!(
        mint_count.load(Ordering::SeqCst),
        3,
        "a rejected cached token must refresh exactly once"
    );

    std::env::remove_var(REFRESH_ENV);
    std::env::remove_var(SECRET_ENV);

    let mcp_recorded = mcp_server.recorded.lock().unwrap();
    let bearers: Vec<_> = mcp_recorded
        .iter()
        .map(|request| authorization_bearer(request).unwrap_or("").to_string())
        .collect();
    assert_eq!(
        bearers,
        [
            "fresh-access-1",
            "fresh-access-2",
            "fresh-access-2",
            "fresh-access-3"
        ],
        "the 401 path must send stale once, refresh, then retry once with fresh"
    );
    for request in mcp_recorded.iter() {
        for line in request.head.lines() {
            if line.contains("fresh-access-") {
                assert!(
                    line.to_ascii_lowercase().starts_with("authorization:"),
                    "access token escaped Authorization: {line}"
                );
            }
            let lower = line.to_ascii_lowercase();
            if lower.starts_with("mcp-") || lower.starts_with("x-mcp-") {
                assert!(!line.contains("fresh-access-"), "token leaked into {line}");
            }
        }
    }
    drop(mcp_recorded);

    let token_recorded = token_server.recorded.lock().unwrap();
    assert_eq!(token_recorded.len(), 3);
    let encoded_resource = mcp_server.url.replace(':', "%3A").replace('/', "%2F");
    for request in token_recorded.iter() {
        assert_eq!(request.method, "POST");
        assert!(request.raw.contains("grant_type=refresh_token"));
        assert!(request.raw.contains("refresh_token=refresh-token-fake"));
        assert!(request.raw.contains("client_id=cid-test"));
        assert!(request.raw.contains("client_secret=csk-secret-fake"));
        assert!(request.raw.contains("scope=mcp"));
        assert!(
            request
                .raw
                .contains(&format!("resource={encoded_resource}")),
            "missing RFC 8707 resource in: {}",
            request.raw
        );
        assert!(authorization_bearer(request).is_none());
        assert!(!request.raw.contains("fresh-access-"));
    }
}

#[test]
fn concurrent_oauth_refreshes_coalesce_to_one_token_post() {
    const REFRESH_ENV: &str = "NH_COALESCED_OAUTH_REFRESH_KEY";
    const SECRET_ENV: &str = "NH_COALESCED_OAUTH_SECRET_KEY";
    std::env::set_var(REFRESH_ENV, "refresh-token-coalesced-fake");
    std::env::set_var(SECRET_ENV, "csk-secret-coalesced-fake");

    let mint_count = Arc::new(AtomicU64::new(0));
    let token_count = Arc::clone(&mint_count);
    let token_server = start_mock(move |_| {
        token_count.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(50));
        (
            200,
            json!({
                "access_token": "fresh-access-coalesced",
                "expires_in": 3_600
            })
            .to_string(),
        )
    });
    let mcp_server = start_mock(|request| rpc_result(request, Value::Null));
    let mut cfg = config(&mcp_server.url, McpTrust::Ask);
    cfg.auth = McpAuth::OAuth2 {
        token_url: token_server.url.clone(),
        client_id: "cid-coalesced-test".into(),
        vault_entry: "coalesced-oauth".into(),
    };
    let client = Arc::new(mcp_client(cfg));
    let barrier = Arc::new(std::sync::Barrier::new(3));
    let mut handles = Vec::new();
    for _ in 0..2 {
        let client = Arc::clone(&client);
        let barrier = Arc::clone(&barrier);
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            client.oauth_access_token()
        }));
    }
    barrier.wait();
    for handle in handles {
        assert_eq!(
            handle.join().unwrap().unwrap().as_str(),
            "fresh-access-coalesced"
        );
    }
    std::env::remove_var(REFRESH_ENV);
    std::env::remove_var(SECRET_ENV);

    assert_eq!(
        mint_count.load(Ordering::SeqCst),
        1,
        "concurrent refreshes must share the first minted token"
    );
}

#[test]
fn oauth2_refresh_failure_is_one_secret_free_actionable_line() {
    const REFRESH_ENV: &str = "NH_MOCK_OAUTH_FAIL_REFRESH_KEY";
    const SECRET_ENV: &str = "NH_MOCK_OAUTH_FAIL_SECRET_KEY";
    const REFRESH: &str = "refresh-token-failure-fake";
    const SECRET: &str = "csk-secret-failure-fake";
    std::env::set_var(REFRESH_ENV, REFRESH);
    std::env::set_var(SECRET_ENV, SECRET);

    let token_server = start_mock(|_| {
        (
            500,
            json!({ "error": "invalid", "detail": REFRESH, "secret": SECRET }).to_string(),
        )
    });
    let mcp_server = start_mock(|request| rpc_result(request, Value::Null));
    let mut cfg = config(&mcp_server.url, McpTrust::Ask);
    cfg.auth = McpAuth::OAuth2 {
        token_url: token_server.url.clone(),
        client_id: "cid-failure-test".into(),
        vault_entry: "mock-oauth-fail".into(),
    };

    let err = mcp_client(cfg)
        .call_tool("peek", json!({}))
        .unwrap_err()
        .to_string();
    std::env::remove_var(REFRESH_ENV);
    std::env::remove_var(SECRET_ENV);

    assert_eq!(
        err,
        "mcp server \"mock\": oauth refresh failed - re-authorize with `nh key add mock-oauth-fail-refresh` and `nh key add mock-oauth-fail-secret` (or check token_url in .nosis/mcp.toml)"
    );
    assert!(!err.contains('\n'));
    assert!(!err.contains(REFRESH));
    assert!(!err.contains(SECRET));
    assert!(mcp_server.recorded.lock().unwrap().is_empty());
    let token_recorded = token_server.recorded.lock().unwrap();
    assert_eq!(token_recorded.len(), 1);
    assert!(authorization_bearer(&token_recorded[0]).is_none());
}

// ---- §3.5 outbound header lint ----

#[test]
fn header_lint_refuses_secret_shaped_x_mcp_value() {
    let err = lint_headers(&[("x-mcp-token".into(), "sk-abcdefghijkl".into())])
        .unwrap_err()
        .to_string();
    assert!(err.contains("x-mcp-token"), "got: {err}");
    assert!(err.contains("looks like a secret"), "got: {err}");
}

#[test]
fn header_lint_covers_mcp_prefix_all_shapes_case_insensitively() {
    assert!(lint_headers(&[("Mcp-Name".into(), "eyJhbGciOi.payload-x.sig-y".into())]).is_err());
    assert!(lint_headers(&[("X-MCP-Auth".into(), "csk-abcdefghijkl".into())]).is_err());
    assert!(lint_headers(&[("MCP-TRACE".into(), "sk-abcdefghijkl".into())]).is_err());
}

#[test]
fn header_lint_allows_authorization_and_plain_values() {
    assert!(lint_headers(&[("authorization".into(), "Bearer sk-abcdefghijkl".into())]).is_ok());
    assert!(lint_headers(&[("x-mcp-mode".into(), "snapshot".into())]).is_ok());
    assert!(lint_headers(&[("content-type".into(), "application/json".into())]).is_ok());
}

// ---- §3.6 tool adapters ----

#[test]
fn adapters_are_namespaced_and_described() {
    let mock = start_mock(full_responder);
    let set = mcp_tools(&[config(&mock.url, McpTrust::Ask)], &|_| true);
    assert!(set.warnings.is_empty(), "warnings: {:?}", set.warnings);
    let specs: Vec<ToolSpec> = set.tools.iter().map(|t| t.spec()).collect();
    assert_eq!(
        specs.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
        ["mcp__mock__peek", "mcp__mock__shout"]
    );
    assert_eq!(specs[0].description, "[MCP mock] Look at the page.");
    assert_eq!(
        specs[0].parameters["properties"]["sel"]["type"],
        json!("string")
    );
}

#[test]
fn adapters_sanitize_untrusted_description_and_schema_strings() {
    let mock = start_mock(|request| {
        rpc_result(
            request,
            json!({
                "tools": [{
                    "name": "tainted",
                    "description": "safe\x1b[2K\rhidden\u{200b}\u{e0001}",
                    "inputSchema": {
                        "type": "object",
                        "description": "arg\x1b[31m\u{200d}",
                        "properties": {}
                    }
                }]
            }),
        )
    });
    let set = mcp_tools(&[config(&mock.url, McpTrust::Ask)], &|_| true);
    assert!(set.warnings.is_empty(), "warnings: {:?}", set.warnings);
    let spec = set.tools[0].spec();
    assert_eq!(spec.description, "[MCP mock] safe\\u{1b}[2K\\rhidden");
    assert_eq!(spec.parameters["description"], json!("arg\\u{1b}[31m"));
    assert!(!spec.description.chars().any(char::is_control));
}

#[test]
fn trust_ask_gates_every_call_and_denial_is_ok_shaped() {
    let mock = start_mock(full_responder);
    let set = mcp_tools(&[config(&mock.url, McpTrust::Ask)], &|_| true);
    let shout = set
        .tools
        .iter()
        .find(|t| t.spec().name == "mcp__mock__shout")
        .unwrap();

    let (ctx, seen) = approving_ctx(true);
    let out = shout.execute(json!({ "text": "hi" }), &ctx).unwrap();
    assert_eq!(out, "posted");
    assert_eq!(
        seen.lock().unwrap().as_slice(),
        ["mcp mock shout {\"text\":\"hi\"}"]
    );

    let (ctx, _) = approving_ctx(false);
    let out = shout.execute(json!({ "text": "hi" }), &ctx).unwrap();
    assert_eq!(out, "user denied: mcp mock shout");
    assert_eq!(
        count_method(&mock, "tools/call"),
        1,
        "denied call must never reach the server"
    );
}

#[test]
fn adapter_result_is_bounded_and_scrubbed_at_the_egress_choke_point() {
    const LITERAL: &str = "mcp-literal-fixture-abc123";
    let payload = format!("{LITERAL}\n{}\nsk-fixture-abc123", "x".repeat(40_000));
    let mock = start_mock(move |request| match request.body["method"].as_str() {
        Some("tools/list") => rpc_result(request, mock_tools_result(None)),
        Some("tools/call") => rpc_result(
            request,
            json!({ "content": [{ "type": "text", "text": payload }] }),
        ),
        _ => rpc_result(request, Value::Null),
    });
    let set = mcp_tools(&[config(&mock.url, McpTrust::Auto)], &|_| true);
    let peek = set
        .tools
        .iter()
        .find(|tool| tool.spec().name == "mcp__mock__peek")
        .unwrap();
    let (ctx, _) = approving_ctx(true);
    let ctx = ctx.with_scrubber(nh_vault::Scrubber::new(vec![LITERAL.to_string()]));

    let result = peek.execute(json!({}), &ctx).unwrap();

    assert!(result.contains("chars elided; digest "), "got: {result}");
    assert!(!result.contains(LITERAL), "literal leaked: {result}");
    assert!(
        !result.contains("sk-fixture-abc123"),
        "shape leaked: {result}"
    );
    assert!(result.matches("[REDACTED]").count() >= 2, "got: {result}");
    assert!(result.chars().count() <= crate::MAX_TOOL_RESULT_CHARS + 100);
}

#[test]
fn send_law_block_stops_mcp_egress_before_trust_or_approval() {
    let mock = start_mock(full_responder);
    let set = mcp_tools(&[config(&mock.url, McpTrust::Auto)], &|_| true);
    let peek = set
        .tools
        .iter()
        .find(|tool| tool.spec().name == "mcp__mock__peek")
        .unwrap();
    let (ctx, approvals) = approving_ctx(true);
    let seen_targets = Arc::new(Mutex::new(Vec::new()));
    let targets = Arc::clone(&seen_targets);
    let ctx = ctx.with_guard(Box::new(move |access| match access {
        crate::Access::Send(target) => {
            targets.lock().unwrap().push((*target).to_string());
            crate::Guard::Block("egress denied".into())
        }
        _ => crate::Guard::Allow,
    }));

    let result = peek.execute(json!({}), &ctx).unwrap();

    assert_eq!(result, "blocked by law: egress denied");
    assert_eq!(
        seen_targets.lock().unwrap().as_slice(),
        ["127.0.0.1"],
        "send policy must receive the bare host, never the full URL"
    );
    assert!(approvals.lock().unwrap().is_empty());
    assert_eq!(count_method(&mock, "tools/call"), 0);
}

#[test]
fn send_law_ask_reuses_the_existing_approval_for_auto_read_only() {
    let mock = start_mock(full_responder);
    let set = mcp_tools(&[config(&mock.url, McpTrust::Auto)], &|_| true);
    let peek = set
        .tools
        .iter()
        .find(|tool| tool.spec().name == "mcp__mock__peek")
        .unwrap();
    let (ctx, approvals) = approving_ctx(true);
    let ctx = ctx.with_guard(Box::new(|access| match access {
        crate::Access::Send(_) => crate::Guard::Ask,
        _ => crate::Guard::Allow,
    }));

    assert_eq!(peek.execute(json!({}), &ctx).unwrap(), "peeked");
    assert_eq!(approvals.lock().unwrap().len(), 1);
    assert_eq!(count_method(&mock, "tools/call"), 1);
}

#[test]
fn trust_auto_read_only_skips_gate_mutating_still_asks() {
    let mock = start_mock(full_responder);
    let set = mcp_tools(&[config(&mock.url, McpTrust::Auto)], &|_| true);
    let peek = set
        .tools
        .iter()
        .find(|t| t.spec().name == "mcp__mock__peek")
        .unwrap();
    let shout = set
        .tools
        .iter()
        .find(|t| t.spec().name == "mcp__mock__shout")
        .unwrap();

    // readOnlyHint == true: runs even when the gate would deny.
    let (ctx, seen) = approving_ctx(false);
    assert_eq!(peek.execute(json!({}), &ctx).unwrap(), "peeked");
    assert!(
        seen.lock().unwrap().is_empty(),
        "read-only must skip the gate"
    );

    // No read-only annotation: still asks, at every autonomy level.
    let (ctx, seen) = approving_ctx(false);
    let out = shout.execute(json!({ "text": "hi" }), &ctx).unwrap();
    assert_eq!(out, "user denied: mcp mock shout");
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[test]
fn trust_block_offers_no_tools_and_never_contacts_the_server() {
    let mock = start_mock(full_responder);
    let set = mcp_tools(&[config(&mock.url, McpTrust::Block)], &|_| true);
    assert!(set.tools.is_empty(), "blocked tools must not be offered");
    assert!(set.warnings.is_empty());
    assert!(
        mock.recorded.lock().unwrap().is_empty(),
        "blocked server must never be contacted"
    );
}

#[test]
fn discovery_send_block_offers_no_tools_and_never_contacts_the_server() {
    let mock = start_mock(full_responder);
    let set = mcp_tools(&[config(&mock.url, McpTrust::Ask)], &|host| {
        assert_eq!(host, "127.0.0.1");
        false
    });

    assert!(set.tools.is_empty());
    assert_eq!(set.warnings.len(), 1);
    assert!(
        set.warnings[0].contains("blocked by law"),
        "{:?}",
        set.warnings
    );
    assert!(
        mock.recorded.lock().unwrap().is_empty(),
        "send-blocked discovery must never contact the server"
    );
}

#[test]
fn discovery_unparseable_host_offers_no_tools_without_network_contact() {
    let mut cfg = config("not a url", McpTrust::Ask);
    cfg.name = "broken-url".into();

    let set = mcp_tools(&[cfg], &|_| true);

    assert!(set.tools.is_empty());
    assert_eq!(set.warnings.len(), 1);
    assert!(
        set.warnings[0].contains("could not parse a host"),
        "{:?}",
        set.warnings
    );
}

#[test]
fn blocked_adapter_execute_names_the_fix() {
    // Defense in depth: even a directly-built Block adapter refuses.
    let adapter = McpToolAdapter {
        server: "mock".into(),
        trust: McpTrust::Block,
        entry: ToolEntry {
            info: McpToolInfo {
                name: "shout".into(),
                description: String::new(),
                input_schema: json!({ "type": "object" }),
            },
            read_only: false,
        },
        client: Arc::new(mcp_client(config(
            "http://127.0.0.1:1/mcp",
            McpTrust::Block,
        ))),
    };
    let (ctx, seen) = approving_ctx(true);
    let out = adapter.execute(json!({}), &ctx).unwrap();
    assert_eq!(
        out,
        "blocked by .nosis/mcp.toml (trust = \"block\") - set trust = \"ask\" to enable"
    );
    assert!(seen.lock().unwrap().is_empty());
}

#[test]
fn failing_server_contributes_warning_not_failure() {
    let mut cfg = config(&refused_url(), McpTrust::Ask);
    cfg.name = "downbeat".into();
    let set = mcp_tools(&[cfg], &|_| true);
    assert!(set.tools.is_empty());
    assert_eq!(set.warnings.len(), 1);
    assert!(
        set.warnings[0].contains("mcp server \"downbeat\""),
        "got: {}",
        set.warnings[0]
    );
}

#[test]
fn startup_tools_list_uses_the_short_discovery_deadline() {
    let mock = start_mock(|request| {
        std::thread::sleep(Duration::from_secs(1));
        rpc_result(request, mock_tools_result(None))
    });
    let started = Instant::now();

    let set = mcp_tools(&[config(&mock.url, McpTrust::Ask)], &|_| true);

    assert!(set.tools.is_empty());
    assert_eq!(set.warnings.len(), 1);
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "startup discovery used the live-call timeout"
    );
}

#[test]
fn approval_summary_discloses_the_hidden_remainder() {
    let long = "x".repeat(700);
    let summary = args_one_line(&json!({ "data": long }));
    assert!(summary.contains("… (+"), "{summary}");
    assert!(summary.ends_with("more chars)"), "{summary}");
    assert!(!summary.contains('\n'));
}

#[test]
fn approval_summary_short_args_are_shown_whole() {
    let summary = args_one_line(&json!({ "a": 1 }));
    assert!(!summary.contains("more chars"), "{summary}");
    assert!(!summary.contains('…'), "{summary}");
}

#[test]
fn mcp_client_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<McpClient>();
}

#[test]
fn poisoned_tool_cache_returns_an_error_instead_of_panicking() {
    let client = Arc::new(mcp_client(config(&refused_url(), McpTrust::Ask)));
    let poison = Arc::clone(&client);
    let panicked = std::thread::spawn(move || {
        let _guard = poison.cache.lock().unwrap();
        panic!("poison the cache for this test");
    })
    .join();
    assert!(panicked.is_err());

    let error = client.list_tools().unwrap_err().to_string();
    assert!(error.contains("tool cache"), "{error}");
    assert!(error.contains("internal panic"), "{error}");
}

#[test]
fn poisoned_oauth_state_returns_an_error_instead_of_panicking() {
    let mut cfg = config(&refused_url(), McpTrust::Ask);
    cfg.auth = McpAuth::OAuth2 {
        token_url: "https://auth.example.invalid/token".into(),
        client_id: "client".into(),
        vault_entry: "mcp-test".into(),
    };
    let client = Arc::new(mcp_client(cfg));
    let poison = Arc::clone(&client);
    let panicked = std::thread::spawn(move || {
        let _guard = poison.oauth.lock().unwrap();
        panic!("poison OAuth state for this test");
    })
    .join();
    assert!(panicked.is_err());

    let error = client.oauth_access_token().unwrap_err().to_string();
    assert!(error.contains("OAuth state"), "{error}");
    assert!(error.contains("internal panic"), "{error}");
}
