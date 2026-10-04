//! Stateless MCP transport, bounded responses, authentication, and tool cache.

mod oauth;
mod response;

use super::config::{McpAuth, McpServerConfig};
use super::json_text::{scrub_complete_json, ScrubbedJson};
use anyhow::{bail, Context as _};
use nh_vault::{EnvFallbackVault, KeyringVault, SecretValue, Vault};
use oauth::OAuthState;
use response::{read_rpc_reply, RpcReply};
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub(super) const SPEC_DEFAULT: &str = "2026-07-28";
/// Pinned default when `tools/list` carries no `result._meta.ttlMs`.
pub(super) const DEFAULT_TTL_MS: u64 = 60_000;
pub(super) const MAX_MCP_BODY_BYTES: usize = 4 * 1024 * 1024;
pub(super) const MAX_TTL_MS: u64 = 24 * 60 * 60 * 1000; // clamp remote cache TTL to 24h
pub(super) const MAX_TOOLS: usize = 512;
/// MCP tool calls (browser runs, long jobs) can legitimately take minutes;
/// reqwest's blocking default would kill them at 30 s. Generous total cap -
/// a dead server still fails fast on connect.
pub(super) const REQUEST_TIMEOUT: Duration = Duration::from_secs(600);
/// Startup discovery must not hold session initialization hostage to a hung server.
#[cfg(not(test))]
pub(super) const STARTUP_LIST_TIMEOUT: Duration = Duration::from_secs(10);
#[cfg(test)]
pub(super) const STARTUP_LIST_TIMEOUT: Duration = Duration::from_millis(250);
pub(super) const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Approval prompts show args on one line and disclose any hidden character count.
pub(super) const ARGS_SUMMARY_MAX: usize = 500;

pub(super) fn read_body_capped(
    resp: reqwest::blocking::Response,
    max: usize,
) -> anyhow::Result<String> {
    use std::io::Read;
    if let Some(len) = resp.content_length() {
        if len > max as u64 {
            anyhow::bail!("mcp response too large: {len} bytes exceeds cap {max}");
        }
    }
    let mut buf = Vec::new();
    resp.take(max as u64 + 1).read_to_end(&mut buf)?;
    if buf.len() > max {
        anyhow::bail!("mcp response exceeded cap of {max} bytes");
    }
    String::from_utf8(buf).context("mcp response was not valid UTF-8")
}

