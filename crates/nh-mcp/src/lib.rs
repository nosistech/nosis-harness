//! Stateless, loopback-only MCP preview server for Nosis routes and fleets.
//!
//! The server mirrors `nh_tools::mcp::McpClient`: blocking JSON-RPC over HTTP,
//! no initialize handshake, no sessions, and durable run IDs as ordinary handles.

mod fleet_tools;
mod protocol;
mod receipts;
mod response;
mod route_tools;

use fleet_tools::*;
use protocol::*;
use receipts::*;
use response::*;
use route_tools::*;

use std::collections::{BTreeSet, VecDeque};
use std::fs::File;
use std::io::{Read as _, Seek as _, SeekFrom};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{bail, Context as _};
use chrono::{Local, Utc};
use nh_core::credential;
use nh_vault::{SecretRegistry, SecretValue};
use serde::Deserialize;
use serde_json::{json, Value};
use subtle::ConstantTimeEq;
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

const PREVIEW_NOTICE: &str =
    "nh-mcp preview - local only; do not expose publicly before the MCP final spec (2026-07-28).";
const MAX_BODY_BYTES: usize = 1024 * 1024;
const MAX_RECEIPT_TAIL_BYTES: usize = 8 * 1024 * 1024;
const MAX_MCP_FLEET_BUDGET_TOKENS: u64 = 1_000_000;
const MIN_CALLER_TOKEN_BYTES: usize = 32;
const MAX_ACTIVE_RUNS: usize = 4;

pub struct ServeConfig {
    pub addr: SocketAddr,
    pub catalog: String,
    pub law: nh_law::Law,
    pub default_route: String,
    pub run_root: PathBuf,
    pub token: Option<SecretValue>,
    pub max_workers: usize,
}

struct Runtime {
    config: Arc<ServeConfig>,
    token: SecretValue,
    token_generated: bool,
    scrubber: nh_vault::Scrubber,
    active_runs: Arc<AtomicUsize>,
}

struct ActiveRunGuard(Arc<AtomicUsize>);

impl Drop for ActiveRunGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

pub struct McpServer {
    addr: SocketAddr,
    token: SecretValue,
    shutdown: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
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
            handle: Some(handle),
        })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn token(&self) -> &str {
        self.token.as_str()
    }

    pub fn shutdown(mut self) -> anyhow::Result<()> {
        self.shutdown.store(true, Ordering::Relaxed);
        match self.handle.take() {
            Some(handle) => handle
                .join()
                .map_err(|_| anyhow::anyhow!("nh-mcp server thread panicked")),
            None => Ok(()),
        }
    }
}

impl Drop for McpServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
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
        bail!("nh-mcp only binds 127.0.0.1 - use 127.0.0.1:PORT");
    }
    let requested = config.addr;
    let server = Server::http(requested)
        .map_err(|error| anyhow::anyhow!("could not bind nh-mcp to {requested}: {error}"))?;
    let addr = server
        .server_addr()
        .to_ip()
        .ok_or_else(|| anyhow::anyhow!("nh-mcp did not bind a TCP address"))?;
    config.addr = addr;
    let (token, token_generated) = match config.token.take() {
        Some(caller) => {
            if caller.len() < MIN_CALLER_TOKEN_BYTES {
                bail!("nh-mcp caller token must be at least {MIN_CALLER_TOKEN_BYTES} bytes");
            }
            (caller, false)
        }
        None => (mint_token(), true),
    };
    let mut token_registry = SecretRegistry::new();
    token_registry.insert(token.clone());
    let scrubber = token_registry.scrubber();
    let runtime = Runtime {
        config: Arc::new(config),
        token,
        token_generated,
        scrubber,
        active_runs: Arc::new(AtomicUsize::new(0)),
    };
    Ok((server, addr, Arc::new(runtime)))
}

/// Loopback preview token from the operating system CSPRNG; not a long-term credential.
fn mint_token() -> SecretValue {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).expect("OS CSPRNG");
    nh_vault::secret(
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
    )
}

fn print_banner(addr: SocketAddr, runtime: &Runtime) {
    for line in banner_lines(addr, runtime) {
        println!("{line}");
    }
}

fn banner_lines(addr: SocketAddr, runtime: &Runtime) -> [String; 2] {
    let notice = nh_vault::safe_line(&runtime.scrubber, PREVIEW_NOTICE);
    let connect_scrubber = nh_vault::Scrubber::new(Vec::new());
    let credential = if runtime.token_generated {
        format!("Bearer {}", runtime.token.as_str())
    } else {
        "the configured Bearer token (value not printed)".into()
    };
    let connect = nh_vault::safe_line(
        &connect_scrubber,
        &format!(
            "connect http://{addr}/mcp with {credential}   (tools: route_resolve, fleet_run, fleet_status, why, route_cost, receipts)"
        ),
    );
    [notice, connect]
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
    let expected = format!("Bearer {}", runtime.token.as_str());
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

#[cfg(test)]
mod tests;
