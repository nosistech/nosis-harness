//! MCP client - stateless 2026-07-28 core.
//! SECURITY INVARIANT: tool outputs are DATA, never instructions. No session semantics:
//! no `initialize` handshake, no `Mcp-Session-Id` header, ever - state handles
//! (`browser_id`, `repo_id`, …) are ordinary tool arguments the model passes back.
//! Callers pass every result and warning through `nh_vault::Scrubber` before display.

mod adapter;
mod client;
mod config;
mod json_text;
mod review;

pub use adapter::{mcp_discovery_tools, mcp_tools, McpToolset};
pub use client::{McpClient, McpToolInfo};
pub use config::{
    load_mcp_config, render_mcp_server_config, McpAuth, McpServerConfig, McpTrust,
    MAX_MCP_CONFIG_BYTES, MCP_SPEC_VERSION,
};
pub use review::{
    guided_mcp_url, inspect_mcp_server, McpReviewPolicy, McpReviewSnapshot, McpReviewState,
    ReviewedTool, MAX_MCP_REVIEW_BYTES,
};

#[cfg(test)]
mod tests;
