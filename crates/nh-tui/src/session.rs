//! Terminal session lifecycle, worker orchestration, and shared UI helpers.

use super::*;

/// Run the full-screen TUI until the user quits.
pub fn run(config: TuiConfig) -> anyhow::Result<()> {
    let route = config.resolver.resolve(&config.model_id)?;
    if route.class() == RouteClass::Delegate {
        anyhow::bail!("delegate routes arrive in M4 - pick an api route");
    }
    let connect_policy = config.law.policy.clone();
    let connect: ConnectFn = Box::new(move |route, output_cap| {
        let vault = EnvFallbackVault {
            inner: KeyringVault,
        };
        credential::connect(
            &vault,
            route,
            &connect_policy.approved_audiences(route.vault_entry()),
            output_cap,
        )
    });
    run_with_connect(config, route, connect)
}

pub(super) fn run_with_connect(
    config: TuiConfig,
    route: ResolvedRoute,
    connect: ConnectFn,
) -> anyhow::Result<()> {
    with_terminal_panic_hook(|panic_abort, terminal_state| {
        run_tui_session(config, route, connect, panic_abort, terminal_state)
    })
}

pub(super) fn run_tui_session(
    config: TuiConfig,
    route: ResolvedRoute,
    connect: ConnectFn,
    panic_abort: &PanicAbort,
    terminal_state: TerminalStateHandle,
) -> anyhow::Result<()> {
    let TuiConfig {
        resolver,
        model_id: _,
        profiles,
        profile,
        law,
        budget,
        repo_root,
        workdir,
        palette_entries,
    } = config;
    let scrubber = Arc::new(RwLock::new(Scrubber::new(Vec::new())));
    let execution_policy = profiles.effective(&profile, &route);
    let initial = connect(&route, execution_policy.output_cap);
    if let Ok((_, literal)) = &initial {
        install_literal(&scrubber, &mut SecretRegistry::new(), literal.clone());
    }
    let policy_view = law.policy.view();
    let mut worker = spawn_worker(WorkerConfig {
        route: route.clone(),
        law,
        repo_root,
        workdir,
        scrubber: Arc::clone(&scrubber),
        connect,
        initial: Some(initial),
        profiles: profiles.clone(),
        active_profile: execution_policy.profile.clone(),
    })?;
    // Keep App after Worker: unwinding drops its approval sender before Worker::drop.
    let mut app = App::new(
        resolver,
        route,
        budget,
        scrubber,
        policy_view,
        palette_entries,
        (profiles, execution_policy.profile),
    );

    let terminal_guard = TerminalGuard::enter(terminal_state)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend).context("could not open the terminal")?;
    terminal.clear().context("could not clear the terminal")?;
    let result = ui_loop(&mut terminal, &mut app, &mut worker, panic_abort);
    drop(terminal);
    drop(terminal_guard);
    app.close_pending_approval();
    finish_worker_shutdown(result, worker.shutdown())
}

pub(super) fn finish_worker_shutdown(
    result: anyhow::Result<()>,
    shutdown: WorkerShutdown,
) -> anyhow::Result<()> {
    let failure = match shutdown {
        WorkerShutdown::Clean => return result,
        WorkerShutdown::Panicked => "agent worker panicked during shutdown".to_owned(),
        WorkerShutdown::Detached => format!(
            "agent worker did not stop within {} ms; detached",
            SHUTDOWN_TIMEOUT.as_millis()
        ),
    };
    match result {
        Ok(()) => Err(anyhow::anyhow!(failure)),
        Err(error) => Err(error.context(failure)),
    }
}

/// The honest-identity system prompt: names the real route + provider and forbids
/// claiming to be Claude/GPT, then appends the law constitution. Shared with the CLI
/// `run`/`chat` paths so every agent surface — not just the TUI — is honest.
pub fn identity_constitution(law_constitution: &str, route: &ResolvedRoute) -> String {
    nh_core::agent::identity_constitution(law_constitution, route.id(), route.provider())
}

