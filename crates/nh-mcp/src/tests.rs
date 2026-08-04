use crate::fleet_tools::preflight_fleet_run;
use crate::response::scrub_json;
use crate::route_tools::{route_cost_at, why_at};
use chrono::{TimeZone as _, Utc};
use std::fs::File;
use std::io::{Read as _, Write as _};
use std::net::{Shutdown, TcpStream};
use std::path::Path;

use super::*;

const METER_CATALOG: &str = r#"
    [routes.cheap]
    provider = "fixture"
    model_id = "cheap"
    base_url = "https://example.invalid"
    wire = "openai"
    vault_entry = "fixture"
    class = "api"
    context = 100000
    thinking_dialect = "none"
    [routes.cheap.price]
    currency = "CNY"
    unit = "per_million_tokens"
    cache_hit = 0.1
    cache_miss = 1.0
    output = 2.0
    price_confidence = "confirmed"

    [routes.expensive]
    provider = "fixture"
    model_id = "expensive"
    base_url = "https://example.invalid"
    wire = "openai"
    vault_entry = "fixture"
    class = "api"
    context = 100000
    thinking_dialect = "glm-hm"
    [routes.expensive.price]
    currency = "CNY"
    unit = "per_million_tokens"
    cache_hit = 0.2
    cache_miss = 4.0
    output = 8.0
    price_confidence = "reported"

    [routes.too-small]
    provider = "fixture"
    model_id = "too-small"
    base_url = "https://example.invalid"
    wire = "openai"
    vault_entry = "fixture"
    class = "api"
    context = 10
    [routes.too-small.price]
    currency = "CNY"
    unit = "per_million_tokens"
    cache_hit = 0.01
    cache_miss = 0.01
    output = 0.01
    price_confidence = "confirmed"
"#;

fn test_server() -> (tempfile::TempDir, McpServer) {
    let catalog =
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../catalog.toml")).to_string();
    test_server_with_catalog(&catalog, "deepseek-v4-flash")
}

fn test_server_with_catalog(catalog: &str, default_route: &str) -> (tempfile::TempDir, McpServer) {
    let root = tempfile::tempdir().unwrap();
    let law = nh_law::load(root.path(), &nh_law::LoadOptions { cli_autonomy: None });
    let server = McpServer::start(ServeConfig {
        addr: "127.0.0.1:0".parse().unwrap(),
        catalog: catalog.to_string(),
        law,
        default_route: default_route.into(),
        run_root: root.path().to_path_buf(),
        token: None,
        max_workers: 1,
    })
    .unwrap();
    (root, server)
}

fn test_runtime(root: &Path, catalog: &str) -> Runtime {
    let law = nh_law::load(root, &nh_law::LoadOptions { cli_autonomy: None });
    Runtime {
        config: Arc::new(ServeConfig {
            addr: "127.0.0.1:0".parse().unwrap(),
            catalog: catalog.to_string(),
            law,
            default_route: "cheap".into(),
            run_root: root.to_path_buf(),
            token: None,
            max_workers: 1,
        }),
        token: nh_vault::secret("fixture-token"),
        token_generated: false,
        scrubber: nh_vault::Scrubber::new(vec!["fixture-literal".into()]),
        active_runs: Arc::new(AtomicUsize::new(0)),
    }
}

fn response_value(response: &str) -> Value {
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .expect("HTTP response has a body");
    serde_json::from_str(body).expect("HTTP response body is JSON")
}

#[test]
fn banner_never_prints_a_caller_supplied_token() {
    let root = tempfile::tempdir().unwrap();
    let runtime = test_runtime(root.path(), METER_CATALOG);
    let lines = banner_lines("127.0.0.1:9841".parse().unwrap(), &runtime);

    assert!(lines[1].contains("configured Bearer token"));
    assert!(!lines
        .iter()
        .any(|line| line.contains(runtime.token.as_str())));
}

#[test]
fn fleet_preflight_refuses_an_unapproved_route_origin_before_key_access() {
    let root = tempfile::tempdir().unwrap();
    let resolver = nh_routes::RouteResolver::from_toml(
        r#"
        [routes.scoped]
        provider = "fixture"
        model_id = "fixture-model"
        base_url = "https://api.deepseek.com:8443/v1"
        wire = "openai"
        vault_entry = "deepseek"
        "#,
    )
    .unwrap();
    let law = nh_law::load(root.path(), &nh_law::LoadOptions { cli_autonomy: None });
    let tasks = [nh_fleet::TaskSpec {
        id: Some("origin-check".into()),
        task: "fixture task".into(),
        model: Some("scoped".into()),
        defer_offpeak: None,
        backend: Some(nh_fleet::Backend::Native),
    }];

    let error = preflight_fleet_run(&resolver, "scoped", &tasks, &law)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("not approved for https://api.deepseek.com:8443"),
        "{error}"
    );
}

