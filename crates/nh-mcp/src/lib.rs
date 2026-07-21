//! Stateless, loopback-only MCP preview server for Nosis routes and fleets.
//!
//! The server mirrors `nh_tools::mcp::McpClient`: blocking JSON-RPC over HTTP,
//! no initialize handshake, no sessions, and durable run IDs as ordinary handles.

use std::collections::{BTreeSet, HashSet};
use std::io::Read as _;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{bail, Context as _};
use chrono::{Local, Utc};
use nh_vault::Vault as _;
use serde::Deserialize;
use serde_json::{json, Value};
use subtle::ConstantTimeEq;
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

const PREVIEW_NOTICE: &str =
    "nh-mcp preview — local only; do not expose publicly before the MCP final spec (2026-07-28).";
const MAX_BODY_BYTES: usize = 1024 * 1024;

pub struct ServeConfig {
    pub addr: SocketAddr,
    pub catalog: String,
    pub law: nh_law::Law,
    pub default_route: String,
    pub run_root: PathBuf,
    pub token: Option<String>,
    pub max_workers: usize,
}

struct Runtime {
    config: Arc<ServeConfig>,
    token: String,
    scrubber: nh_vault::Scrubber,
}

pub struct McpServer {
    addr: SocketAddr,
    token: String,
    shutdown: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}

