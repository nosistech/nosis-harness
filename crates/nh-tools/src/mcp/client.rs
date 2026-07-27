//! Stateless MCP transport, bounded responses, OAuth token flow, and tool cache.

use super::*;

pub(super) const SPEC_DEFAULT: &str = "2026-07-28";
pub(super) const SPEC_FALLBACK: &str = "2025-11-25";
/// Pinned default when `tools/list` carries no `result._meta.ttlMs` (CONTRACTS_M1 §3.2).
pub(super) const DEFAULT_TTL_MS: u64 = 60_000;
pub(super) const MAX_MCP_BODY_BYTES: usize = 4 * 1024 * 1024;
pub(super) const MAX_OAUTH_BODY_BYTES: usize = 256 * 1024;
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
pub(super) const OAUTH_EXPIRY_SKEW: Duration = Duration::from_secs(30);
pub(super) const OAUTH_DEFAULT_EXPIRES_IN: u64 = 3_600;
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
    Ok(String::from_utf8_lossy(&buf).into_owned())
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

#[derive(Default)]
pub(super) struct OAuthState {
    pub(super) access: Option<SecretValue>,
    pub(super) expires_at: Option<Instant>,
    pub(super) refresh: Option<SecretValue>,
}

#[derive(serde::Deserialize)]
pub(super) struct OAuthTokenResponse {
    pub(super) access_token: Option<String>,
    pub(super) expires_in: Option<u64>,
    pub(super) refresh_token: Option<String>,
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
    pub(super) oauth: Mutex<OAuthState>,
    pub(super) refresh_lock: Mutex<()>,
}