fn raw_post(
    addr: SocketAddr,
    host: &str,
    origin: Option<&str>,
    token: Option<&str>,
    body: &Value,
) -> String {
    let body = body.to_string();
    raw_request(addr, "POST", "/mcp", host, origin, token, &body)
}

fn raw_request(
    addr: SocketAddr,
    method: &str,
    path: &str,
    host: &str,
    origin: Option<&str>,
    token: Option<&str>,
    body: &str,
) -> String {
    let mut headers = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    if let Some(origin) = origin {
        headers.push_str(&format!("Origin: {origin}\r\n"));
    }
    if let Some(token) = token {
        headers.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    let request = format!("{headers}\r\n{body}");
    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    stream.shutdown(Shutdown::Write).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

fn tools_call(name: &str, arguments: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": { "name": name, "arguments": arguments }
    })
}

fn write_fleet_ledger(root: &Path, run_id: &str, events: &[Value]) {
    let run_dir = root.join(".nosis").join("fleet").join(run_id);
    std::fs::create_dir_all(&run_dir).unwrap();
    let mut text = events
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    text.push('\n');
    std::fs::write(run_dir.join("ledger.jsonl"), text).unwrap();
}

#[test]
fn why_matches_resolver_cost_and_rejection_trace_without_cold_savings_claim() {
    let root = tempfile::tempdir().unwrap();
    let runtime = test_runtime(root.path(), METER_CATALOG);
    let at = Utc.with_ymd_and_hms(2026, 7, 20, 12, 0, 0).unwrap();
    let arguments = json!({ "prompt_tokens": 1_000, "output_tokens": 100 });

    let result = why_at(&arguments, &runtime, at);
    let resolver = nh_routes::RouteResolver::from_toml(METER_CATALOG).unwrap();
    let allowed = resolver.available();
    let allowed_refs: Vec<&str> = allowed.iter().map(String::as_str).collect();
    let (route, trace) = resolver
        .resolve_capable(1_000, 100, &allowed_refs, at)
        .unwrap();
    let quote = route.price_at(at).unwrap();
    let actual = nh_routes::cost_of(&quote, 1_000, 0, 100).unwrap();
    let naive = resolver.naive_cost(&route, 1_000, 0, 100, at).unwrap();
    let expected_saved = nh_routes::saved_pct(actual, naive.no_cache);

    assert_eq!(result["structuredContent"]["route"]["id"], route.id());
    assert_eq!(
        result["structuredContent"]["cost"]["value"].as_f64(),
        Some(actual)
    );
    assert_eq!(
        result["structuredContent"]["rejected"]
            .as_array()
            .unwrap()
            .len(),
        trace.rejections.len()
    );
    assert_eq!(expected_saved, None);
    assert!(result["structuredContent"].get("savings").is_none());
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("\u{a5}0.0012 (est) | price confirmed"),
        "{text}"
    );
    assert_eq!(result["structuredContent"]["cost"]["estimated"], true);
    assert_eq!(
        result["structuredContent"]["cost"]["price_confidence"],
        "confirmed"
    );
    assert!(!text.contains("saved"), "{text}");
}

#[test]
fn route_cost_matches_quote_and_omits_usd_when_fx_absent_or_stale() {
    let root = tempfile::tempdir().unwrap();
    let runtime = test_runtime(root.path(), METER_CATALOG);
    let at = Utc.with_ymd_and_hms(2026, 7, 20, 12, 0, 0).unwrap();
    let arguments = json!({
        "model": "cheap",
        "prompt_tokens": 1_000,
        "cached_tokens": 600,
        "output_tokens": 100
    });

    let result = route_cost_at(&arguments, &runtime, at);
    let resolver = nh_routes::RouteResolver::from_toml(METER_CATALOG).unwrap();
    let quote = resolver.resolve("cheap").unwrap().price_at(at).unwrap();
    assert_eq!(
        result["structuredContent"]["quote"]["cache_hit"].as_f64(),
        Some(quote.cache_hit)
    );
    assert_eq!(
        result["structuredContent"]["quote"]["cache_miss"].as_f64(),
        Some(quote.cache_miss)
    );
    assert_eq!(
        result["structuredContent"]["quote"]["confidence"],
        quote.confidence.as_str()
    );
    assert!(result["structuredContent"]["quote"].get("stale").is_none());
    assert!(result["structuredContent"]["cost"]
        .get("usd_approx")
        .is_none());

    let stale_catalog = format!(
        r#"
        [fx]
        usd_per_cny = 0.139
        valid_until = "2020-01-01"
        price_confidence = "reported"
        {METER_CATALOG}
        "#
    );
    let stale_runtime = test_runtime(root.path(), &stale_catalog);
    let stale_result = route_cost_at(&arguments, &stale_runtime, at);
    assert!(stale_result["structuredContent"]["cost"]
        .get("usd_approx")
        .is_none());
}

