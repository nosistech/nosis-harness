//! Command, tool, trust, and MCP palette projection and filtering.

use crate::state::{McpState, PaletteAction, PaletteEntry};
use nh_law::{Autonomy, PolicyView};
use nh_tools::{builtin_tools, McpAuth, McpServerConfig, McpToolset, McpTrust};

/// Project configured MCP servers and discovered tools into immutable palette rows.
pub fn mcp_palette_entries(configs: &[McpServerConfig], toolset: &McpToolset) -> Vec<PaletteEntry> {
    if configs.is_empty() {
        return if toolset.warnings.is_empty() {
            Vec::new()
        } else {
            vec![PaletteEntry {
                kind: "server",
                name: "MCP configuration".into(),
                description: "configuration could not be loaded".into(),
                state: Some(McpState::Stale),
                action: PaletteAction::Describe,
            }]
        };
    }

    let specs: Vec<_> = toolset.tools.iter().map(|tool| tool.spec()).collect();
    let mut entries = Vec::new();
    for config in configs {
        let state = mcp_state(config, &toolset.warnings);
        entries.push(PaletteEntry {
            kind: "server",
            name: config.name.clone(),
            description: "configured MCP server".into(),
            state: Some(state),
            action: PaletteAction::Describe,
        });

        let prefix = format!("mcp__{}__", config.name);
        for spec in specs.iter().filter(|spec| spec.name.starts_with(&prefix)) {
            entries.push(PaletteEntry {
                kind: "tool",
                name: spec.name.clone(),
                description: spec.description.clone(),
                state: Some(state),
                action: PaletteAction::Describe,
            });
        }
    }
    entries
}

/// Case-insensitive substring filter over an in-memory palette.
pub fn filter_palette<'a>(entries: &'a [PaletteEntry], query: &str) -> Vec<&'a PaletteEntry> {
    let query = query.to_lowercase();
    entries
        .iter()
        .filter(|entry| {
            query.is_empty()
                || entry.kind.to_lowercase().contains(&query)
                || entry.name.to_lowercase().contains(&query)
                || entry.description.to_lowercase().contains(&query)
                || entry
                    .state
                    .is_some_and(|state| state.as_str().contains(&query))
        })
        .collect()
}

pub(super) fn mcp_state(config: &McpServerConfig, warnings: &[String]) -> McpState {
    if config.trust == McpTrust::Block || matches!(config.auth, McpAuth::OAuth2 { .. }) {
        return McpState::DiscoverOnly;
    }
    let warning_prefix = format!("mcp server \"{}\"", config.name);
    if warnings
        .iter()
        .any(|warning| warning.contains(&warning_prefix))
    {
        return McpState::Stale;
    }
    match config.auth {
        McpAuth::ApiKey { .. } => McpState::AuthOk,
        McpAuth::None => McpState::Enabled,
        McpAuth::OAuth2 { .. } => McpState::DiscoverOnly,
    }
}

pub(super) fn builtin_palette_entries() -> Vec<PaletteEntry> {
    let commands = [
        (
            "/help",
            "show commands, tools, and MCP state",
            PaletteAction::Palette,
        ),
        ("/?", "alias for /help", PaletteAction::Palette),
        (
            "/trust",
            "view session autonomy and policy rules",
            PaletteAction::TrustDial,
        ),
        (
            "/timeline",
            "view session receipts and answers",
            PaletteAction::Timeline,
        ),
        (
            "/search",
            "search the displayed transcript",
            PaletteAction::Search,
        ),
        (
            "/why",
            "explain the chosen route and the cheaper ones it beat",
            PaletteAction::Why,
        ),
        (
            "/profile <frugal|balanced|max-quality>",
            "set thinking and output spend for the next turn",
            PaletteAction::Prefill("/profile "),
        ),
        (
            "/model <id>",
            "switch model route and keep context",
            PaletteAction::Prefill("/model "),
        ),
        (
            "/provider <name>",
            "switch to a provider's default route",
            PaletteAction::Prefill("/provider "),
        ),
        (
            "/effort <none|low|high|max>",
            "set reasoning effort for subsequent turns",
            PaletteAction::Prefill("/effort "),
        ),
        ("/quit", "quit Nosis Harness", PaletteAction::Quit),
    ];
    let mut entries: Vec<PaletteEntry> = commands
        .into_iter()
        .map(|(name, description, action)| PaletteEntry {
            kind: "command",
            name: name.into(),
            description: description.into(),
            state: None,
            action,
        })
        .collect();
    entries.extend(builtin_tools().into_iter().map(|tool| {
        let spec = tool.spec();
        PaletteEntry {
            kind: "tool",
            name: spec.name,
            description: spec.description,
            state: None,
            action: PaletteAction::Describe,
        }
    }));
    entries
}

pub(super) fn trust_dial_lines(view: &PolicyView) -> Vec<String> {
    let autonomy = match view.autonomy {
        Autonomy::Ask => "ask",
        Autonomy::Auto => "auto",
    };
    let mut lines = vec![format!("session autonomy: {autonomy}")];
    append_rules(&mut lines, "auto-approve", &view.auto_paths);
    append_rules(&mut lines, "always-ask", &view.ask_paths);
    append_rules(&mut lines, "hard-block/protected", &view.block_paths);
    append_rules(&mut lines, "blocked command", &view.block_commands);
    lines
}

pub(super) fn append_rules(lines: &mut Vec<String>, label: &str, rules: &[String]) {
    if rules.is_empty() {
        lines.push(format!("{label}: none"));
    } else {
        lines.extend(rules.iter().map(|rule| format!("{label}: {rule}")));
    }
}

impl PaletteEntry {
    pub(super) fn line(&self) -> String {
        match self.state {
            Some(state) => format!(
                "{}: {} - {} [{}]",
                self.kind,
                self.name,
                self.description,
                state.as_str()
            ),
            None => format!("{}: {} - {}", self.kind, self.name, self.description),
        }
    }
}

pub(super) fn short_text(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let head: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{head}…")
    } else {
        head
    }
}
