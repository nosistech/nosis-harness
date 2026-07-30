//! Tool adapters that project remote MCP tools into the Nosis approval model.

use super::client::{McpClient, ToolEntry, ARGS_SUMMARY_MAX};
use super::config::{McpServerConfig, McpTrust};
use crate::{Tool, ToolCtx, ToolSpec};
use serde_json::Value;
use std::sync::Arc;

/// Adapters for every configured server, plus one friendly warning line per
/// server whose tools could not be listed (never a hard failure).
pub struct McpToolset {
    pub tools: Vec<Box<dyn Tool>>,
    pub warnings: Vec<String>,
}

/// Build one adapter per server tool, named `mcp__<server>__<tool>`.
/// `trust = "block"` servers are never contacted and offer no tools.
pub fn mcp_tools(configs: &[McpServerConfig], send_allowed: &dyn Fn(&str) -> bool) -> McpToolset {
    let mut tools: Vec<Box<dyn Tool>> = Vec::new();
    let mut warnings = Vec::new();
    for config in configs {
        if config.trust == McpTrust::Block {
            continue;
        }
        let server = config.name.clone();
        let Some(host) = nh_vault::host_of(&config.url) else {
            warnings.push(format!(
                "mcp server \"{server}\": could not parse a host from its url — not contacted"
            ));
            continue;
        };
        if !send_allowed(&host) {
            warnings.push(format!(
                "mcp server \"{server}\": destination {host} is blocked by law ([send]) — not contacted"
            ));
            continue;
        }
        let trust = config.trust;
        let client = match McpClient::new(config.clone()) {
            Ok(client) => Arc::new(client),
            Err(error) => {
                warnings.push(format!("mcp server \"{server}\": {error}"));
                continue;
            }
        };
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

pub(super) struct McpToolAdapter {
    pub(super) server: String,
    pub(super) trust: McpTrust,
    pub(super) entry: ToolEntry,
    pub(super) client: Arc<McpClient>,
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
        let Some(host) = nh_vault::host_of(&self.client.config.url) else {
            return Ok("blocked by law: could not parse the MCP server host".to_string());
        };
        let send_asks = match (ctx.guard)(&crate::Access::Send(&host)) {
            crate::Guard::Block(reason) => return Ok(format!("blocked by law: {reason}")),
            crate::Guard::Ask => true,
            crate::Guard::Allow => false,
        };
        let tool = &self.entry.info.name;
        match self.trust {
            McpTrust::Block => {
                return Ok(
                    "blocked by .nosis/mcp.toml (trust = \"block\") — set trust = \"ask\" to enable"
                        .to_string(),
                );
            }
            // Safe because nh-cli only accepts auto trust from user-global config.
            McpTrust::Auto if self.entry.read_only && !send_asks => {}
            // Ask, and every possibly state-mutating call at any autonomy level.
            _ => {
                let ask = format!("mcp {} {} {}", self.server, tool, args_one_line(&args));
                if !(ctx.approve)(&ask) {
                    // Ok-shaped so the model can read the denial and adapt.
                    return Ok(format!("user denied: mcp {} {}", self.server, tool));
                }
            }
        }
        let raw = self.client.call_tool(tool, args)?;
        Ok(crate::ToolResultEnvelope::new(raw, &ctx.scrubber).render())
    }
}

/// Compact JSON args on one line with an honest overflow marker.
pub(super) fn args_one_line(args: &Value) -> String {
    let compact = serde_json::to_string(args).unwrap_or_else(|_| "{}".to_string());
    let total = compact.chars().count();
    if total <= ARGS_SUMMARY_MAX {
        return compact;
    }
    let head: String = compact.chars().take(ARGS_SUMMARY_MAX).collect();
    format!("{head}… (+{} more chars)", total - ARGS_SUMMARY_MAX)
}
