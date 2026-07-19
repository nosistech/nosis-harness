//! `nh run` - resolve the route, fetch the key, drive the agent loop.
//! Progress = one short line per tool call via `on_event`; exec_shell additionally
//! surfaces through its approval prompt. All errors: one friendly line, exit 1.

use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use nh_core::agent::AgentLoop;
use nh_core::receipt::{Outcome, ReceiptWriter};
use nh_core::wire::{cache_hit_pct, make_client, resolve_effort, ThinkingEffort, Usage};
use nh_law::{Autonomy, LoadOptions};
use nh_routes::{
    cost_of, money, money_with_gloss, saved_pct, PriceConfidence, ResolvedRoute, RouteClass,
    RouteResolver, ThinkingDialect, ThinkingPosture, Wire,
};
use nh_tools::{builtin_tools, Access, McpAuth, McpServerConfig, ToolCtx};
use nh_vault::{EnvFallbackVault, KeyringVault, Scrubber};

use crate::guard_from;

/// What callers print when a route resolves to a subscription delegate (M4 scope).
pub(crate) const DELEGATE_MSG: &str = "delegate routes arrive in M4 - pick an api route";

/// `--think` levels; clap renders them as none|low|high|max.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum ThinkArg {
    None,
    Low,
    High,
    Max,
}

/// `--autonomy` levels; absence defers to user/bundled law.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum AutonomyArg {
    Ask,
    Auto,
}

/// The only CLI-autonomy translation point.
pub(crate) fn autonomy_for(autonomy: Option<AutonomyArg>) -> Option<Autonomy> {
    autonomy.map(|value| match value {
        AutonomyArg::Ask => Autonomy::Ask,
        AutonomyArg::Auto => Autonomy::Auto,
    })
}

/// Translate the CLI override once, then delegate the posture × capability
/// matrix to nh-core.
pub(crate) fn effort_for(
    think: Option<ThinkArg>,
    posture: ThinkingPosture,
    dialect: ThinkingDialect,
    wire: Wire,
) -> ThinkingEffort {
    let explicit = think.map(|value| match value {
        ThinkArg::None => ThinkingEffort::None,
        ThinkArg::Low => ThinkingEffort::Low,
        ThinkArg::High => ThinkingEffort::High,
        ThinkArg::Max => ThinkingEffort::Max,
    });
    resolve_effort(explicit, posture, dialect, wire)
}

pub(crate) fn profile_fallback_warning(requested: &str, effective: &str) -> Option<String> {
    (requested != effective).then(|| {
        format!(
            "unknown profile '{requested}' - using {effective}; run `nh profile` to list choices"
        )
    })
}

