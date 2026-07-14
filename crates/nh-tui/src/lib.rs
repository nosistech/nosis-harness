//! Slice A TUI: one status, one worker, and a small Windows-safe renderer.

use std::io::{self, Write};
use std::panic;
use std::path::PathBuf;
use std::sync::{
    mpsc::{self, Receiver, Sender, TryRecvError},
    Arc, Mutex, RwLock,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::Context as _;
use chrono::{DateTime, FixedOffset, Utc};
use crossterm::{
    cursor::{Hide, Show},
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use nh_core::agent::AgentLoop;
use nh_core::receipt::ReceiptWriter;
use nh_core::wire::{cache_hit_pct, make_client, ChatClient, ChatMessage, ThinkingEffort, Usage};
use nh_law::{Law, Verdict};
use nh_routes::{ResolvedRoute, RouteClass, RouteResolver, ThinkingDialect};
use nh_tools::{builtin_tools, Access, Guard, ToolCtx};
use nh_vault::{EnvFallbackVault, KeyringVault, Scrubber, Vault};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
    Frame, Terminal,
};

type SharedScrubber = Arc<RwLock<Scrubber>>;
type ConnectFn =
    Box<dyn Fn(&ResolvedRoute) -> anyhow::Result<(Box<dyn ChatClient>, String)> + Send + Sync>;

const EVENT_POLL: Duration = Duration::from_millis(50);
const BUDGET_REASON: &str = "budget reached";

/// The single status shown by the semáforo.
#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Idle,
    Working,
    Waiting,
    Blocked(String),
}

/// Everything the render loop learns from the worker.
pub enum AgentEvent {
    Progress(String),
    Approval(ApprovalRequest),
    Usage(Usage),
    Answer(String),
    Failed(String),
}

/// One approval decision waiting for the main-thread UI.
pub struct ApprovalRequest {
    pub prompt: String,
    pub reply: Sender<bool>,
}

/// Resolved inputs for one TUI session.
pub struct TuiConfig {
    pub resolver: RouteResolver,
    pub model_id: String,
    pub law: Law,
    pub budget: Option<u64>,
    pub repo_root: PathBuf,
    pub workdir: PathBuf,
}

#[derive(Clone, Copy)]
enum TranscriptKind {
    Task,
    Answer,
    Progress,
    Approval,
    Error,
}

struct TranscriptLine {
    text: String,
    kind: TranscriptKind,
}

/// Unit-testable state for the renderer.
pub struct App {
    status: Status,
    route: ResolvedRoute,
    transcript: Vec<TranscriptLine>,
    pending_approval: Option<ApprovalRequest>,
    usage: Usage,
    input: String,
    budget: Option<u64>,
    scroll_back: u16,
    scrubber: SharedScrubber,
    local_offset: FixedOffset,
}

impl App {
    fn new(route: ResolvedRoute, budget: Option<u64>, scrubber: SharedScrubber) -> Self {
        Self {
            status: if budget == Some(0) {
                Status::Blocked(BUDGET_REASON.into())
            } else {
                Status::Idle
            },
            route,
            transcript: Vec::new(),
            pending_approval: None,
            usage: Usage::default(),
            input: String::new(),
            budget,
            scroll_back: 0,
            scrubber,
            local_offset: *chrono::Local::now().offset(),
        }
    }

    fn push_line(&mut self, text: &str, kind: TranscriptKind) {
        self.transcript.push(TranscriptLine {
            text: safe_line(&self.scrubber, text),
            kind,
        });
        self.scroll_back = 0;
    }

    fn push_text(&mut self, prefix: &str, text: &str, kind: TranscriptKind) {
        let mut saw_line = false;
        for line in text.lines() {
            saw_line = true;
            self.push_line(&format!("{prefix}{line}"), kind);
        }
        if !saw_line {
            self.push_line(prefix, kind);
        }
    }

    fn used_tokens(&self) -> u64 {
        self.usage
            .prompt_tokens
            .saturating_add(self.usage.completion_tokens)
    }

