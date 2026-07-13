//! `nh run` — resolve the route, fetch the key, drive the agent loop.
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
        anyhow::bail!("{model} uses the anthropic wire — not supported yet, pick an openai-wire model");
    }

    let vault = EnvFallbackVault { inner: KeyringVault };
    let key = vault.get(&route.vault_entry)?;
    // Scrubbers hold the literal so no output path can leak it — receipts, stdout,
    // AND every stderr path (progress lines, approval prompt) pass one.
    let key_literal: String = key.as_str().to_owned();

    let client = OpenAiCompatClient::new(route.base_url.clone(), key);
    let approve_scrubber = Scrubber::new(vec![key_literal.clone()]);
    let event_scrubber = Scrubber::new(vec![key_literal.clone()]);
    let ctx = ToolCtx {
        workdir: cwd,
        // Model-supplied commands are scrubbed + control-char-escaped before display
        // so the approval gate always shows one faithful line.
        approve: Box::new(move |action| approve_on_stdin(&safe_line(&approve_scrubber, action))),
    };
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
        on_event: Some(Box::new(move |line| eprintln!("  {}", safe_line(&event_scrubber, line)))),
    };

    eprintln!("running {} (max {max_turns} turns)", route.model_id);
    let scrubber = Scrubber::new(vec![key_literal]);
    let (answer, receipt) = agent
        .run(task)
        .map_err(|e| anyhow::anyhow!("{}", scrubber.scrub(&e.to_string())))?;

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
            "stopped at max turns ({max_turns}) — rerun with --max-turns {}",
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
    anyhow::bail!("no catalog.toml found — run `nh init` to create one")
}

/// Max chars of untrusted text shown on one terminal line before a visible marker.
const MAX_DISPLAY_CHARS: usize = 500;

/// Scrub secrets, then escape for display. Every stderr line built from
/// model-controlled text goes through this — one choke point.
pub(crate) fn safe_line(scrubber: &Scrubber, text: &str) -> String {
    sanitize_line(&scrubber.scrub(text))
}

/// Render untrusted text as one safe terminal line: control characters (\n, \r,
/// ESC/ANSI, …) become visible escapes so model output cannot spoof the display,
/// and very long text truncates with an explicit marker.
fn sanitize_line(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_control() {
            escaped.extend(c.escape_debug());
        } else {
            escaped.push(c);
        }
    }
    let len = escaped.chars().count();
    if len > MAX_DISPLAY_CHARS {
        let head: String = escaped.chars().take(MAX_DISPLAY_CHARS).collect();
        format!("{head}… (+{} more chars)", len - MAX_DISPLAY_CHARS)
    } else {
        escaped
    }
}

/// Approval gate: one line on stderr, default deny. `display` is the command
/// already scrubbed and control-char-escaped by the caller (see `safe_line`).
fn approve_on_stdin(display: &str) -> bool {
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
    fn sanitize_line_escapes_control_chars_visibly() {
        // A spoof attempt: CR + ANSI erase-line to hide the real command.
        let spoofed = "echo safe\r\x1b[2K && rm -rf /";
        let display = sanitize_line(spoofed);
        assert!(!display.chars().any(|c| c.is_control()), "got: {display}");
        assert!(display.contains("\\r"), "CR must be visible: {display}");
        assert!(display.contains("\\u{1b}"), "ESC must be visible: {display}");
        assert!(display.contains("rm -rf /"), "payload must stay visible: {display}");
    }

    #[test]
    fn sanitize_line_truncates_with_visible_marker() {
        let display = sanitize_line(&"x".repeat(600));
        assert!(display.chars().count() < 600, "got len {}", display.chars().count());
        assert!(display.contains("(+100 more chars)"), "got: {display}");
        // Short text passes through untouched.
        assert_eq!(sanitize_line("cargo test"), "cargo test");
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