pub fn run(
    task: &str,
    model: &str,
    max_turns: u32,
    think: Option<ThinkArg>,
    autonomy: Option<AutonomyArg>,
    profile: &str,
) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let (root, catalog) = find_catalog(&cwd)?;
    let law = nh_law::load(
        &root,
        &LoadOptions {
            cli_autonomy: autonomy_for(autonomy),
        },
    );
    let warning_scrubber = Scrubber::new(Vec::new());
    for warning in &law.warnings {
        eprintln!("warning: {}", safe_line(&warning_scrubber, warning));
    }
    let resolver = RouteResolver::from_toml(&catalog)?;
    let route = resolver.resolve(model)?;
    let (profiles, profile_warnings) = nh_routes::Profiles::load(&root);
    for warning in &profile_warnings {
        eprintln!("warning: {}", safe_line(&warning_scrubber, warning));
    }
    let execution_policy = profiles.effective(profile, &route);
    if let Some(warning) = profile_fallback_warning(profile, &execution_policy.profile) {
        eprintln!("warning: {}", safe_line(&warning_scrubber, &warning));
    }
    if route.class == RouteClass::Delegate {
        anyhow::bail!("{DELEGATE_MSG}");
    }

    let vault = EnvFallbackVault {
        inner: KeyringVault,
    };
    let approved = law.policy.approved_audiences(&route.vault_entry);
    let host = nh_vault::normalized_host(&route.base_url).unwrap_or_default();
    let key = nh_vault::get_scoped(&vault, &route.vault_entry, &host, &approved)?;
    // Audience validation happens before this best-effort registry fetch, so an
    // untrusted catalog cannot materialize a credential by redirecting its route.
    let vault_entries = catalog_vault_entries(&resolver);
    let session_scrubber = nh_vault::from_vault(&vault, &vault_entries);
    // Scrubbers hold every resolvable catalog credential so no output path can
    // leak one - receipts, stdout, progress, and approvals all pass one.
    let client = make_client(&execution_policy.clamp_route(&route), key);
    let approve_scrubber = session_scrubber.clone();
    let event_scrubber = session_scrubber.clone();
    let policy = law.policy.clone();
    let ctx = ToolCtx::new(
        cwd,
        // Model-supplied commands are scrubbed + control-char-escaped before display
        // so the approval gate always shows one faithful line.
        Box::new(move |action| approve_on_stdin(&safe_line(&approve_scrubber, action))),
    )
    .with_guard(Box::new(move |access| match access {
        Access::Read(path) => guard_from(policy.read_verdict(path)),
        Access::Write(path) => guard_from(policy.write_verdict(path)),
        Access::Exec(command) => guard_from(policy.exec_verdict(command)),
        Access::Send(target) => guard_from(policy.send_verdict(target)),
    }));
    let receipts = ReceiptWriter {
        path: root.join(".nosis").join("receipts.jsonl"),
        scrubber: session_scrubber.clone(),
    };
    let mut agent = AgentLoop {
        client,
        tools: builtin_tools(),
        ctx,
        receipts,
        model_id: route.model_id.clone(),
        max_turns,
        thinking: effort_for(
            think,
            execution_policy.posture,
            route.thinking_dialect,
            route.wire.clone(),
        ),
        profile: Some(execution_policy.profile.clone()),
        // Honest identity: name the real route + forbid claiming to be Claude/GPT.
        constitution: Some(nh_tui::identity_constitution(&law.constitution, &route)),
        context_limit: route.context,
        on_event: Some(Box::new(move |line| {
            eprintln!("  {}", safe_line(&event_scrubber, line))
        })),
    };

    eprintln!("running {} (max {max_turns} turns)", route.model_id);
    let scrubber = session_scrubber;
    let (answer, receipt) = agent
        .run(task)
        .map_err(|e| anyhow::anyhow!("{}", scrubber.scrub(&e.to_string())))?;

    println!("{}", scrubber.scrub(&answer));
    let usage = receipt.usage.clone().unwrap_or_default();
    let cache = cache_hit_pct(usage.prompt_tokens, usage.cached_tokens.unwrap_or(0))
        .map(|pct| format!(" | cache {pct:.0}%"))
        .unwrap_or_default();
    println!(
        "{}",
        safe_line(
            &scrubber,
            &format!(
                "turns {} | tool calls {} | tokens {} in / {} out / {} cached{}",
                receipt.turns,
                receipt.tool_calls,
                usage.prompt_tokens,
                usage.completion_tokens,
                usage.cached_tokens.unwrap_or(0),
                cache
            )
        )
    );
    if let Some(line) = turn_cost_line(&resolver, &route, &usage, Utc::now()) {
        println!("{}", safe_line(&scrubber, &line));
    }
    if receipt.outcome == Outcome::Timeout {
        anyhow::bail!(
            "stopped at max turns ({max_turns}) - rerun with --max-turns {}",
            max_turns.saturating_mul(2)
        );
    }
    Ok(())
}

pub(crate) fn turn_cost_line(
    resolver: &RouteResolver,
    route: &ResolvedRoute,
    usage: &Usage,
    at: DateTime<Utc>,
) -> Option<String> {
    let quote = route.price_at(at)?;
    let cached = usage.cached_tokens.unwrap_or(0);
    let actual = cost_of(&quote, usage.prompt_tokens, cached, usage.completion_tokens);
    let mut paid = money_with_gloss(actual, quote.currency, resolver.fx(), at);
    if quote.stale || quote.confidence == PriceConfidence::VerifyLive {
        paid.push('*');
    }
    let mut line = format!("cost {paid}");
    let naive = resolver.naive_cost(
        route,
        usage.prompt_tokens,
        cached,
        usage.completion_tokens,
        at,
    );
    if let Some(percent) = naive
        .as_ref()
        .and_then(|costs| saved_pct(actual, costs.no_cache))
    {
        line.push_str(&format!(" - saved {percent}% vs no-cache"));
    }
    if let Some(costs) = naive {
        line.push_str(&format!(
            "   (peak {} · no-cache {} · top-tier {})",
            money(costs.peak, costs.currency),
            money(costs.no_cache, costs.currency),
            money(costs.top_tier, costs.currency)
        ));
    }
    if quote.stale {
        line.push_str(" · *price stale");
    } else if quote.confidence == PriceConfidence::VerifyLive {
        line.push_str(" · *price verify_live");
    }
    Some(line)
}

