//! Terminal session lifecycle, worker orchestration, and shared UI helpers.

use crate::input::{handle_action, handle_input_event, reduce_agent_event};
use crate::palette::resolve_color_mode;
use crate::render::render;
use crate::state::{
    AgentEvent, App, RouteTimingHistory, Status, TranscriptKind, TuiConfig, UiInputs,
};
use crate::terminal::{with_terminal_panic_hook, PanicAbort, TerminalGuard, TerminalStateHandle};
use crate::timeline::record_restored_turn_cost;
use crate::worker::{spawn_worker, Worker, WorkerConfig, WorkerShutdown, SHUTDOWN_TIMEOUT};
use crate::{
    ConnectFn, SharedScrubber, EVENT_POLL, TASKBAR_CLEAR, TASKBAR_WAITING, TITLE_BLOCKED,
    TITLE_IDLE, TURN_BELL_MIN,
};
use anyhow::Context as _;
use chrono::{DateTime, Utc};
use crossterm::event;
use nh_core::credential;
use nh_core::receipt::{parse_receipt_jsonl, read_receipt_tail};
use nh_core::session_ledger::{RestoredSession, Surface};
use nh_core::wire::{resolve_effort, ThinkingEffort};
use nh_routes::{ResolvedRoute, RouteClass, ThinkingDialect, ThinkingPosture, Wire};
use nh_vault::{EnvFallbackVault, KeyringVault, Scrubber, SecretRegistry, SecretValue};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::{self, Write};
use std::sync::{mpsc::TryRecvError, Arc, RwLock};

fn validate_resume(
    resolver: &nh_routes::RouteResolver,
    route: &ResolvedRoute,
    restored: &RestoredSession,
) -> anyhow::Result<()> {
    if restored.surface != Surface::Tui {
        anyhow::bail!(
            "session belongs to chat - run `nh resume {}`",
            restored.session_id
        );
    }
    if route.model_id() != restored.model_id {
        anyhow::bail!(
            "session route {} now points to a different model - restore the recorded catalog entry, then retry",
            restored.route_id
        );
    }
    for turn in &restored.turns {
        resolver.resolve(&turn.route_id).map_err(|_| {
            anyhow::anyhow!(
                "session route {} is no longer available - restore it in catalog.toml, then retry",
                turn.route_id
            )
        })?;
        DateTime::parse_from_rfc3339(&turn.ts_utc).map_err(|_| {
            anyhow::anyhow!("session has an invalid turn timestamp - inspect its ledger")
        })?;
    }
    Ok(())
}

pub(super) fn restore_app(
    app: &mut App,
    restored: &RestoredSession,
    law_constitution: &str,
) -> anyhow::Result<()> {
    app.resumed = true;
    for turn in &restored.turns {
        crate::worker::add_usage(&mut app.usage, turn.usage.as_ref());
        let route = app.resolver.resolve(&turn.route_id)?;
        let at = DateTime::parse_from_rfc3339(&turn.ts_utc)?.with_timezone(&Utc);
        record_restored_turn_cost(app, &route, turn.usage.as_ref(), at);
    }
    app.push_line(
        &format!(
            "resumed {} - {} turns restored on {}",
            restored.session_id,
            restored.turns.len(),
            restored.route_id
        ),
        TranscriptKind::Progress,
    );
    if restored.dropped_torn_tail {
        app.push_line(
            "last session record was incomplete and was dropped - continuing safely",
            TranscriptKind::Progress,
        );
    }
    let constitution_changed = restored
        .history
        .first()
        .filter(|message| message.role == "system")
        .and_then(|message| message.content.as_deref())
        .is_some_and(|recorded| !recorded.ends_with(law_constitution));
    if constitution_changed {
        app.push_line(
            "session kept its original constitution - start a new session to use the current one",
            TranscriptKind::Progress,
        );
    }
    if app.budget_reached() {
        app.set_status(Status::Blocked(crate::BUDGET_REASON.into()), Utc::now());
    }
    Ok(())
}

