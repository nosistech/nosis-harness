//! nh-routes — RouteResolver: the ONLY component that may mint a resolved route (plan §2).
//! M0 scope: parse catalog.toml, resolve by model id, reject banned strings.
//! M1 adds: clock-aware pricing, modality dispatch, thinking dialects.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wire {
    OpenAi,
    AnthropicMessages,
}

#[derive(Debug, Clone)]
pub struct ResolvedRoute {
    pub provider: String,
    pub model_id: String,
    pub base_url: String,
    pub wire: Wire,
    /// nh-vault entry name; env fallback is NH_<ENTRY>_KEY.
    pub vault_entry: String,
}

/// Dead/deprecated model ids (plan §A.9). Exact ids and prefixes; `mimo-v2-` does NOT
/// match `mimo-v2.5-*` (those are current). Rejection errors must name the replacement
/// (e.g. "deepseek-chat is dead as of 2026-07-24 — use deepseek-v4-flash").
pub const BANNED_EXACT: &[&str] = &["deepseek-chat", "deepseek-reasoner"];
pub const BANNED_PREFIXES: &[&str] = &["mimo-v2-", "gpt-5.2", "gpt-5.3-codex", "moonshot-v1-"];

pub fn is_banned(_model_id: &str) -> bool {
    todo!("build agent")
}

pub struct RouteResolver {
    // catalog parsed from catalog.toml
}

impl RouteResolver {
    /// Parse a catalog.toml string (see repo-root catalog.toml for the schema).
    pub fn from_toml(_toml_str: &str) -> anyhow::Result<Self> {
        todo!("build agent")
    }
    /// Resolve by model id. Banned strings error with the replacement suggestion;
    /// unknown ids error listing available routes (friendly UX, no stack traces).
    pub fn resolve(&self, _model_id: &str) -> anyhow::Result<ResolvedRoute> {
        todo!("build agent")
    }
    /// All routable model ids, for `--model` help text and error messages.
    pub fn available(&self) -> Vec<String> {
        todo!("build agent")
    }
}
