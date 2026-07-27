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

use client::{ToolEntry, ARGS_SUMMARY_MAX, SPEC_DEFAULT, SPEC_FALLBACK};

#[cfg(test)]
use adapter::{args_one_line, McpToolAdapter};
#[cfg(test)]
use client::{lint_headers, MAX_MCP_BODY_BYTES, MAX_TOOLS};

use anyhow::{bail, Context};
use nh_vault::{EnvFallbackVault, KeyringVault, SecretRegistry, SecretValue, Vault};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::{Tool, ToolCtx, ToolSpec};

#[cfg(test)]
mod tests;
