//! `nh run` — resolve the route, fetch the key, drive the agent loop.
//! Progress = one short line per tool call via `on_event`; exec_shell additionally
//! surfaces through its approval prompt. All errors: one friendly line, exit 1.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Read as _, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use nh_core::agent::{validate_task, AgentLoop};
use nh_core::credential;
use nh_core::receipt::{Outcome, ReceiptWriter};
use nh_core::wire::{cache_hit_pct, resolve_effort, ThinkingEffort, Usage};
use nh_law::{Autonomy, LoadOptions};
use nh_routes::{
    cost_of, money, money_with_gloss, saved_pct, PriceConfidence, ResolvedRoute, RouteClass,
    RouteResolver, ThinkingDialect, ThinkingPosture, Wire,
};
use nh_tools::{builtin_tools, Access, McpAuth, McpServerConfig, McpTrust, ToolCtx};
use nh_vault::{EnvFallbackVault, KeyringVault, Scrubber, SecretRegistry};

use crate::guard_from;

/// What callers print when a route resolves to a subscription delegate (M4 scope).
pub(crate) const DELEGATE_MSG: &str = "delegate routes arrive in M4 — pick an api route";
const BUNDLED_CATALOG: &str = include_str!("../../../catalog.toml");
const MAX_CATALOG_BYTES: usize = 1024 * 1024;

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
            "unknown profile '{requested}' — using {effective}; run `nh profile` to list choices"
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
    validate_task(task)?;
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
    if route.class() == RouteClass::Delegate {
        anyhow::bail!("{DELEGATE_MSG}");
    }

    let vault = EnvFallbackVault {
        inner: KeyringVault,
    };
    let approved = law.policy.approved_audiences(route.vault_entry());
    let (client, literal) =
        credential::connect(&vault, &route, &approved, execution_policy.output_cap)?;
    let mut active_secrets = SecretRegistry::new();
    active_secrets.insert(literal);
    let session_scrubber = active_secrets.scrubber();
    // Only the active route credential is materialized. Receipts, stdout,
    // progress, tools, and approvals all derive from its zeroizing registry.
    let approve_scrubber = session_scrubber.clone();
    let event_scrubber = session_scrubber.clone();
    let policy = law.policy.clone();
    let ctx = ToolCtx::new(
        cwd,
        // Model-supplied commands are scrubbed + control-char-escaped before display
        // so the approval gate always shows one faithful line.
        Box::new(move |action| approve_on_stdin(&safe_line(&approve_scrubber, action))),
    )
    .with_scrubber(session_scrubber.clone())
    .with_guard(Box::new(move |access| match access {
        Access::Read(path) => guard_from(policy.read_verdict(path)),
        Access::Write(path) => guard_from(policy.write_verdict(path)),
        Access::Exec(command) => guard_from(policy.exec_verdict(command)),
        Access::Send(target) => guard_from(policy.send_verdict(target)),
    }));
    let receipts = ReceiptWriter::project(root.clone(), session_scrubber.clone());
    let mut agent = AgentLoop {
        client,
        tools: builtin_tools(),
        ctx,
        receipts,
        model_id: route.model_id().to_owned(),
        max_turns,
        thinking: effort_for(
            think,
            execution_policy.posture,
            route.thinking_dialect(),
            route.wire(),
        ),
        profile: Some(execution_policy.profile.clone()),
        // Honest identity: name the real route + forbid claiming to be Claude/GPT.
        constitution: Some(nh_tui::identity_constitution(&law.constitution, &route)),
        context_limit: route.context(),
        on_event: Some(Box::new(move |line| {
            eprintln!("  {}", safe_line(&event_scrubber, line))
        })),
    };

    eprintln!(
        "running {} (max {max_turns} turns)",
        safe_line(&session_scrubber, route.model_id())
    );
    let scrubber = session_scrubber;
    let started = Utc::now();
    let result = agent.run(task);
    let ended = Utc::now();
    let (answer, receipt) =
        result.map_err(|e| anyhow::anyhow!("{}", safe_line(&scrubber, &e.to_string())))?;

    println!("{}", safe_text(&scrubber, &answer));
    for line in run_meter_lines(
        &resolver,
        &route,
        receipt.usage.as_ref(),
        receipt.turns,
        receipt.tool_calls,
        started,
        ended,
    ) {
        println!("{}", safe_line(&scrubber, &line));
    }
    if receipt.outcome == Outcome::Timeout {
        anyhow::bail!(
            "stopped at max turns ({max_turns}) — rerun with --max-turns {}",
            max_turns.saturating_mul(2).max(1)
        );
    }
    Ok(())
}