impl McpServer {
    /// Bind first, then run the blocking accept loop on a background thread.
    pub fn start(config: ServeConfig) -> anyhow::Result<McpServer> {
        let (server, addr, runtime) = bind(config)?;
        let token = runtime.token.clone();
        let shutdown = Arc::new(AtomicBool::new(false));
        let loop_shutdown = Arc::clone(&shutdown);
        let handle = thread::Builder::new()
            .name("nh-mcp".into())
            .spawn(move || accept_loop(server, runtime, loop_shutdown))
            .context("could not start the nh-mcp server thread")?;
        Ok(Self {
            addr,
            token,
            shutdown,
            handle,
        })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn shutdown(self) -> anyhow::Result<()> {
        self.shutdown.store(true, Ordering::Relaxed);
        self.handle
            .join()
            .map_err(|_| anyhow::anyhow!("nh-mcp server thread panicked"))
    }
}

/// Bind, print the two-line preview banner, and serve on the current thread.
pub fn serve(config: ServeConfig) -> anyhow::Result<()> {
    let (server, addr, runtime) = bind(config)?;
    print_banner(addr, &runtime);
    accept_loop(server, runtime, Arc::new(AtomicBool::new(false)));
    Ok(())
}

fn bind(mut config: ServeConfig) -> anyhow::Result<(Server, SocketAddr, Arc<Runtime>)> {
    if config.addr.ip() != IpAddr::V4(Ipv4Addr::LOCALHOST) {
        bail!("nh-mcp only binds 127.0.0.1 — use 127.0.0.1:PORT");
    }
    let requested = config.addr;
    let server = Server::http(requested)
        .map_err(|error| anyhow::anyhow!("could not bind nh-mcp to {requested}: {error}"))?;
    let addr = server
        .server_addr()
        .to_ip()
        .ok_or_else(|| anyhow::anyhow!("nh-mcp did not bind a TCP address"))?;
    config.addr = addr;
    let token = config.token.take().unwrap_or_else(mint_token);
    let scrubber = nh_vault::Scrubber::new(vec![token.clone()]);
    let runtime = Runtime {
        config: Arc::new(config),
        token,
        scrubber,
    };
    Ok((server, addr, Arc::new(runtime)))
}

/// Loopback preview token from the operating system CSPRNG; not a long-term credential.
fn mint_token() -> String {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).expect("OS CSPRNG");
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn print_banner(addr: SocketAddr, runtime: &Runtime) {
    println!("{}", nh_vault::safe_line(&runtime.scrubber, PREVIEW_NOTICE));
    let connect_scrubber = nh_vault::Scrubber::new(Vec::new());
    println!(
        "{}",
        nh_vault::safe_line(
            &connect_scrubber,
            &format!(
                "connect http://{addr}/mcp with Bearer {}   (tools: route_resolve, fleet_run, fleet_status)",
                runtime.token
            ),
        )
    );
}

fn accept_loop(server: Server, runtime: Arc<Runtime>, shutdown: Arc<AtomicBool>) {
    while !shutdown.load(Ordering::Relaxed) {
        match server.recv_timeout(Duration::from_millis(100)) {
            Ok(Some(request)) => handle(request, &runtime),
            Ok(None) => continue,
            Err(error) => {
                if !shutdown.load(Ordering::Relaxed) {
                    eprintln!(
                        "warning: {}",
                        nh_vault::safe_line(
                            &runtime.scrubber,
                            &format!("nh-mcp server stopped after receive error: {error}"),
                        )
                    );
                }
                break;
            }
        }
    }
}

fn handle(mut request: Request, runtime: &Runtime) {
    if !loopback_headers(&request, runtime.config.addr.port()) {
        respond_json(request, runtime, 403, &json!({}));
        return;
    }
    if !authorized(&request, runtime) {
        respond_json(request, runtime, 401, &json!({}));
        return;
    }

    if request.method() == &Method::Get {
        if request.url() == "/.well-known/mcp.json" {
            respond_json(request, runtime, 200, &business_card());
        } else {
            respond_json(request, runtime, 404, &json!({}));
        }
        return;
    }

    if request.method() != &Method::Post || request.url() != "/mcp" {
        respond_json(request, runtime, 404, &json!({}));
        return;
    }

    let mut body = Vec::with_capacity(8 * 1024);
    if request
        .as_reader()
        .take((MAX_BODY_BYTES + 1) as u64)
        .read_to_end(&mut body)
        .is_err()
    {
        respond_json(
            request,
            runtime,
            200,
            &rpc_error(Value::Null, -32700, "parse error"),
        );
        return;
    }
    if body.len() > MAX_BODY_BYTES {
        respond_json(
            request,
            runtime,
            413,
            &rpc_error(Value::Null, -32700, "request too large"),
        );
        return;
    }
    let message: Value = match serde_json::from_slice(&body) {
        Ok(message) => message,
        Err(_) => {
            respond_json(
                request,
                runtime,
                200,
                &rpc_error(Value::Null, -32700, "parse error"),
            );
            return;
        }
    };
    let id = message.get("id").cloned().unwrap_or(Value::Null);
    let response = match message.get("method").and_then(Value::as_str) {
        Some("tools/list") => rpc_success(id, tools_list()),
        Some("tools/call") => rpc_success(
            id,
            tools_call(message.get("params").unwrap_or(&Value::Null), runtime),
        ),
        _ => rpc_error(id, -32601, "method not found"),
    };
    respond_json(request, runtime, 200, &response);
}

fn loopback_headers(request: &Request, port: u16) -> bool {
    let expected_ip = format!("127.0.0.1:{port}");
    let expected_name = format!("localhost:{port}");
    let hosts: Vec<_> = request
        .headers()
        .iter()
        .filter(|header| header.field.equiv("Host"))
        .collect();
    let host_ok = hosts.len() == 1
        && (hosts[0].value.as_str().eq_ignore_ascii_case(&expected_ip)
            || hosts[0].value.as_str().eq_ignore_ascii_case(&expected_name));
    if !host_ok {
        return false;
    }
    request
        .headers()
        .iter()
        .filter(|header| header.field.equiv("Origin"))
        .all(|header| localhost_origin(header.value.as_str()))
}

fn localhost_origin(origin: &str) -> bool {
    let origin = origin.trim().to_ascii_lowercase();
    let Some(authority) = origin
        .strip_prefix("http://")
        .and_then(|rest| rest.split('/').next())
    else {
        return false;
    };
    if matches!(authority, "127.0.0.1" | "localhost") {
        return true;
    }
    ["127.0.0.1:", "localhost:"].iter().any(|prefix| {
        authority
            .strip_prefix(prefix)
            .is_some_and(|port| port.parse::<u16>().is_ok())
    })
}

fn authorized(request: &Request, runtime: &Runtime) -> bool {
    let expected = format!("Bearer {}", runtime.token);
    request
        .headers()
        .iter()
        .filter(|header| header.field.equiv("Authorization"))
        .any(|header| {
            let provided = header.value.as_str();
            provided.len() == expected.len()
                && bool::from(provided.as_bytes().ct_eq(expected.as_bytes()))
        })
}

fn business_card() -> Value {
    json!({
        "name": "nh-mcp",
        "spec": "2026-07-28",
        "tools": ["fleet_run", "fleet_status", "receipts", "route_cost", "route_resolve", "why"],
        "notice": "local/preview only"
    })
}

fn rpc_success(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
}

fn tools_list() -> Value {
    json!({
        "tools": [
            {
                "name": "route_resolve",
                "description": "Resolve one catalog route with current peak status.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "model": { "type": "string" },
                        "prefer_offpeak": { "type": "boolean" }
                    }
                },
                "outputSchema": {
                    "type": "object",
                    "properties": {
                        "route": { "type": "object" },
                        "would_park_offpeak": { "type": "boolean" }
                    },
                    "required": ["route", "would_park_offpeak"]
                },
                "annotations": { "readOnlyHint": true }
            },
            {
                "name": "why",
                "description": "Choose the cheapest capable route and explain every skipped route.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "task": { "type": "string" },
                        "prompt_tokens": { "type": "integer", "minimum": 0 },
                        "output_tokens": { "type": "integer", "minimum": 0 },
                        "allowed": {
                            "type": "array",
                            "items": { "type": "string" }
                        },
                        "prefer_offpeak": { "type": "boolean" }
                    },
                    "required": ["prompt_tokens", "output_tokens"]
                },
                "outputSchema": {
                    "type": "object",
                    "properties": {
                        "route": { "type": "object" },
                        "cost": { "type": "object" },
                        "savings": { "type": "object" },
                        "rejected": { "type": "array", "items": { "type": "object" } }
                    },
                    "required": ["route", "cost", "rejected"]
                },
                "annotations": { "readOnlyHint": true }
            },
            {
                "name": "route_cost",
                "description": "Price one catalog route for explicit prompt, cache, and output tokens.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "model": { "type": "string" },
                        "prompt_tokens": { "type": "integer", "minimum": 0 },
                        "cached_tokens": { "type": "integer", "minimum": 0 },
                        "output_tokens": { "type": "integer", "minimum": 0 }
                    },
                    "required": ["prompt_tokens", "output_tokens"]
                },
                "outputSchema": {
                    "type": "object",
                    "properties": {
                        "route": { "type": "object" },
                        "quote": { "type": "object" },
                        "cost": { "type": "object" }
                    },
                    "required": ["route", "quote", "cost"]
                },
                "annotations": { "readOnlyHint": true }
            },
            {
                "name": "receipts",
                "description": "Read recent metered receipts from this server's repository root.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "limit": { "type": "integer", "minimum": 1, "maximum": 100 }
                    }
                },
                "outputSchema": {
                    "type": "object",
                    "properties": {
                        "count": { "type": "integer" },
                        "receipts": { "type": "array", "items": { "type": "object" } }
                    },
                    "required": ["count", "receipts"]
                },
                "annotations": { "readOnlyHint": true }
            },
            {
                "name": "fleet_run",
                "description": "Start a durable fleet run and return its run ID.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "tasks": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "id": { "type": "string" },
                                    "task": { "type": "string" },
                                    "model": { "type": "string" },
                                    "defer_offpeak": { "type": "boolean" },
                                    "backend": { "type": "string" }
                                },
                                "required": ["task"]
                            }
                        },
                        "max_workers": { "type": "integer" },
                        "budget": { "type": "integer" },
                        "defer_offpeak": { "type": "boolean" }
                    },
                    "required": ["tasks"]
                },
                "outputSchema": {
                    "type": "object",
                    "properties": {
                        "run_id": { "type": "string" },
                        "task_count": { "type": "integer" }
                    },
                    "required": ["run_id", "task_count"]
                },
                "annotations": { "readOnlyHint": false }
            },
            {
                "name": "fleet_status",
                "description": "Read current counts from a durable fleet ledger.",
                "inputSchema": {
                    "type": "object",
                    "properties": { "run_id": { "type": "string" } },
                    "required": ["run_id"]
                },
                "outputSchema": {
                    "type": "object",
                    "properties": {
                        "run_id": { "type": "string" },
                        "state": {
                            "type": "string",
                            "enum": ["finished", "failed", "running", "starting", "unknown"]
                        },
                        "failed_reason": { "type": "string" },
                        "done": { "type": "integer" },
                        "failed": { "type": "integer" },
                        "gated": { "type": "integer" },
                        "pending": { "type": "integer" },
                        "unmetered": { "type": "integer" }
                    },
                    "required": ["run_id", "state", "done", "failed", "gated", "pending", "unmetered"]
                },
                "annotations": { "readOnlyHint": true }
            }
        ],
        "_meta": { "ttlMs": 60000 }
    })
}

