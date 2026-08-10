use super::*;

#[test]
fn parses_init() {
    let cli = Cli::try_parse_from(["nh", "init"]).unwrap();
    assert!(matches!(cli.cmd, Cmd::Init));
}

#[test]
fn parses_doctor() {
    let cli = Cli::try_parse_from(["nh", "doctor"]).unwrap();
    assert!(matches!(cli.cmd, Cmd::Doctor));
}

#[test]
fn parses_global_ascii_mode_before_or_after_the_subcommand() {
    let before = Cli::try_parse_from(["nh", "--ascii", "on", "doctor"]).unwrap();
    assert_eq!(before.ascii, Some(AsciiArg::On));
    assert!(matches!(before.cmd, Cmd::Doctor));

    let after = Cli::try_parse_from(["nh", "doctor", "--ascii", "off"]).unwrap();
    assert_eq!(after.ascii, Some(AsciiArg::Off));
    assert!(matches!(after.cmd, Cmd::Doctor));

    let default = Cli::try_parse_from(["nh", "doctor"]).unwrap();
    assert_eq!(default.ascii, None);
}

#[test]
fn parses_key_add_with_entry() {
    let cli = Cli::try_parse_from(["nh", "key", "add", "deepseek"]).unwrap();
    match cli.cmd {
        Cmd::Key {
            action: KeyAction::Add { entry },
        } => assert_eq!(entry, "deepseek"),
        _ => panic!("expected key add"),
    }
}

#[test]
fn key_add_requires_entry() {
    assert!(Cli::try_parse_from(["nh", "key", "add"]).is_err());
}

#[test]
fn parses_key_remove_with_entry() {
    let cli = Cli::try_parse_from(["nh", "key", "remove", "deepseek"]).unwrap();
    match cli.cmd {
        Cmd::Key {
            action: KeyAction::Remove { entry },
        } => assert_eq!(entry, "deepseek"),
        _ => panic!("expected key remove"),
    }
}