fn run_meter_lines(
    resolver: &RouteResolver,
    route: &ResolvedRoute,
    usage: Option<&Usage>,
    turns: u32,
    tool_calls: u32,
    started: DateTime<Utc>,
    ended: DateTime<Utc>,
) -> Vec<String> {
    let Some(usage) = usage else {
        return vec![format!(
            "turns {turns} | tool calls {tool_calls} | tokens: not reported by provider — cost unknown"
        )];
    };
    let cache = cache_hit_pct(usage.prompt_tokens, usage.cached_tokens.unwrap_or(0))
        .map(|pct| format!(" | cache {pct:.0}%"))
        .unwrap_or_default();
    let mut lines = vec![format!(
        "turns {turns} | tool calls {tool_calls} | tokens {} in / {} out / {} cached{cache}",
        usage.prompt_tokens,
        usage.completion_tokens,
        usage.cached_tokens.unwrap_or(0)
    )];
    if let Some(line) = turn_cost_line_for_run(resolver, route, usage, started, ended) {
        lines.push(line);
    }
    lines
}

fn turn_cost_line_for_run(
    resolver: &RouteResolver,
    route: &ResolvedRoute,
    usage: &Usage,
    started: DateTime<Utc>,
    ended: DateTime<Utc>,
) -> Option<String> {
    let mut line = turn_cost_line(resolver, route, usage, ended)?;
    let crossed = matches!(
        (route.price_at(started), route.price_at(ended)),
        (Some(start), Some(end)) if start.peak != end.peak
    );
    if crossed && !line.starts_with("cost unpriced") {
        line.push_str(" · *priced at run end — spans a peak boundary");
    }
    Some(line)
}