fn tools_call(params: &Value, runtime: &Runtime) -> Value {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return tool_error(runtime, "tools/call needs a tool name");
    };
    let empty_arguments = json!({});
    let arguments = params.get("arguments").unwrap_or(&empty_arguments);
    match name {
        "route_resolve" => route_resolve(arguments, runtime),
        "why" => why(arguments, runtime),
        "route_cost" => route_cost(arguments, runtime),
        "receipts" => receipts(arguments, runtime),
        "fleet_run" => fleet_run(arguments, runtime),
        "fleet_status" => fleet_status(arguments, runtime),
        other => tool_error(runtime, &format!("unknown tool '{other}' — use tools/list")),
    }
}

#[derive(Deserialize)]
struct RouteResolveArgs {
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    prefer_offpeak: Option<bool>,
}

fn route_resolve(arguments: &Value, runtime: &Runtime) -> Value {
    let args: RouteResolveArgs = match serde_json::from_value(arguments.clone()) {
        Ok(args) => args,
        Err(error) => return tool_error(runtime, &error.to_string()),
    };
    let resolver = match nh_routes::RouteResolver::from_toml(&runtime.config.catalog) {
        Ok(resolver) => resolver,
        Err(error) => return tool_error(runtime, &error.to_string()),
    };
    let route = match resolver.resolve(
        args.model
            .as_deref()
            .unwrap_or(&runtime.config.default_route),
    ) {
        Ok(route) => route,
        Err(error) => return tool_error(runtime, &error.to_string()),
    };
    let now = Utc::now();
    let local = *Local::now().offset();
    let mut text = format!(
        "route {} · {} · {} thinking · {}",
        route.id,
        route.provider,
        route.thinking_dialect.as_str(),
        route.peak_status(now, local)
    );
    if args.prefer_offpeak == Some(true)
        && route.price_at(now).map(|quote| quote.peak) == Some(true)
    {
        text.push_str(" · would park until off-peak");
    }
    let would_park_offpeak = args.prefer_offpeak == Some(true)
        && route.price_at(now).map(|quote| quote.peak) == Some(true);
    let structured = json!({
        "route": {
            "id": route.id,
            "provider": route.provider,
            "thinking": route.thinking_dialect.as_str(),
            "peak_status": route.peak_status(now, local)
        },
        "would_park_offpeak": would_park_offpeak
    });
    tool_result(runtime, &text, structured, false)
}