#[test]
fn route_cost_rejects_caller_usage_with_more_cached_than_prompt_tokens() {
    let root = tempfile::tempdir().unwrap();
    let runtime = test_runtime(root.path(), METER_CATALOG);
    let at = Utc.with_ymd_and_hms(2026, 7, 20, 12, 0, 0).unwrap();
    let arguments = json!({
        "model": "cheap",
        "prompt_tokens": 10,
        "cached_tokens": 11,
        "output_tokens": 1
    });

    let result = route_cost_at(&arguments, &runtime, at);

    assert_eq!(result["isError"], true);
    assert_eq!(result["content"][0]["text"], "usage is not priceable");
    assert!(result.get("structuredContent").is_none());
}

#[test]
fn route_cost_http_uses_canonical_tiny_money_and_exposes_estimate_provenance() {
    let (_root, server) = test_server_with_catalog(METER_CATALOG, "cheap");
    let response = raw_post(
        server.addr(),
        &server.addr().to_string(),
        None,
        Some(server.token()),
        &tools_call(
            "route_cost",
            json!({
                "model": "cheap",
                "prompt_tokens": 1,
                "cached_tokens": 0,
                "output_tokens": 0
            }),
        ),
    );
    let value = response_value(&response);
    let result = &value["result"];
    let text = result["content"][0]["text"].as_str().unwrap();

    assert_eq!(
        text,
        "cheap | <\u{a5}0.0001 (est) | price confirmed | 1 prompt (0 cached) | 0 output"
    );
    assert!(!text.contains("0.000000"), "{text}");
    assert_eq!(result["structuredContent"]["cost"]["estimated"], true);
    assert_eq!(
        result["structuredContent"]["cost"]["price_confidence"],
        "confirmed"
    );
    assert_eq!(
        result["structuredContent"]["cost"]["value"].as_f64(),
        Some(0.000001)
    );
    server.shutdown().unwrap();
}

#[test]
fn route_cost_http_exposes_reported_price_provenance() {
    let (_root, server) = test_server_with_catalog(METER_CATALOG, "cheap");
    let response = raw_post(
        server.addr(),
        &server.addr().to_string(),
        None,
        Some(server.token()),
        &tools_call(
            "route_cost",
            json!({
                "model": "expensive",
                "prompt_tokens": 1_000,
                "cached_tokens": 0,
                "output_tokens": 100
            }),
        ),
    );
    let result = &response_value(&response)["result"];
    let text = result["content"][0]["text"].as_str().unwrap();

    assert!(text.contains("(est) | price reported"), "{text}");
    assert_eq!(
        result["structuredContent"]["cost"]["price_confidence"],
        "reported"
    );
    server.shutdown().unwrap();
}

#[test]
fn why_http_preserves_verify_live_price_asterisk_convention() {
    let catalog = METER_CATALOG.replacen(
        "price_confidence = \"confirmed\"",
        "price_confidence = \"verify_live\"",
        1,
    );
    let (_root, server) = test_server_with_catalog(&catalog, "cheap");
    let response = raw_post(
        server.addr(),
        &server.addr().to_string(),
        None,
        Some(server.token()),
        &tools_call(
            "why",
            json!({
                "prompt_tokens": 1_000,
                "output_tokens": 100,
                "allowed": ["cheap"]
            }),
        ),
    );
    let result = &response_value(&response)["result"];
    let text = result["content"][0]["text"].as_str().unwrap();

    assert!(text.contains("(est) | *price verify_live"), "{text}");
    assert_eq!(
        result["structuredContent"]["cost"]["price_confidence"],
        "verify_live"
    );
    server.shutdown().unwrap();
}