// ---------------------------------------------------------------------------
// §3.1 .nosis/mcp.toml
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// §3.2-§3.5 McpClient
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct McpToolInfo {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// A listed tool plus the server's read-only annotation (drives the trust gate).
#[derive(Clone)]
pub(super) struct ToolEntry {
    pub(super) info: McpToolInfo,
    pub(super) read_only: bool,
}

pub(super) struct ToolCache {
    pub(super) expires_at: Instant,
    pub(super) entries: Vec<ToolEntry>,
}

/// Blocking JSON-RPC 2.0 over Streamable HTTP POST. Stateless per the 2026-07-28
/// core: every request is self-contained (`_meta` carries protocol version,
/// client info, capabilities). Cache is interior so `&self` works and the type
/// stays `Send + Sync`.
pub struct McpClient {
    pub(super) config: McpServerConfig,
    pub(super) http: reqwest::blocking::Client,
    pub(super) startup_http: reqwest::blocking::Client,
    pub(super) next_id: AtomicU64,
    pub(super) cache: Mutex<Option<ToolCache>>,
    pub(super) unsupported_header_tools: Mutex<HashSet<String>>,
    pub(super) oauth: Mutex<OAuthState>,
    pub(super) refresh_lock: Mutex<()>,
}

impl McpClient {
    pub fn new(config: McpServerConfig) -> anyhow::Result<Self> {
        if config.spec != SPEC_DEFAULT {
            bail!(
                "mcp server \"{}\": unsupported protocol version {:?}; only {:?} is supported and legacy initialize negotiation is unavailable",
                config.name,
                config.spec,
                SPEC_DEFAULT
            );
        }
        reject_literal_link_local_destination(&config.name, "server", &config.url)?;
        if let McpAuth::OAuth2 { token_url, .. } = &config.auth {
            reject_literal_link_local_destination(&config.name, "OAuth token", token_url)?;
        }
        // Explicit timeouts, never the hidden 30 s blocking default.
        let http = reqwest::blocking::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .connect_timeout(CONNECT_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .context("could not initialize the MCP HTTP client")?;
        let startup_http = reqwest::blocking::Client::builder()
            .timeout(STARTUP_LIST_TIMEOUT)
            .connect_timeout(CONNECT_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .context("could not initialize the MCP startup HTTP client")?;
        Ok(Self {
            config,
            http,
            startup_http,
            next_id: AtomicU64::new(1),
            cache: Mutex::new(None),
            unsupported_header_tools: Mutex::new(HashSet::new()),
            oauth: Mutex::new(OAuthState::default()),
            refresh_lock: Mutex::new(()),
        })
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

    pub(super) fn list_tools_full(&self) -> anyhow::Result<Vec<ToolEntry>> {
        if let Some(cache) = self.cache_state()?.as_ref() {
            if Instant::now() < cache.expires_at {
                return Ok(cache.entries.clone());
            }
        }
        let result = self.rpc_with(&self.startup_http, "tools/list", json!({}))?;
        let mut unsupported = HashSet::new();
        let mut entries = Vec::new();
        for tool in result
            .get("tools")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let name = tool.get("name").and_then(Value::as_str).unwrap_or("");
            if tool
                .get("inputSchema")
                .is_some_and(schema_uses_parameter_headers)
            {
                eprintln!(
                    "warning: an MCP tool uses unsupported x-mcp-header annotations; excluding it"
                );
                unsupported.insert(name.to_string());
                continue;
            }
            if let Some(entry) = parse_tool(tool) {
                entries.push(entry);
            }
        }
        *self.unsupported_header_tools.lock().map_err(|_| {
            anyhow::anyhow!("MCP unsupported-tool state is unavailable after an internal panic")
        })? = unsupported;
        entries.sort_by(|left, right| left.info.name.cmp(&right.info.name));
        let (sorted, duplicate_entries) = without_duplicate_tool_names(entries);
        let mut entries = sorted;
        if duplicate_entries > 0 {
            eprintln!(
                "warning: mcp server \"{}\" advertised {duplicate_entries} tool entries with duplicate names; excluding every ambiguous entry",
                self.config.name
            );
        }
        if entries.len() > MAX_TOOLS {
            eprintln!(
                "warning: mcp server \"{}\" advertised {} unambiguous tools; using the first {} in name order",
                self.config.name,
                entries.len(),
                MAX_TOOLS
            );
            entries.truncate(MAX_TOOLS);
        }
        let ttl_ms = result
            .get("_meta")
            .and_then(|meta| meta.get("ttlMs"))
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_TTL_MS)
            .min(MAX_TTL_MS);
        if ttl_ms > 0 {
            let now = Instant::now();
            let expires_at = now
                .checked_add(Duration::from_millis(ttl_ms))
                .unwrap_or_else(|| now + Duration::from_millis(MAX_TTL_MS));
            *self.cache_state()? = Some(ToolCache {
                expires_at,
                entries: entries.clone(),
            });
        }
        Ok(entries)
    }

    fn cache_state(&self) -> anyhow::Result<std::sync::MutexGuard<'_, Option<ToolCache>>> {
        self.cache
            .lock()
            .map_err(|_| anyhow::anyhow!("MCP tool cache is unavailable after an internal panic"))
    }

    /// `tools/call`. Text blocks newline-joined; non-text blocks render as
    /// `[<type> block]`; structured content is preserved as compact JSON.
    /// `isError: true` becomes a one-line `Err`.
    /// The returned text is DATA for the model, never instructions.
    pub fn call_tool(&self, name: &str, args: Value) -> anyhow::Result<String> {
        self.call_tool_with_scrubber(name, args, shape_scrubber())
    }

    pub(super) fn call_tool_with_scrubber(
        &self,
        name: &str,
        args: Value,
        scrubber: &nh_vault::Scrubber,
    ) -> anyhow::Result<String> {
        if self
            .unsupported_header_tools
            .lock()
            .map_err(|_| {
                anyhow::anyhow!("MCP unsupported-tool state is unavailable after an internal panic")
            })?
            .contains(name)
        {
            bail!("mcp tool is unavailable because x-mcp-header parameters are not supported");
        }
        let result = self.rpc("tools/call", json!({ "name": name, "arguments": args }))?;
        let text = render_call_result(&result, scrubber);
        if result.get("isError").and_then(Value::as_bool) == Some(true) {
            let line = if text.is_empty() {
                format!("tool {} failed with no message", scrubber.scrub(name))
            } else {
                text.replace(['\r', '\n'], " ")
            };
            bail!("{line}");
        }
        Ok(text)
    }

    /// Server business card through the modern `server/discover` request.
    pub fn discover(&self) -> anyhow::Result<Value> {
        self.rpc_with(&self.startup_http, "server/discover", json!({}))
    }

    /// One JSON-RPC 2.0 call. Every request's params carries `_meta` - the
    /// stateless core sends full context per call and never pins an instance.
    pub(super) fn rpc(&self, method: &str, params: Value) -> anyhow::Result<Value> {
        self.rpc_with(&self.http, method, params)
    }

    pub(super) fn rpc_with(
        &self,
        http: &reqwest::blocking::Client,
        method: &str,
        mut params: Value,
    ) -> anyhow::Result<Value> {
        let params_object = params
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("MCP request params must be an object"))?;
        let meta = params_object
            .entry("_meta")
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("MCP request _meta must be an object"))?;
        meta.insert(
            "io.modelcontextprotocol/protocolVersion".into(),
            json!(self.config.spec),
        );
        meta.insert(
            "io.modelcontextprotocol/clientInfo".into(),
            json!({ "name": "nosis-harness", "version": env!("CARGO_PKG_VERSION") }),
        );
        meta.insert(
            "io.modelcontextprotocol/clientCapabilities".into(),
            json!({}),
        );
        let request_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let body = json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
            "params": params
        });
        let mcp_name = if method == "tools/call" {
            let name = body["params"]["name"]
                .as_str()
                .filter(|name| !name.is_empty())
                .ok_or_else(|| anyhow::anyhow!("tools/call needs a non-empty tool name"))?;
            Some(encode_header_value("Mcp-Name", name)?)
        } else {
            None
        };
        let url = &self.config.url;
        let mut retried_oauth = false;
        loop {
            let headers = self.request_headers_with(http)?;
            let mut request = http
                .post(url)
                .header(
                    reqwest::header::ACCEPT,
                    "application/json, text/event-stream",
                )
                .header("MCP-Protocol-Version", self.config.spec.as_str())
                .header("Mcp-Method", method)
                .json(&body);
            if let Some(name) = &mcp_name {
                request = request.header("Mcp-Name", name);
            }
            for (name, value) in &headers {
                request = request.header(name.as_str(), value.as_str());
            }
            let response = request
                .send()
                .map_err(|e| anyhow::anyhow!("could not reach {url}: {e}"))?;
            let status = response.status();
            if status.as_u16() == 401
                && matches!(self.config.auth, McpAuth::OAuth2 { .. })
                && !retried_oauth
            {
                let rejected_access = headers.iter().find_map(|(name, value)| {
                    name.eq_ignore_ascii_case("authorization")
                        .then(|| value.as_str().strip_prefix("Bearer "))
                        .flatten()
                });
                self.refresh_oauth(http, rejected_access)?;
                if method == "tools/call" {
                    bail!(
                        "{url} rejected MCP authorization; credentials were refreshed, but the tool call was not replayed - retry explicitly"
                    );
                }
                retried_oauth = true;
                continue;
            }
            if !status.is_success() {
                let hint = match status.as_u16() {
                    401 | 403 => " - key rejected; check vault_entry in .nosis/mcp.toml",
                    429 => " - rate limited; retry later",
                    _ => "",
                };
                bail!("{url} returned HTTP {}{hint}", status.as_u16());
            }
            match read_rpc_reply(response, &json!(request_id), url)? {
                RpcReply::Complete(result) => return Ok(result),
                RpcReply::ServerError(message) => bail!("server error: {message}"),
            }
        }
    }

    /// Auth (§3.4) + outbound header lint (§3.5) - the single choke point every
    /// send passes through. The key is fetched per call, zeroized, never logged.
    pub(super) fn request_headers_with(
        &self,
        http: &reqwest::blocking::Client,
    ) -> anyhow::Result<Vec<(String, SecretValue)>> {
        let mut headers = Vec::new();
        match &self.config.auth {
            McpAuth::None => {}
            McpAuth::OAuth2 { .. } => {
                let access = self.oauth_access_token_with(http)?;
                headers.push((
                    "authorization".to_string(),
                    nh_vault::secret(format!("Bearer {}", access.as_str())),
                ));
            }
            McpAuth::ApiKey { vault_entry } => {
                let vault = EnvFallbackVault {
                    inner: KeyringVault,
                };
                let secret = vault.get(vault_entry)?;
                headers.push((
                    "authorization".to_string(),
                    nh_vault::secret(format!("Bearer {}", secret.as_str())),
                ));
            }
        }
        for (name, value) in &headers {
            lint_header(name, value.as_str())?;
        }
        Ok(headers)
    }
}