#[derive(Deserialize)]
struct WhyArgs {
    #[serde(default, rename = "task")]
    _task: Option<String>,
    prompt_tokens: u64,
    output_tokens: u64,
    #[serde(default)]
    allowed: Option<Vec<String>>,
    #[serde(default, rename = "prefer_offpeak")]
    _prefer_offpeak: Option<bool>,
}

fn why(arguments: &Value, runtime: &Runtime) -> Value {
    why_at(arguments, runtime, Utc::now())
}

fn why_at(arguments: &Value, runtime: &Runtime, at: chrono::DateTime<Utc>) -> Value {
    let args: WhyArgs = match serde_json::from_value(arguments.clone()) {
        Ok(args) => args,
        Err(error) => return tool_error(runtime, &error.to_string()),
    };
    let resolver = match nh_routes::RouteResolver::from_toml(&runtime.config.catalog) {
        Ok(resolver) => resolver,
        Err(error) => return tool_error(runtime, &error.to_string()),
    };
    let allowed = match args.allowed {
        Some(allowed) if !allowed.is_empty() => allowed,
        _ => resolver
            .available()
            .into_iter()
            .filter(|id| {
                resolver
                    .resolve(id)
                    .is_ok_and(|route| route.class == nh_routes::RouteClass::Api)
            })
            .collect(),
    };
    let allowed_refs: Vec<&str> = allowed.iter().map(String::as_str).collect();
    let (route, trace) =
        match resolver.resolve_capable(args.prompt_tokens, args.output_tokens, &allowed_refs, at) {
            Ok(result) => result,
            Err(error) => return tool_error(runtime, &error.to_string()),
        };
    let quote = match route.price_at(at) {
        Some(quote) => quote,
        None => return tool_error(runtime, "chosen route has no price quote"),
    };
    let actual = nh_routes::cost_of(&quote, args.prompt_tokens, 0, args.output_tokens);
    let usd_approx = resolver
        .fx()
        .and_then(|fx| nh_routes::to_usd_approx(actual, quote.currency, fx, at));
    let naive = resolver.naive_cost(&route, args.prompt_tokens, 0, args.output_tokens, at);
    let saved_pct = naive
        .as_ref()
        .and_then(|naive| nh_routes::saved_pct(actual, naive.no_cache));

    let mut cost = json!({
        "value": actual,
        "currency": quote.currency.as_str()
    });
    if let Some(usd_approx) = usd_approx {
        cost["usd_approx"] = json!(usd_approx);
    }
    let local = *Local::now().offset();
    let mut structured = json!({
        "route": {
            "id": route.id,
            "provider": route.provider,
            "thinking": route.thinking_dialect.as_str(),
            "peak_status": route.peak_status(at, local)
        },
        "cost": cost,
        "rejected": trace.rejections.iter().map(|rejection| json!({
            "route_id": rejection.route_id,
            "reason": rejection.reason
        })).collect::<Vec<_>>()
    });
    if let (Some(naive), Some(saved_pct)) = (naive, saved_pct) {
        structured["savings"] = json!({
            "saved_pct": saved_pct,
            "no_cache": naive.no_cache,
            "peak": naive.peak,
            "top_tier": naive.top_tier,
            "currency": naive.currency.as_str()
        });
    }
    let text = why_text(
        &route.id,
        &route.provider,
        actual,
        quote.currency,
        usd_approx,
        saved_pct,
        trace.rejections.len(),
    );
    tool_result(runtime, &text, structured, false)
}

