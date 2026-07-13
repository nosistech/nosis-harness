//! `nh run` - resolve the route, fetch the key, drive the agent loop.
//! Progress = one short line per tool call via `on_event`; exec_shell additionally
//! surfaces through its approval prompt. All errors: one friendly line, exit 1.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use nh_core::agent::AgentLoop;
use nh_core::receipt::{Outcome, ReceiptWriter};
use nh_core::wire::OpenAiCompatClient;
use nh_routes::{RouteResolver, Wire};
use nh_tools::{builtin_tools, ToolCtx};
use nh_vault::{EnvFallbackVault, KeyringVault, Scrubber, Vault};

pub fn run(task: &str, model: &str, max_turns: u32) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let (root, catalog) = find_catalog(&cwd)?;
    let resolver = RouteResolver::from_toml(&catalog)?;
    let route = resolver.resolve(model)?;
    if route.wire != Wire::OpenAi {
        anyhow::bail!("{model} uses the anthropic wire - not supported yet, pick an openai-wire model");
    }

    let vault = EnvFallbackVault { inner: KeyringVault };
    let key = vault.get(&route.vault_entry)?;
    // The scrubber holds the literal so no output path can leak it.
    let key_literal: String = key.as_str().to_owned();

    let client = OpenAiCompatClient::new(route.base_url.clone(), key);
    let ctx = ToolCtx { workdir: cwd, approve: Box::new(approve_on_stdin) };
    let receipts = ReceiptWriter {
        path: root.join(".nosis").join("receipts.jsonl"),
        scrubber: Scrubber::new(vec![key_literal.clone()]),
    };
    let mut agent = AgentLoop {
        client: Box::new(client),
        tools: builtin_tools(),
        ctx,
        receipts,
        model_id: route.model_id.clone(),
        max_turns,
        on_event: Some(Box::new(|line| eprintln!("  {line}"))),
    };

    eprintln!("running {} (max {max_turns} turns)", route.model_id);
    let (answer, receipt) = agent.run(task)?;

    let scrubber = Scrubber::new(vec![key_literal]);
    println!("{}", scrubber.scrub(&answer));
    let usage = receipt.usage.clone().unwrap_or_default();
    println!(
        "turns {} | tool calls {} | tokens {} in / {} out / {} cached",
        receipt.turns,
        receipt.tool_calls,
        usage.prompt_tokens,
        usage.completion_tokens,
        usage.cached_tokens.unwrap_or(0)
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
fn find_catalog(start: &Path) -> anyhow::Result<(PathBuf, String)> {
    for dir in start.ancestors() {
        let candidate = dir.join("catalog.toml");
        if candidate.is_file() {
            let text = fs::read_to_string(&candidate)?;
            return Ok((dir.to_path_buf(), text));
        }
    }
    anyhow::bail!("no catalog.toml found - run from the repo root")
}

/// Approval gate: one line on stderr, exact command shown, default deny.
fn approve_on_stdin(action: &str) -> bool {
    eprint!("  approve? {action}  [y/N] ");
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
        assert!(err.to_string().contains("run from the repo root"));
    }

    #[test]
    fn yes_parsing_defaults_to_deny() {
        assert!(is_yes("y\n"));
        assert!(is_yes("  yes  \n"));
        assert!(!is_yes("\n"));
        assert!(!is_yes("n\n"));
        assert!(!is_yes("whatever\n"));
    }
}