#[test]
fn receipts_redact_literals_and_shapes_tolerate_torn_tail_and_never_mutate() {
    let (root, server) = test_server();
    let addr = server.addr();
    let host = addr.to_string();
    let token = server.token().to_string();
    let missing = raw_post(
        addr,
        &host,
        None,
        Some(&token),
        &tools_call("receipts", json!({})),
    );
    assert_eq!(
        response_value(&missing)["result"]["structuredContent"]["count"],
        0
    );

    let shaped = format!("{}{}", "sk-", "fixture-token-0000");
    let receipt = json!({
        "ts_utc": "2026-07-20T12:00:00Z",
        "model_id": format!("fixture {token} {shaped}"),
        "task": format!("literal {token} and shape {shaped}"),
        "turns": 2,
        "tool_calls": 1,
        "outcome": "pass",
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 5,
            "cached_tokens": 4
        }
    });
    let path = root.path().join(".nosis").join("receipts.jsonl");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let bytes = format!("\n{}\n{{\"ts_utc\":", receipt);
    std::fs::write(&path, bytes.as_bytes()).unwrap();
    let before = std::fs::read(&path).unwrap();

    let response = raw_post(
        addr,
        &host,
        None,
        Some(&token),
        &tools_call("receipts", json!({ "limit": 10 })),
    );
    let after = std::fs::read(&path).unwrap();
    let value = response_value(&response);
    let result = &value["result"];
    let task = result["structuredContent"]["receipts"][0]["task"]
        .as_str()
        .unwrap();

    assert_eq!(before, after);
    assert_eq!(result["structuredContent"]["count"], 1);
    assert_eq!(
        result["structuredContent"]["receipts"][0]["usage"]["evidence"],
        "unknown"
    );
    assert!(!response.contains(&token), "{response}");
    assert!(!response.contains(&shaped), "{response}");
    assert!(task.matches("[REDACTED]").count() >= 2, "{task}");
    let content = result["content"][0]["text"].as_str().unwrap();
    assert!(!content.contains(&token));
    assert!(!content.contains(&shaped));
    assert!(content.matches("[REDACTED]").count() >= 2, "{content}");
    assert!(content.contains("usage unknown"), "{content}");
    assert!(!content.contains("15 tokens"), "{content}");
    server.shutdown().unwrap();
}

#[test]
fn receipts_http_marks_partial_usage_and_refuses_overflow_totals() {
    let (root, server) = test_server();
    let receipt = |task: &str, usage: Option<Value>| {
        let mut receipt = json!({
            "ts_utc": "2026-08-03T12:00:00Z",
            "model_id": task,
            "task": task,
            "turns": 1,
            "tool_calls": 0,
            "outcome": "pass"
        });
        if let Some(usage) = usage {
            receipt["usage"] = usage;
        }
        receipt
    };
    let receipts = [
        receipt(
            "measured",
            Some(json!({
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "evidence": "measured"
            })),
        ),
        receipt(
            "partial",
            Some(json!({
                "prompt_tokens": 7,
                "completion_tokens": 2,
                "evidence": "partial"
            })),
        ),
        receipt(
            "overflow",
            Some(json!({
                "prompt_tokens": u64::MAX,
                "completion_tokens": 1,
                "evidence": "measured"
            })),
        ),
        receipt("absent", None),
    ];
    let path = root.path().join(".nosis").join("receipts.jsonl");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut bytes = receipts
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
        .into_bytes();
    bytes.push(b'\n');
    std::fs::write(path, bytes).unwrap();

    let response = raw_post(
        server.addr(),
        &server.addr().to_string(),
        None,
        Some(server.token()),
        &tools_call("receipts", json!({ "limit": 10 })),
    );
    let result = &response_value(&response)["result"];
    let text = result["content"][0]["text"].as_str().unwrap();

    assert!(
        text.contains("measured | pass | 1 turns | 15 tokens"),
        "{text}"
    );
    assert!(
        text.contains("partial | pass | 1 turns | ~9 tokens (lower bound)"),
        "{text}"
    );
    assert!(
        text.contains("overflow | pass | 1 turns | token total unavailable (overflow)"),
        "{text}"
    );
    assert!(
        text.contains("absent | pass | 1 turns | unmetered"),
        "{text}"
    );
    assert!(!text.contains(&u64::MAX.to_string()), "{text}");
    server.shutdown().unwrap();
}