    fn budget_reached(&self) -> bool {
        self.budget.is_some_and(|limit| self.used_tokens() >= limit)
    }

    fn dispatch(&mut self) -> Option<String> {
        if matches!(self.status, Status::Working | Status::Waiting) || self.budget_reached() {
            return None;
        }
        let task = self.input.trim().to_owned();
        if task.is_empty() {
            return None;
        }
        self.input.clear();
        self.push_line(&format!("> {task}"), TranscriptKind::Task);
        self.status = Status::Working;
        Some(task)
    }

    fn answer_approval(&mut self, approved: bool) {
        if let Some(request) = self.pending_approval.take() {
            let _ = request.reply.send(approved);
            self.push_line(
                if approved {
                    "approval: yes"
                } else {
                    "approval: no"
                },
                TranscriptKind::Progress,
            );
            self.status = Status::Working;
        }
    }

    fn hud_line(&self, now: DateTime<Utc>) -> String {
        let cached = self.usage.cached_tokens.unwrap_or(0);
        let mut line = format!(
            "{} | {} | tokens {} in / {} out / {} cached",
            self.route.id,
            self.route.peak_status(now, self.local_offset),
            self.usage.prompt_tokens,
            self.usage.completion_tokens,
            cached
        );
        if let Some(pct) = cache_hit_pct(self.usage.prompt_tokens, cached) {
            line.push_str(&format!(" | cache {pct:.0}%"));
        }
        if let Some(limit) = self.budget {
            line.push_str(&format!(
                " | budget {} {}/{}",
                budget_bar(self.used_tokens(), limit),
                self.used_tokens(),
                limit
            ));
        }
        safe_line(&self.scrubber, &line)
    }
}

/// Fold one worker event into application state.
pub fn apply_event(app: &mut App, event: AgentEvent) -> &Status {
    match event {
        AgentEvent::Progress(line) => app.push_line(&format!("  {line}"), TranscriptKind::Progress),
        AgentEvent::Approval(request) => {
            let line = format!("approve? {}  [y/N]", request.prompt);
            app.push_line(&line, TranscriptKind::Approval);
            app.pending_approval = Some(request);
            app.status = Status::Waiting;
        }
        AgentEvent::Usage(usage) => {
            app.usage = usage;
            if app.budget_reached() {
                app.status = Status::Blocked(BUDGET_REASON.into());
            }
        }
        AgentEvent::Answer(answer) => {
            app.push_text("  ", &answer, TranscriptKind::Answer);
            app.status = if app.budget_reached() {
                Status::Blocked(BUDGET_REASON.into())
            } else {
                Status::Idle
            };
        }
        AgentEvent::Failed(reason) => {
            let reason = safe_line(&app.scrubber, &reason);
            app.push_line(&format!("error: {reason}"), TranscriptKind::Error);
            app.status = Status::Blocked(reason);
        }
    }
    &app.status
}

/// Run the full-screen TUI until the user quits.
pub fn run(config: TuiConfig) -> anyhow::Result<()> {
    let route = config.resolver.resolve(&config.model_id)?;
    if route.class == RouteClass::Delegate {
        anyhow::bail!("delegate routes arrive in M4 - pick an api route");
    }
    let connect: ConnectFn = Box::new(|route| {
        let vault = EnvFallbackVault {
            inner: KeyringVault,
        };
        let key = vault.get(&route.vault_entry)?;
        let literal = key.as_str().to_owned();
        Ok((make_client(route, key), literal))
    });
    run_with_connect(config, route, connect)
}

