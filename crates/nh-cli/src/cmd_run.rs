//! `nh run` — resolve the route, fetch the key, drive the agent loop.
//! Progress = one short line per tool call via `on_event`; exec_shell additionally
//! surfaces through its approval prompt. All errors: one friendly line, exit 1.

mod config;
mod meter;

pub(crate) use config::{find_catalog, load_and_vet_mcp_configs};
pub(crate) use meter::turn_cost_line;

#[cfg(test)]
use config::{
    filter_mcp_audiences_with, find_catalog_with_home, merge_and_vet, unapproved_mcp_target,
    BUNDLED_CATALOG,
};
use meter::run_meter_lines;
#[cfg(test)]
use meter::turn_cost_line_for_run;

#[cfg(test)]
use std::fs;
use std::io::{self, BufRead, IsTerminal, Write};

use chrono::Utc;
use nh_core::agent::{validate_task, AgentLoop};
use nh_core::credential;
use nh_core::receipt::{Outcome, ReceiptWriter};
#[cfg(test)]
use nh_core::wire::Usage;
use nh_core::wire::{resolve_effort, ThinkingEffort};
use nh_law::{Autonomy, LoadOptions};
use nh_routes::{RouteClass, RouteResolver, ThinkingDialect, ThinkingPosture, Wire};
use nh_tools::{builtin_tools, Access, ToolCtx};
#[cfg(test)]
use nh_tools::{McpAuth, McpServerConfig, McpTrust};
use nh_vault::{EnvFallbackVault, KeyringVault, Scrubber, SecretRegistry};

use crate::guard_from;

/// What callers print when a route resolves to a subscription delegate (M4 scope).
pub(crate) const DELEGATE_MSG: &str = "delegate routes arrive in M4 — pick an api route";
pub(crate) const MAX_RUN_TURNS: u32 = 100;

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

pub(crate) fn agent_constitution(
    law_constitution: &str,
    route: &nh_routes::ResolvedRoute,
) -> String {
    nh_tui::identity_constitution(law_constitution, route)
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
        constitution: Some(agent_constitution(&law.constitution, &route)),
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

    let meter_lines = run_meter_lines(
        &resolver,
        &route,
        receipt.usage.as_ref(),
        receipt.turns,
        receipt.tool_calls,
        started,
        ended,
    );
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    let stderr = io::stderr();
    let mut stderr = stderr.lock();
    write_run_output(&mut stdout, &mut stderr, &scrubber, &answer, &meter_lines)?;
    if receipt.outcome == Outcome::Timeout {
        anyhow::bail!("{}", max_turns_timeout_message(max_turns));
    }
    Ok(())
}

fn write_run_output<W: Write, E: Write>(
    stdout: &mut W,
    stderr: &mut E,
    scrubber: &Scrubber,
    answer: &str,
    meter_lines: &[String],
) -> io::Result<()> {
    writeln!(stdout, "{}", safe_text(scrubber, answer))?;
    for line in meter_lines {
        writeln!(stderr, "{}", safe_line(scrubber, line))?;
    }
    Ok(())
}

fn max_turns_timeout_message(max_turns: u32) -> String {
    if max_turns < MAX_RUN_TURNS {
        let next = max_turns.saturating_mul(2).clamp(1, MAX_RUN_TURNS);
        format!("stopped at max turns ({max_turns}) — rerun with --max-turns {next}")
    } else {
        format!(
            "stopped at max turns ({max_turns}) — split the task or make the request more focused"
        )
    }
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
    let stdin = io::stdin();
    let terminal = stdin.is_terminal();
    let mut input = stdin.lock();
    let stderr = io::stderr();
    let mut stderr = stderr.lock();
    approve_with_io(display, terminal, &mut input, &mut stderr)
}

fn approve_with_io<R: BufRead, W: Write>(
    display: &str,
    terminal: bool,
    input: &mut R,
    stderr: &mut W,
) -> bool {
    if !terminal {
        let _ = writeln!(
            stderr,
            "  approval refused: stdin is not a terminal; piped input cannot approve shell commands"
        );
        return false;
    }
    let _ = write!(stderr, "  approve? {display}  [y/N] ");
    let _ = stderr.flush();
    let mut line = String::new();
    input.read_line(&mut line).is_ok() && is_yes(&line)
}

fn is_yes(line: &str) -> bool {
    matches!(line.trim(), "y" | "Y" | "yes" | "Yes" | "YES")
}

#[cfg(test)]
mod tests;
