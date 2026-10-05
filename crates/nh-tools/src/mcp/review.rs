use super::client::McpClient;
use super::config::{McpAuth, McpServerConfig};
use anyhow::Context as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_MCP_REVIEW_BYTES: usize = 4 * 1024 * 1024;
pub(super) const MAX_REVIEWED_TOOL_BYTES: usize = 64 * 1024;
const REVIEW_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ReviewedTool {
    pub descriptor: Value,
    pub fingerprint: String,
}

impl ReviewedTool {
    pub fn name(&self) -> &str {
        self.descriptor
            .get("name")
            .and_then(Value::as_str)
            .expect("validated reviewed tool has a name")
    }

    pub fn description(&self) -> Option<&str> {
        self.descriptor.get("description").and_then(Value::as_str)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct McpReviewState {
    pub version: u32,
    pub server: String,
    pub connection: Value,
    pub connection_fingerprint: String,
    pub snapshot: Vec<ReviewedTool>,
    pub enabled: Vec<String>,
}

impl McpReviewState {
    pub fn from_snapshot(
        snapshot: McpReviewSnapshot,
        enabled: impl IntoIterator<Item = String>,
    ) -> anyhow::Result<Self> {
        let enabled = enabled.into_iter().collect::<BTreeSet<_>>();
        let available = snapshot
            .tools
            .iter()
            .map(|tool| tool.name().to_string())
            .collect::<BTreeSet<_>>();
        if let Some(name) = enabled.difference(&available).next() {
            anyhow::bail!("cannot enable unknown MCP tool {name:?}")
        }
        let state = Self {
            version: REVIEW_VERSION,
            server: snapshot.server,
            connection: snapshot.connection,
            connection_fingerprint: snapshot.connection_fingerprint,
            snapshot: snapshot.tools,
            enabled: enabled.into_iter().collect(),
        };
        state.validate()?;
        Ok(state)
    }

    pub fn parse(text: &str) -> anyhow::Result<Self> {
        if text.len() > MAX_MCP_REVIEW_BYTES {
            anyhow::bail!(
                "MCP review state exceeds the {} byte limit",
                MAX_MCP_REVIEW_BYTES
            )
        }
        let state: Self =
            serde_json::from_str(text).context("MCP review state is not valid versioned JSON")?;
        state.validate()?;
        Ok(state)
    }

    pub fn to_pretty_json(&self) -> anyhow::Result<String> {
        self.validate()?;
        let mut text = serde_json::to_string_pretty(self)
            .context("could not encode MCP review state safely")?;
        finish_encoded_state(&mut text)?;
        Ok(text)
    }

    pub fn to_compact_json(&self) -> anyhow::Result<String> {
        self.validate()?;
        let mut text =
            serde_json::to_string(self).context("could not encode MCP review state safely")?;
        finish_encoded_state(&mut text)?;
        Ok(text)
    }

    pub fn connection_matches(&self, config: &McpServerConfig) -> bool {
        connection_descriptor(config).is_ok_and(|descriptor| descriptor == self.connection)
            && fingerprint(&self.connection).is_ok_and(|value| value == self.connection_fingerprint)
    }

    pub fn tool(&self, name: &str) -> Option<&ReviewedTool> {
        self.snapshot
            .binary_search_by(|tool| tool.name().cmp(name))
            .ok()
            .map(|index| &self.snapshot[index])
    }

    pub fn is_enabled(&self, name: &str) -> bool {
        self.enabled
            .binary_search_by(|enabled| enabled.as_str().cmp(name))
            .is_ok()
    }

    fn validate(&self) -> anyhow::Result<()> {
        if self.version != REVIEW_VERSION {
            anyhow::bail!(
                "unsupported MCP review state version {}; expected {}",
                self.version,
                REVIEW_VERSION
            )
        }
        if !safe_name_component(&self.server) {
            anyhow::bail!("MCP review state has an invalid server name")
        }
        validate_connection_descriptor(&self.connection)?;
        validate_fingerprint(&self.connection_fingerprint)?;
        if fingerprint(&self.connection)? != self.connection_fingerprint {
            anyhow::bail!("MCP review connection fingerprint does not match its descriptor")
        }
        if self.snapshot.len() > super::client::MAX_TOOLS {
            anyhow::bail!("MCP review state contains too many tools")
        }
        let mut previous = None;
        let mut names = BTreeSet::new();
        for tool in &self.snapshot {
            validate_tool_descriptor(&tool.descriptor)?;
            validate_fingerprint(&tool.fingerprint)?;
            if fingerprint(&tool.descriptor)? != tool.fingerprint {
                anyhow::bail!("MCP review tool fingerprint does not match its descriptor")
            }
            if canonical_bytes(&tool.descriptor)?.len() > MAX_REVIEWED_TOOL_BYTES {
                anyhow::bail!("MCP review tool descriptor exceeds its size limit")
            }
            let name = tool.name();
            if previous.is_some_and(|previous: &str| previous >= name) || !names.insert(name) {
                anyhow::bail!("MCP review tool snapshot must use unique sorted names")
            }
            previous = Some(name);
        }
        let mut previous_enabled = None;
        for name in &self.enabled {
            if previous_enabled.is_some_and(|previous: &str| previous >= name.as_str())
                || !names.contains(name.as_str())
            {
                anyhow::bail!("MCP review enabled tools must be a unique sorted snapshot subset")
            }
            previous_enabled = Some(name);
        }
        Ok(())
    }
}

fn finish_encoded_state(text: &mut String) -> anyhow::Result<()> {
    text.push('\n');
    if text.len() > MAX_MCP_REVIEW_BYTES {
        anyhow::bail!(
            "MCP review state exceeds the {} byte limit",
            MAX_MCP_REVIEW_BYTES
        )
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct McpReviewSnapshot {
    pub server: String,
    pub connection: Value,
    pub connection_fingerprint: String,
    pub tools: Vec<ReviewedTool>,
    pub omitted_tools: usize,
}

#[derive(Default)]
pub struct McpReviewPolicy {
    states: BTreeMap<String, McpReviewState>,
    #[cfg(test)]
    allow_current_for_tests: bool,
}

impl McpReviewPolicy {
    pub fn new(states: impl IntoIterator<Item = McpReviewState>) -> anyhow::Result<Self> {
        let mut by_server = BTreeMap::new();
        for state in states {
            state.validate()?;
            let server = state.server.clone();
            if by_server.insert(server.clone(), state).is_some() {
                anyhow::bail!("duplicate MCP review state for server {server:?}")
            }
        }
        Ok(Self {
            states: by_server,
            #[cfg(test)]
            allow_current_for_tests: false,
        })
    }

    pub(super) fn state(&self, server: &str) -> Option<&McpReviewState> {
        self.states.get(server)
    }

    #[cfg(test)]
    pub(super) fn allow_current_for_tests() -> Self {
        Self {
            states: BTreeMap::new(),
            allow_current_for_tests: true,
        }
    }

    #[cfg(test)]
    pub(super) fn accepts_current_for_tests(&self) -> bool {
        self.allow_current_for_tests
    }

    #[cfg(not(test))]
    pub(super) const fn accepts_current_for_tests(&self) -> bool {
        false
    }
}

pub fn inspect_mcp_server(config: &McpServerConfig) -> anyhow::Result<McpReviewSnapshot> {
    connection_descriptor(config)?;
    McpClient::new(config.clone())?.review_snapshot()
}

pub(super) fn connection_descriptor(config: &McpServerConfig) -> anyhow::Result<Value> {
    let mut descriptor = Map::new();
    descriptor.insert("url".into(), Value::String(reviewable_url(&config.url)?));
    descriptor.insert("protocol".into(), Value::String(config.spec.clone()));
    let auth = match &config.auth {
        McpAuth::None => json!({ "kind": "none" }),
        McpAuth::ApiKey { vault_entry } => json!({
            "kind": "api_key",
            "vault_entry": vault_entry
        }),
        McpAuth::OAuth2 {
            token_url,
            client_id,
            vault_entry,
        } => json!({
            "kind": "oauth2",
            "token_url": reviewable_url(token_url)?,
            "client_id": client_id,
            "vault_entry": vault_entry
        }),
    };
    descriptor.insert("auth".into(), auth);
    descriptor.insert(
        "scopes".into(),
        Value::Array(config.scopes.iter().cloned().map(Value::String).collect()),
    );
    if let Some(mode) = &config.default_mode {
        descriptor.insert("defaultMode".into(), Value::String(mode.clone()));
    }
    let descriptor = Value::Object(descriptor);
    validate_connection_descriptor(&descriptor)?;
    ensure_no_shaped_secret(&descriptor, "connection metadata")?;
    Ok(descriptor)
}

pub(super) fn reviewed_tool(
    descriptor: Value,
    credential_scrubber: &nh_vault::Scrubber,
) -> anyhow::Result<Option<ReviewedTool>> {
    validate_tool_descriptor(&descriptor)?;
    let mut scrubbed = descriptor.clone();
    if !super::client::scrub_json_strings(&mut scrubbed, credential_scrubber)
        || scrubbed != descriptor
        || escaped_json_reveals_secret(&descriptor, credential_scrubber)
    {
        return Ok(None);
    }
    let bytes = canonical_bytes(&descriptor)?;
    if bytes.len() > MAX_REVIEWED_TOOL_BYTES {
        return Ok(None);
    }
    Ok(Some(ReviewedTool {
        fingerprint: fingerprint_bytes(&bytes),
        descriptor,
    }))
}

fn escaped_json_reveals_secret(value: &Value, scrubber: &nh_vault::Scrubber) -> bool {
    let reveals = |text: &str| {
        let escaped = nh_vault::escape_untrusted(text);
        scrubber.scrub(&escaped) != escaped
    };
    match value {
        Value::String(text) => reveals(text),
        Value::Array(values) => values
            .iter()
            .any(|value| escaped_json_reveals_secret(value, scrubber)),
        Value::Object(fields) => fields
            .iter()
            .any(|(name, value)| reveals(name) || escaped_json_reveals_secret(value, scrubber)),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

pub(super) fn snapshot_from_tools(
    config: &McpServerConfig,
    mut tools: Vec<ReviewedTool>,
    mut omitted_tools: usize,
) -> anyhow::Result<McpReviewSnapshot> {
    tools.sort_by(|left, right| left.name().cmp(right.name()));
    let connection = connection_descriptor(config)?;
    let connection_fingerprint = fingerprint(&connection)?;
    let mut retained = Vec::with_capacity(tools.len());
    let mut estimated_bytes = canonical_bytes(&connection)?.len();
    for tool in tools {
        let bytes = canonical_bytes(&tool.descriptor)?.len().saturating_add(128);
        if estimated_bytes.saturating_add(bytes) > MAX_MCP_REVIEW_BYTES {
            omitted_tools = omitted_tools.saturating_add(1);
        } else {
            estimated_bytes = estimated_bytes.saturating_add(bytes);
            retained.push(tool);
        }
    }
    Ok(McpReviewSnapshot {
        server: config.name.clone(),
        connection,
        connection_fingerprint,
        tools: retained,
        omitted_tools,
    })
}

pub(super) fn fingerprint(value: &Value) -> anyhow::Result<String> {
    canonical_bytes(value).map(|bytes| fingerprint_bytes(&bytes))
}

fn fingerprint_bytes(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn canonical_bytes(value: &Value) -> anyhow::Result<Vec<u8>> {
    serde_json::to_vec(&canonical_value(value))
        .context("could not encode canonical MCP review metadata")
}

fn canonical_value(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(canonical_value).collect()),
        Value::Object(fields) => {
            let sorted = fields
                .iter()
                .map(|(name, value)| (name.clone(), canonical_value(value)))
                .collect::<BTreeMap<_, _>>();
            Value::Object(sorted.into_iter().collect())
        }
        other => other.clone(),
    }
}

fn validate_connection_descriptor(value: &Value) -> anyhow::Result<()> {
    let Some(object) = value.as_object() else {
        anyhow::bail!("MCP review connection descriptor must be an object")
    };
    let allowed = ["url", "protocol", "auth", "scopes", "defaultMode"];
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        anyhow::bail!("MCP review connection descriptor has an unknown field")
    }
    let url = required_string(object, "url", "connection")?;
    reviewable_url(url)?;
    required_string(object, "protocol", "connection")?;
    let auth = object
        .get("auth")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow::anyhow!("MCP review connection auth must be an object"))?;
    validate_auth_descriptor(auth)?;
    let scopes = object
        .get("scopes")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("MCP review connection scopes must be an array"))?;
    if scopes.iter().any(|scope| !scope.is_string()) {
        anyhow::bail!("MCP review connection scopes must contain strings")
    }
    if object
        .get("defaultMode")
        .is_some_and(|mode| !mode.is_string())
    {
        anyhow::bail!("MCP review connection defaultMode must be a string")
    }
    ensure_no_shaped_secret(value, "connection metadata")
}

fn validate_auth_descriptor(auth: &Map<String, Value>) -> anyhow::Result<()> {
    let kind = required_string(auth, "kind", "connection auth")?;
    let required: &[&str] = match kind {
        "none" => &["kind"],
        "api_key" => &["kind", "vault_entry"],
        "oauth2" => &["kind", "token_url", "client_id", "vault_entry"],
        _ => anyhow::bail!("MCP review connection auth kind is unsupported"),
    };
    if auth.len() != required.len() || auth.keys().any(|key| !required.contains(&key.as_str())) {
        anyhow::bail!("MCP review connection auth fields do not match its kind")
    }
    for field in required.iter().copied().filter(|field| *field != "kind") {
        required_string(auth, field, "connection auth")?;
    }
    if let Some(token_url) = auth.get("token_url").and_then(Value::as_str) {
        reviewable_url(token_url)?;
    }
    Ok(())
}

fn validate_tool_descriptor(value: &Value) -> anyhow::Result<()> {
    let Some(object) = value.as_object() else {
        anyhow::bail!("MCP review tool descriptor must be an object")
    };
    let allowed = [
        "name",
        "description",
        "inputSchema",
        "outputSchema",
        "annotations",
    ];
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        anyhow::bail!("MCP review tool descriptor has an unknown field")
    }
    let name = required_string(object, "name", "tool")?;
    if !safe_name_component(name) {
        anyhow::bail!("MCP review tool name is invalid")
    }
    if object
        .get("description")
        .is_some_and(|description| !description.is_string())
    {
        anyhow::bail!("MCP review tool description must be a string")
    }
    for field in ["inputSchema", "outputSchema", "annotations"] {
        if object.get(field).is_some_and(|value| !value.is_object()) {
            anyhow::bail!("MCP review tool {field} must be an object")
        }
    }
    Ok(())
}

fn required_string<'a>(
    object: &'a Map<String, Value>,
    field: &str,
    label: &str,
) -> anyhow::Result<&'a str> {
    object
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("MCP review {label} {field} must be a non-empty string"))
}