fn why_text(
    route_id: &str,
    provider: &str,
    actual: f64,
    currency: nh_routes::Currency,
    usd_approx: Option<f64>,
    saved_pct: Option<u8>,
    skipped: usize,
) -> String {
    let mut text = format!(
        "cheapest capable: {route_id} | {provider} | {actual:.6} {}",
        currency.as_str()
    );
    if let Some(usd) = usd_approx {
        text.push_str(&format!(" (~${usd:.6})"));
    }
    if let Some(saved_pct) = saved_pct {
        text.push_str(&format!(" | saved {saved_pct}% vs no-cache"));
    }
    text.push_str(&format!(" | {skipped} routes skipped"));
    text
}

#[derive(Deserialize)]
struct RouteCostArgs {
    #[serde(default)]
    model: Option<String>,
    prompt_tokens: u64,
    #[serde(default)]
    cached_tokens: u64,
    output_tokens: u64,
}

fn route_cost(arguments: &Value, runtime: &Runtime) -> Value {
    route_cost_at(arguments, runtime, Utc::now())
}

fn route_cost_at(arguments: &Value, runtime: &Runtime, at: chrono::DateTime<Utc>) -> Value {
    let args: RouteCostArgs = match serde_json::from_value(arguments.clone()) {
        Ok(args) => args,
        Err(error) => return tool_error(runtime, &error.to_string()),
    };
    let resolver = match nh_routes::RouteResolver::from_toml(&runtime.config.catalog) {
        Ok(resolver) => resolver,
        Err(error) => return tool_error(runtime, &error.to_string()),
    };
    let route = match resolver.resolve(
        args.model
            .as_deref()
            .unwrap_or(&runtime.config.default_route),
    ) {
        Ok(route) => route,
        Err(error) => return tool_error(runtime, &error.to_string()),
    };
    let quote = match route.price_at(at) {
        Some(quote) => quote,
        None => return tool_error(runtime, "route has no token price quote"),
    };
    let value = nh_routes::cost_of(
        &quote,
        args.prompt_tokens,
        args.cached_tokens,
        args.output_tokens,
    );
    let usd_approx = resolver
        .fx()
        .and_then(|fx| nh_routes::to_usd_approx(value, quote.currency, fx, at));
    let mut cost = json!({
        "value": value,
        "currency": quote.currency.as_str()
    });
    if let Some(usd_approx) = usd_approx {
        cost["usd_approx"] = json!(usd_approx);
    }
    let structured = json!({
        "route": {
            "id": route.id,
            "provider": route.provider,
            "thinking": route.thinking_dialect.as_str()
        },
        "quote": {
            "cache_hit": quote.cache_hit,
            "cache_miss": quote.cache_miss,
            "output": quote.output,
            "currency": quote.currency.as_str(),
            "peak": quote.peak,
            "confidence": quote.confidence.as_str(),
            "stale": quote.stale
        },
        "cost": cost
    });
    let mut text = format!("{} | {value:.6} {}", route.id, quote.currency.as_str());
    if let Some(usd) = usd_approx {
        text.push_str(&format!(" (~${usd:.6})"));
    }
    text.push_str(&format!(
        " | {} prompt ({} cached) | {} output",
        args.prompt_tokens, args.cached_tokens, args.output_tokens
    ));
    tool_result(runtime, &text, structured, false)
}

#[derive(Deserialize)]
struct ReceiptsArgs {
    #[serde(default)]
    limit: Option<usize>,
}

fn receipts(arguments: &Value, runtime: &Runtime) -> Value {
    let args: ReceiptsArgs = match serde_json::from_value(arguments.clone()) {
        Ok(args) => args,
        Err(error) => return tool_error(runtime, &error.to_string()),
    };
    let limit = args.limit.unwrap_or(10).clamp(1, 100);
    let path = runtime
        .config
        .run_root
        .join(".nosis")
        .join("receipts.jsonl");
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return tool_error(runtime, &format!("could not read receipts: {error}")),
    };
    let mut values = match parse_receipt_jsonl(&bytes) {
        Ok(values) => values,
        Err(error) => return tool_error(runtime, &error.to_string()),
    };
    if values.len() > limit {
        values.drain(..values.len() - limit);
    }
    let text = receipts_text(&values);
    let structured = json!({
        "count": values.len(),
        "receipts": values
    });
    tool_result(runtime, &text, structured, false)
}