fn run_with_connect(
    config: TuiConfig,
    route: ResolvedRoute,
    connect: ConnectFn,
) -> anyhow::Result<()> {
    let scrubber = Arc::new(RwLock::new(Scrubber::new(Vec::new())));
    let initial = connect(&route);
    if let Ok((_, literal)) = &initial {
        install_literal(&scrubber, &mut Vec::new(), literal.clone());
    }
    let mut app = App::new(route.clone(), config.budget, Arc::clone(&scrubber));
    let mut worker = spawn_worker(WorkerConfig {
        route,
        law: config.law,
        repo_root: config.repo_root,
        workdir: config.workdir,
        scrubber,
        connect,
        initial: Some(initial),
    })?;

    let _panic_hook = PanicHookGuard::install();
    let _terminal_guard = TerminalGuard::enter()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend).context("could not open the terminal")?;
    terminal.clear().context("could not clear the terminal")?;
    let result = ui_loop(&mut terminal, &mut app, &mut worker);
    drop(terminal);
    result
}

enum WorkerCommand {
    Task(String),
    Stop,
}

struct Worker {
    commands: Sender<WorkerCommand>,
    events: Receiver<AgentEvent>,
    join: Option<JoinHandle<()>>,
}

impl Worker {
    fn stop(&mut self) {
        let _ = self.commands.send(WorkerCommand::Stop);
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop();
        if self.join.as_ref().is_some_and(JoinHandle::is_finished) {
            if let Some(join) = self.join.take() {
                let _ = join.join();
            }
        }
    }
}

struct WorkerConfig {
    route: ResolvedRoute,
    law: Law,
    repo_root: PathBuf,
    workdir: PathBuf,
    scrubber: SharedScrubber,
    connect: ConnectFn,
    initial: Option<anyhow::Result<(Box<dyn ChatClient>, String)>>,
}