fn reviewable_url(raw: &str) -> anyhow::Result<String> {
    let parsed = reqwest::Url::parse(raw)
        .map_err(|_| anyhow::anyhow!("MCP connection URL is not reviewable"))?;
    if !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        anyhow::bail!(
            "MCP connection URLs with user info, query parameters, or fragments cannot be reviewed safely"
        )
    }
    Ok(raw.to_string())
}

pub(super) fn explicit_http_url(raw: &str) -> anyhow::Result<reqwest::Url> {
    let raw = raw.trim();
    let explicit_http = raw
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("http://"));
    let explicit_https = raw
        .get(..8)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("https://"));
    if !explicit_http && !explicit_https {
        anyhow::bail!("MCP connection URL must begin with explicit http:// or https://")
    }
    let parsed = reqwest::Url::parse(raw)
        .map_err(|_| anyhow::anyhow!("MCP connection URL is not a valid HTTP(S) URL"))?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.cannot_be_a_base()
        || parsed.host_str().is_none()
    {
        anyhow::bail!("MCP connection URL must be an explicit hierarchical HTTP(S) URL with a host")
    }
    Ok(parsed)
}

/// Parse and canonicalize a credential-free hierarchical HTTP(S) URL for guided setup or display.
///
/// This is intentionally stricter than the manual MCP configuration parser and the persisted
/// review compatibility path. It must not be used to reinterpret an existing operator-authored
/// destination.
pub fn guided_mcp_url(raw: &str) -> anyhow::Result<String> {
    let parsed = explicit_http_url(raw)?;
    if !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        anyhow::bail!(
            "MCP connection URLs with user info, query parameters, or fragments cannot be reviewed safely"
        )
    }
    Ok(parsed.as_str().to_owned())
}

