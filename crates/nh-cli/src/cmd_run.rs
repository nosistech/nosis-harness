//! `nh run` - resolve the route, fetch the key, drive the agent loop.
//! Progress = one short line per tool call via `on_event`; exec_shell additionally
//! surfaces through its approval prompt. All errors: one friendly line, exit 1.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use nh_core::agent::AgentLoop;
use nh_core::receipt::{Outcome, ReceiptWriter};
use nh_core::wire::{cache_hit_pct, make_client, ThinkingEffort};
use nh_law::{Autonomy, LoadOptions};
use nh_routes::{RouteClass, RouteResolver, ThinkingDialect};
use nh_tools::{builtin_tools, Access, ToolCtx};
use nh_vault::{EnvFallbackVault, KeyringVault, Scrubber, Vault};

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

/// Map the `--think` flag to a wire `ThinkingEffort`. When the flag is absent,
/// default per route dialect: routes that always think (or only think high/max)
/// run at High; effort-toggle routes stay at None - cheap until the user asks.
pub(crate) fn effort_for(think: Option<ThinkArg>, dialect: ThinkingDialect) -> ThinkingEffort {
    match think {
        Some(ThinkArg::None) => ThinkingEffort::None,
        Some(ThinkArg::Low) => ThinkingEffort::Low,
        Some(ThinkArg::High) => ThinkingEffort::High,
        Some(ThinkArg::Max) => ThinkingEffort::Max,
        None => match dialect {
            ThinkingDialect::AlwaysThinking | ThinkingDialect::GlmHm => ThinkingEffort::High,
            ThinkingDialect::DeepseekNhm | ThinkingDialect::None => ThinkingEffort::None,
        },
    }
}

pub fn run(
    task: &str,
    model: &str,
    max_turns: u32,
    think: Option<ThinkArg>,
    autonomy: Option<AutonomyArg>,
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
    if route.class == RouteClass::Delegate {
        anyhow::bail!("{DELEGATE_MSG}");
    }

    let vault = EnvFallbackVault { inner: KeyringVault };
    let key = vault.get(&route.vault_entry)?;
    // Scrubbers hold the literal so no output path can leak it - receipts, stdout,
    // AND every stderr path (progress lines, approval prompt) pass one.
    let key_literal: String = key.as_str().to_owned();

    let client = make_client(&route, key);
    let approve_scrubber = Scrubber::new(vec![key_literal.clone()]);
    let event_scrubber = Scrubber::new(vec![key_literal.clone()]);
    let policy = law.policy.clone();
    let ctx = ToolCtx::new(
        cwd,
        // Model-supplied commands are scrubbed + control-char-escaped before display
        // so the approval gate always shows one faithful line.
        Box::new(move |action| approve_on_stdin(&safe_line(&approve_scrubber, action))),
    )
    .with_guard(Box::new(move |access| match access {
        Access::Write(path) => guard_from(policy.write_verdict(path)),
        Access::Exec(command) => guard_from(policy.exec_verdict(command)),
    }));
    let receipts = ReceiptWriter {
        path: root.join(".nosis").join("receipts.jsonl"),
        scrubber: Scrubber::new(vec![key_literal.clone()]),
    };
    let mut agent = AgentLoop {
        client,
        tools: builtin_tools(),
        ctx,
        receipts,
        model_id: route.model_id.clone(),
        max_turns,
        thinking: effort_for(think, route.thinking_dialect),
        // Honest identity: name the real route + forbid claiming to be Claude/GPT.
        constitution: Some(nh_tui::identity_constitution(&law.constitution, &route)),
        context_limit: route.context,
        on_event: Some(Box::new(move |line| eprintln!("  {}", safe_line(&event_scrubber, line)))),
    };

    eprintln!("running {} (max {max_turns} turns)", route.model_id);
    let scrubber = Scrubber::new(vec![key_literal]);
    let (answer, receipt) = agent
        .run(task)
        .map_err(|e| anyhow::anyhow!("{}", scrubber.scrub(&e.to_string())))?;

    println!("{}", scrubber.scrub(&answer));
    let usage = receipt.usage.clone().unwrap_or_default();
    let cache = cache_hit_pct(usage.prompt_tokens, usage.cached_tokens.unwrap_or(0))
        .map(|pct| format!(" | cache {pct:.0}%"))
        .unwrap_or_default();
    println!(
        "turns {} | tool calls {} | tokens {} in / {} out / {} cached{}",
        receipt.turns,
        receipt.tool_calls,
        usage.prompt_tokens,
        usage.completion_tokens,
        usage.cached_tokens.unwrap_or(0),
        cache
    );
    if receipt.outcome == Outcome::Timeout {
        anyhow::bail!(
            "stopped at max turns ({max_turns}) - rerun with --max-turns {}",
            max_turns.saturating_mul(2)
        );
    }
    Ok(())
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
    fn yes_parsing_defaults_to_deny() {
        assert!(is_yes("y\n"));
        assert!(is_yes("  yes  \n"));
        assert!(!is_yes("\n"));
        assert!(!is_yes("n\n"));
        assert!(!is_yes("whatever\n"));
    }

    #[test]
    fn think_flag_overrides_any_dialect() {
        let cases = [
            (ThinkArg::None, ThinkingEffort::None),
            (ThinkArg::Low, ThinkingEffort::Low),
            (ThinkArg::High, ThinkingEffort::High),
            (ThinkArg::Max, ThinkingEffort::Max),
        ];
        for (arg, want) in cases {
            assert_eq!(effort_for(Some(arg), ThinkingDialect::DeepseekNhm), want);
            assert_eq!(effort_for(Some(arg), ThinkingDialect::AlwaysThinking), want);
        }
    }

    #[test]
    fn think_default_follows_route_dialect() {
        // Always-thinking and high/max-only routes run at High; effort-toggle
        // routes stay at None until the user asks (cheap by default).
        assert_eq!(effort_for(None, ThinkingDialect::AlwaysThinking), ThinkingEffort::High);
        assert_eq!(effort_for(None, ThinkingDialect::GlmHm), ThinkingEffort::High);
        assert_eq!(effort_for(None, ThinkingDialect::DeepseekNhm), ThinkingEffort::None);
        assert_eq!(effort_for(None, ThinkingDialect::None), ThinkingEffort::None);
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
