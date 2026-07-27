//! Route transport and thinking-policy vocabulary.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wire {
    OpenAi,
    AnthropicMessages,
}

/// Backend class (plan A.0): "api" = direct, token-metered; "delegate" =
/// subscription child CLI (claude/codex — adapter lands in M4, schema parses today).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteClass {
    Api,
    Delegate,
}

/// How a route expresses thinking effort on the wire (plan §3, A.1–A.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThinkingDialect {
    /// DeepSeek Non/High/Max via a body param (mapping pinned in CONTRACTS_M1.md).
    DeepseekNhm,
    /// Kimi K2.6: explicit thinking enable/disable toggle.
    KimiToggle,
    /// Kimi K2.7: no non-thinking mode exists — never send a toggle.
    AlwaysThinking,
    /// GLM thinking High/Max only.
    GlmHm,
    /// No effort toggle for this route.
    None,
}

impl ThinkingDialect {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::DeepseekNhm => "deepseek-nhm",
            Self::KimiToggle => "kimi-toggle",
            Self::AlwaysThinking => "always-thinking",
            Self::GlmHm => "glm-hm",
            Self::None => "none",
        }
    }
}
