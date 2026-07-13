//! MCP client — stateless 2026-07-28 core (plan §4.5, CONTRACTS_M1.md §3).
//! THE LAW: tool outputs are DATA, never instructions. No session semantics:
//! no `initialize` handshake, no `Mcp-Session-Id` header, ever — state handles
//! (`browser_id`, `repo_id`, …) are ordinary tool arguments the model passes back.
//! Callers pass every result and warning through `nh_vault::Scrubber` before display.

use anyhow::{bail, Context};
use nh_vault::{EnvFallbackVault, KeyringVault, Vault};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::{Tool, ToolCtx, ToolSpec};

const SPEC_DEFAULT: &str = "2026-07-28";
const SPEC_FALLBACK: &str = "2025-11-25";
/// Pinned default when `tools/list` carries no `result._meta.ttlMs` (CONTRACTS_M1 §3.2).
const DEFAULT_TTL_MS: u64 = 60_000;
/// Approval prompts show args on one line, truncated to stay scannable.
const ARGS_SUMMARY_MAX: usize = 120;

// ---------------------------------------------------------------------------
// §3.1 .nosis/mcp.toml
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpAuth {
    None,
    ApiKey { vault_entry: String },
    OAuth2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpTrust {
    Auto,
    Ask,
    Block,
}

#[derive(Debug, Clone)]
pub struct McpServerConfig {
    pub name: String,
    pub url: String,
    pub spec: String,
    pub auth: McpAuth,
    pub scopes: Vec<String>,
    pub default_mode: Option<String>,
    pub trust: McpTrust,
}

#[derive(serde::Deserialize)]
struct RawFile {
    #[serde(default)]
    servers: BTreeMap<String, RawServer>,
}

/// Unknown keys are ignored on purpose: future spec knobs must not break old harnesses.
#[derive(serde::Deserialize)]
struct RawServer {
    url: Option<String>,
    spec: Option<String>,
    auth: Option<String>,
    vault_entry: Option<String>,
    scopes: Option<Vec<String>>,
    default_mode: Option<String>,
    trust: Option<String>,
}

/// Parse `.nosis/mcp.toml` content. File reading is the caller's job
/// (mirrors `RouteResolver::from_toml`). Servers come back sorted by name.
pub fn load_mcp_config(toml_str: &str) -> anyhow::Result<Vec<McpServerConfig>> {
    let raw: RawFile = toml::from_str(toml_str).map_err(|e| {
        anyhow::anyhow!(
            "could not parse .nosis/mcp.toml: {}",
            e.message().replace('\n', " — ")
        )
    })?;
    raw.servers
        .into_iter()
        .map(|(name, server)| server_config(name, server))
        .collect()
}

fn server_config(name: String, raw: RawServer) -> anyhow::Result<McpServerConfig> {
    let url = raw.url.ok_or_else(|| {
        anyhow::anyhow!(
            "mcp server \"{name}\": missing url — add url = \"http://host:port/mcp\" to .nosis/mcp.toml"
        )
    })?;
    let spec = raw.spec.unwrap_or_else(|| SPEC_DEFAULT.to_string());
    if spec != SPEC_DEFAULT && spec != SPEC_FALLBACK {
        bail!(
            "mcp server \"{name}\": unknown spec \"{spec}\" — use \"{SPEC_DEFAULT}\" (default) or \"{SPEC_FALLBACK}\""
        );
    }
    let auth = match raw.auth.as_deref().unwrap_or("none") {
        "none" => McpAuth::None,
        "apikey" => McpAuth::ApiKey {
            vault_entry: raw.vault_entry.ok_or_else(|| {
                anyhow::anyhow!(
                    "mcp server \"{name}\": auth = \"apikey\" needs vault_entry — add vault_entry = \"{name}\" and run `nh key add {name}`"
                )
            })?,
        },
        "oauth2" => McpAuth::OAuth2,
        other => bail!(
            "mcp server \"{name}\": unknown auth \"{other}\" — use \"none\", \"apikey\", or \"oauth2\""
        ),
    };
    let trust = match raw.trust.as_deref().unwrap_or("ask") {
        "auto" => McpTrust::Auto,
        "ask" => McpTrust::Ask,
        "block" => McpTrust::Block,
        other => bail!(
            "mcp server \"{name}\": unknown trust \"{other}\" — use \"auto\", \"ask\", or \"block\""
        ),
    };
    Ok(McpServerConfig {
        name,
        url,
        spec,
        auth,
        scopes: raw.scopes.unwrap_or_default(),
        default_mode: raw.default_mode,
        trust,
    })
}

// ---------------------------------------------------------------------------
// §3.2–§3.5 McpClient
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct McpToolInfo {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// A listed tool plus the server's read-only annotation (drives the trust gate).
#[derive(Clone)]
struct ToolEntry {
    info: McpToolInfo,
    read_only: bool,
}

struct ToolCache {
    expires_at: Instant,
    entries: Vec<ToolEntry>,
}

/// Blocking JSON-RPC 2.0 over Streamable HTTP POST. Stateless per the 2026-07-28
/// core: every request is self-contained (`_meta` carries protocol version,
/// client info, capabilities). Cache is interior so `&self` works and the type
/// stays `Send + Sync`.
pub struct McpClient {
    config: McpServerConfig,
    http: reqwest::blocking::Client,
    next_id: AtomicU64,
    cache: Mutex<Option<ToolCache>>,
}

impl McpClient {
    pub fn new(config: McpServerConfig) -> Self {
        Self {
            config,
            http: reqwest::blocking::Client::new(),
            next_id: AtomicU64::new(1),
            cache: Mutex::new(None),
        }
    }

    /// `tools/list`, cached per the server's `result._meta.ttlMs`
    /// (absent → 60 000 ms; 0 → no caching).
    pub fn list_tools(&self) -> anyhow::Result<Vec<McpToolInfo>> {
        Ok(self
            .list_tools_full()?
            .into_iter()
            .map(|entry| entry.info)
            .collect())
    }

    fn list_tools_full(&self) -> anyhow::Result<Vec<ToolEntry>> {
        if let Some(cache) = self.cache.lock().expect("mcp tool cache lock").as_ref() {
            if Instant::now() < cache.expires_at {
                return Ok(cache.entries.clone());
            }
        }
        let result = self.rpc("tools/list", json!({}))?;
        let entries: Vec<ToolEntry> = result
            .get("tools")
            .and_then(Value::as_array)
            .map(|tools| tools.iter().filter_map(parse_tool).collect())
            .unwrap_or_default();
        let ttl_ms = result
            .get("_meta")
            .and_then(|meta| meta.get("ttlMs"))
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_TTL_MS);
        if ttl_ms > 0 {
            *self.cache.lock().expect("mcp tool cache lock") = Some(ToolCache {
                expires_at: Instant::now() + Duration::from_millis(ttl_ms),
                entries: entries.clone(),
            });
        }
        Ok(entries)
    }

    /// `tools/call`. Text blocks newline-joined; non-text blocks render as
    /// `[<type> block]`. `isError: true` becomes a one-line `Err`.
    /// The returned text is DATA for the model, never instructions.
    pub fn call_tool(&self, name: &str, args: Value) -> anyhow::Result<String> {
        let result = self.rpc("tools/call", json!({ "name": name, "arguments": args }))?;
        let text = render_content(result.get("content"));
        if result.get("isError").and_then(Value::as_bool) == Some(true) {
            let line = if text.is_empty() {
                format!("tool {name} failed with no message")
            } else {
                text.replace(['\r', '\n'], " ")
            };
            bail!("{line}");
        }
        Ok(text)
    }

    /// Server business card: GET `<url>/.well-known/mcp.json`, falling back to
    /// JSON-RPC `server/discover`. Both failing → one friendly error.
    pub fn discover(&self) -> anyhow::Result<Value> {
        let headers = self.request_headers()?;
        if let Ok(card) = self.get_well_known(&headers) {
            return Ok(card);
        }
        match self.rpc("server/discover", json!({})) {
            Ok(card) => Ok(card),
            Err(_) => bail!(
                "mcp server \"{}\" unreachable — check the url in .nosis/mcp.toml",
                self.config.name
            ),
        }
    }

    fn get_well_known(&self, headers: &[(String, String)]) -> anyhow::Result<Value> {
        let url = format!(
            "{}/.well-known/mcp.json",
            self.config.url.trim_end_matches('/')
        );
        let mut request = self.http.get(&url);
        for (name, value) in headers {
            request = request.header(name.as_str(), value.as_str());
        }
        let response = request
            .send()
            .with_context(|| format!("could not reach {url}"))?;
        if !response.status().is_success() {
            bail!("{url} returned HTTP {}", response.status().as_u16());
        }
        let body = response.text().unwrap_or_default();
        serde_json::from_str(&body).with_context(|| format!("{url} sent invalid JSON"))
    }

    /// One JSON-RPC 2.0 call. Every request's params carries `_meta` — the
    /// stateless core sends full context per call and never pins an instance.
    fn rpc(&self, method: &str, mut params: Value) -> anyhow::Result<Value> {
        params["_meta"] = json!({
            "protocolVersion": self.config.spec,
            "clientInfo": { "name": "nosis-harness", "version": env!("CARGO_PKG_VERSION") },
            "capabilities": {}
        });
        let body = json!({
            "jsonrpc": "2.0",
            "id": self.next_id.fetch_add(1, Ordering::Relaxed),
            "method": method,
            "params": params
        });
        let headers = self.request_headers()?;
        let url = &self.config.url;
        let mut request = self.http.post(url).json(&body);
        for (name, value) in &headers {
            request = request.header(name.as_str(), value.as_str());
        }
        let response = request
            .send()
            .map_err(|e| anyhow::anyhow!("could not reach {url}: {e}"))?;
        let status = response.status();
        if !status.is_success() {
            let hint = match status.as_u16() {
                401 | 403 => " — key rejected; check vault_entry in .nosis/mcp.toml",
                429 => " — rate limited; retry later",
                _ => "",
            };
            bail!("{url} returned HTTP {}{hint}", status.as_u16());
        }
        let text = response.text().unwrap_or_default();
        let reply: Value = serde_json::from_str(&text)
            .map_err(|_| anyhow::anyhow!("{url} sent invalid JSON — is it an MCP endpoint?"))?;
        if let Some(error) = reply.get("error") {
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown error");
            bail!("server error: {message}");
        }
        Ok(reply.get("result").cloned().unwrap_or(Value::Null))
    }

    /// Auth (§3.4) + outbound header lint (§3.5) — the single choke point every
    /// send passes through. The key is fetched per call, zeroized, never logged.
    fn request_headers(&self) -> anyhow::Result<Vec<(String, String)>> {
        let mut headers = Vec::new();
        match &self.config.auth {
            McpAuth::None => {}
            McpAuth::OAuth2 => bail!("oauth2 arrives in M4 — use apikey or none for now"),
            McpAuth::ApiKey { vault_entry } => {
                let vault = EnvFallbackVault {
                    inner: KeyringVault,
                };
                let secret = vault.get(vault_entry)?;
                headers.push((
                    "authorization".to_string(),
                    format!("Bearer {}", secret.as_str()),
                ));
            }
        }
        lint_headers(&headers)?;
        Ok(headers)
    }
}

fn parse_tool(tool: &Value) -> Option<ToolEntry> {
    let name = tool.get("name")?.as_str()?.to_string();
    let description = tool
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let input_schema = tool
        .get("inputSchema")
        .cloned()
        .unwrap_or_else(|| json!({ "type": "object" }));
    let read_only = tool
        .get("annotations")
        .and_then(|a| a.get("readOnlyHint"))
        .and_then(Value::as_bool)
        == Some(true);
    Some(ToolEntry {
        info: McpToolInfo {
            name,
            description,
            input_schema,
        },
        read_only,
    })
}

fn render_content(content: Option<&Value>) -> String {
    let Some(blocks) = content.and_then(Value::as_array) else {
        return String::new();
    };
    blocks
        .iter()
        .map(|block| match block.get("type").and_then(Value::as_str) {
            Some("text") => block
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            Some(other) => format!("[{other} block]"),
            None => "[unknown block]".to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Outbound header lint (plan §4.5 — the Akamai leak vector, closed): a
/// secret-shaped value in an `Mcp-*` / `x-mcp-*` header is refused before send.
/// `Authorization` is the one sanctioned credential channel and is exempt.
fn lint_headers(headers: &[(String, String)]) -> anyhow::Result<()> {
    for (name, value) in headers {
        let lower = name.to_ascii_lowercase();
        if lower == "authorization" {
            continue;
        }
        let mcp_shaped = lower.starts_with("mcp-") || lower.starts_with("x-mcp-");
        if mcp_shaped && looks_like_secret(value) {
            bail!(
                "refused to send header \"{name}\" — its value looks like a secret; keys go through nh-vault (auth = \"apikey\"), never MCP headers"
            );
        }
    }
    Ok(())
}

/// Same key shapes the Scrubber redacts (`sk-…`, `csk-…`, JWT) — one source of truth.
fn looks_like_secret(value: &str) -> bool {
    static SCRUBBER: OnceLock<nh_vault::Scrubber> = OnceLock::new();
    let scrubber = SCRUBBER.get_or_init(|| nh_vault::Scrubber::new(vec![]));
    scrubber.scrub(value) != value
}

// ---------------------------------------------------------------------------
// §3.6 Tool adapters
// ---------------------------------------------------------------------------

/// Adapters for every configured server, plus one friendly warning line per
/// server whose tools could not be listed (never a hard failure).
pub struct McpToolset {
    pub tools: Vec<Box<dyn Tool>>,
    pub warnings: Vec<String>,
}

/// Build one adapter per server tool, named `mcp__<server>__<tool>`.
/// `trust = "block"` servers are never contacted and offer no tools.
pub fn mcp_tools(configs: &[McpServerConfig]) -> McpToolset {
    let mut tools: Vec<Box<dyn Tool>> = Vec::new();
    let mut warnings = Vec::new();
    for config in configs {
        if config.trust == McpTrust::Block {
            continue;
        }
        let server = config.name.clone();
        let trust = config.trust;
        let client = Arc::new(McpClient::new(config.clone()));
        match client.list_tools_full() {
            Ok(entries) => {
                for entry in entries {
                    tools.push(Box::new(McpToolAdapter {
                        server: server.clone(),
                        trust,
                        entry,
                        client: Arc::clone(&client),
                    }));
                }
            }
            Err(e) => warnings.push(format!("mcp server \"{server}\": {e}")),
        }
    }
    McpToolset { tools, warnings }
}

struct McpToolAdapter {
    server: String,
    trust: McpTrust,
    entry: ToolEntry,
    client: Arc<McpClient>,
}

impl Tool for McpToolAdapter {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: format!("mcp__{}__{}", self.server, self.entry.info.name),
            description: format!("[MCP {}] {}", self.server, self.entry.info.description),
            parameters: self.entry.info.input_schema.clone(),
        }
    }

    fn execute(&self, args: Value, ctx: &ToolCtx) -> anyhow::Result<String> {
        let tool = &self.entry.info.name;
        match self.trust {
            McpTrust::Block => {
                return Ok(
                    "blocked by .nosis/mcp.toml (trust = \"block\") — set trust = \"ask\" to enable"
                        .to_string(),
                );
            }
            // Only server-annotated read-only tools skip the gate on auto.
            McpTrust::Auto if self.entry.read_only => {}
            // Ask, and every possibly state-mutating call at any autonomy level.
            _ => {
                let ask = format!("mcp {} {} {}", self.server, tool, args_one_line(&args));
                if !(ctx.approve)(&ask) {
                    // Ok-shaped so the model can read the denial and adapt.
                    return Ok(format!("user denied: mcp {} {}", self.server, tool));
                }
            }
        }
        self.client.call_tool(tool, args)
    }
}

/// Compact JSON args on one line, truncated so approval prompts stay scannable.
fn args_one_line(args: &Value) -> String {
    let compact = serde_json::to_string(args).unwrap_or_else(|_| "{}".to_string());
    if compact.chars().count() <= ARGS_SUMMARY_MAX {
        return compact;
    }
    let truncated: String = compact.chars().take(ARGS_SUMMARY_MAX).collect();
    format!("{truncated}…")
}

// ---------------------------------------------------------------------------
// Tests — every server is a local hand-rolled mock; no live calls.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::path::PathBuf;

    // ---- tiny hand-rolled HTTP mock (std only) ----

    struct Recorded {
        method: String,
        path: String,
        head: String,
        body: Value,
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
        Some(Recorded {
            method,
            path,
            head,
            body: serde_json::from_slice(&body).unwrap_or(Value::Null),
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

    fn approving_ctx(answer: bool) -> (ToolCtx, Arc<Mutex<Vec<String>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let record = Arc::clone(&seen);
        let ctx = ToolCtx {
            workdir: PathBuf::from("."),
            approve: Box::new(move |description| {
                record.lock().unwrap().push(description.to_string());
                answer
            }),
        };
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
"#;
        let configs = load_mcp_config(toml_str).unwrap();
        assert_eq!(configs[0].auth, McpAuth::OAuth2);
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
    fn config_missing_url_is_actionable() {
        let err = load_mcp_config("[servers.a]\ntrust = \"ask\"")
            .unwrap_err()
            .to_string();
        assert!(err.contains("mcp server \"a\": missing url"), "got: {err}");
    }

    #[test]
    fn config_bad_toml_is_one_friendly_line() {
        let err = load_mcp_config("not = = toml").unwrap_err().to_string();
        assert!(err.starts_with("could not parse .nosis/mcp.toml"), "got: {err}");
        assert!(!err.contains('\n'), "must be one line, got: {err}");
    }

    // ---- §3.3 statelessness (M1 exit criterion) ----

    #[test]
    fn stateless_invariant_no_session_no_initialize_meta_on_every_request() {
        let mock = start_mock(full_responder);
        let client = McpClient::new(config(&mock.url, McpTrust::Ask));

        client.list_tools().unwrap();
        let opened = client.call_tool("browser_open", json!({})).unwrap();
        assert!(opened.contains("b-42"));
        client
            .call_tool("browser_click", json!({ "browser_id": "b-42" }))
            .unwrap();
        client.discover().unwrap();

        let recorded = mock.recorded.lock().unwrap();
        assert!(recorded.len() >= 5, "expected list + 2 calls + GET + fallback POST");
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
                assert_eq!(meta["clientInfo"]["version"], json!(env!("CARGO_PKG_VERSION")));
                assert_eq!(meta["capabilities"], json!({}));
            }
        }
    }

    #[test]
    fn fallback_spec_is_echoed_in_meta() {
        let mock = start_mock(|req| rpc_result(req, mock_tools_result(None)));
        let mut cfg = config(&mock.url, McpTrust::Ask);
        cfg.spec = SPEC_FALLBACK.into();
        McpClient::new(cfg).list_tools().unwrap();
        let recorded = mock.recorded.lock().unwrap();
        assert_eq!(
            recorded[0].body["params"]["_meta"]["protocolVersion"],
            json!(SPEC_FALLBACK)
        );
    }

    #[test]
    fn handle_passes_back_as_ordinary_argument() {
        let mock = start_mock(full_responder);
        let client = McpClient::new(config(&mock.url, McpTrust::Ask));
        let opened = client.call_tool("browser_open", json!({})).unwrap();
        assert_eq!(opened, "browser_id: b-42");
        // The model reads the handle and passes it back as a plain argument —
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
        let client = McpClient::new(config(&mock.url, McpTrust::Ask));
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
        let client = McpClient::new(config(&mock.url, McpTrust::Ask));
        client.list_tools().unwrap();
        client.list_tools().unwrap();
        assert_eq!(count_method(&mock, "tools/list"), 2);
    }

    #[test]
    fn tools_list_absent_ttl_defaults_to_60s_cache() {
        let mock = start_mock(|req| rpc_result(req, mock_tools_result(None)));
        let client = McpClient::new(config(&mock.url, McpTrust::Ask));
        client.list_tools().unwrap();
        client.list_tools().unwrap();
        assert_eq!(count_method(&mock, "tools/list"), 1);
    }

    #[test]
    fn tools_list_cache_expires_after_ttl() {
        let mock = start_mock(|req| rpc_result(req, mock_tools_result(Some(1))));
        let client = McpClient::new(config(&mock.url, McpTrust::Ask));
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
        let client = McpClient::new(config(&mock.url, McpTrust::Ask));
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
        let client = McpClient::new(config(&mock.url, McpTrust::Ask));
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
        let client = McpClient::new(config(&mock.url, McpTrust::Ask));
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
        let client = McpClient::new(config(&mock.url, McpTrust::Ask));
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
        let client = McpClient::new(config(&mock.url, McpTrust::Ask));
        let card = client.discover().unwrap();
        assert_eq!(card["name"], json!("mock-server"));
        let recorded = mock.recorded.lock().unwrap();
        assert_eq!(recorded.len(), 2);
        assert_eq!(recorded[0].method, "GET");
        assert_eq!(recorded[1].body["method"], json!("server/discover"));
    }

    #[test]
    fn discover_unreachable_is_one_friendly_error() {
        let client = McpClient::new(config(&refused_url(), McpTrust::Ask));
        let err = client.discover().unwrap_err().to_string();
        assert!(
            err.contains("unreachable — check the url in .nosis/mcp.toml"),
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
        McpClient::new(cfg).list_tools().unwrap();
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
        McpClient::new(config(&mock.url, McpTrust::Ask))
            .list_tools()
            .unwrap();
        let recorded = mock.recorded.lock().unwrap();
        assert!(
            !recorded[0].head.to_ascii_lowercase().contains("authorization:"),
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
        let err = McpClient::new(cfg).list_tools().unwrap_err().to_string();
        assert!(err.contains("nh key add mcp-missing-test"), "got: {err}");
    }

    #[test]
    fn oauth2_is_deferred_to_m4_with_one_message() {
        let mut cfg = config("http://127.0.0.1:1/mcp", McpTrust::Ask);
        cfg.auth = McpAuth::OAuth2;
        let client = McpClient::new(cfg);
        let expected = "oauth2 arrives in M4 — use apikey or none for now";
        assert_eq!(client.list_tools().unwrap_err().to_string(), expected);
        assert_eq!(
            client.call_tool("x", json!({})).unwrap_err().to_string(),
            expected
        );
        assert_eq!(client.discover().unwrap_err().to_string(), expected);
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
        let set = mcp_tools(&[config(&mock.url, McpTrust::Ask)]);
        assert!(set.warnings.is_empty(), "warnings: {:?}", set.warnings);
        let specs: Vec<ToolSpec> = set.tools.iter().map(|t| t.spec()).collect();
        assert_eq!(
            specs.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
            ["mcp__mock__peek", "mcp__mock__shout"]
        );
        assert_eq!(specs[0].description, "[MCP mock] Look at the page.");
        assert_eq!(specs[0].parameters["properties"]["sel"]["type"], json!("string"));
    }

    #[test]
    fn trust_ask_gates_every_call_and_denial_is_ok_shaped() {
        let mock = start_mock(full_responder);
        let set = mcp_tools(&[config(&mock.url, McpTrust::Ask)]);
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
    fn trust_auto_read_only_skips_gate_mutating_still_asks() {
        let mock = start_mock(full_responder);
        let set = mcp_tools(&[config(&mock.url, McpTrust::Auto)]);
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
        assert!(seen.lock().unwrap().is_empty(), "read-only must skip the gate");

        // No read-only annotation: still asks, at every autonomy level.
        let (ctx, seen) = approving_ctx(false);
        let out = shout.execute(json!({ "text": "hi" }), &ctx).unwrap();
        assert_eq!(out, "user denied: mcp mock shout");
        assert_eq!(seen.lock().unwrap().len(), 1);
    }

    #[test]
    fn trust_block_offers_no_tools_and_never_contacts_the_server() {
        let mock = start_mock(full_responder);
        let set = mcp_tools(&[config(&mock.url, McpTrust::Block)]);
        assert!(set.tools.is_empty(), "blocked tools must not be offered");
        assert!(set.warnings.is_empty());
        assert!(
            mock.recorded.lock().unwrap().is_empty(),
            "blocked server must never be contacted"
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
            client: Arc::new(McpClient::new(config("http://127.0.0.1:1/mcp", McpTrust::Block))),
        };
        let (ctx, seen) = approving_ctx(true);
        let out = adapter.execute(json!({}), &ctx).unwrap();
        assert_eq!(
            out,
            "blocked by .nosis/mcp.toml (trust = \"block\") — set trust = \"ask\" to enable"
        );
        assert!(seen.lock().unwrap().is_empty());
    }

    #[test]
    fn failing_server_contributes_warning_not_failure() {
        let mut cfg = config(&refused_url(), McpTrust::Ask);
        cfg.name = "downbeat".into();
        let set = mcp_tools(&[cfg]);
        assert!(set.tools.is_empty());
        assert_eq!(set.warnings.len(), 1);
        assert!(
            set.warnings[0].contains("mcp server \"downbeat\""),
            "got: {}",
            set.warnings[0]
        );
    }

    #[test]
    fn approval_summary_is_one_truncated_line() {
        let long = "x".repeat(500);
        let summary = args_one_line(&json!({ "data": long }));
        assert!(summary.chars().count() <= ARGS_SUMMARY_MAX + 1);
        assert!(summary.ends_with('…'));
        assert!(!summary.contains('\n'));
    }

    #[test]
    fn mcp_client_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<McpClient>();
    }
}