#[test]
fn legacy_fleet_ledger_usage_bytes_parse_as_unknown() {
    let root = tempfile::tempdir().unwrap();
    let run_id = "legacy-usage";
    write_fleet_ledger(
        root.path(),
        run_id,
        &[json!({
            "event": "task_receipt",
            "task_id": "one",
            "attempt": 1,
            "receipt": {
                "ts_utc": "2026-07-20T12:00:00Z",
                "model_id": "fixture",
                "task": "legacy",
                "turns": 1,
                "tool_calls": 0,
                "outcome": "pass",
                "usage": {
                    "prompt_tokens": 10,
                    "completion_tokens": 5,
                    "cached_tokens": 4
                }
            }
        })],
    );
    let path = root
        .path()
        .join(".nosis")
        .join("fleet")
        .join(run_id)
        .join("ledger.jsonl");
    let before = std::fs::read(&path).unwrap();

    let receipt = nh_fleet::read_run_ledger(root.path(), run_id)
        .unwrap()
        .into_iter()
        .find_map(|event| match event {
            nh_fleet::LedgerEvent::TaskReceipt { receipt, .. } => Some(receipt),
            _ => None,
        })
        .unwrap();

    assert_eq!(
        receipt.usage.unwrap().evidence,
        nh_core::wire::UsageEvidence::Unknown
    );
    assert_eq!(std::fs::read(path).unwrap(), before);
}

#[test]
fn receipts_reads_a_bounded_tail_instead_of_the_whole_file() {
    let (root, server) = test_server();
    let path = root.path().join(".nosis").join("receipts.jsonl");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut file = File::create(&path).unwrap();
    file.write_all(&vec![b'x'; MAX_RECEIPT_TAIL_BYTES + 1024])
        .unwrap();
    let receipt = json!({
        "ts_utc": "2026-07-20T12:00:00Z",
        "model_id": "fixture",
        "task": "recent",
        "turns": 1,
        "tool_calls": 0,
        "outcome": "pass"
    });
    writeln!(file, "\n{receipt}").unwrap();
    drop(file);

    let response = raw_post(
        server.addr(),
        &server.addr().to_string(),
        None,
        Some(server.token()),
        &tools_call("receipts", json!({ "limit": 1 })),
    );
    let result = &response_value(&response)["result"];

    assert_eq!(result["structuredContent"]["count"], 1);
    assert_eq!(result["structuredContent"]["receipts"][0]["task"], "recent");
    server.shutdown().unwrap();
}

#[test]
fn route_resolve_keeps_text_and_adds_structured_route() {
    let (_root, server) = test_server();
    let addr = server.addr();
    let host = addr.to_string();
    let token = server.token().to_string();
    let response = raw_post(
        addr,
        &host,
        None,
        Some(&token),
        &tools_call("route_resolve", json!({})),
    );
    let value = response_value(&response);
    let result = &value["result"];

    assert!(result["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("route deepseek-v4-flash"));
    assert_eq!(
        result["structuredContent"]["route"]["id"],
        "deepseek-v4-flash"
    );
    assert!(result["structuredContent"]["route"]["peak_status"].is_string());
    assert_eq!(result["structuredContent"]["would_park_offpeak"], false);
    server.shutdown().unwrap();
}

#[test]
fn new_tools_keep_auth_and_origin_fail_closed() {
    let (_root, server) = test_server();
    let addr = server.addr();
    let host = addr.to_string();
    let token = server.token().to_string();

    for name in ["why", "route_cost", "receipts"] {
        let request = tools_call(name, json!({}));
        let missing = raw_post(addr, &host, None, None, &request);
        let blank = raw_post(addr, &host, None, Some(""), &request);
        let bad_origin = raw_post(
            addr,
            &host,
            Some("https://evil.example"),
            Some(&token),
            &request,
        );
        assert!(missing.starts_with("HTTP/1.1 401"), "{name}: {missing}");
        assert!(blank.starts_with("HTTP/1.1 401"), "{name}: {blank}");
        assert!(
            bad_origin.starts_with("HTTP/1.1 403"),
            "{name}: {bad_origin}"
        );
    }
    server.shutdown().unwrap();
}

#[test]
fn scrub_json_redacts_structured_keys_and_values() {
    let literal = "fixture-literal";
    let shaped = format!("{}{}", "sk-", "fixture-token-0000");
    let scrubber = nh_vault::Scrubber::new(vec![literal.into()]);
    let mut value = json!({
        format!("key-{literal}"): format!("{literal} {shaped}"),
        "nested": [format!("prefix {shaped}")]
    });

    scrub_json(&mut value, &scrubber);
    let rendered = value.to_string();
    assert!(!rendered.contains(literal), "{rendered}");
    assert!(!rendered.contains(&shaped), "{rendered}");
    assert!(rendered.matches("[REDACTED]").count() >= 3, "{rendered}");
}

#[test]
fn caller_supplied_token_must_meet_32_byte_floor() {
    let root = tempfile::tempdir().unwrap();
    let config = |token| ServeConfig {
        addr: "127.0.0.1:0".parse().unwrap(),
        catalog: METER_CATALOG.to_string(),
        law: nh_law::load(root.path(), &nh_law::LoadOptions { cli_autonomy: None }),
        default_route: "cheap".into(),
        run_root: root.path().to_path_buf(),
        token: Some(token),
        max_workers: 1,
    };

    let error = McpServer::start(config(nh_vault::secret("short")))
        .err()
        .expect("short caller token must be rejected")
        .to_string();
    assert!(
        error.contains("caller token must be at least 32 bytes"),
        "{error}"
    );

    let accepted = "x".repeat(MIN_CALLER_TOKEN_BYTES);
    let server = McpServer::start(config(nh_vault::secret(&accepted))).unwrap();
    assert_eq!(server.token(), accepted);
    server.shutdown().unwrap();
}

#[test]
fn dropping_server_signals_shutdown_and_joins() {
    let (_root, server) = test_server();
    drop(server);
}

#[test]
fn minted_token_is_required_and_host_origin_fail_closed() {
    let (_root, server) = test_server();
    let addr = server.addr();
    let token = server.token().to_string();
    assert_eq!(token.len(), 64);
    assert!(token.chars().all(|c| c.is_ascii_hexdigit()));

    let fleet_run = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "fleet_run",
            "arguments": { "tasks": [{ "task": "must not dispatch" }] }
        }
    });
    let no_auth = raw_post(addr, &addr.to_string(), None, None, &fleet_run);
    assert!(no_auth.starts_with("HTTP/1.1 401"), "{no_auth}");

    let bad_host = raw_post(
        addr,
        "evil.example",
        None,
        Some(&token),
        &json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}),
    );
    assert!(bad_host.starts_with("HTTP/1.1 403"), "{bad_host}");

    let bad_origin = raw_post(
        addr,
        &addr.to_string(),
        Some("https://evil.example"),
        Some(&token),
        &json!({"jsonrpc":"2.0","id":3,"method":"tools/list","params":{}}),
    );
    assert!(bad_origin.starts_with("HTTP/1.1 403"), "{bad_origin}");

    let allowed = raw_post(
        addr,
        &addr.to_string(),
        Some("http://localhost:3000"),
        Some(&token),
        &json!({"jsonrpc":"2.0","id":4,"method":"tools/list","params":{}}),
    );
    assert!(allowed.starts_with("HTTP/1.1 200"), "{allowed}");
    assert!(allowed.contains("\"fleet_run\""), "{allowed}");

    server.shutdown().unwrap();
}

