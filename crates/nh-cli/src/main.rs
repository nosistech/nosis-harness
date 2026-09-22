//! nh - the Nosis Harness CLI. Run `nh setup` for guided first use or `nh --help` for commands.
//! UX IS THE PRODUCT: every message short, concrete, actionable. Errors say what to do
//! next, never stack traces. Approval prompts show the command on one safe line
//! (scrubbed, control chars escaped), y/N, default deny.

use clap::{Parser, Subcommand};

mod cmd_catalog;
mod cmd_chat;
mod cmd_doctor;
mod cmd_fleet;
mod cmd_init;
mod cmd_key;
mod cmd_mcp;
mod cmd_profile;
mod cmd_resume;
mod cmd_run;
mod cmd_setup;
mod cmd_tui;
mod cmd_why;
mod model_preference;
mod usage_tracker;

#[derive(Parser)]
#[command(
    name = "nh",
    about = "Nosis Harness - multi-model terminal agent",
    version
)]
struct Cli {
    /// Force one-column ASCII fallback glyphs on or off (default: decide from stdout)
    #[arg(long, global = true, value_enum)]
    ascii: Option<AsciiArg>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum AsciiArg {
    On,
    Off,
}

impl AsciiArg {
    const fn enabled(self) -> bool {
        matches!(self, Self::On)
    }
}

#[derive(Subcommand)]
enum Cmd {
    /// Set up this project, choose a model, and optionally start chat
    Setup,
    /// Review and migrate a known historical bundled catalog
    Catalog {
        #[command(subcommand)]
        action: CatalogAction,
    },
    /// Set up .nosis/ in this repo (receipts dir, .gitignore, secret-pattern pre-commit hook)
    Init,
    /// Manage API keys in the OS-native vault (never echoed, never stored in files)
    Key {
        #[command(subcommand)]
        action: KeyAction,
    },
    /// Manage the operator-owned default model used when --model is omitted
    Model {
        #[command(subcommand)]
        action: ModelAction,
    },
    /// Run an agent task
    Run {
        /// The task, in plain words
        task: String,
        /// Model id override; otherwise use the saved model or bundled default
        #[arg(long)]
        model: Option<String>,
        /// Max agent turns before giving up with a timeout receipt
        #[arg(long, default_value_t = 20)]
        #[arg(value_parser = parse_max_turns)]
        max_turns: u32,
        /// Thinking effort (default picked per route dialect)
        #[arg(long, value_enum)]
        think: Option<cmd_run::ThinkArg>,
        /// Session autonomy override (default comes from law files)
        #[arg(long, value_enum)]
        autonomy: Option<cmd_run::AutonomyArg>,
        /// Execution profile: frugal, balanced, or max-quality
        #[arg(long, default_value = "balanced")]
        profile: String,
        /// Attach a PNG or JPEG image (repeatable; maximum 4)
        #[arg(long, value_name = "PATH")]
        image: Vec<String>,
    },
    /// Chat with a model - /model and /provider switch routes mid-session
    Chat {
        /// Model id override; otherwise use the saved model or bundled default
        #[arg(long)]
        model: Option<String>,
        /// Execution profile: frugal, balanced, or max-quality
        #[arg(long, default_value = "balanced")]
        profile: String,
    },
    /// Check the install and print what is wrong and how to fix it
    Doctor,
    /// Resume an interrupted chat or TUI session
    Resume {
        /// Session id; omit it to list interrupted sessions
        session_id: Option<String>,
    },
    /// Explain the cheapest capable route for a task estimate
    Why {
        /// Optional task text used for a rough token estimate
        task: Option<String>,
        /// Explicitly selected model to compare with the cheapest capable route
        #[arg(long)]
        model: Option<String>,
    },
    /// List execution profiles and their caps for a model
    Profile {
        /// Model id override; otherwise use the saved model or bundled default
        #[arg(long)]
        model: Option<String>,
    },
    /// Open the full-screen terminal UI
    Tui {
        /// Model id override; otherwise use the saved model or bundled default
        #[arg(long)]
        model: Option<String>,
        /// Observed session token stop; the active turn is allowed to finish
        #[arg(long)]
        budget: Option<u64>,
        /// Execution profile: frugal, balanced, or max-quality
        #[arg(long, default_value = "balanced")]
        profile: String,
    },
    /// Run independent tasks in a durable worker fleet
    Fleet {
        #[command(subcommand)]
        action: FleetAction,
    },
    /// Serve the local MCP endpoint (preview; 127.0.0.1 only)
    Mcp {
        #[command(subcommand)]
        action: McpAction,
    },
}

#[derive(Subcommand)]
enum CatalogAction {
    /// Replace an exact historical bundled catalog after an explicit review
    Migrate,
}

#[derive(Subcommand)]
enum FleetAction {
    /// Start a new fleet from a JSON task file
    Run {
        tasks: std::path::PathBuf,
        #[arg(long)]
        max_workers: Option<usize>,
        /// Required observed-token dispatch budget (or set budget_tokens in the task file)
        #[arg(long)]
        budget: Option<u64>,
        #[arg(long, num_args = 0..=1, default_missing_value = "true")]
        escalate: Option<bool>,
        #[arg(long, num_args = 0..=1, default_missing_value = "true")]
        defer_offpeak: Option<bool>,
    },
    /// Resume the latest incomplete run, or a specific run id
    Resume {
        run_id: Option<String>,
        #[arg(long)]
        max_workers: Option<usize>,
    },
}

#[derive(Subcommand)]
enum McpAction {
    /// Start the local MCP server (route_resolve, fleet_run, fleet_status, why, route_cost, receipts)
    Serve {
        #[arg(long, default_value = "127.0.0.1:8765")]
        addr: String,
        #[arg(long)]
        token_entry: Option<String>,
    },
}

#[derive(Subcommand)]
enum KeyAction {
    /// Prompt for a key and store it (e.g. `nh key add deepseek`)
    Add { entry: String },
    /// Remove a key from the OS-native store (e.g. `nh key remove deepseek`)
    Remove { entry: String },
}

#[derive(Subcommand)]
enum ModelAction {
    /// Save a default model after validating it against the trusted catalog
    Set { model: String },
    /// Show and validate the saved default model
    Show,
    /// Remove the saved default model
    Clear,
}

fn parse_max_turns(value: &str) -> Result<u32, String> {
    let turns = value
        .parse::<u32>()
        .map_err(|_| "max turns must be a whole number from 1 to 100".to_owned())?;
    if (1..=cmd_run::MAX_RUN_TURNS).contains(&turns) {
        Ok(turns)
    } else {
        Err(format!(
            "max turns must be from 1 to {}",
            cmd_run::MAX_RUN_TURNS
        ))
    }
}

fn main() -> anyhow::Result<()> {
    let result = if std::env::args_os().nth(1).is_none() {
        let terminal_capability =
            nh_core::terminal_capability::TerminalCapability::from_process(None);
        cmd_setup::run(terminal_capability, None)
    } else {
        dispatch(Cli::parse())
    };
    finish(result)
}

fn dispatch(cli: Cli) -> anyhow::Result<()> {
    let forced_ascii = cli.ascii.map(AsciiArg::enabled);
    let terminal_capability =
        nh_core::terminal_capability::TerminalCapability::from_process(forced_ascii);
    match cli.cmd {
        Cmd::Setup => cmd_setup::run(terminal_capability, forced_ascii),
        Cmd::Catalog {
            action: CatalogAction::Migrate,
        } => cmd_catalog::migrate(),
        Cmd::Init => cmd_init::run(),
        Cmd::Key { action } => match action {
            KeyAction::Add { entry } => cmd_key::add(&entry),
            KeyAction::Remove { entry } => cmd_key::remove(&entry),
        },
        Cmd::Model { action } => match action {
            ModelAction::Set { model } => model_preference::set(&model),
            ModelAction::Show => model_preference::show(),
            ModelAction::Clear => model_preference::clear(),
        },
        Cmd::Run {
            task,
            model,
            max_turns,
            think,
            autonomy,
            profile,
            image,
        } => cmd_run::run(
            &task,
            model.as_deref(),
            cmd_run::RunOptions {
                max_turns,
                think,
                autonomy,
                profile: &profile,
                images: &image,
                terminal_capability,
            },
        ),
        Cmd::Chat { model, profile } => {
            cmd_chat::run(model.as_deref(), &profile, terminal_capability)
        }
        Cmd::Doctor => cmd_doctor::run(terminal_capability, forced_ascii),
        Cmd::Resume { session_id } => cmd_resume::run(session_id.as_deref(), terminal_capability),
        Cmd::Why { task, model } => {
            cmd_why::run(task.as_deref(), model.as_deref(), terminal_capability)
        }
        Cmd::Profile { model } => cmd_profile::run(model.as_deref(), terminal_capability),
        Cmd::Tui {
            model,
            budget,
            profile,
        } => cmd_tui::run(model.as_deref(), budget, &profile, terminal_capability),
        Cmd::Fleet {
            action:
                FleetAction::Run {
                    tasks,
                    max_workers,
                    budget,
                    escalate,
                    defer_offpeak,
                },
        } => cmd_fleet::run_tasks(
            &tasks,
            max_workers,
            budget,
            escalate,
            defer_offpeak,
            terminal_capability,
        ),
        Cmd::Fleet {
            action:
                FleetAction::Resume {
                    run_id,
                    max_workers,
                },
        } => cmd_fleet::resume_run(run_id.as_deref(), max_workers, terminal_capability),
        Cmd::Mcp {
            action: McpAction::Serve { addr, token_entry },
        } => cmd_mcp::serve(&addr, token_entry.as_deref()),
    }
}

fn finish(result: anyhow::Result<()>) -> anyhow::Result<()> {
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
mod tests;