/// Run the full-screen TUI until the user quits.
pub fn run(config: TuiConfig) -> anyhow::Result<()> {
    let model_id = config
        .resume
        .as_ref()
        .map_or(config.model_id.as_str(), |resume| resume.route_id.as_str());
    let route = match config.resolver.resolve(model_id) {
        Ok(route) => route,
        Err(_) if config.resume.is_some() => anyhow::bail!(
            "session route {model_id} is no longer available - restore it in catalog.toml, then retry"
        ),
        Err(error) => return Err(error),
    };
    if let Some(resume) = &config.resume {
        validate_resume(&config.resolver, &route, resume)?;
    }
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
        credentialed_providers,
        resume,
    } = config;
    let (route_timing_history, timing_history_unavailable) = match read_receipt_tail(&repo_root)
        .and_then(|bytes| parse_receipt_jsonl(&bytes, usize::MAX))
    {
        Ok(receipts) => (
            RouteTimingHistory::from_receipts(&resolver, receipts),
            false,
        ),
        Err(_) => (RouteTimingHistory::default(), true),
    };
    let scrubber = Arc::new(RwLock::new(Scrubber::new(Vec::new())));
    let execution_policy = profiles.effective(&profile, &route);
    let initial = connect(&route, execution_policy.output_cap);
    if let Ok((_, literal)) = &initial {
        install_literal(&scrubber, &mut SecretRegistry::new(), literal.clone());
    }
    let policy_view = law.policy.view();
    let law_constitution = law.constitution.clone();
    let resume_for_app = resume.clone();
    let color_mode = resolve_color_mode(std::env::var_os("NO_COLOR").as_deref());
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
        resume,
    })?;
    // Keep App after Worker: unwinding drops its approval sender before Worker::drop.
    let mut app = App::new(
        resolver,
        route,
        budget,
        scrubber,
        policy_view,
        UiInputs {
            palette_entries,
            credentialed_providers,
            color_mode,
            route_timing_history,
            prompt_base_tokens: Arc::clone(&worker.prompt_base_tokens),
        },
        (profiles, execution_policy.profile),
    );
    if timing_history_unavailable {
        app.push_line(
            "typical timing unavailable - receipt history could not be read",
            TranscriptKind::Progress,
        );
    }
    if let Some(restored) = &resume_for_app {
        restore_app(&mut app, restored, &law_constitution)?;
    }

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
/// `run`/`chat` paths so every agent surface - not just the TUI - is honest.
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

pub(super) fn handle_agent_event(
    app: &mut App,
    worker: &mut Worker,
    event: AgentEvent,
) -> (Status, bool) {
    let (previous, action) = reduce_agent_event(app, event);
    let should_quit = handle_action(app, worker, action);
    (previous, should_quit)
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
                    let ring = matches!(agent_event, AgentEvent::Approval(_))
                        && !matches!(app.status, Status::Waiting);
                    let working_since = app.working_since;
                    let (previous, should_quit) = handle_agent_event(app, worker, agent_event);
                    emit_taskbar_transition(
                        terminal.backend_mut(),
                        &previous,
                        &app.status,
                        working_since,
                        Utc::now(),
                    )
                    .context("could not update terminal status")?;
                    if ring {
                        ring_bell();
                    }
                    if should_quit {
                        return Ok(());
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    return Err(anyhow::anyhow!("agent stopped - retry the task"));
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
        let working_since = app.working_since;
        let should_quit = handle_input_event(app, worker, input);
        emit_taskbar_transition(
            terminal.backend_mut(),
            &previous,
            &app.status,
            working_since,
            Utc::now(),
        )
        .context("could not update terminal status")?;
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
    working_since: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> io::Result<()> {
    let mut changed = false;
    if matches!(current, Status::Waiting) && !matches!(previous, Status::Waiting) {
        writer.write_all(TASKBAR_WAITING)?;
        changed = true;
    } else if matches!(previous, Status::Waiting) && !matches!(current, Status::Waiting) {
        writer.write_all(TASKBAR_CLEAR)?;
        changed = true;
    }

    if matches!(previous, Status::Working | Status::FinishingInterrupted)
        && matches!(current, Status::Idle | Status::Blocked(_))
    {
        writer.write_all(TASKBAR_CLEAR)?;
        writer.write_all(if matches!(current, Status::Idle) {
            TITLE_IDLE
        } else {
            TITLE_BLOCKED
        })?;
        if working_since.is_some_and(|started| {
            now.signed_duration_since(started)
                .to_std()
                .is_ok_and(|elapsed| elapsed >= TURN_BELL_MIN)
        }) {
            writer.write_all(b"\x07")?;
        }
        changed = true;
    }

    if changed {
        writer.flush()?;
    }
    Ok(())
}