#[test]
fn parses_run_with_defaults() {
    let cli = Cli::try_parse_from(["nh", "run", "fix the failing test"]).unwrap();
    match cli.cmd {
        Cmd::Run {
            task,
            model,
            max_turns,
            think,
            autonomy,
            profile,
            image,
        } => {
            assert_eq!(task, "fix the failing test");
            assert_eq!(model, "deepseek-v4-flash");
            assert_eq!(max_turns, 20);
            assert_eq!(think, None, "no --think = per-dialect default");
            assert_eq!(autonomy, None, "no --autonomy = law-file default");
            assert_eq!(profile, "balanced");
            assert!(image.is_empty());
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
        Cmd::Run {
            task,
            model,
            max_turns,
            think,
            autonomy,
            profile,
            image,
        } => {
            assert_eq!(task, "review the diff");
            assert_eq!(model, "deepseek-v4-pro");
            assert_eq!(max_turns, 5);
            assert_eq!(think, None);
            assert_eq!(autonomy, None);
            assert_eq!(profile, "balanced");
            assert!(image.is_empty());
        }
        _ => panic!("expected run"),
    }
}

#[test]
fn run_requires_a_task() {
    assert!(Cli::try_parse_from(["nh", "run"]).is_err());
}

#[test]
fn parses_repeatable_run_images() {
    let cli = Cli::try_parse_from([
        "nh",
        "run",
        "--image",
        "first.png",
        "--image",
        "second.jpg",
        "explain the screenshots",
    ])
    .unwrap();

    match cli.cmd {
        Cmd::Run { task, image, .. } => {
            assert_eq!(task, "explain the screenshots");
            assert_eq!(image, ["first.png", "second.jpg"]);
        }
        _ => panic!("expected run"),
    }
}

#[test]
fn run_help_discovers_supported_image_formats_and_limit() {
    use clap::CommandFactory as _;

    let mut command = Cli::command();
    let run = command.find_subcommand_mut("run").expect("run subcommand");
    let help = run.render_long_help().to_string();

    assert!(help.contains("--image <PATH>"), "got: {help}");
    assert!(help.contains("PNG or JPEG"), "got: {help}");
    assert!(help.contains("maximum 4"), "got: {help}");
}

#[test]
fn parses_run_think_levels() {
    use cmd_run::ThinkArg;
    let cases = [
        ("none", ThinkArg::None),
        ("low", ThinkArg::Low),
        ("high", ThinkArg::High),
        ("max", ThinkArg::Max),
    ];
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
fn parses_profile_overrides_on_live_commands() {
    let run = Cli::try_parse_from(["nh", "run", "task", "--profile", "frugal"]).unwrap();
    assert!(matches!(
        run.cmd,
        Cmd::Run { profile, .. } if profile == "frugal"
    ));
    let chat = Cli::try_parse_from(["nh", "chat", "--profile", "max-quality"]).unwrap();
    assert!(matches!(
        chat.cmd,
        Cmd::Chat { profile, .. } if profile == "max-quality"
    ));
    let tui = Cli::try_parse_from(["nh", "tui", "--profile", "frugal"]).unwrap();
    assert!(matches!(
        tui.cmd,
        Cmd::Tui { profile, .. } if profile == "frugal"
    ));
}

#[test]
fn parses_chat_with_default_model() {
    let cli = Cli::try_parse_from(["nh", "chat"]).unwrap();
    match cli.cmd {
        Cmd::Chat { model, profile } => {
            assert_eq!(model, "deepseek-v4-flash");
            assert_eq!(profile, "balanced");
        }
        _ => panic!("expected chat"),
    }
}

#[test]
fn parses_top_level_resume_with_optional_session_id() {
    let listing = Cli::try_parse_from(["nh", "resume"]).unwrap();
    assert!(matches!(listing.cmd, Cmd::Resume { session_id: None }));

    let named = Cli::try_parse_from(["nh", "resume", "session-123"]).unwrap();
    assert!(matches!(
        named.cmd,
        Cmd::Resume { session_id: Some(id) } if id == "session-123"
    ));
}

#[test]
fn run_rejects_zero_max_turns() {
    let error = match Cli::try_parse_from(["nh", "run", "task", "--max-turns", "0"]) {
        Ok(_) => panic!("zero turns must be rejected"),
        Err(error) => error,
    };
    let error = error.to_string();
    assert!(error.contains("--max-turns"));
    assert!(error.contains("1 to 100"), "got: {error}");
    assert!(!error.contains("4294967295"), "got: {error}");
}

#[test]
fn run_max_turns_has_a_human_sized_inclusive_upper_bound() {
    let accepted = Cli::try_parse_from(["nh", "run", "task", "--max-turns", "100"]).unwrap();
    assert!(matches!(accepted.cmd, Cmd::Run { max_turns: 100, .. }));

    let error = match Cli::try_parse_from(["nh", "run", "task", "--max-turns", "101"]) {
        Ok(_) => panic!("101 turns must be rejected"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("1 to 100"), "got: {error}");
}

#[test]
fn parses_chat_with_model_override() {
    let cli = Cli::try_parse_from(["nh", "chat", "--model", "kimi-k2.6"]).unwrap();
    match cli.cmd {
        Cmd::Chat { model, profile } => {
            assert_eq!(model, "kimi-k2.6");
            assert_eq!(profile, "balanced");
        }
        _ => panic!("expected chat"),
    }
}

#[test]
fn parses_why_with_optional_task_and_model() {
    let cli = Cli::try_parse_from(["nh", "why", "review the diff", "--model", "deepseek-v4-pro"])
        .unwrap();
    match cli.cmd {
        Cmd::Why { task, model } => {
            assert_eq!(task.as_deref(), Some("review the diff"));
            assert_eq!(model.as_deref(), Some("deepseek-v4-pro"));
        }
        _ => panic!("expected why"),
    }

    let cli = Cli::try_parse_from(["nh", "why"]).unwrap();
    assert!(matches!(
        cli.cmd,
        Cmd::Why {
            task: None,
            model: None
        }
    ));
}

#[test]
fn parses_profile_listing_with_default_and_model_override() {
    let default = Cli::try_parse_from(["nh", "profile"]).unwrap();
    assert!(matches!(
        default.cmd,
        Cmd::Profile { model } if model == "deepseek-v4-flash"
    ));
    let selected = Cli::try_parse_from(["nh", "profile", "--model", "kimi-k2.6"]).unwrap();
    assert!(matches!(
        selected.cmd,
        Cmd::Profile { model } if model == "kimi-k2.6"
    ));
}

#[test]
fn parses_tui_with_defaults() {
    let cli = Cli::try_parse_from(["nh", "tui"]).unwrap();
    match cli.cmd {
        Cmd::Tui {
            model,
            budget,
            profile,
        } => {
            assert_eq!(model, "deepseek-v4-flash");
            assert_eq!(budget, None);
            assert_eq!(profile, "balanced");
        }
        _ => panic!("expected tui"),
    }
}

#[test]
fn parses_tui_with_model_and_budget() {
    let cli =
        Cli::try_parse_from(["nh", "tui", "--model", "kimi-k2.6", "--budget", "12000"]).unwrap();
    match cli.cmd {
        Cmd::Tui {
            model,
            budget,
            profile,
        } => {
            assert_eq!(model, "kimi-k2.6");
            assert_eq!(budget, Some(12000));
            assert_eq!(profile, "balanced");
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
            assert_eq!(escalate, Some(true));
            assert_eq!(defer_offpeak, Some(true));
        }
        _ => panic!("expected fleet run"),
    }
}

#[test]
fn parses_fleet_run_false_flag_overrides() {
    let cli = Cli::try_parse_from([
        "nh",
        "fleet",
        "run",
        "tasks.json",
        "--escalate=false",
        "--defer-offpeak=false",
    ])
    .unwrap();
    match cli.cmd {
        Cmd::Fleet {
            action:
                FleetAction::Run {
                    escalate,
                    defer_offpeak,
                    ..
                },
        } => {
            assert_eq!(escalate, Some(false));
            assert_eq!(defer_offpeak, Some(false));
        }
        _ => panic!("expected fleet run"),
    }
}

#[test]
fn parses_fleet_resume_with_optional_id() {
    let latest = Cli::try_parse_from(["nh", "fleet", "resume"]).unwrap();
    assert!(matches!(
        latest.cmd,
        Cmd::Fleet {
            action: FleetAction::Resume { run_id: None, .. }
        }
    ));
    let named =
        Cli::try_parse_from(["nh", "fleet", "resume", "run-123", "--max-workers", "2"]).unwrap();
    match named.cmd {
        Cmd::Fleet {
            action:
                FleetAction::Resume {
                    run_id,
                    max_workers,
                },
        } => {
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
    let cli = Cli::try_parse_from(["nh", "mcp", "serve", "--addr", "127.0.0.1:9000"]).unwrap();
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
    let cli = Cli::try_parse_from(["nh", "mcp", "serve", "--token-entry", "korvin-mcp"]).unwrap();
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

#[test]
fn mcp_serve_help_names_all_six_runtime_tools() {
    use clap::CommandFactory as _;

    let mut command = Cli::command();
    let serve = command
        .find_subcommand_mut("mcp")
        .and_then(|mcp| mcp.find_subcommand_mut("serve"))
        .expect("mcp serve subcommand");
    let help = serve.render_long_help().to_string();
    for tool in [
        "route_resolve",
        "fleet_run",
        "fleet_status",
        "why",
        "route_cost",
        "receipts",
    ] {
        assert!(help.contains(tool), "missing {tool}: {help}");
    }
}
