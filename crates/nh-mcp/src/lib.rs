//! Stateless, loopback-only MCP preview server for Nosis routes and fleets.
//!
//! The server mirrors `nh_tools::mcp::McpClient`: blocking JSON-RPC over HTTP,
//! no initialize handshake, no sessions, and durable run IDs as ordinary handles.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{bail, Context as _};
use chrono::{Local, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

const PREVIEW_NOTICE: &str =
    "nh-mcp preview — local only; do not expose publicly before the MCP final spec (2026-07-28).";

pub struct ServeConfig {
    pub addr: SocketAddr,
    pub catalog: String,
    pub law: nh_law::Law,
    pub default_route: String,
    pub run_root: PathBuf,
    pub token: Option<String>,
    pub max_workers: usize,
}

struct State {
    catalog: String,
    law: nh_law::Law,
    default_route: String,
    run_root: PathBuf,
    token: Option<String>,
    max_workers: usize,
}

impl From<ServeConfig> for State {
    fn from(config: ServeConfig) -> Self {
        Self {
            catalog: config.catalog,
            law: config.law,
            default_route: config.default_route,
            run_root: config.run_root,
            token: config.token,
            max_workers: config.max_workers,
        }
    }
}

pub struct McpServer {
    addr: SocketAddr,
    shutdown: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}

impl McpServer {
    /// Bind first, then run the blocking accept loop on a background thread.
    pub fn start(config: ServeConfig) -> anyhow::Result<McpServer> {
        let (server, addr, state) = bind(config)?;
        let shutdown = Arc::new(AtomicBool::new(false));
        let loop_shutdown = Arc::clone(&shutdown);
        let handle = thread::Builder::new()
            .name("nh-mcp".into())
            .spawn(move || accept_loop(server, state, loop_shutdown))
            .context("could not start the nh-mcp server thread")?;
        Ok(Self {
            addr,
            shutdown,
            handle,
        })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
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
    let (server, addr, state) = bind(config)?;
    print_banner(addr, &state);
    accept_loop(server, state, Arc::new(AtomicBool::new(false)));
    Ok(())
}

fn bind(config: ServeConfig) -> anyhow::Result<(Server, SocketAddr, Arc<State>)> {
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
    Ok((server, addr, Arc::new(State::from(config))))
}

fn print_banner(addr: SocketAddr, state: &State) {
    let scrubber = scrubber(state);
    println!("{}", nh_vault::safe_line(&scrubber, PREVIEW_NOTICE));
    println!(
        "{}",
        nh_vault::safe_line(
            &scrubber,
            &format!(
                "listening on http://{addr}/mcp   (tools: route_resolve, fleet_run, fleet_status)"
            ),
        )
    );
}

fn accept_loop(server: Server, state: Arc<State>, shutdown: Arc<AtomicBool>) {
    while !shutdown.load(Ordering::Relaxed) {
        match server.recv_timeout(Duration::from_millis(100)) {
            Ok(Some(request)) => handle(request, &state),
            Ok(None) => continue,
            Err(_) => break,
        }
    }
}

fn handle(mut request: Request, state: &State) {
    if !authorized(&request, state) {
        respond_json(request, state, 401, &json!({}));
        return;
    }

    if request.method() == &Method::Get {
        if request.url().ends_with("/.well-known/mcp.json") {
            respond_json(request, state, 200, &business_card());
        } else {
            respond_json(request, state, 404, &json!({}));
        }
        return;
    }

    if request.method() != &Method::Post {
        respond_json(request, state, 404, &json!({}));
        return;
    }

    let mut body = String::new();
    if request.as_reader().read_to_string(&mut body).is_err() {
        respond_json(
            request,
            state,
            200,
            &rpc_error(Value::Null, -32700, "parse error"),
        );
        return;
    }
    let message: Value = match serde_json::from_str(&body) {
        Ok(message) => message,
        Err(_) => {
            respond_json(
                request,
                state,
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
            tools_call(message.get("params").unwrap_or(&Value::Null), state),
        ),
        _ => rpc_error(id, -32601, "method not found"),
    };
    respond_json(request, state, 200, &response);
}

fn authorized(request: &Request, state: &State) -> bool {
    let Some(token) = state.token.as_deref() else {
        return true;
    };
    let expected = format!("Bearer {token}");
    request
        .headers()
        .iter()
        .any(|header| header.field.equiv("Authorization") && header.value.as_str() == expected)
}

fn business_card() -> Value {
    json!({
        "name": "nh-mcp",
        "spec": "2026-07-28",
        "tools": ["route_resolve", "fleet_run", "fleet_status"],
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
                "annotations": { "readOnlyHint": true }
            }
        ],
        "_meta": { "ttlMs": 60000 }
    })
}

fn tools_call(params: &Value, state: &State) -> Value {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return tool_error(state, "tools/call needs a tool name");
    };
    let empty_arguments = json!({});
    let arguments = params.get("arguments").unwrap_or(&empty_arguments);
    match name {
        "route_resolve" => route_resolve(arguments, state),
        "fleet_run" => fleet_run(arguments, state),
        "fleet_status" => fleet_status(arguments, state),
        other => tool_error(state, &format!("unknown tool '{other}' — use tools/list")),
    }
}

