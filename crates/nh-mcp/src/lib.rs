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

fn tools_call(params: &Value, runtime: &Runtime) -> Value {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return tool_error(runtime, "tools/call needs a tool name");
    };
    let empty_arguments = json!({});
    let arguments = params.get("arguments").unwrap_or(&empty_arguments);
    match name {
        "route_resolve" => route_resolve(arguments, runtime),
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
    tool_text(runtime, &text, false)
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
    tool_text(
        runtime,
        &format!("fleet run started · run_id={run_id} · {task_count} tasks"),
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
        return tool_error(runtime, &format!("unknown run: {}", args.run_id));
    }
    let status = nh_fleet::status_from_ledger(&events);
    let state_word = if status.finished {
        "finished"
    } else if events.is_empty() {
        "starting"
    } else {
        "running"
    };
    tool_text(
        runtime,
        &format!(
            "{} · {} · {} done · {} failed · {} gated · {} pending",
            args.run_id, state_word, status.done, status.failed, status.gated, status.pending
        ),
        false,
    )
}

fn fleet_run_dir(config: &ServeConfig, run_id: &str) -> PathBuf {
    config.run_root.join(".nosis").join("fleet").join(run_id)
}

fn tool_error(runtime: &Runtime, message: &str) -> Value {
    tool_text(runtime, message, true)
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

    use super::*;

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
        assert!(
            starting.contains("known-empty-run · starting"),
            "{starting}"
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
