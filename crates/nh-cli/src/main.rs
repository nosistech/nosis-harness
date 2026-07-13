//! nh - the Nosis Harness CLI. M0 surface: init / key / run.
//! UX IS THE PRODUCT: every message short, concrete, actionable. Errors say what to do
//! next, never stack traces. Approval prompts show the command on one safe line
//! (scrubbed, control chars escaped), y/N, default deny.

use clap::{Parser, Subcommand};

mod cmd_init;
mod cmd_key;
mod cmd_run;

#[derive(Parser)]
#[command(name = "nh", about = "Nosis Harness - multi-model terminal agent", version)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Set up .nosis/ in this repo (receipts dir, .gitignore, secret-pattern pre-commit hook)
    Init,
    /// Manage API keys in the OS-native vault (never echoed, never stored in files)
    Key {
        #[command(subcommand)]
        action: KeyAction,
    },
    /// Run an agent task
    Run {
        /// The task, in plain words
        task: String,
        /// Model id from catalog.toml
        #[arg(long, default_value = "deepseek-v4-flash")]
        model: String,
        /// Max agent turns before giving up with a timeout receipt
        #[arg(long, default_value_t = 20)]
        max_turns: u32,
    },
}

#[derive(Subcommand)]
enum KeyAction {
    /// Prompt for a key and store it (e.g. `nh key add deepseek`)
    Add { entry: String },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let result = match cli.cmd {
        Cmd::Init => cmd_init::run(),
        Cmd::Key { action: KeyAction::Add { entry } } => cmd_key::add(&entry),
        Cmd::Run { task, model, max_turns } => cmd_run::run(&task, &model, max_turns),
    };
    // UX: one friendly line, what to do next, exit 1. Never a debug dump.
    // Every output path passes the Scrubber - this final line included (key
    // literals are scrubbed at the source; this catches key shapes).
    if let Err(err) = result {
        let scrubber = nh_vault::Scrubber::new(vec![]);
        eprintln!("nh: {}", cmd_run::safe_line(&scrubber, &err.to_string()));
        std::process::exit(1);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_init() {
        let cli = Cli::try_parse_from(["nh", "init"]).unwrap();
        assert!(matches!(cli.cmd, Cmd::Init));
    }

    #[test]
    fn parses_key_add_with_entry() {
        let cli = Cli::try_parse_from(["nh", "key", "add", "deepseek"]).unwrap();
        match cli.cmd {
            Cmd::Key { action: KeyAction::Add { entry } } => assert_eq!(entry, "deepseek"),
            _ => panic!("expected key add"),
        }
    }

    #[test]
    fn key_add_requires_entry() {
        assert!(Cli::try_parse_from(["nh", "key", "add"]).is_err());
    }

    #[test]
    fn parses_run_with_defaults() {
        let cli = Cli::try_parse_from(["nh", "run", "fix the failing test"]).unwrap();
        match cli.cmd {
            Cmd::Run { task, model, max_turns } => {
                assert_eq!(task, "fix the failing test");
                assert_eq!(model, "deepseek-v4-flash");
                assert_eq!(max_turns, 20);
            }
            _ => panic!("expected run"),
        }
    }

    #[test]
    fn parses_run_with_overrides() {
        let cli = Cli::try_parse_from([
            "nh",
            "run",
            "review the diff",
            "--model",
            "deepseek-v4-pro",
            "--max-turns",
            "5",
        ])
        .unwrap();
        match cli.cmd {
            Cmd::Run { task, model, max_turns } => {
                assert_eq!(task, "review the diff");
                assert_eq!(model, "deepseek-v4-pro");
                assert_eq!(max_turns, 5);
            }
            _ => panic!("expected run"),
        }
    }

    #[test]
    fn run_requires_a_task() {
        assert!(Cli::try_parse_from(["nh", "run"]).is_err());
    }
}
