//! Route transport and thinking-policy vocabulary.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wire {
    OpenAi,
    AnthropicMessages,
}

/// Backend class: "api" = direct and token-metered, "local" = an explicitly
/// selected loopback OpenAI-compatible runtime, and "delegate" = a subscription
/// child CLI (the delegate schema parses, but its adapter does not ship).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteClass {
    Api,
    Local,
    Delegate,
}

impl RouteClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Api => "api",
            Self::Local => "local",
            Self::Delegate => "delegate",
        }
    }
}

/// Verbatim user-facing qualifier for local-route metering.
pub const LOCAL_METER_COPY: &str = "Local: no billed tokens; hardware and power are not metered.";

/// How a route expresses thinking effort on the wire (plan §3, A.1-A.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThinkingDialect {
    /// DeepSeek Non/High/Max via a body param (mapping pinned in CONTRACTS_M1.md).
    DeepseekNhm,
    /// Kimi K2.6: explicit thinking enable/disable toggle.
    KimiToggle,
    /// Kimi K2.7: no non-thinking mode exists - never send a toggle.
    AlwaysThinking,
    /// Always-thinking route with Low/High/Max effort control.
    AlwaysThinkingEffort,
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
            Self::AlwaysThinkingEffort => "always-thinking-effort",
            Self::GlmHm => "glm-hm",
            Self::None => "none",
        }
    }
}