#[test]
fn minted_tokens_are_independent_csprng_values() {
    let first = mint_token().expect("OS randomness is available");
    let second = mint_token().expect("OS randomness is available");

    assert_eq!(first.len(), 64);
    assert_eq!(second.len(), 64);
    assert!(first.chars().all(|character| character.is_ascii_hexdigit()));
    assert!(second
        .chars()
        .all(|character| character.is_ascii_hexdigit()));
    assert_ne!(first, second);
}

#[test]
fn exact_routes_accept_only_well_known_get_and_mcp_post() {
    let (_root, server) = test_server();
    let addr = server.addr();
    let host = addr.to_string();
    let token = server.token().to_string();
    let list = json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}});

    let exact_get = raw_request(
        addr,
        "GET",
        "/.well-known/mcp.json",
        &host,
        None,
        Some(&token),
        "",
    );
    let loose_get = raw_request(
        addr,
        "GET",
        "/x/.well-known/mcp.json",
        &host,
        None,
        Some(&token),
        "",
    );
    let loose_post = raw_request(
        addr,
        "POST",
        "/nope",
        &host,
        None,
        Some(&token),
        &list.to_string(),
    );
    let exact_post = raw_post(addr, &host, None, Some(&token), &list);

    assert!(exact_get.starts_with("HTTP/1.1 200"), "{exact_get}");
    assert!(loose_get.starts_with("HTTP/1.1 404"), "{loose_get}");
    assert!(loose_post.starts_with("HTTP/1.1 404"), "{loose_post}");
    assert!(exact_post.starts_with("HTTP/1.1 200"), "{exact_post}");
    server.shutdown().unwrap();
}

#[test]
fn oversized_request_body_is_rejected_with_413() {
    let (_root, server) = test_server();
    let addr = server.addr();
    let host = addr.to_string();
    let token = server.token().to_string();
    let body = "x".repeat(MAX_BODY_BYTES + 1);

    let response = raw_request(addr, "POST", "/mcp", &host, None, Some(&token), &body);

    assert!(response.starts_with("HTTP/1.1 413"), "{response}");
    assert!(response.contains("request too large"), "{response}");
    server.shutdown().unwrap();
}