fn reject_literal_link_local_destination(
    server: &str,
    kind: &str,
    destination: &str,
) -> anyhow::Result<()> {
    if nh_vault::host_of(destination)
        .as_deref()
        .is_some_and(nh_vault::is_link_local_or_metadata)
    {
        bail!(
            "mcp server \"{server}\": literal link-local or metadata {kind} destinations are not allowed"
        );
    }
    Ok(())
}

fn without_duplicate_tool_names(entries: Vec<ToolEntry>) -> (Vec<ToolEntry>, usize) {
    let mut unique = Vec::with_capacity(entries.len());
    let mut duplicate_entries = 0_usize;
    let mut index = 0_usize;
    while index < entries.len() {
        let name = &entries[index].info.name;
        let mut end = index + 1;
        while end < entries.len() && entries[end].info.name == *name {
            end += 1;
        }
        if end == index + 1 {
            unique.push(entries[index].clone());
        } else {
            duplicate_entries = duplicate_entries.saturating_add(end - index);
        }
        index = end;
    }
    (unique, duplicate_entries)
}

fn schema_uses_parameter_headers(value: &Value) -> bool {
    match value {
        Value::Array(values) => values.iter().any(schema_uses_parameter_headers),
        Value::Object(fields) => {
            fields
                .keys()
                .any(|name| name.eq_ignore_ascii_case("x-mcp-header"))
                || fields.values().any(schema_uses_parameter_headers)
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => false,
    }
}

fn encode_header_value(header_name: &str, raw: &str) -> anyhow::Result<String> {
    lint_header(header_name, raw)?;
    if raw.is_empty() {
        bail!("refused to send empty {header_name} header");
    }
    let bytes = raw.as_bytes();
    let sentinel = raw.starts_with("=?base64?") && raw.ends_with("?=");
    let plain = bytes
        .iter()
        .all(|byte| *byte == b'\t' || (b' '..=b'~').contains(byte))
        && !bytes.first().is_some_and(u8::is_ascii_whitespace)
        && !bytes.last().is_some_and(u8::is_ascii_whitespace)
        && !sentinel;
    if plain {
        Ok(raw.to_string())
    } else {
        Ok(format!("=?base64?{}?=", crate::encode_base64(bytes)))
    }
}

pub(super) fn parse_tool(tool: &Value) -> Option<ToolEntry> {
    let name = tool.get("name")?.as_str()?.to_string();
    let description = nh_vault::sanitize_untrusted_text(
        tool.get("description")
            .and_then(Value::as_str)
            .unwrap_or(""),
    );
    let mut input_schema = tool
        .get("inputSchema")
        .cloned()
        .unwrap_or_else(|| json!({ "type": "object" }));
    sanitize_json_strings(&mut input_schema);
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
pub(super) fn sanitize_json_strings(value: &mut Value) {
    match value {
        Value::String(text) => *text = nh_vault::sanitize_untrusted_text(text),
        Value::Array(values) => {
            for value in values {
                sanitize_json_strings(value);
            }
        }
        Value::Object(fields) => {
            for value in fields.values_mut() {
                sanitize_json_strings(value);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

pub(super) fn scrub_json_strings(value: &mut Value, scrubber: &nh_vault::Scrubber) -> bool {
    match value {
        Value::String(text) => *text = scrubber.scrub(text),
        Value::Array(values) => {
            for value in values {
                if !scrub_json_strings(value, scrubber) {
                    return false;
                }
            }
        }
        Value::Object(fields) => {
            let mut scrubbed = Map::new();
            for (name, mut value) in std::mem::take(fields) {
                let name = scrubber.scrub(&name);
                if scrubbed.contains_key(&name) || !scrub_json_strings(&mut value, scrubber) {
                    return false;
                }
                scrubbed.insert(name, value);
            }
            *fields = scrubbed;
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
    true
}

fn render_call_result(result: &Value, scrubber: &nh_vault::Scrubber) -> String {
    const STRUCTURED_MARKER: &str = "[structured content]";
    const OMITTED_MARKER: &str =
        "[structured content omitted: redaction made field names ambiguous]";

    let text = render_scrubbed_content(result.get("content"), scrubber);
    let Some(structured) = result.get("structuredContent") else {
        return text;
    };
    let mut structured = structured.clone();
    if !scrub_json_strings(&mut structured, scrubber) {
        return join_result_parts(&text, OMITTED_MARKER);
    }
    let Ok(encoded) = serde_json::to_string(&structured) else {
        return join_result_parts(
            &text,
            "[structured content omitted: could not render safely]",
        );
    };
    if !text.is_empty() && text_duplicates_structured(&text, &structured) {
        return text;
    }
    if text.is_empty() {
        encoded
    } else {
        format!("{text}\n{STRUCTURED_MARKER}\n{encoded}")
    }
}

fn render_scrubbed_content(content: Option<&Value>, scrubber: &nh_vault::Scrubber) -> String {
    const OMITTED_COLLISION: &str = "[text content omitted: redaction made field names ambiguous]";
    const OMITTED_ENCODING: &str = "[text content omitted: could not render safely]";

    let Some(blocks) = content.and_then(Value::as_array) else {
        return String::new();
    };
    let text = blocks
        .iter()
        .map(|block| match block.get("type").and_then(Value::as_str) {
            Some("text") => scrub_json_text(
                block.get("text").and_then(Value::as_str).unwrap_or(""),
                scrubber,
                OMITTED_COLLISION,
                OMITTED_ENCODING,
            ),
            Some(other) => scrubber.scrub(&format!("[{other} block]")),
            None => "[unknown block]".to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n");

    // A server may split one complete JSON document across text blocks. Each
    // complete block was already handled above; this pass covers that joined
    // representation without relying on it for multi-block safety.
    scrub_json_text(&text, scrubber, OMITTED_COLLISION, OMITTED_ENCODING)
}

fn scrub_json_text(
    text: &str,
    scrubber: &nh_vault::Scrubber,
    collision_marker: &str,
    unsafe_marker: &str,
) -> String {
    match scrub_complete_json(text, scrubber) {
        ScrubbedJson::NotJson => scrubber.scrub(text),
        ScrubbedJson::Unchanged => text.to_string(),
        ScrubbedJson::Rewritten(text) => text,
        ScrubbedJson::Collision => collision_marker.to_string(),
        ScrubbedJson::Unsafe => unsafe_marker.to_string(),
    }
}

fn join_result_parts(text: &str, suffix: &str) -> String {
    if text.is_empty() {
        suffix.to_string()
    } else {
        format!("{text}\n{suffix}")
    }
}

fn text_duplicates_structured(text: &str, structured: &Value) -> bool {
    structured.as_str() == Some(text)
        || serde_json::from_str::<Value>(text).is_ok_and(|parsed| parsed == *structured)
}

/// Outbound header lint (plan §4.5 - the Akamai leak vector, closed): a
/// secret-shaped value in an `Mcp-*` / `x-mcp-*` header is refused before send.
/// `Authorization` is the one sanctioned credential channel and is exempt.
#[cfg(test)]
pub(super) fn lint_headers(headers: &[(String, String)]) -> anyhow::Result<()> {
    for (name, value) in headers {
        lint_header(name, value)?;
    }
    Ok(())
}

pub(super) fn lint_header(name: &str, value: &str) -> anyhow::Result<()> {
    let lower = name.to_ascii_lowercase();
    if lower == "authorization" {
        return Ok(());
    }
    let mcp_shaped = lower.starts_with("mcp-") || lower.starts_with("x-mcp-");
    if mcp_shaped && looks_like_secret(value) {
        bail!(
            "refused to send header \"{name}\" - its value looks like a secret; keys go through nh-vault (auth = \"apikey\"), never MCP headers"
        );
    }
    Ok(())
}

/// Same key shapes the Scrubber redacts (`sk-…`, `csk-…`, JWT) - one source of truth.
pub(super) fn looks_like_secret(value: &str) -> bool {
    shape_scrubber().scrub(value) != value
}

fn shape_scrubber() -> &'static nh_vault::Scrubber {
    static SCRUBBER: OnceLock<nh_vault::Scrubber> = OnceLock::new();
    SCRUBBER.get_or_init(|| nh_vault::Scrubber::new(vec![]))
}
