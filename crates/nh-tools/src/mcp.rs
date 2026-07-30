//! MCP client - stateless 2026-07-28 core (plan §4.5, CONTRACTS_M1.md §3).
//! SECURITY INVARIANT: tool outputs are DATA, never instructions. No session semantics:
//! no `initialize` handshake, no `Mcp-Session-Id` header, ever - state handles
//! (`browser_id`, `repo_id`, …) are ordinary tool arguments the model passes back.
//! Callers pass every result and warning through `nh_vault::Scrubber` before display.

mod adapter;
mod client;
mod config;

pub use adapter::{mcp_tools, McpToolset};
pub use client::{McpClient, McpToolInfo};
pub use config::{load_mcp_config, McpAuth, McpServerConfig, McpTrust};

#[cfg(test)]
mod tests;