#[test]
fn same_length_wrong_bearer_is_rejected_and_exact_bearer_works() {
    let (_root, server) = test_server();
    let addr = server.addr();
    let host = addr.to_string();
    let token = server.token().to_string();
    let wrong = "0".repeat(token.len());
    let list = json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}});

    let rejected = raw_post(addr, &host, None, Some(&wrong), &list);
    let accepted = raw_post(addr, &host, None, Some(&token), &list);

    assert!(rejected.starts_with("HTTP/1.1 401"), "{rejected}");
    assert!(accepted.starts_with("HTTP/1.1 200"), "{accepted}");
    server.shutdown().unwrap();
}

#[test]
fn fleet_status_distinguishes_unknown_run_from_existing_empty_run() {
    let (root, server) = test_server();
    let addr = server.addr();
    let host = addr.to_string();
    let token = server.token().to_string();

    let unknown = raw_post(
        addr,
        &host,
        None,
        Some(&token),
        &tools_call("fleet_status", json!({"run_id": "unknown-run"})),
    );
    let starting_id = "known-empty-run";
    std::fs::create_dir_all(root.path().join(".nosis").join("fleet").join(starting_id)).unwrap();
    let starting = raw_post(
        addr,
        &host,
        None,
        Some(&token),
        &tools_call("fleet_status", json!({"run_id": starting_id})),
    );

    assert!(unknown.contains("unknown run: unknown-run"), "{unknown}");
    assert!(!unknown.contains("starting"), "{unknown}");
    assert_eq!(
        response_value(&unknown)["result"]["structuredContent"]["state"],
        "unknown"
    );
    assert!(
        starting.contains("known-empty-run · starting"),
        "{starting}"
    );
    assert_eq!(
        response_value(&starting)["result"]["structuredContent"]["state"],
        "starting"
    );
    server.shutdown().unwrap();
}