fn spawn_worker(config: WorkerConfig) -> anyhow::Result<Worker> {
    let (command_tx, command_rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::channel();
    let join = thread::Builder::new()
        .name("nh-agent".into())
        .spawn(move || worker_loop(config, command_rx, event_tx))
        .context("could not start the agent worker")?;
    Ok(Worker {
        commands: command_tx,
        events: event_rx,
        join: Some(join),
    })
}

fn worker_loop(
    config: WorkerConfig,
    commands: Receiver<WorkerCommand>,
    events: Sender<AgentEvent>,
) {
    let WorkerConfig {
        route,
        law,
        repo_root,
        workdir,
        scrubber,
        connect,
        initial,
    } = config;
    let connection = match initial {
        Some(connection) => connection,
        None => connect(&route),
    };
    let (client, mut key_literals, mut connected) = match connection {
        Ok((client, literal)) => {
            let mut literals = Vec::new();
            install_literal(&scrubber, &mut literals, literal);
            (client, literals, true)
        }
        Err(error) => (
            Box::new(NotConnected {
                message: error.to_string(),
            }) as Box<dyn ChatClient>,
            Vec::new(),
            false,
        ),
    };

    let approval_pair = Arc::new(Mutex::new(new_approval_pair()));
    let approval_events = events.clone();
    let approval_scrubber = Arc::clone(&scrubber);
    let approval_pair_for_ctx = Arc::clone(&approval_pair);
    let approve = Box::new(move |prompt: &str| {
        let mut pair = match approval_pair_for_ctx.lock() {
            Ok(pair) => pair,
            Err(_) => return false,
        };
        let Some(reply) = pair.0.take() else {
            return false;
        };
        let request = ApprovalRequest {
            prompt: safe_line(&approval_scrubber, prompt),
            reply,
        };
        if approval_events.send(AgentEvent::Approval(request)).is_err() {
            *pair = new_approval_pair();
            return false;
        }
        let approved = pair.1.recv().unwrap_or(false);
        *pair = new_approval_pair();
        approved
    });

    let policy = law.policy.clone();
    let event_scrubber = Arc::clone(&scrubber);
    let progress_events = events.clone();
    let ctx = ToolCtx::new(workdir, approve).with_guard(Box::new(move |access| match access {
        Access::Write(path) => verdict_to_guard(policy.write_verdict(path)),
        Access::Exec(command) => verdict_to_guard(policy.exec_verdict(command)),
    }));
    let mut agent = AgentLoop {
        client,
        tools: builtin_tools(),
        ctx,
        receipts: ReceiptWriter {
            path: repo_root.join(".nosis").join("receipts.jsonl"),
            scrubber: Scrubber::new(key_literals.clone()),
        },
        model_id: route.model_id.clone(),
        max_turns: 20,
        thinking: effort_for(route.thinking_dialect),
        constitution: Some(law.constitution),
        context_limit: route.context,
        on_event: Some(Box::new(move |line| {
            let _ = progress_events.send(AgentEvent::Progress(safe_line(&event_scrubber, line)));
        })),
    };

    let mut history: Vec<ChatMessage> = Vec::new();
    let mut session_usage = Usage::default();
    while let Ok(command) = commands.recv() {
        let WorkerCommand::Task(task) = command else {
            break;
        };
        if !connected {
            match connect(&route) {
                Ok((client, literal)) => {
                    install_literal(&scrubber, &mut key_literals, literal);
                    agent.receipts.scrubber = Scrubber::new(key_literals.clone());
                    agent.client = client;
                    connected = true;
                }
                Err(error) => {
                    let _ =
                        events.send(AgentEvent::Failed(safe_line(&scrubber, &error.to_string())));
                    continue;
                }
            }
        }
        match agent.run_with_history(&mut history, &task) {
            Ok((answer, receipt)) => {
                if let Some(usage) = receipt.usage {
                    add_usage(&mut session_usage, &usage);
                    let _ = events.send(AgentEvent::Usage(session_usage.clone()));
                }
                let _ = events.send(AgentEvent::Answer(answer));
            }
            Err(error) => {
                let _ = events.send(AgentEvent::Failed(safe_line(&scrubber, &error.to_string())));
            }
        }
    }
}

fn new_approval_pair() -> (Option<Sender<bool>>, Receiver<bool>) {
    let (reply, answers) = mpsc::channel();
    (Some(reply), answers)
}

struct NotConnected {
    message: String,
}

impl ChatClient for NotConnected {
    fn complete(
        &self,
        _request: &nh_core::wire::ChatRequest,
    ) -> anyhow::Result<nh_core::wire::ChatResponse> {
        anyhow::bail!("{}", self.message)
    }
}

fn install_literal(scrubber: &SharedScrubber, literals: &mut Vec<String>, literal: String) {
    if !literal.is_empty() && !literals.contains(&literal) {
        literals.push(literal);
    }
    match scrubber.write() {
        Ok(mut guard) => *guard = Scrubber::new(literals.clone()),
        Err(poisoned) => *poisoned.into_inner() = Scrubber::new(literals.clone()),
    }
}

fn add_usage(total: &mut Usage, usage: &Usage) {
    total.prompt_tokens = total.prompt_tokens.saturating_add(usage.prompt_tokens);
    total.completion_tokens = total
        .completion_tokens
        .saturating_add(usage.completion_tokens);
    if let Some(cached) = usage.cached_tokens {
        let current = total.cached_tokens.get_or_insert(0);
        *current = current.saturating_add(cached);
    }
}

fn verdict_to_guard(verdict: Verdict) -> Guard {
    match verdict {
        Verdict::Allow => Guard::Allow,
        Verdict::Ask => Guard::Ask,
        Verdict::Block(reason) => Guard::Block(reason),
    }
}

fn effort_for(dialect: ThinkingDialect) -> ThinkingEffort {
    match dialect {
        ThinkingDialect::AlwaysThinking | ThinkingDialect::GlmHm => ThinkingEffort::High,
        ThinkingDialect::DeepseekNhm | ThinkingDialect::None => ThinkingEffort::None,
    }
}

fn ui_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    worker: &mut Worker,
) -> anyhow::Result<()> {
    loop {
        loop {
            match worker.events.try_recv() {
                Ok(agent_event) => {
                    let ring = matches!(agent_event, AgentEvent::Approval(_))
                        && !matches!(app.status, Status::Waiting);
                    apply_event(app, agent_event);
                    if ring {
                        ring_bell();
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    return Err(anyhow::anyhow!("agent stopped - retry the task"));
                }
            }
        }

        terminal
            .draw(|frame| render(frame, app))
            .context("could not draw the terminal")?;
        if !event::poll(EVENT_POLL).context("could not read terminal input")? {
            continue;
        }
        match event::read().context("could not read terminal input")? {
            Event::Key(key) if key.kind == KeyEventKind::Press && handle_key(app, worker, key) => {
                worker.stop();
                return Ok(());
            }
            _ => {}
        }
    }
}

fn handle_key(app: &mut App, worker: &mut Worker, key: KeyEvent) -> bool {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        if matches!(app.status, Status::Waiting) {
            app.answer_approval(false);
        }
        return true;
    }
    if matches!(app.status, Status::Waiting) {
        app.answer_approval(matches!(key.code, KeyCode::Char('y' | 'Y')));
        return false;
    }
    if matches!(app.status, Status::Working) {
        return false;
    }
    match key.code {
        KeyCode::Char('q') if app.input.is_empty() => return true,
        KeyCode::Char('?' | 't' | 'R') if app.input.is_empty() => {}
        KeyCode::Enter => {
            if let Some(task) = app.dispatch() {
                if worker.commands.send(WorkerCommand::Task(task)).is_err() {
                    apply_event(
                        app,
                        AgentEvent::Failed("agent stopped - retry the task".into()),
                    );
                }
            }
        }
        KeyCode::Backspace => {
            app.input.pop();
        }
        KeyCode::PageUp => app.scroll_back = app.scroll_back.saturating_add(5),
        KeyCode::PageDown => app.scroll_back = app.scroll_back.saturating_sub(5),
        KeyCode::End => app.scroll_back = 0,
        KeyCode::Char(character)
            if !character.is_control()
                && !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            app.input.push(character);
        }
        _ => {}
    }
    false
}

