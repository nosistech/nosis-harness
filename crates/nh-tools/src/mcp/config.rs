//! MCP server configuration, authentication, and trust policy.

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpAuth {
    None,
    ApiKey {
        vault_entry: String,
    },
    OAuth2 {
        token_url: String,
        client_id: String,
        vault_entry: String,
    },
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
    token_url: Option<String>,
    client_id: Option<String>,
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
        "oauth2" => match (raw.token_url, raw.client_id, raw.vault_entry) {
            (Some(token_url), Some(client_id), Some(vault_entry)) => McpAuth::OAuth2 {
                token_url,
                client_id,
                vault_entry,
            },
            (_, _, vault_entry) => {
                let entry = vault_entry.as_deref().unwrap_or(&name);
                bail!(
                    "mcp server \"{name}\": auth = \"oauth2\" needs token_url, client_id, and vault_entry — add them to .nosis/mcp.toml and run `nh key add {entry}-refresh` and `nh key add {entry}-secret`"
                )
            }
        },
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