fn parse_receipt_jsonl(bytes: &[u8]) -> anyhow::Result<Vec<Value>> {
    let ends_in_newline = bytes.last() == Some(&b'\n');
    let lines: Vec<&[u8]> = bytes.split(|byte| *byte == b'\n').collect();
    let last_non_empty = lines
        .iter()
        .rposition(|line| !line.iter().all(|byte| byte.is_ascii_whitespace()));
    let mut receipts = Vec::new();
    for (index, line) in lines.into_iter().enumerate() {
        if line.iter().all(|byte| byte.is_ascii_whitespace()) {
            continue;
        }
        let value: Value = match serde_json::from_slice(line) {
            Ok(value) => value,
            Err(_) if !ends_in_newline && Some(index) == last_non_empty => continue,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("receipts line {} is invalid", index + 1));
            }
        };
        let event: nh_fleet::LedgerEvent = serde_json::from_value(json!({
            "event": "task_receipt",
            "task_id": "nh-mcp-receipt",
            "attempt": 1,
            "receipt": value
        }))
        .with_context(|| format!("receipts line {} is invalid", index + 1))?;
        let nh_fleet::LedgerEvent::TaskReceipt { receipt, .. } = event else {
            unreachable!("static wrapper always selects task_receipt");
        };
        receipts
            .push(serde_json::to_value(receipt).context("could not serialize a parsed receipt")?);
    }
    Ok(receipts)
}