pub(crate) fn turn_cost_line(
    resolver: &RouteResolver,
    route: &ResolvedRoute,
    usage: &Usage,
    at: DateTime<Utc>,
) -> Option<String> {
    let quote = route.price_at(at)?;
    let cached = usage.cached_tokens.unwrap_or(0);
    let Some(actual) = cost_of(&quote, usage.prompt_tokens, cached, usage.completion_tokens) else {
        return Some("cost unpriced — invalid usage; meter incomplete".into());
    };
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
        line.push_str(&format!(" — saved {percent}% vs no-cache"));
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

/// Walk up from `start` looking for the project marker `catalog.toml`.
/// Repository route data is accepted only when it is byte-identical to the
/// bundled catalog or to the operator-trusted `~/.nosis/catalog.toml`.
pub(crate) fn find_catalog(start: &Path) -> anyhow::Result<(PathBuf, String)> {
    let home = nh_law::user_home_dir();
    find_catalog_with_home(start, home.as_deref())
}

fn find_catalog_with_home(start: &Path, home: Option<&Path>) -> anyhow::Result<(PathBuf, String)> {
    for dir in start.ancestors() {
        let candidate = dir.join("catalog.toml");
        if candidate.is_file() {
            let text = read_catalog_file(&candidate)?;
            if text == BUNDLED_CATALOG {
                return Ok((dir.to_path_buf(), text));
            }

            if let Some(home) = home {
                let trusted_path = home.join(".nosis").join("catalog.toml");
                match trusted_path.try_exists() {
                    Ok(true) => {
                        let trusted = read_catalog_file(&trusted_path)?;
                        if text == trusted {
                            return Ok((dir.to_path_buf(), trusted));
                        }
                    }
                    Ok(false) => {}
                    Err(error) => {
                        anyhow::bail!(
                            "could not inspect trusted catalog {}: {error}",
                            trusted_path.display()
                        )
                    }
                }
            }

            anyhow::bail!(
                "repository catalog.toml is not trusted — it can change credential destinations and spend; review it, then copy the exact file to ~/.nosis/catalog.toml to trust it"
            );
        }
    }
    anyhow::bail!("no catalog.toml found - run `nh init` to create one")
}

fn read_catalog_file(path: &Path) -> anyhow::Result<String> {
    let file = fs::File::open(path)
        .map_err(|error| anyhow::anyhow!("could not read {}: {error}", path.display()))?;
    let mut bytes = Vec::new();
    file.take((MAX_CATALOG_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| anyhow::anyhow!("could not read {}: {error}", path.display()))?;
    if bytes.len() > MAX_CATALOG_BYTES {
        anyhow::bail!(
            "{} is too large — catalogs are limited to {} bytes",
            path.display(),
            MAX_CATALOG_BYTES
        );
    }
    String::from_utf8(bytes).map_err(|_| anyhow::anyhow!("{} is not valid UTF-8", path.display()))
}

/// Assemble the effective MCP server set. User-global `~/.nosis/mcp.toml` is the trust source;
/// the repository `.nosis/mcp.toml` is RESTRICT-ONLY (ratified Q2): it may only tighten trust and
/// may not redirect a user-global server's url/auth or introduce a new destination. Finally, drop
/// any server whose credential audience is unapproved. Each drop contributes one secret-free
/// warning line.
pub(crate) fn load_and_vet_mcp_configs(
    repo_root: &Path,
    home: Option<&Path>,
    policy: &nh_law::Policy,
    warnings: &mut Vec<String>,
) -> Vec<McpServerConfig> {
    let user_global = home.map_or_else(Vec::new, |home| {
        read_optional_mcp_config(
            &home.join(".nosis").join("mcp.toml"),
            "user-global ~/.nosis/mcp.toml",
            warnings,
        )
    });
    let repo = read_optional_mcp_config(
        &repo_root.join(".nosis").join("mcp.toml"),
        "repository .nosis/mcp.toml",
        warnings,
    );
    merge_and_vet(
        user_global,
        repo,
        |entry| policy.approved_audiences(entry),
        warnings,
    )
}

fn read_optional_mcp_config(
    path: &Path,
    label: &str,
    warnings: &mut Vec<String>,
) -> Vec<McpServerConfig> {
    match path.try_exists() {
        Ok(false) => return Vec::new(),
        Ok(true) => {}
        Err(error) => {
            warnings.push(format!(
                "could not inspect {label} ({error}) — continuing without MCP from that file"
            ));
            return Vec::new();
        }
    }
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => {
            warnings.push(format!(
                "could not read {label} ({error}) — continuing without MCP from that file"
            ));
            return Vec::new();
        }
    };
    match nh_tools::load_mcp_config(&text) {
        Ok(configs) => configs,
        Err(error) => {
            warnings.push(format!(
                "{label}: {error} — continuing without MCP from that file"
            ));
            Vec::new()
        }
    }
}

fn merge_and_vet(
    user_global: Vec<McpServerConfig>,
    repo: Vec<McpServerConfig>,
    approved_for: impl Fn(&str) -> Vec<String>,
    warnings: &mut Vec<String>,
) -> Vec<McpServerConfig> {
    let mut user_by_name: BTreeMap<_, _> = user_global
        .into_iter()
        .map(|config| (config.name.clone(), config))
        .collect();
    let mut repo_by_name: BTreeMap<_, _> = repo
        .into_iter()
        .map(|config| (config.name.clone(), config))
        .collect();
    let names = user_by_name
        .keys()
        .chain(repo_by_name.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut merged = Vec::with_capacity(names.len());

    for name in names {
        match (user_by_name.remove(&name), repo_by_name.remove(&name)) {
            (Some(mut user_config), Some(repo_config)) => {
                user_config.trust =
                    more_restrictive_mcp_trust(user_config.trust, repo_config.trust);
                merged.push(user_config);
            }
            (Some(user_config), None) => merged.push(user_config),
            (None, Some(_repo_config)) => {
                warnings.push(format!(
                    "mcp server \"{name}\": repository config cannot introduce a destination — declare it in ~/.nosis/mcp.toml first; dropped"
                ));
            }
            (None, None) => unreachable!("server name came from one of the two maps"),
        }
    }

    filter_mcp_audiences_with(merged, warnings, approved_for)
}

fn more_restrictive_mcp_trust(left: McpTrust, right: McpTrust) -> McpTrust {
    fn rank(trust: McpTrust) -> u8 {
        match trust {
            McpTrust::Block => 0,
            McpTrust::Ask => 1,
            McpTrust::Auto => 2,
        }
    }

    if rank(left) <= rank(right) {
        left
    } else {
        right
    }
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
#[allow(dead_code)] // Retained policy-backed entry point; provenance merge uses the injected variant.
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
                    "mcp server \"{}\" dropped — credential \"{entry}\" is not approved for {}",
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
/// model-controlled text goes through this — one choke point.
pub(crate) fn safe_line(scrubber: &Scrubber, text: &str) -> String {
    nh_vault::safe_line(scrubber, text)
}

/// Scrub and control-escape each answer line while preserving every newline.
pub(crate) fn safe_text(scrubber: &Scrubber, text: &str) -> String {
    let mut safe = String::new();
    for segment in text.split_inclusive('\n') {
        let (line, newline) = segment
            .strip_suffix('\n')
            .map_or((segment, false), |line| (line, true));
        safe.push_str(&nh_vault::escape_untrusted(&scrubber.scrub(line)));
        if newline {
            safe.push('\n');
        }
    }
    safe
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
mod tests;