/// Walk up from `start` looking for catalog.toml; return its directory and contents.
pub(crate) fn find_catalog(start: &Path) -> anyhow::Result<(PathBuf, String)> {
    for dir in start.ancestors() {
        let candidate = dir.join("catalog.toml");
        if candidate.is_file() {
            let text = fs::read_to_string(&candidate)?;
            return Ok((dir.to_path_buf(), text));
        }
    }
    anyhow::bail!("no catalog.toml found - run `nh init` to create one")
}

pub(crate) fn catalog_vault_entries(resolver: &RouteResolver) -> Vec<String> {
    resolver
        .available()
        .into_iter()
        .filter_map(|id| resolver.resolve(&id).ok())
        .map(|route| route.vault_entry)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn unapproved_mcp_target<'a>(
    config: &'a McpServerConfig,
    approved: &[String],
) -> Option<(&'a str, &'a str)> {
    match &config.auth {
        McpAuth::None => None,
        McpAuth::ApiKey { vault_entry } => (!nh_vault::audience_allows(&config.url, approved))
            .then_some((vault_entry.as_str(), config.url.as_str())),
        McpAuth::OAuth2 {
            token_url,
            vault_entry,
            ..
        } => {
            if !nh_vault::audience_allows(&config.url, approved) {
                Some((vault_entry.as_str(), config.url.as_str()))
            } else if !nh_vault::audience_allows(token_url, approved) {
                Some((vault_entry.as_str(), token_url.as_str()))
            } else {
                None
            }
        }
    }
}

/// Drop MCP servers whose configured credential could be sent outside its
/// trusted law audience. Each dropped server contributes one secret-free line.
pub(crate) fn filter_mcp_audiences(
    configs: Vec<McpServerConfig>,
    policy: &nh_law::Policy,
    warnings: &mut Vec<String>,
) -> Vec<McpServerConfig> {
    filter_mcp_audiences_with(configs, warnings, |entry| policy.approved_audiences(entry))
}

fn filter_mcp_audiences_with(
    configs: Vec<McpServerConfig>,
    warnings: &mut Vec<String>,
    approved_for: impl Fn(&str) -> Vec<String>,
) -> Vec<McpServerConfig> {
    configs
        .into_iter()
        .filter_map(|config| {
            let entry = match &config.auth {
                McpAuth::None => return Some(config),
                McpAuth::ApiKey { vault_entry } | McpAuth::OAuth2 { vault_entry, .. } => {
                    vault_entry
                }
            };
            let approved = approved_for(entry);
            if let Some((entry, target)) = unapproved_mcp_target(&config, &approved) {
                warnings.push(format!(
                    "mcp server \"{}\" dropped - credential \"{entry}\" is not approved for {}",
                    config.name,
                    nh_vault::normalized_host(target).as_deref().unwrap_or("")
                ));
                None
            } else {
                Some(config)
            }
        })
        .collect()
}

/// Scrub secrets, then escape for display. Every stderr line built from
/// model-controlled text goes through this - one choke point.
pub(crate) fn safe_line(scrubber: &Scrubber, text: &str) -> String {
    nh_vault::safe_line(scrubber, text)
}

/// Approval gate: one line on stderr, default deny. `display` is the command
/// already scrubbed and control-char-escaped by the caller (see `safe_line`).
pub(crate) fn approve_on_stdin(display: &str) -> bool {
    eprint!("  approve? {display}  [y/N] ");
    let _ = io::stderr().flush();
    let mut line = String::new();
    if io::stdin().read_line(&mut line).is_err() {
        return false;
    }
    is_yes(&line)
}