pub(super) fn install_literal(
    scrubber: &SharedScrubber,
    literals: &mut SecretRegistry,
    literal: SecretValue,
) {
    literals.insert(literal);
    match scrubber.write() {
        Ok(mut guard) => *guard = literals.scrubber(),
        Err(poisoned) => *poisoned.into_inner() = literals.scrubber(),
    }
}

pub(super) fn effort_for(
    posture: ThinkingPosture,
    dialect: ThinkingDialect,
    wire: Wire,
) -> ThinkingEffort {
    resolve_effort(None, posture, dialect, wire)
}

pub(super) fn effort_name(effort: ThinkingEffort) -> &'static str {
    match effort {
        ThinkingEffort::None => "none",
        ThinkingEffort::Low => "low",
        ThinkingEffort::High => "high",
        ThinkingEffort::Max => "max",
    }
}

pub(super) fn parse_effort(value: &str) -> Option<ThinkingEffort> {
    match value.trim().to_ascii_lowercase().as_str() {
        "none" => Some(ThinkingEffort::None),
        "low" => Some(ThinkingEffort::Low),
        "high" => Some(ThinkingEffort::High),
        "max" => Some(ThinkingEffort::Max),
        _ => None,
    }
}

pub(super) fn ui_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    worker: &mut Worker,
    panic_abort: &PanicAbort,
) -> anyhow::Result<()> {
    loop {
        if panic_abort.requested() {
            return Ok(());
        }
        loop {
            match worker.events.try_recv() {
                Ok(agent_event) => {
                    let previous = app.status.clone();
                    let ring = matches!(agent_event, AgentEvent::Approval(_))
                        && !matches!(app.status, Status::Waiting);
                    apply_event(app, agent_event);
                    emit_taskbar_transition(terminal.backend_mut(), &previous, &app.status)
                        .context("could not update taskbar status")?;
                    if ring {
                        ring_bell();
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    return Err(anyhow::anyhow!("agent stopped — retry the task"));
                }
            }
        }
        if panic_abort.requested() {
            return Ok(());
        }
        terminal
            .draw(|frame| render(frame, app))
            .context("could not draw the terminal")?;
        if !event::poll(EVENT_POLL).context("could not read terminal input")? {
            continue;
        }
        let input = event::read().context("could not read terminal input")?;
        let previous = app.status.clone();
        let should_quit = handle_input_event(app, worker, input);
        emit_taskbar_transition(terminal.backend_mut(), &previous, &app.status)
            .context("could not update taskbar status")?;
        if should_quit {
            return Ok(());
        }
    }
}

pub(super) fn safe_line(scrubber: &SharedScrubber, text: &str) -> String {
    match scrubber.read() {
        Ok(guard) => nh_vault::safe_line(&guard, text),
        Err(poisoned) => {
            let guard = poisoned.into_inner();
            nh_vault::safe_line(&guard, text)
        }
    }
}

pub(super) fn scrub_full_line(scrubber: &SharedScrubber, text: &str) -> String {
    let scrubbed = match scrubber.read() {
        Ok(guard) => guard.scrub(text),
        Err(poisoned) => poisoned.into_inner().scrub(text),
    };
    nh_vault::escape_untrusted(&scrubbed)
}

pub(super) fn ring_bell() {
    let mut stdout = io::stdout();
    let _ = stdout.write_all(b"\x07");
    let _ = stdout.flush();
}

pub(super) fn emit_taskbar_transition(
    writer: &mut impl Write,
    previous: &Status,
    current: &Status,
) -> io::Result<()> {
    if matches!(current, Status::Waiting) && !matches!(previous, Status::Waiting) {
        writer.write_all(TASKBAR_WAITING)?;
        writer.flush()
    } else if matches!(previous, Status::Waiting) && !matches!(current, Status::Waiting) {
        writer.write_all(TASKBAR_CLEAR)?;
        writer.flush()
    } else {
        Ok(())
    }
}