fn render(frame: &mut Frame<'_>, app: &App) {
    let regions = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(frame.area());
    render_header(frame, app, regions[0]);
    render_transcript(frame, app, regions[1]);
    frame.render_widget(Paragraph::new(app.hud_line(Utc::now())), regions[2]);
    let input = safe_line(&app.scrubber, &format!("> {}", app.input));
    frame.render_widget(
        Paragraph::new(input).style(Style::default().fg(Color::Cyan)),
        regions[3],
    );
}

fn render_header(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);
    let (status, style) = status_chip(&app.status);
    frame.render_widget(
        Paragraph::new(safe_line(&app.scrubber, &status)).style(style),
        columns[0],
    );
    frame.render_widget(
        Paragraph::new(safe_line(&app.scrubber, &app.route.id))
            .alignment(Alignment::Right)
            .style(Style::default().fg(Color::Cyan)),
        columns[1],
    );
}

fn render_transcript(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let lines: Vec<Line<'static>> = app
        .transcript
        .iter()
        .map(|line| Line::from(Span::styled(line.text.clone(), transcript_style(line.kind))))
        .collect();
    let rows = wrapped_rows(&app.transcript, area.width.max(1));
    let max_scroll = rows.saturating_sub(area.height);
    let scroll = max_scroll.saturating_sub(app.scroll_back.min(max_scroll));
    let transcript = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    frame.render_widget(transcript, area);
}

fn wrapped_rows(lines: &[TranscriptLine], width: u16) -> u16 {
    let width = usize::from(width.max(1));
    lines.iter().fold(0_u16, |rows, line| {
        let chars = line.text.chars().count().max(1);
        let line_rows = chars.div_ceil(width).min(usize::from(u16::MAX)) as u16;
        rows.saturating_add(line_rows)
    })
}