impl McpClient {
    pub fn new(config: McpServerConfig) -> Self {
        Self {
            config,
            // Explicit timeouts, never the hidden 30 s blocking default.
            // Panics only where `Client::new` would (TLS backend unavailable).
            http: reqwest::blocking::Client::builder()
                .timeout(REQUEST_TIMEOUT)
                .connect_timeout(CONNECT_TIMEOUT)
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("HTTP client"),
            startup_http: reqwest::blocking::Client::builder()
                .timeout(STARTUP_LIST_TIMEOUT)
                .connect_timeout(CONNECT_TIMEOUT)
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("startup HTTP client"),
            next_id: AtomicU64::new(1),
            cache: Mutex::new(None),
            oauth: Mutex::new(OAuthState::default()),
            refresh_lock: Mutex::new(()),
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

    pub(super) fn list_tools_full(&self) -> anyhow::Result<Vec<ToolEntry>> {
        if let Some(cache) = self.cache.lock().expect("mcp tool cache lock").as_ref() {
            if Instant::now() < cache.expires_at {
                return Ok(cache.entries.clone());
            }
        }
        let result = self.rpc_with(&self.startup_http, "tools/list", json!({}))?;
        let mut entries: Vec<ToolEntry> = result
            .get("tools")
            .and_then(Value::as_array)
            .map(|tools| tools.iter().filter_map(parse_tool).collect())
            .unwrap_or_default();
        if entries.len() > MAX_TOOLS {
            eprintln!(
                "warning: mcp server \"{}\" advertised {} tools; using the first {}",
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
            *self.cache.lock().expect("mcp tool cache lock") = Some(ToolCache {
                expires_at,
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
        let headers = self.request_headers_with(&self.startup_http)?;
        if let Ok(card) = self.get_well_known(&headers) {
            return Ok(card);
        }
        match self.rpc_with(&self.startup_http, "server/discover", json!({})) {
            Ok(card) => Ok(card),
            Err(_) => bail!(
                "mcp server \"{}\" unreachable - check the url in .nosis/mcp.toml",
                self.config.name
            ),
        }
    }

    pub(super) fn get_well_known(
        &self,
        headers: &[(String, SecretValue)],
    ) -> anyhow::Result<Value> {
        let url = format!(
            "{}/.well-known/mcp.json",
            self.config.url.trim_end_matches('/')
        );
        let mut request = self.startup_http.get(&url);
        for (name, value) in headers {
            request = request.header(name.as_str(), value.as_str());
        }
        let response = request
            .send()
            .with_context(|| format!("could not reach {url}"))?;
        if !response.status().is_success() {
            bail!("{url} returned HTTP {}", response.status().as_u16());
        }
        let body = read_body_capped(response, MAX_MCP_BODY_BYTES)?;
        serde_json::from_str(&body).with_context(|| format!("{url} sent invalid JSON"))
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
        let url = &self.config.url;
        let mut retried_oauth = false;
        loop {
            let headers = self.request_headers_with(http)?;
            let mut request = http.post(url).json(&body);
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
            let text = read_body_capped(response, MAX_MCP_BODY_BYTES)?;
            let reply: Value = serde_json::from_str(&text)
                .map_err(|_| anyhow::anyhow!("{url} sent invalid JSON - is it an MCP endpoint?"))?;
            if let Some(error) = reply.get("error") {
                let message = error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown error");
                bail!("server error: {message}");
            }
            return Ok(reply.get("result").cloned().unwrap_or(Value::Null));
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

    pub(super) fn oauth_access_token_with(
        &self,
        http: &reqwest::blocking::Client,
    ) -> anyhow::Result<SecretValue> {
        let valid = {
            let state = self.oauth.lock().expect("mcp oauth lock");
            state.access.is_some()
                && state
                    .expires_at
                    .and_then(|expires_at| expires_at.checked_duration_since(Instant::now()))
                    .is_some_and(|remaining| remaining > OAUTH_EXPIRY_SKEW)
        };
        if !valid {
            self.refresh_oauth(http, None)?;
        }
        self.oauth
            .lock()
            .expect("mcp oauth lock")
            .access
            .clone()
            .ok_or_else(|| anyhow::anyhow!("mcp oauth refresh completed without an access token"))
    }

    pub(super) fn refresh_oauth(
        &self,
        http: &reqwest::blocking::Client,
        rejected_access: Option<&str>,
    ) -> anyhow::Result<()> {
        let McpAuth::OAuth2 {
            token_url,
            client_id,
            vault_entry,
        } = &self.config.auth
        else {
            return Ok(());
        };
        let _refresh = self.refresh_lock.lock().expect("mcp oauth refresh lock");
        let already_refreshed = {
            let state = self.oauth.lock().expect("mcp oauth lock");
            let valid = state.access.is_some()
                && state
                    .expires_at
                    .and_then(|expires_at| expires_at.checked_duration_since(Instant::now()))
                    .is_some_and(|remaining| remaining > OAUTH_EXPIRY_SKEW);
            match rejected_access {
                Some(rejected) => {
                    valid && state.access.as_ref().map(|value| value.as_str()) != Some(rejected)
                }
                None => valid,
            }
        };
        if already_refreshed {
            return Ok(());
        }
        let failure = || {
            anyhow::anyhow!(
                "mcp server \"{}\": oauth refresh failed - re-authorize with `nh key add {}-refresh` and `nh key add {}-secret` (or check token_url in .nosis/mcp.toml)",
                self.config.name,
                vault_entry,
                vault_entry
            )
        };
        let vault = EnvFallbackVault {
            inner: KeyringVault,
        };
        let cached_refresh = self.oauth.lock().expect("mcp oauth lock").refresh.clone();
        let refresh_entry = format!("{vault_entry}-refresh");
        let refresh_from_vault = if cached_refresh.is_none() {
            Some(vault.get(&refresh_entry).map_err(|_| failure())?)
        } else {
            None
        };
        let refresh_token = cached_refresh
            .as_ref()
            .map(|value| value.as_str())
            .or_else(|| refresh_from_vault.as_ref().map(|value| value.as_str()))
            .ok_or_else(failure)?;
        let secret_entry = format!("{vault_entry}-secret");
        let client_secret = vault.get(&secret_entry).map_err(|_| failure())?;
        let scope = self.config.scopes.join(" ");
        let mut form = vec![
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", client_id.as_str()),
            ("client_secret", client_secret.as_str()),
        ];
        if !scope.is_empty() {
            form.push(("scope", scope.as_str()));
        }
        form.push(("resource", self.config.url.trim_end_matches('/')));

        let response = http
            .post(token_url)
            .form(&form)
            .send()
            .map_err(|_| failure())?;
        if !response.status().is_success() {
            return Err(failure());
        }
        let body = nh_vault::secret(
            read_body_capped(response, MAX_OAUTH_BODY_BYTES).map_err(|_| failure())?,
        );
        let token: OAuthTokenResponse =
            serde_json::from_str(body.as_str()).map_err(|_| failure())?;
        let access = token
            .access_token
            .filter(|value| !value.is_empty())
            .map(nh_vault::secret)
            .ok_or_else(failure)?;
        let now = Instant::now();
        let expires_at = now
            .checked_add(Duration::from_secs(
                token.expires_in.unwrap_or(OAUTH_DEFAULT_EXPIRES_IN),
            ))
            .unwrap_or(now + Duration::from_secs(OAUTH_DEFAULT_EXPIRES_IN));

        let mut state = self.oauth.lock().expect("mcp oauth lock");
        state.access = Some(access);
        state.expires_at = Some(expires_at);
        if let Some(refresh) = token
            .refresh_token
            .filter(|value| !value.is_empty())
            .map(nh_vault::secret)
        {
            if vault.set(&refresh_entry, refresh.as_str()).is_err() {
                let mut registry = SecretRegistry::new();
                registry.insert(refresh.clone());
                let scrubber = registry.scrubber();
                eprintln!(
                    "warning: {}",
                    scrubber.scrub(&format!(
                        "could not persist rotated refresh token for {vault_entry} - re-auth may be needed next session"
                    ))
                );
            }
            state.refresh = Some(refresh);
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn expire_oauth_for_test(&self) {
        self.oauth.lock().expect("mcp oauth lock").expires_at = Some(Instant::now());
    }

    #[cfg(test)]
    pub(super) fn oauth_access_token(&self) -> anyhow::Result<SecretValue> {
        self.oauth_access_token_with(&self.http)
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

pub(super) fn render_content(content: Option<&Value>) -> String {
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
    static SCRUBBER: OnceLock<nh_vault::Scrubber> = OnceLock::new();
    let scrubber = SCRUBBER.get_or_init(|| nh_vault::Scrubber::new(vec![]));
    scrubber.scrub(value) != value
}