fn receipts_text(receipts: &[Value]) -> String {
    if receipts.is_empty() {
        return "receipts: 0".into();
    }
    let rows = receipts
        .iter()
        .map(|receipt| {
            let ts = receipt.get("ts_utc").and_then(Value::as_str).unwrap_or("?");
            let model = receipt
                .get("model_id")
                .and_then(Value::as_str)
                .unwrap_or("?");
            let outcome = receipt
                .get("outcome")
                .and_then(Value::as_str)
                .unwrap_or("?");
            let turns = receipt.get("turns").and_then(Value::as_u64).unwrap_or(0);
            let tokens = receipt.get("usage").map_or_else(
                || "unmetered".to_string(),
                |usage| {
                    let prompt = usage
                        .get("prompt_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    let completion = usage
                        .get("completion_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    format!("{} tokens", prompt.saturating_add(completion))
                },
            );
            format!("{ts} | {model} | {outcome} | {turns} turns | {tokens}")
        })
        .collect::<Vec<_>>()
        .join(" || ");
    format!("receipts: {} | {rows}", receipts.len())
}

#[derive(Deserialize)]
struct FleetRunArgs {
    #[serde(default)]
    tasks: Vec<nh_fleet::TaskSpec>,
    #[serde(default)]
    max_workers: Option<usize>,
    #[serde(default)]
    budget: Option<u64>,
    #[serde(default)]
    defer_offpeak: Option<bool>,
}

fn fleet_run(arguments: &Value, runtime: &Runtime) -> Value {
    let args: FleetRunArgs = match serde_json::from_value(arguments.clone()) {
        Ok(args) => args,
        Err(error) => return tool_error(runtime, &error.to_string()),
    };
    if args.tasks.is_empty() {
        return tool_error(runtime, "fleet_run needs a non-empty tasks array");
    }
    let max_workers = args.max_workers.unwrap_or(runtime.config.max_workers);
    if max_workers == 0 {
        return tool_error(runtime, "fleet_run max_workers must be at least 1");
    }
    let resolver = match nh_routes::RouteResolver::from_toml(&runtime.config.catalog) {
        Ok(resolver) => resolver,
        Err(error) => {
            return tool_error(runtime, &format!("fleet run rejected: {error}"));
        }
    };
    if let Err(error) = preflight_fleet_run(&resolver, &runtime.config.default_route, &args.tasks) {
        return tool_error(runtime, &format!("fleet run rejected: {error}"));
    }
    let task_count = args.tasks.len();
    let run_id = nh_fleet::new_run_id();
    if let Err(error) = std::fs::create_dir_all(fleet_run_dir(&runtime.config, &run_id)) {
        return tool_error(
            runtime,
            &format!("fleet run rejected: could not create run directory: {error}"),
        );
    }
    let config = nh_fleet::FleetConfig {
        resolver,
        law: runtime.config.law.clone(),
        default_route: runtime.config.default_route.clone(),
        tasks: args.tasks,
        max_workers,
        budget_tokens: args.budget,
        clock: None,
        defer_offpeak: args.defer_offpeak.unwrap_or(false),
        ladder: None,
        escalate_on_partial: false,
        swarm: None,
        run_root: runtime.config.run_root.clone(),
        on_event: None,
    };
    let id = run_id.clone();
    let warning_scrubber = runtime.scrubber.clone();
    let spawn = thread::Builder::new()
        .name(format!("nh-mcp-fleet-{id}"))
        .spawn(move || {
            if let Err(error) = nh_fleet::run_with_id(id, config) {
                eprintln!(
                    "warning: {}",
                    nh_vault::safe_line(
                        &warning_scrubber,
                        &format!("nh-mcp fleet run failed after startup: {error}"),
                    )
                );
            }
        });
    if let Err(error) = spawn {
        return tool_error(
            runtime,
            &format!("fleet run rejected: could not start worker thread: {error}"),
        );
    }
    tool_result(
        runtime,
        &format!("fleet run started · run_id={run_id} · {task_count} tasks"),
        json!({ "run_id": run_id, "task_count": task_count }),
        false,
    )
}

fn preflight_fleet_run(
    resolver: &nh_routes::RouteResolver,
    default_route: &str,
    tasks: &[nh_fleet::TaskSpec],
) -> anyhow::Result<()> {
    let using_test_provider = std::env::var("NH_FLEET_TEST_PROVIDER").as_deref() == Ok("echo");
    let vault = nh_vault::EnvFallbackVault {
        inner: nh_vault::KeyringVault,
    };
    let mut ids = HashSet::new();
    let mut entries = BTreeSet::new();
    for (index, task) in tasks.iter().enumerate() {
        if task.task.trim().is_empty() {
            bail!("task {} is empty — add a task description", index + 1);
        }
        if let Some(id) = task.id.as_deref() {
            if id.trim().is_empty() {
                bail!("task ids cannot be empty");
            }
            if !ids.insert(id) {
                bail!("task id collision — choose unique ids");
            }
        }
        let route = resolver.resolve(task.model.as_deref().unwrap_or(default_route))?;
        if route.class == nh_routes::RouteClass::Delegate {
            bail!("delegate routes are not available to fleet workers — pick an api route");
        }
        let native = task.backend.unwrap_or(nh_fleet::Backend::Native) == nh_fleet::Backend::Native;
        if native && !using_test_provider && entries.insert(route.vault_entry.clone()) {
            vault.get(&route.vault_entry)?;
        }
    }
    Ok(())
}

#[derive(Deserialize)]
struct FleetStatusArgs {
    run_id: String,
}

fn fleet_status(arguments: &Value, runtime: &Runtime) -> Value {
    let args: FleetStatusArgs = match serde_json::from_value(arguments.clone()) {
        Ok(args) => args,
        Err(_) => return tool_error(runtime, "fleet_status needs a run_id"),
    };
    let events = match nh_fleet::read_run_ledger(&runtime.config.run_root, &args.run_id) {
        Ok(events) => events,
        Err(error) => return tool_error(runtime, &error.to_string()),
    };
    if !fleet_run_dir(&runtime.config, &args.run_id).is_dir() {
        return tool_result(
            runtime,
            &format!("unknown run: {}", args.run_id),
            json!({
                "run_id": args.run_id,
                "state": "unknown",
                "done": 0,
                "failed": 0,
                "gated": 0,
                "pending": 0,
                "unmetered": 0
            }),
            true,
        );
    }
    let status = nh_fleet::status_from_ledger(&events);
    let state = if status.finished {
        "finished"
    } else if status.failed_reason.is_some() {
        "failed"
    } else if events.is_empty() {
        "starting"
    } else {
        "running"
    };
    let state_word = if status.finished {
        "finished".to_string()
    } else if let Some(reason) = &status.failed_reason {
        format!("failed: {reason}")
    } else if events.is_empty() {
        "starting".to_string()
    } else {
        "running".to_string()
    };
    let unmetered_suffix = if status.unmetered > 0 {
        format!(" · {} unmetered", status.unmetered)
    } else {
        String::new()
    };
    let text = format!(
        "{} · {} · {} done · {} failed · {} gated · {} pending{}",
        args.run_id,
        state_word,
        status.done,
        status.failed,
        status.gated,
        status.pending,
        unmetered_suffix
    );
    let mut structured = json!({
        "run_id": args.run_id,
        "state": state,
        "done": status.done,
        "failed": status.failed,
        "gated": status.gated,
        "pending": status.pending,
        "unmetered": status.unmetered
    });
    if let Some(reason) = status.failed_reason {
        structured["failed_reason"] = json!(reason);
    }
    tool_result(runtime, &text, structured, false)
}

fn fleet_run_dir(config: &ServeConfig, run_id: &str) -> PathBuf {
    config.run_root.join(".nosis").join("fleet").join(run_id)
}

fn tool_error(runtime: &Runtime, message: &str) -> Value {
    tool_text(runtime, message, true)
}

fn tool_result(runtime: &Runtime, text: &str, mut structured: Value, is_error: bool) -> Value {
    let text = nh_vault::safe_line(&runtime.scrubber, text);
    scrub_json(&mut structured, &runtime.scrubber);
    json!({
        "content": [{ "type": "text", "text": text }],
        "structuredContent": structured,
        "isError": is_error
    })
}

fn tool_text(runtime: &Runtime, text: &str, is_error: bool) -> Value {
    let text = nh_vault::safe_line(&runtime.scrubber, text);
    json!({
        "content": [{ "type": "text", "text": text }],
        "isError": is_error
    })
}

fn respond_json(request: Request, runtime: &Runtime, status: u16, value: &Value) {
    let mut value = value.clone();
    scrub_json(&mut value, &runtime.scrubber);
    let body = serde_json::to_string(&value).unwrap_or_else(|_| "{}".into());
    let body = runtime.scrubber.scrub(&body);
    let content_type =
        Header::from_bytes("Content-Type", "application/json").expect("static content-type header");
    let response = Response::from_string(body)
        .with_status_code(StatusCode(status))
        .with_header(content_type);
    let _ = request.respond(response);
}

fn scrub_json(value: &mut Value, scrubber: &nh_vault::Scrubber) {
    match value {
        Value::String(text) => *text = scrubber.scrub(text),
        Value::Array(values) => {
            for value in values {
                scrub_json(value, scrubber);
            }
        }
        Value::Object(object) => {
            let fields = std::mem::take(object);
            for (key, mut value) in fields {
                scrub_json(&mut value, scrubber);
                object.insert(scrubber.scrub(&key), value);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read as _, Write as _};
    use std::net::{Shutdown, TcpStream};
    use std::path::Path;

    use chrono::TimeZone as _;

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
        valid_until = "2099-01-01"
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
        valid_until = "2099-01-01"
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
        valid_until = "2099-01-01"
        price_confidence = "confirmed"
    "#;

    fn test_server() -> (tempfile::TempDir, McpServer) {
        let root = tempfile::tempdir().unwrap();
        let catalog =
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../catalog.toml")).to_string();
        let law = nh_law::load(root.path(), &nh_law::LoadOptions { cli_autonomy: None });
        let server = McpServer::start(ServeConfig {
            addr: "127.0.0.1:0".parse().unwrap(),
            catalog,
            law,
            default_route: "deepseek-v4-flash".into(),
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
            token: "fixture-token".into(),
            scrubber: nh_vault::Scrubber::new(vec!["fixture-literal".into()]),
        }
    }

    fn response_value(response: &str) -> Value {
        let body = response
            .split_once("\r\n\r\n")
            .map(|(_, body)| body)
            .expect("HTTP response has a body");
        serde_json::from_str(body).expect("HTTP response body is JSON")
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
        let actual = nh_routes::cost_of(&quote, 1_000, 0, 100);
        let naive = resolver.naive_cost(&route, 1_000, 0, 100, at).unwrap();
        let expected_saved = nh_routes::saved_pct(actual, naive.no_cache);

        assert_eq!(result["structuredContent"]["route"]["id"], route.id);
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
        assert!(!response.contains(&token), "{response}");
        assert!(!response.contains(&shaped), "{response}");
        assert!(task.matches("[REDACTED]").count() >= 2, "{task}");
        let content = result["content"][0]["text"].as_str().unwrap();
        assert!(!content.contains(&token));
        assert!(!content.contains(&shaped));
        assert!(content.matches("[REDACTED]").count() >= 2, "{content}");
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
        let first = mint_token();
        let second = mint_token();

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
        std::fs::create_dir_all(root.path().join(".nosis").join("fleet").join(starting_id))
            .unwrap();
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
                json!({"tasks": [{"task": "fixture task", "model": "missing-route"}]}),
            ),
        );

        assert!(response.contains("fleet run rejected:"), "{response}");
        assert!(!response.contains("fleet run started"), "{response}");
        server.shutdown().unwrap();
    }

    #[test]
    fn fleet_run_preflights_missing_key_and_valid_config_returns_run_id() {
        let root = tempfile::tempdir().unwrap();
        let vault_entry = format!("w2-missing-{}", mint_token());
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

        let missing_key = raw_post(
            addr,
            &host,
            None,
            Some(&token),
            &tools_call("fleet_run", json!({"tasks": [{"task": "native task"}]})),
        );
        let started = raw_post(
            addr,
            &host,
            None,
            Some(&token),
            &tools_call(
                "fleet_run",
                json!({"tasks": [{"task": "swarm task", "backend": "kimi-swarm"}]}),
            ),
        );

        assert!(missing_key.contains("fleet run rejected:"), "{missing_key}");
        assert!(missing_key.contains("run `nh key add"), "{missing_key}");
        assert!(!missing_key.contains("fleet run started"), "{missing_key}");
        assert!(started.contains("fleet run started · run_id="), "{started}");
        server.shutdown().unwrap();
    }
}