fn status_chip(status: &Status) -> (String, Style) {
    match status {
        Status::Idle => (
            ". IDLE".into(),
            Style::default().fg(Color::Gray).add_modifier(Modifier::DIM),
        ),
        Status::Working => (
            "> WORKING".into(),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Status::Waiting => (
            "! WAITING ON YOU".into(),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Status::Blocked(reason) => (
            format!("x BLOCKED: {reason}"),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
    }
}

fn transcript_style(kind: TranscriptKind) -> Style {
    match kind {
        TranscriptKind::Task => Style::default().fg(Color::Cyan),
        TranscriptKind::Answer => Style::default().fg(Color::White),
        TranscriptKind::Progress => Style::default().fg(Color::DarkGray),
        TranscriptKind::Approval => Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
        TranscriptKind::Error => Style::default().fg(Color::Red),
    }
}

fn budget_bar(used: u64, limit: u64) -> String {
    const WIDTH: u64 = 10;
    let filled = used
        .saturating_mul(WIDTH)
        .saturating_add(limit.saturating_sub(1))
        .checked_div(limit)
        .unwrap_or(WIDTH)
        .min(WIDTH);
    format!(
        "[{}{}]",
        "#".repeat(filled as usize),
        "-".repeat((WIDTH - filled) as usize)
    )
}

fn safe_line(scrubber: &SharedScrubber, text: &str) -> String {
    match scrubber.read() {
        Ok(guard) => nh_vault::safe_line(&guard, text),
        Err(poisoned) => {
            let guard = poisoned.into_inner();
            nh_vault::safe_line(&guard, text)
        }
    }
}

fn ring_bell() {
    let mut stdout = io::stdout();
    let _ = stdout.write_all(b"\x07");
    let _ = stdout.flush();
}

struct TerminalGuard {
    restore: Option<Box<dyn FnMut()>>,
}

impl TerminalGuard {
    fn enter() -> anyhow::Result<Self> {
        if let Err(error) = enable_raw_mode() {
            restore_terminal();
            return Err(error).context("could not enable terminal raw mode");
        }
        let mut stdout = io::stdout();
        if let Err(error) = execute!(stdout, EnterAlternateScreen, Hide) {
            restore_terminal();
            return Err(error).context("could not enter the alternate screen");
        }
        Ok(Self {
            restore: Some(Box::new(restore_terminal)),
        })
    }

    #[cfg(test)]
    fn with_restore(restore: impl FnMut() + 'static) -> Self {
        Self {
            restore: Some(Box::new(restore)),
        }
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if let Some(mut restore) = self.restore.take() {
            restore();
        }
    }
}

fn restore_terminal() {
    let _ = disable_raw_mode();
    let mut stdout = io::stdout();
    let _ = execute!(stdout, Show);
    let _ = execute!(stdout, LeaveAlternateScreen);
    let _ = stdout.flush();
}

type PanicHook = Box<dyn Fn(&panic::PanicHookInfo<'_>) + Send + Sync + 'static>;

struct PanicHookGuard {
    previous: Arc<Mutex<Option<PanicHook>>>,
}

impl PanicHookGuard {
    fn install() -> Self {
        let previous = Arc::new(Mutex::new(Some(panic::take_hook())));
        let hook_previous = Arc::clone(&previous);
        panic::set_hook(Box::new(move |info| {
            restore_terminal();
            let guard = hook_previous
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(previous) = guard.as_ref() {
                previous(info);
            }
        }));
        Self { previous }
    }
}

impl Drop for PanicHookGuard {
    fn drop(&mut self) {
        if thread::panicking() {
            return;
        }
        let installed = panic::take_hook();
        drop(installed);
        let previous = self
            .previous
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        if let Some(previous) = previous {
            panic::set_hook(previous);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nh_core::wire::{ChatRequest, ChatResponse};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    const TEST_CATALOG: &str = r#"
        [routes.test-route]
        provider = "test"
        model_id = "test-route"
        base_url = "https://example.invalid"
        wire = "openai"
        vault_entry = "test"
        context = 1000
    "#;

    fn test_route() -> ResolvedRoute {
        RouteResolver::from_toml(TEST_CATALOG)
            .unwrap()
            .resolve("test-route")
            .unwrap()
    }

    fn test_app(budget: Option<u64>) -> App {
        App::new(
            test_route(),
            budget,
            Arc::new(RwLock::new(Scrubber::new(Vec::new()))),
        )
    }

    fn approval(prompt: &str) -> (AgentEvent, Receiver<bool>) {
        let (reply, answers) = mpsc::channel();
        (
            AgentEvent::Approval(ApprovalRequest {
                prompt: prompt.into(),
                reply,
            }),
            answers,
        )
    }

    #[test]
    fn reducer_drives_every_semaforo_transition() {
        let mut app = test_app(None);
        assert_eq!(app.status, Status::Idle);
        app.input = "first task".into();
        assert_eq!(app.dispatch().as_deref(), Some("first task"));
        assert_eq!(app.status, Status::Working);

        let (event, answer) = approval("cargo test");
        assert_eq!(apply_event(&mut app, event), &Status::Waiting);
        app.answer_approval(true);
        assert!(answer.recv().unwrap());
        assert_eq!(app.status, Status::Working);
        assert_eq!(
            apply_event(&mut app, AgentEvent::Answer("done".into())),
            &Status::Idle
        );

        app.input = "second task".into();
        app.dispatch().unwrap();
        assert_eq!(
            apply_event(&mut app, AgentEvent::Failed("offline".into())),
            &Status::Blocked("offline".into())
        );
        app.input = "retry".into();
        app.dispatch().unwrap();
        assert_eq!(app.status, Status::Working);
    }

    #[test]
    fn approval_forwards_yes_and_no_then_returns_to_working() {
        for approved in [true, false] {
            let mut app = test_app(None);
            app.status = Status::Working;
            let (event, answer) = approval("edit src/lib.rs");
            apply_event(&mut app, event);
            assert_eq!(app.status, Status::Waiting);
            app.answer_approval(approved);
            assert_eq!(answer.recv().unwrap(), approved);
            assert_eq!(app.status, Status::Working);
        }
    }

    #[test]
    fn cost_hud_omits_cache_before_usage_and_shows_it_after() {
        let mut app = test_app(None);
        let before = app.hud_line(Utc::now());
        assert!(!before.contains("| cache "), "got: {before}");
        apply_event(
            &mut app,
            AgentEvent::Usage(Usage {
                prompt_tokens: 100,
                completion_tokens: 20,
                cached_tokens: Some(25),
            }),
        );
        let after = app.hud_line(Utc::now());
        assert!(after.contains("cache 25%"), "got: {after}");
    }

    #[test]
    fn budget_reached_blocks_and_refuses_another_dispatch() {
        let mut app = test_app(Some(100));
        app.status = Status::Working;
        assert_eq!(
            apply_event(
                &mut app,
                AgentEvent::Usage(Usage {
                    prompt_tokens: 80,
                    completion_tokens: 20,
                    cached_tokens: None,
                }),
            ),
            &Status::Blocked(BUDGET_REASON.into())
        );
        app.input = "must not run".into();
        assert!(app.dispatch().is_none());
        assert!(app.hud_line(Utc::now()).contains("100/100"));
        apply_event(&mut app, AgentEvent::Answer("finished".into()));
        assert_eq!(app.status, Status::Blocked(BUDGET_REASON.into()));
    }

    #[test]
    fn rendered_line_is_redacted_and_has_no_control_characters() {
        let secret = "hunter2-fake-tui-secret";
        let scrubber = Arc::new(RwLock::new(Scrubber::new(vec![secret.into()])));
        let mut app = App::new(test_route(), None, scrubber);
        apply_event(
            &mut app,
            AgentEvent::Progress(format!("value={secret}\r\x1b[2K")),
        );
        let rendered = &app.transcript[0].text;
        assert!(rendered.contains("[REDACTED]"), "got: {rendered}");
        assert!(!rendered.contains(secret), "got: {rendered}");
        assert!(!rendered.chars().any(char::is_control), "got: {rendered}");
    }

    #[test]
    fn terminal_guard_drop_runs_teardown() {
        let restored = Arc::new(AtomicBool::new(false));
        {
            let restored_for_guard = Arc::clone(&restored);
            let _guard = TerminalGuard::with_restore(move || {
                restored_for_guard.store(true, Ordering::SeqCst);
            });
            assert!(!restored.load(Ordering::SeqCst));
        }
        assert!(restored.load(Ordering::SeqCst));
    }

    struct MockClient {
        request_lengths: Arc<Mutex<Vec<usize>>>,
    }

    impl ChatClient for MockClient {
        fn complete(&self, request: &ChatRequest) -> anyhow::Result<ChatResponse> {
            self.request_lengths
                .lock()
                .unwrap()
                .push(request.messages.len());
            let mut message = request.messages.last().cloned().expect("user message");
            message.role = "assistant".into();
            message.content = Some("ok".into());
            message.tool_calls = None;
            message.tool_call_id = None;
            message.reasoning_content = None;
            Ok(ChatResponse {
                message,
                finish_reason: "stop".into(),
                usage: Some(Usage {
                    prompt_tokens: 10,
                    completion_tokens: 2,
                    cached_tokens: Some(4),
                }),
            })
        }
    }

    fn temp_dir() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nonce = NEXT.fetch_add(1, Ordering::Relaxed);
        let epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "nh-tui-test-{}-{epoch}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn worker_uses_injected_client_and_keeps_one_history_across_tasks() {
        let root = temp_dir();
        let request_lengths = Arc::new(Mutex::new(Vec::new()));
        let lengths_for_connect = Arc::clone(&request_lengths);
        let connect: ConnectFn = Box::new(move |_| {
            Ok((
                Box::new(MockClient {
                    request_lengths: Arc::clone(&lengths_for_connect),
                }),
                "fake-worker-secret".into(),
            ))
        });
        let law = nh_law::load(&root, &nh_law::LoadOptions { cli_autonomy: None });
        let scrubber = Arc::new(RwLock::new(Scrubber::new(Vec::new())));
        let mut worker = spawn_worker(WorkerConfig {
            route: test_route(),
            law,
            repo_root: root.clone(),
            workdir: root.clone(),
            scrubber,
            connect,
            initial: None,
        })
        .unwrap();

        for task in ["one", "two"] {
            worker
                .commands
                .send(WorkerCommand::Task(task.into()))
                .unwrap();
            loop {
                match worker.events.recv_timeout(Duration::from_secs(2)).unwrap() {
                    AgentEvent::Answer(answer) => {
                        assert_eq!(answer, "ok");
                        break;
                    }
                    AgentEvent::Usage(_) | AgentEvent::Progress(_) => {}
                    AgentEvent::Approval(_) => panic!("mock never asks for approval"),
                    AgentEvent::Failed(reason) => panic!("worker failed: {reason}"),
                }
            }
        }
        worker.stop();
        if let Some(join) = worker.join.take() {
            join.join().unwrap();
        }
        assert_eq!(*request_lengths.lock().unwrap(), vec![2, 4]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn keyless_worker_starts_and_task_surfaces_the_add_key_line() {
        let root = temp_dir();
        let message = "no key found for \"test\" - run `nh key add test`";
        let connect: ConnectFn = Box::new(move |_| anyhow::bail!("{message}"));
        let law = nh_law::load(&root, &nh_law::LoadOptions { cli_autonomy: None });
        let mut worker = spawn_worker(WorkerConfig {
            route: test_route(),
            law,
            repo_root: root.clone(),
            workdir: root.clone(),
            scrubber: Arc::new(RwLock::new(Scrubber::new(Vec::new()))),
            connect,
            initial: Some(Err(anyhow::anyhow!("{message}"))),
        })
        .unwrap();
        worker
            .commands
            .send(WorkerCommand::Task("hello".into()))
            .unwrap();
        match worker.events.recv_timeout(Duration::from_secs(2)).unwrap() {
            AgentEvent::Failed(reason) => {
                assert!(reason.contains("nh key add test"), "got: {reason}");
                assert!(!reason.chars().any(char::is_control), "got: {reason}");
            }
            _ => panic!("keyless task must fail with one friendly line"),
        }
        worker.stop();
        if let Some(join) = worker.join.take() {
            join.join().unwrap();
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