fn ensure_no_shaped_secret(value: &Value, label: &str) -> anyhow::Result<()> {
    let scrubber = nh_vault::Scrubber::new(Vec::new());
    let mut scrubbed = value.clone();
    if !super::client::scrub_json_strings(&mut scrubbed, &scrubber) || scrubbed != *value {
        anyhow::bail!("MCP {label} contains secret-shaped data and cannot be reviewed safely")
    }
    Ok(())
}

pub(super) fn safe_name_component(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn validate_fingerprint(value: &str) -> anyhow::Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        anyhow::bail!("MCP review fingerprint must be 64 lowercase hexadecimal characters")
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> McpServerConfig {
        McpServerConfig {
            name: "sample".into(),
            url: "https://mcp.example.invalid/mcp".into(),
            spec: super::super::client::SPEC_DEFAULT.into(),
            auth: McpAuth::None,
            scopes: vec!["read".into(), "write".into()],
            default_mode: None,
            trust: super::super::config::McpTrust::Ask,
        }
    }

    fn reviewed(descriptor: Value) -> ReviewedTool {
        ReviewedTool {
            fingerprint: fingerprint(&descriptor).unwrap(),
            descriptor,
        }
    }

    #[test]
    fn state_round_trip_preserves_native_field_presence_and_sorted_authority() {
        let first = reviewed(json!({
            "name": "alpha",
            "description": "Read one item.",
            "inputSchema": {"type": "object"},
            "outputSchema": {"type": "object"},
            "annotations": {"readOnlyHint": true}
        }));
        let second = reviewed(json!({"name": "beta"}));
        let connection = connection_descriptor(&config()).unwrap();
        let snapshot = McpReviewSnapshot {
            server: "sample".into(),
            connection_fingerprint: fingerprint(&connection).unwrap(),
            connection,
            tools: vec![first, second],
            omitted_tools: 0,
        };

        let state =
            McpReviewState::from_snapshot(snapshot, ["beta".to_string(), "alpha".to_string()])
                .unwrap();
        let pretty = state.to_pretty_json().unwrap();
        let compact = state.to_compact_json().unwrap();
        let decoded = McpReviewState::parse(&compact).unwrap();

        assert!(pretty.len() > compact.len());
        assert_eq!(decoded.enabled, ["alpha", "beta"]);
        assert!(decoded.snapshot[0].descriptor.get("outputSchema").is_some());
        assert!(decoded.snapshot[1].descriptor.get("description").is_none());
        assert!(decoded.connection.get("defaultMode").is_none());
        assert_eq!(decoded.connection["scopes"], json!(["read", "write"]));
    }

    #[test]
    fn tampered_fingerprints_and_oversized_state_are_rejected() {
        let descriptor = json!({"name": "alpha"});
        let connection = connection_descriptor(&config()).unwrap();
        let mut state = McpReviewState {
            version: REVIEW_VERSION,
            server: "sample".into(),
            connection_fingerprint: fingerprint(&connection).unwrap(),
            connection,
            snapshot: vec![reviewed(descriptor)],
            enabled: vec!["alpha".into()],
        };
        state.snapshot[0].fingerprint = "0".repeat(64);
        let text = serde_json::to_string(&state).unwrap();
        assert!(McpReviewState::parse(&text)
            .unwrap_err()
            .to_string()
            .contains("tool fingerprint"));

        let oversized = format!("{{\"padding\":\"{}\"}}", "x".repeat(MAX_MCP_REVIEW_BYTES));
        assert!(McpReviewState::parse(&oversized)
            .unwrap_err()
            .to_string()
            .contains("exceeds"));
    }

    #[test]
    fn secret_bearing_or_invalid_metadata_is_unreviewable_before_hashing() {
        let literal = concat!("sk-", "test-review-", "0123456789abcdef");
        let mut secrets = nh_vault::SecretRegistry::new();
        secrets.insert(nh_vault::secret(literal.to_string()));
        let scrubber = secrets.scrubber();

        assert!(reviewed_tool(
            json!({"name": "peek", "description": format!("uses {literal}")}),
            &scrubber
        )
        .unwrap()
        .is_none());
        assert!(reviewed_tool(
            json!({"name": "peek", "inputSchema": {(literal): {"type": "string"}}}),
            &scrubber
        )
        .unwrap()
        .is_none());
        assert!(reviewed_tool(
            json!({"name": "peek", "inputSchema": null}),
            &nh_vault::Scrubber::new(Vec::new())
        )
        .is_err());
    }

    #[test]
    fn connection_identity_rejects_secret_bearing_urls_and_preserves_scope_order() {
        let mut configured = config();
        configured.url = "https://mcp.example.invalid/mcp?token=value".into();
        assert!(connection_descriptor(&configured)
            .unwrap_err()
            .to_string()
            .contains("cannot be reviewed safely"));

        configured.url = "https://mcp.example.invalid/mcp".into();
        configured.auth = McpAuth::OAuth2 {
            token_url: "https://auth.example.invalid/token".into(),
            client_id: "client".into(),
            vault_entry: "mcp-example".into(),
        };
        configured.default_mode = Some("discover".into());
        let descriptor = connection_descriptor(&configured).unwrap();
        assert_eq!(descriptor["scopes"], json!(["read", "write"]));
        assert_eq!(descriptor["defaultMode"], json!("discover"));
        assert_eq!(descriptor["auth"]["kind"], json!("oauth2"));
    }

    #[test]
    fn guided_urls_require_explicit_hierarchical_http_and_canonicalize_parser_whitespace() {
        for invalid in [
            "alice:hunter2@example.invalid/mcp",
            "localhost:8765/mcp",
            "mailto:x@example.invalid",
            "https:user@example.invalid/mcp",
            "https:example.invalid/mcp",
            "https:/example.invalid/mcp",
            "http:/203.0.113.5/mcp",
            "https:\t//example.invalid/mcp",
        ] {
            assert!(guided_mcp_url(invalid).is_err(), "accepted {invalid:?}");
        }

        assert_eq!(
            guided_mcp_url(" \thttps://EXAMPLE.invalid:443/m\tcp\r ").unwrap(),
            "https://example.invalid/mcp"
        );
    }

    #[test]
    fn every_reviewed_metadata_field_changes_descriptor_identity() {
        let base = json!({
            "name": "peek",
            "description": "Read one item.",
            "inputSchema": {"type": "object"},
            "outputSchema": {"type": "object"},
            "annotations": {"readOnlyHint": true}
        });
        let original = fingerprint(&base).unwrap();
        let changes = [
            json!({
                "name": "other",
                "description": "Read one item.",
                "inputSchema": {"type": "object"},
                "outputSchema": {"type": "object"},
                "annotations": {"readOnlyHint": true}
            }),
            json!({
                "name": "peek",
                "description": "Changed.",
                "inputSchema": {"type": "object"},
                "outputSchema": {"type": "object"},
                "annotations": {"readOnlyHint": true}
            }),
            json!({
                "name": "peek",
                "description": "Read one item.",
                "inputSchema": {"type": "object", "required": ["id"]},
                "outputSchema": {"type": "object"},
                "annotations": {"readOnlyHint": true}
            }),
            json!({
                "name": "peek",
                "description": "Read one item.",
                "inputSchema": {"type": "object"},
                "outputSchema": {"type": "array"},
                "annotations": {"readOnlyHint": true}
            }),
            json!({
                "name": "peek",
                "description": "Read one item.",
                "inputSchema": {"type": "object"},
                "outputSchema": {"type": "object"},
                "annotations": {"readOnlyHint": false}
            }),
        ];

        for changed in changes {
            assert_ne!(fingerprint(&changed).unwrap(), original);
        }
    }
}