fn is_yes(line: &str) -> bool {
    matches!(line.trim(), "y" | "Y" | "yes" | "Yes" | "YES")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_catalog_walking_up_from_nested_dir() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("catalog.toml"), "# test catalog").unwrap();
        let nested = tmp.path().join("a").join("b");
        fs::create_dir_all(&nested).unwrap();

        let (dir, text) = find_catalog(&nested).unwrap();
        assert_eq!(dir, tmp.path());
        assert_eq!(text, "# test catalog");
    }

    #[test]
    fn missing_catalog_error_is_actionable() {
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("deep").join("nowhere");
        fs::create_dir_all(&nested).unwrap();
        let err = find_catalog(&nested).unwrap_err();
        assert!(err.to_string().contains("nh init"), "got: {err}");
    }

    #[test]
    fn mcp_audience_checks_are_host_only() {
        let approved = vec!["api.deepseek.com".to_string()];
        let api = McpServerConfig {
            name: "api".into(),
            url: "https://evil.example/mcp".into(),
            spec: "2026-07-28".into(),
            auth: McpAuth::ApiKey {
                vault_entry: "deepseek".into(),
            },
            scopes: Vec::new(),
            default_mode: None,
            trust: nh_tools::McpTrust::Ask,
        };
        assert_eq!(
            unapproved_mcp_target(&api, &approved),
            Some(("deepseek", "https://evil.example/mcp"))
        );

        let mut oauth = api.clone();
        oauth.url = "https://api.deepseek.com/mcp".into();
        oauth.auth = McpAuth::OAuth2 {
            token_url: "https://evil.example/token".into(),
            client_id: "client".into(),
            vault_entry: "deepseek".into(),
        };
        assert_eq!(
            unapproved_mcp_target(&oauth, &approved),
            Some(("deepseek", "https://evil.example/token"))
        );

        let mut warnings = Vec::new();
        let kept = filter_mcp_audiences_with(vec![api, oauth], &mut warnings, |_| approved.clone());
        assert!(kept.is_empty());
        assert_eq!(warnings.len(), 2, "one warning per dropped server");
        assert!(warnings.iter().all(|warning| warning.contains("dropped")));
    }

    #[test]
    fn yes_parsing_defaults_to_deny() {
        assert!(is_yes("y\n"));
        assert!(is_yes("  yes  \n"));
        assert!(!is_yes("\n"));
        assert!(!is_yes("n\n"));
        assert!(!is_yes("whatever\n"));
    }

    #[test]
    fn think_flag_resolves_within_route_capability() {
        let cases = [
            (ThinkArg::None, ThinkingEffort::None),
            (ThinkArg::Low, ThinkingEffort::None),
            (ThinkArg::High, ThinkingEffort::High),
            (ThinkArg::Max, ThinkingEffort::Max),
        ];
        for (arg, want) in cases {
            assert_eq!(
                effort_for(
                    Some(arg),
                    ThinkingPosture::Default,
                    ThinkingDialect::DeepseekNhm,
                    Wire::OpenAi,
                ),
                want
            );
            assert_eq!(
                effort_for(
                    Some(arg),
                    ThinkingPosture::Default,
                    ThinkingDialect::AlwaysThinking,
                    Wire::OpenAi,
                ),
                ThinkingEffort::High
            );
        }
    }

    #[test]
    fn anthropic_wire_effort_is_provider_default() {
        assert_eq!(
            effort_for(
                Some(ThinkArg::High),
                ThinkingPosture::Ceiling,
                ThinkingDialect::DeepseekNhm,
                Wire::AnthropicMessages,
            ),
            ThinkingEffort::None
        );
    }

    #[test]
    fn think_default_follows_route_dialect() {
        // Always-thinking and high/max-only routes run at High; effort-toggle
        // routes stay at None until the user asks (cheap by default).
        assert_eq!(
            effort_for(
                None,
                ThinkingPosture::Default,
                ThinkingDialect::AlwaysThinking,
                Wire::OpenAi,
            ),
            ThinkingEffort::High
        );
        assert_eq!(
            effort_for(
                None,
                ThinkingPosture::Default,
                ThinkingDialect::GlmHm,
                Wire::OpenAi,
            ),
            ThinkingEffort::High
        );
        assert_eq!(
            effort_for(
                None,
                ThinkingPosture::Default,
                ThinkingDialect::DeepseekNhm,
                Wire::OpenAi,
            ),
            ThinkingEffort::None
        );
        assert_eq!(
            effort_for(
                None,
                ThinkingPosture::Default,
                ThinkingDialect::None,
                Wire::OpenAi,
            ),
            ThinkingEffort::None
        );
    }

    #[test]
    fn autonomy_mapping_is_optional_and_exact() {
        assert_eq!(autonomy_for(None), None);
        assert_eq!(autonomy_for(Some(AutonomyArg::Ask)), Some(Autonomy::Ask));
        assert_eq!(autonomy_for(Some(AutonomyArg::Auto)), Some(Autonomy::Auto));
    }

    #[test]
    fn safe_line_redacts_key_shapes_and_literals_before_stderr() {
        let scrubber = Scrubber::new(vec!["fake-literal-secret".to_string()]);
        let line = safe_line(
            &scrubber,
            "curl -H 'Authorization: sk-test-00000000' fake-literal-secret\x1b[1A",
        );
        assert!(!line.contains("sk-test-00000000"), "got: {line}");
        assert!(!line.contains("fake-literal-secret"), "got: {line}");
        assert!(line.contains("[REDACTED]"), "got: {line}");
        assert!(!line.chars().any(|c| c.is_control()), "got: {line}");
    }
}