#[test]
fn fleet_status_renders_failed_and_unmetered_without_changing_finished_shape() {
    let (root, server) = test_server();
    let addr = server.addr();
    let host = addr.to_string();
    let token = server.token().to_string();

    write_fleet_ledger(
        root.path(),
        "failed-run",
        &[json!({
            "event": "run_failed",
            "run_id": "failed-run",
            "reason": "provider unavailable"
        })],
    );
    let failed = raw_post(
        addr,
        &host,
        None,
        Some(&token),
        &tools_call("fleet_status", json!({"run_id": "failed-run"})),
    );
    assert!(
        failed.contains("failed-run · failed: provider unavailable"),
        "{failed}"
    );
    let failed_value = response_value(&failed);
    assert_eq!(
        failed_value["result"]["structuredContent"]["state"],
        "failed"
    );
    assert_eq!(
        failed_value["result"]["structuredContent"]["failed_reason"],
        "provider unavailable"
    );

    write_fleet_ledger(
        root.path(),
        "unmetered-run",
        &[
            json!({
                "event": "run_started",
                "run_id": "unmetered-run",
                "created_utc": "2026-07-19T00:00:00Z",
                "task_count": 1,
                "max_workers": 1,
                "budget_tokens": 10
            }),
            json!({
                "event": "task_queued",
                "task_id": "one",
                "task": "work",
                "route_id": "fixture"
            }),
            json!({
                "event": "task_receipt",
                "task_id": "one",
                "attempt": 1,
                "receipt": {
                    "ts_utc": "2026-07-19T00:00:00Z",
                    "model_id": "fixture",
                    "task": "work",
                    "turns": 1,
                    "tool_calls": 0,
                    "outcome": "pass"
                }
            }),
        ],
    );
    let unmetered = raw_post(
        addr,
        &host,
        None,
        Some(&token),
        &tools_call("fleet_status", json!({"run_id": "unmetered-run"})),
    );
    assert!(unmetered.contains("· 1 unmetered"), "{unmetered}");

    write_fleet_ledger(
        root.path(),
        "finished-run",
        &[
            json!({
                "event": "run_started",
                "run_id": "finished-run",
                "created_utc": "2026-07-19T00:00:00Z",
                "task_count": 0,
                "max_workers": 1,
                "budget_tokens": null
            }),
            json!({
                "event": "run_finished",
                "run_id": "finished-run",
                "done": 0,
                "failed": 0,
                "gated": 0
            }),
        ],
    );
    let finished = raw_post(
        addr,
        &host,
        None,
        Some(&token),
        &tools_call("fleet_status", json!({"run_id": "finished-run"})),
    );
    assert!(
        finished.contains("finished-run · finished · 0 done · 0 failed · 0 gated · 0 pending"),
        "{finished}"
    );
    let finished_value = response_value(&finished);
    assert!(!finished_value["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("unmetered"));
    assert_eq!(
        finished_value["result"]["content"][0]["text"],
        "finished-run · finished · 0 done · 0 failed · 0 gated · 0 pending"
    );
    assert_eq!(
        finished_value["result"]["structuredContent"]["state"],
        "finished"
    );
    server.shutdown().unwrap();
}

#[test]
fn fleet_run_rejects_unresolvable_route_before_claiming_started() {
    let (_root, server) = test_server();
    let addr = server.addr();
    let host = addr.to_string();
    let token = server.token().to_string();

    let response = raw_post(
        addr,
        &host,
        None,
        Some(&token),
        &tools_call(
            "fleet_run",
            json!({
                "tasks": [{"task": "fixture task", "model": "missing-route"}],
                "budget": 1_000
            }),
        ),
    );

    assert!(response.contains("fleet run rejected:"), "{response}");
    assert!(!response.contains("fleet run started"), "{response}");
    server.shutdown().unwrap();
}

#[test]
fn fleet_run_requires_a_bounded_budget() {
    let (root, server) = test_server();
    for (budget, expected) in [
        (None, "requires a token budget"),
        (Some(0), "must be at least 1"),
        (
            Some(MAX_MCP_FLEET_BUDGET_TOKENS + 1),
            "exceeds the MCP ceiling",
        ),
    ] {
        let mut arguments = json!({"tasks": [{"task": "bounded", "backend": "kimi-swarm"}]});
        if let Some(budget) = budget {
            arguments["budget"] = json!(budget);
        }
        let response = raw_post(
            server.addr(),
            &server.addr().to_string(),
            None,
            Some(server.token()),
            &tools_call("fleet_run", arguments),
        );
        assert!(response.contains(expected), "{response}");
        assert!(!response.contains("fleet run started"), "{response}");
    }
    assert!(!root.path().join(".nosis").join("fleet").exists());
    server.shutdown().unwrap();
}

#[test]
fn fleet_run_rejects_oversized_tasks_before_creating_a_run() {
    let (root, server) = test_server();
    let response = raw_post(
        server.addr(),
        &server.addr().to_string(),
        None,
        Some(server.token()),
        &tools_call(
            "fleet_run",
            json!({
                "tasks": [{
                    "task": "x".repeat(nh_core::agent::MAX_TASK_BYTES + 1),
                    "backend": "kimi-swarm"
                }],
                "budget": 1_000
            }),
        ),
    );

    assert!(response.contains("maximum"), "{response}");
    assert!(!response.contains("fleet run started"), "{response}");
    assert!(!root.path().join(".nosis").join("fleet").exists());
    server.shutdown().unwrap();
}

#[test]
fn fleet_run_preflights_undeclared_audience_and_valid_config_returns_run_id() {
    let root = tempfile::tempdir().unwrap();
    let unique = mint_token().expect("OS randomness is available");
    let vault_entry = format!("w2-unapproved-{}", unique.as_str());
    let catalog = format!(
        r#"
        [routes.fixture-route]
        provider = "fixture"
        model_id = "fixture-route"
        base_url = "https://example.invalid"
        wire = "openai"
        vault_entry = "{vault_entry}"
        class = "api"
        "#
    );
    let law = nh_law::load(root.path(), &nh_law::LoadOptions { cli_autonomy: None });
    let server = McpServer::start(ServeConfig {
        addr: "127.0.0.1:0".parse().unwrap(),
        catalog,
        law,
        default_route: "fixture-route".into(),
        run_root: root.path().to_path_buf(),
        token: None,
        max_workers: 1,
    })
    .unwrap();
    let addr = server.addr();
    let host = addr.to_string();
    let token = server.token().to_string();

    let unapproved = raw_post(
        addr,
        &host,
        None,
        Some(&token),
        &tools_call(
            "fleet_run",
            json!({"tasks": [{"task": "native task"}], "budget": 1_000}),
        ),
    );
    let started = raw_post(
        addr,
        &host,
        None,
        Some(&token),
        &tools_call(
            "fleet_run",
            json!({
                "tasks": [{"task": "swarm task", "backend": "kimi-swarm"}],
                "budget": 1_000
            }),
        ),
    );

    assert!(unapproved.contains("fleet run rejected:"), "{unapproved}");
    assert!(unapproved.contains("is not approved for"), "{unapproved}");
    assert!(!unapproved.contains("fleet run started"), "{unapproved}");
    assert!(started.contains("fleet run started · run_id="), "{started}");
    server.shutdown().unwrap();
}