#[derive(Deserialize)]
struct RouteResolveArgs {
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    prefer_offpeak: Option<bool>,
}

fn route_resolve(arguments: &Value, state: &State) -> Value {
    let args: RouteResolveArgs = match serde_json::from_value(arguments.clone()) {
        Ok(args) => args,
        Err(error) => return tool_error(state, &error.to_string()),
    };
    let resolver = match nh_routes::RouteResolver::from_toml(&state.catalog) {
        Ok(resolver) => resolver,
        Err(error) => return tool_error(state, &error.to_string()),
    };
    let route = match resolver.resolve(args.model.as_deref().unwrap_or(&state.default_route)) {
        Ok(route) => route,
        Err(error) => return tool_error(state, &error.to_string()),
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
    tool_text(state, &text, false)
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

fn fleet_run(arguments: &Value, state: &State) -> Value {
    let args: FleetRunArgs = match serde_json::from_value(arguments.clone()) {
        Ok(args) => args,
        Err(error) => return tool_error(state, &error.to_string()),
    };
    if args.tasks.is_empty() {
        return tool_error(state, "fleet_run needs a non-empty tasks array");
    }
    let max_workers = args.max_workers.unwrap_or(state.max_workers);
    if max_workers == 0 {
        return tool_error(state, "fleet_run max_workers must be at least 1");
    }
    let resolver = match nh_routes::RouteResolver::from_toml(&state.catalog) {
        Ok(resolver) => resolver,
        Err(error) => return tool_error(state, &error.to_string()),
    };
    let task_count = args.tasks.len();
    let run_id = nh_fleet::new_run_id();
    let config = nh_fleet::FleetConfig {
        resolver,
        law: state.law.clone(),
        default_route: state.default_route.clone(),
        tasks: args.tasks,
        max_workers,
        budget_tokens: args.budget,
        clock: None,
        defer_offpeak: args.defer_offpeak.unwrap_or(false),
        ladder: None,
        escalate_on_partial: false,
        swarm: None,
        run_root: state.run_root.clone(),
        on_event: None,
    };
    let id = run_id.clone();
    thread::spawn(move || {
        let _ = nh_fleet::run_with_id(id, config);
    });
    tool_text(
        state,
        &format!("fleet run started · run_id={run_id} · {task_count} tasks"),
        false,
    )
}

#[derive(Deserialize)]
struct FleetStatusArgs {
    run_id: String,
}

fn fleet_status(arguments: &Value, state: &State) -> Value {
    let args: FleetStatusArgs = match serde_json::from_value(arguments.clone()) {
        Ok(args) => args,
        Err(_) => return tool_error(state, "fleet_status needs a run_id"),
    };
    let events = match nh_fleet::read_run_ledger(&state.run_root, &args.run_id) {
        Ok(events) => events,
        Err(error) => return tool_error(state, &error.to_string()),
    };
    let status = nh_fleet::status_from_ledger(&events);
    let state_word = if status.finished {
        "finished"
    } else if events.is_empty() {
        "starting"
    } else {
        "running"
    };
    tool_text(
        state,
        &format!(
            "{} · {} · {} done · {} failed · {} gated · {} pending",
            args.run_id, state_word, status.done, status.failed, status.gated, status.pending
        ),
        false,
    )
}

fn tool_error(state: &State, message: &str) -> Value {
    tool_text(state, message, true)
}

fn tool_text(state: &State, text: &str, is_error: bool) -> Value {
    let text = nh_vault::safe_line(&scrubber(state), text);
    json!({
        "content": [{ "type": "text", "text": text }],
        "isError": is_error
    })
}

fn scrubber(state: &State) -> nh_vault::Scrubber {
    nh_vault::Scrubber::new(state.token.iter().cloned().collect())
}

fn respond_json(request: Request, state: &State, status: u16, value: &Value) {
    let scrubber = scrubber(state);
    let mut value = value.clone();
    scrub_json(&mut value, &scrubber);
    let body = serde_json::to_string(&value).unwrap_or_else(|_| "{}".into());
    let body = scrubber.scrub(&body);
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
