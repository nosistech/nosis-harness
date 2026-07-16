//! nh — the Nosis Harness CLI. M0 surface: init / key / run; M1 adds chat.
//! UX IS THE PRODUCT: every message short, concrete, actionable. Errors say what to do
//! next, never stack traces. Approval prompts show the command on one safe line
//! (scrubbed, control chars escaped), y/N, default deny.

use clap::{Parser, Subcommand};

mod cmd_chat;
mod cmd_fleet;
mod cmd_init;
mod cmd_key;
mod cmd_mcp;
mod cmd_run;
mod cmd_tui;

fn guard_from(verdict: nh_law::Verdict) -> nh_tools::Guard {
    match verdict {
        nh_law::Verdict::Allow => nh_tools::Guard::Allow,
        nh_law::Verdict::Ask => nh_tools::Guard::Ask,
        nh_law::Verdict::Block(reason) => nh_tools::Guard::Block(reason),
    }
}

#[derive(Parser)]
#[command(name = "nh", about = "Nosis Harness — multi-model terminal agent", version)]
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
        /// Thinking effort (default picked per route dialect)
        #[arg(long, value_enum)]
        think: Option<cmd_run::ThinkArg>,
        /// Session autonomy override (default comes from law files)
        #[arg(long, value_enum)]
        autonomy: Option<cmd_run::AutonomyArg>,
    },
    /// Chat with a model — /model and /provider switch routes mid-session
    Chat {
        /// Model id from catalog.toml
        #[arg(long, default_value = "deepseek-v4-flash")]
        model: String,
    },
    /// Open the full-screen terminal UI
    Tui {
        /// Model id from catalog.toml
        #[arg(long, default_value = "deepseek-v4-flash")]
        model: String,
        /// Hard session token budget
        #[arg(long)]
        budget: Option<u64>,
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
enum FleetAction {
    /// Start a new fleet from a JSON task file
    Run {
        tasks: std::path::PathBuf,
        #[arg(long)]
        max_workers: Option<usize>,
        #[arg(long)]
        budget: Option<u64>,
        #[arg(long)]
        escalate: bool,
        #[arg(long)]
        defer_offpeak: bool,
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
    /// Start the local MCP server (route_resolve, fleet_run, fleet_status)
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
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let result = match cli.cmd {
        Cmd::Init => cmd_init::run(),
        Cmd::Key { action: KeyAction::Add { entry } } => cmd_key::add(&entry),
        Cmd::Run { task, model, max_turns, think, autonomy } => {
            cmd_run::run(&task, &model, max_turns, think, autonomy)
        }
        Cmd::Chat { model } => cmd_chat::run(&model),
        Cmd::Tui { model, budget } => cmd_tui::run(&model, budget),
        Cmd::Fleet {
            action:
                FleetAction::Run {
                    tasks,
                    max_workers,
                    budget,
                    escalate,
                    defer_offpeak,
                },
        } => {
            cmd_fleet::run_tasks(&tasks, max_workers, budget, escalate, defer_offpeak)
        }
        Cmd::Fleet { action: FleetAction::Resume { run_id, max_workers } } => {
            cmd_fleet::resume_run(run_id.as_deref(), max_workers)
        }
        Cmd::Mcp {
            action: McpAction::Serve { addr, token_entry },
        } => cmd_mcp::serve(&addr, token_entry.as_deref()),
    };
    // UX: one friendly line, what to do next, exit 1. Never a debug dump.
    // Every output path passes the Scrubber — this final line included (key
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
            Cmd::Run { task, model, max_turns, think, autonomy } => {
                assert_eq!(task, "fix the failing test");
                assert_eq!(model, "deepseek-v4-flash");
                assert_eq!(max_turns, 20);
                assert_eq!(think, None, "no --think = per-dialect default");
                assert_eq!(autonomy, None, "no --autonomy = law-file default");
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
            Cmd::Run { task, model, max_turns, think, autonomy } => {
                assert_eq!(task, "review the diff");
                assert_eq!(model, "deepseek-v4-pro");
                assert_eq!(max_turns, 5);
                assert_eq!(think, None);
                assert_eq!(autonomy, None);
            }
            _ => panic!("expected run"),
        }
    }

    #[test]
    fn run_requires_a_task() {
        assert!(Cli::try_parse_from(["nh", "run"]).is_err());
    }

    #[test]
    fn parses_run_think_levels() {
        use cmd_run::ThinkArg;
        let cases =
            [("none", ThinkArg::None), ("low", ThinkArg::Low), ("high", ThinkArg::High), ("max", ThinkArg::Max)];
        for (value, want) in cases {
            let cli = Cli::try_parse_from(["nh", "run", "task", "--think", value]).unwrap();
            match cli.cmd {
                Cmd::Run { think, .. } => assert_eq!(think, Some(want), "--think {value}"),
                _ => panic!("expected run"),
            }
        }
    }

    #[test]
    fn run_rejects_unknown_think_level() {
        assert!(Cli::try_parse_from(["nh", "run", "task", "--think", "ultra"]).is_err());
    }

    #[test]
    fn parses_optional_run_autonomy() {
        use cmd_run::AutonomyArg;
        for (value, want) in [("ask", AutonomyArg::Ask), ("auto", AutonomyArg::Auto)] {
            let cli = Cli::try_parse_from(["nh", "run", "task", "--autonomy", value]).unwrap();
            match cli.cmd {
                Cmd::Run { autonomy, .. } => assert_eq!(autonomy, Some(want)),
                _ => panic!("expected run"),
            }
        }
    }

    #[test]
    fn run_rejects_unknown_autonomy() {
        assert!(Cli::try_parse_from(["nh", "run", "task", "--autonomy", "always"]).is_err());
    }

    #[test]
    fn parses_chat_with_default_model() {
        let cli = Cli::try_parse_from(["nh", "chat"]).unwrap();
        match cli.cmd {
            Cmd::Chat { model } => assert_eq!(model, "deepseek-v4-flash"),
            _ => panic!("expected chat"),
        }
    }

    #[test]
    fn parses_chat_with_model_override() {
        let cli = Cli::try_parse_from(["nh", "chat", "--model", "kimi-k2.6"]).unwrap();
        match cli.cmd {
            Cmd::Chat { model } => assert_eq!(model, "kimi-k2.6"),
            _ => panic!("expected chat"),
        }
    }

    #[test]
    fn parses_tui_with_defaults() {
        let cli = Cli::try_parse_from(["nh", "tui"]).unwrap();
        match cli.cmd {
            Cmd::Tui { model, budget } => {
                assert_eq!(model, "deepseek-v4-flash");
                assert_eq!(budget, None);
            }
            _ => panic!("expected tui"),
        }
    }

    #[test]
    fn parses_tui_with_model_and_budget() {
        let cli = Cli::try_parse_from([
            "nh",
            "tui",
            "--model",
            "kimi-k2.6",
            "--budget",
            "12000",
        ])
        .unwrap();
        match cli.cmd {
            Cmd::Tui { model, budget } => {
                assert_eq!(model, "kimi-k2.6");
                assert_eq!(budget, Some(12000));
            }
            _ => panic!("expected tui"),
        }
    }

    #[test]
    fn parses_fleet_run_overrides() {
        let cli = Cli::try_parse_from([
            "nh",
            "fleet",
            "run",
            "tasks.json",
            "--max-workers",
            "3",
            "--budget",
            "900",
            "--escalate",
            "--defer-offpeak",
        ])
        .unwrap();
        match cli.cmd {
            Cmd::Fleet {
                action:
                    FleetAction::Run {
                        tasks,
                        max_workers,
                        budget,
                        escalate,
                        defer_offpeak,
                    },
            } => {
                assert_eq!(tasks, std::path::PathBuf::from("tasks.json"));
                assert_eq!(max_workers, Some(3));
                assert_eq!(budget, Some(900));
                assert!(escalate);
                assert!(defer_offpeak);
            }
            _ => panic!("expected fleet run"),
        }
    }

    #[test]
    fn parses_fleet_resume_with_optional_id() {
        let latest = Cli::try_parse_from(["nh", "fleet", "resume"]).unwrap();
        assert!(matches!(
            latest.cmd,
            Cmd::Fleet { action: FleetAction::Resume { run_id: None, .. } }
        ));
        let named = Cli::try_parse_from([
            "nh", "fleet", "resume", "run-123", "--max-workers", "2",
        ])
        .unwrap();
        match named.cmd {
            Cmd::Fleet { action: FleetAction::Resume { run_id, max_workers } } => {
                assert_eq!(run_id.as_deref(), Some("run-123"));
                assert_eq!(max_workers, Some(2));
            }
            _ => panic!("expected fleet resume"),
        }
    }

    #[test]
    fn parses_mcp_serve_with_default_addr() {
        let cli = Cli::try_parse_from(["nh", "mcp", "serve"]).unwrap();
        match cli.cmd {
            Cmd::Mcp {
                action: McpAction::Serve { addr, token_entry },
            } => {
                assert_eq!(addr, "127.0.0.1:8765");
                assert_eq!(token_entry, None);
            }
            _ => panic!("expected mcp serve"),
        }
    }

    #[test]
    fn parses_mcp_serve_addr_override() {
        let cli = Cli::try_parse_from([
            "nh",
            "mcp",
            "serve",
            "--addr",
            "127.0.0.1:9000",
        ])
        .unwrap();
        match cli.cmd {
            Cmd::Mcp {
                action: McpAction::Serve { addr, token_entry },
            } => {
                assert_eq!(addr, "127.0.0.1:9000");
                assert_eq!(token_entry, None);
            }
            _ => panic!("expected mcp serve"),
        }
    }

    #[test]
    fn parses_mcp_serve_token_entry() {
        let cli = Cli::try_parse_from([
            "nh",
            "mcp",
            "serve",
            "--token-entry",
            "korvin-mcp",
        ])
        .unwrap();
        match cli.cmd {
            Cmd::Mcp {
                action: McpAction::Serve { addr, token_entry },
            } => {
                assert_eq!(addr, "127.0.0.1:8765");
                assert_eq!(token_entry.as_deref(), Some("korvin-mcp"));
            }
            _ => panic!("expected mcp serve"),
        }
    }
}
