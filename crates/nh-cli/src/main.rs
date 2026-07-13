//! nh - the Nosis Harness CLI. M0 surface: init / key / run.
//! UX IS THE PRODUCT: every message short, concrete, actionable. Errors say what to do
//! next, never stack traces. Approval prompts show the exact command, one line, y/N.

use clap::{Parser, Subcommand};

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
    match cli.cmd {
        Cmd::Init => todo!("build agent: create .nosis/, .gitignore, pre-commit hook"),
        Cmd::Key { action: KeyAction::Add { entry: _ } } => {
            todo!("build agent: hidden prompt, vault.set, confirm without echoing")
        }
        Cmd::Run { task: _, model: _, max_turns: _ } => {
            todo!("build agent: resolve route, vault key, wire AgentLoop, stdin approval gate")
        }
    }
}
