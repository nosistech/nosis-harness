//! M3 TUI: one status, one worker, and small Windows-safe views.

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
use nh_core::receipt::{FailureClass, Outcome, Receipt, ReceiptWriter};
use nh_core::wire::{cache_hit_pct, make_client, ChatClient, ChatMessage, ThinkingEffort, Usage};
use nh_law::{Autonomy, Law, PolicyView, Verdict};
use nh_routes::{ResolvedRoute, RouteClass, RouteResolver, ThinkingDialect};
use nh_tools::{
    builtin_tools, Access, Guard, McpAuth, McpServerConfig, McpToolset, McpTrust, ToolCtx,
};
use nh_vault::{EnvFallbackVault, KeyringVault, Scrubber, Vault};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph, Wrap},
    Frame, Terminal,
};

type SharedScrubber = Arc<RwLock<Scrubber>>;
type ConnectFn =
    Box<dyn Fn(&ResolvedRoute) -> anyhow::Result<(Box<dyn ChatClient>, String)> + Send + Sync>;

const EVENT_POLL: Duration = Duration::from_millis(50);
const BUDGET_REASON: &str = "budget reached";
const RESTORE_DEFERRED: &str = "restore arrives in a later slice";
const MAX_NOTIFY_CHARS: usize = 160;
const TELEGRAM_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const TELEGRAM_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

/// The single status shown by the semáforo.
#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Idle,
    Working,
    Waiting,
    Blocked(String),
}

/// Optional remote notification settings loaded once before terminal takeover.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NotifyConfig {
    pub telegram: Option<TelegramNotifyConfig>,
}

/// Non-secret Telegram settings. The bot token always comes from nh-vault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelegramNotifyConfig {
    pub enabled: bool,
    pub chat_id: String,
}

impl NotifyConfig {
    fn telegram_enabled(&self) -> bool {
        self.telegram
            .as_ref()
            .is_some_and(|telegram| telegram.enabled && !telegram.chat_id.trim().is_empty())
    }
}

/// Parse the small optional `.nosis/notify.toml` surface.
pub fn parse_notify_config(text: &str) -> anyhow::Result<NotifyConfig> {
    let value: toml::Value = toml::from_str(text).context("invalid notify configuration")?;
    let Some(raw_telegram) = value.get("telegram") else {
        return Ok(NotifyConfig::default());
    };
    let telegram = raw_telegram
        .as_table()
        .context("[telegram] must be a table")?;
    let enabled = match telegram.get("enabled") {
        Some(value) => value
            .as_bool()
            .context("telegram.enabled must be true or false")?,
        None => false,
    };
    let chat_id = match telegram.get("chat_id") {
        Some(value) => value
            .as_str()
            .context("telegram.chat_id must be a string")?
            .trim()
            .to_owned(),
        None => String::new(),
    };
    if enabled && chat_id.is_empty() {
        anyhow::bail!("telegram.chat_id is required when telegram is enabled");
    }
    Ok(NotifyConfig {
        telegram: Some(TelegramNotifyConfig { enabled, chat_id }),
    })
}

/// One completed task projected from its receipt and in-memory answer.
#[derive(Debug, Clone)]
pub struct TimelineEntry {
    pub turn: usize,
    pub ts_utc: String,
    pub model_id: String,
    pub task: String,
    pub turns: u32,
    pub tool_calls: u32,
    pub outcome: Outcome,
    pub failure_class: Option<FailureClass>,
    pub usage: Option<Usage>,
    pub answer: String,
    pub compacted: bool,
}

impl TimelineEntry {
    /// Build a timeline row without mutating the source receipt.
    pub fn from_receipt(turn: usize, receipt: Receipt, answer: String, compacted: bool) -> Self {
        Self {
            turn,
            ts_utc: receipt.ts_utc,
            model_id: receipt.model_id,
            task: short_text(&receipt.task, 120),
            turns: receipt.turns,
            tool_calls: receipt.tool_calls,
            outcome: receipt.outcome,
            failure_class: receipt.failure_class,
            usage: receipt.usage,
            answer,
            compacted,
        }
    }

    fn tokens(&self) -> (u64, u64, u64) {
        self.usage.as_ref().map_or((0, 0, 0), |usage| {
            (
                usage.prompt_tokens,
                usage.completion_tokens,
                usage.cached_tokens.unwrap_or(0),
            )
        })
    }
}

/// Additive worker payload carrying the receipt alongside its existing answer event.
pub struct TimelineSummary {
    pub receipt: Receipt,
    pub answer: String,
}

/// One immutable row in the discoverability palette.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteEntry {
    kind: &'static str,
    name: String,
    description: String,
    state: Option<McpState>,
    action: PaletteAction,
}

/// Startup state shown for an MCP server or tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpState {
    Enabled,
    AuthOk,
    Stale,
    DiscoverOnly,
}

impl McpState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Enabled => "enabled",
            Self::AuthOk => "auth-ok",
            Self::Stale => "stale",
            Self::DiscoverOnly => "discover-only",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PaletteAction {
    Quit,
    TrustDial,
    Timeline,
    Palette,
    ScrollUp,
    ScrollDown,
    ScrollEnd,
    Reserved,
    Describe,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Overlay {
    None,
    TrustDial,
    Timeline {
        selected: usize,
        inspecting: bool,
        note: Option<String>,
    },
    Palette {
        filter: String,
        selected: usize,
        detail: Option<String>,
    },
}

/// Everything the render loop learns from the worker.
pub enum AgentEvent {
    Progress(String),
    Approval(ApprovalRequest),
    Usage(Usage),
    TaskReceipt(TimelineSummary),
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
    pub palette_entries: Vec<PaletteEntry>,
    pub notify: NotifyConfig,
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

/// Unit-testable state for the Slice A renderer.
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
    policy_view: PolicyView,
    palette_entries: Vec<PaletteEntry>,
    overlay: Overlay,
    timeline: Vec<TimelineEntry>,
    current_task_compacted: bool,
}

impl App {
    fn new(
        route: ResolvedRoute,
        budget: Option<u64>,
        scrubber: SharedScrubber,
        policy_view: PolicyView,
        mcp_entries: Vec<PaletteEntry>,
    ) -> Self {
        let mut palette_entries = builtin_palette_entries();
        palette_entries.extend(mcp_entries);
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
            policy_view,
            palette_entries,
            overlay: Overlay::None,
            timeline: Vec::new(),
            current_task_compacted: false,
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
        self.current_task_compacted = false;
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

/// Project configured MCP servers and discovered tools into immutable palette rows.
pub fn mcp_palette_entries(configs: &[McpServerConfig], toolset: &McpToolset) -> Vec<PaletteEntry> {
    if configs.is_empty() {
        return if toolset.warnings.is_empty() {
            Vec::new()
        } else {
            vec![PaletteEntry {
                kind: "server",
                name: "MCP configuration".into(),
                description: "configuration could not be loaded".into(),
                state: Some(McpState::Stale),
                action: PaletteAction::Describe,
            }]
        };
    }

    let specs: Vec<_> = toolset.tools.iter().map(|tool| tool.spec()).collect();
    let mut entries = Vec::new();
    for config in configs {
        let state = mcp_state(config, &toolset.warnings);
        entries.push(PaletteEntry {
            kind: "server",
            name: config.name.clone(),
            description: "configured MCP server".into(),
            state: Some(state),
            action: PaletteAction::Describe,
        });

        let prefix = format!("mcp__{}__", config.name);
        for spec in specs.iter().filter(|spec| spec.name.starts_with(&prefix)) {
            entries.push(PaletteEntry {
                kind: "tool",
                name: spec.name.clone(),
                description: spec.description.clone(),
                state: Some(state),
                action: PaletteAction::Describe,
            });
        }
    }
    entries
}

/// Case-insensitive substring filter over an in-memory palette.
pub fn filter_palette<'a>(entries: &'a [PaletteEntry], query: &str) -> Vec<&'a PaletteEntry> {
    let query = query.to_lowercase();
    entries
        .iter()
        .filter(|entry| {
            query.is_empty()
                || entry.kind.to_lowercase().contains(&query)
                || entry.name.to_lowercase().contains(&query)
                || entry.description.to_lowercase().contains(&query)
                || entry
                    .state
                    .is_some_and(|state| state.as_str().contains(&query))
        })
        .collect()
}

fn mcp_state(config: &McpServerConfig, warnings: &[String]) -> McpState {
    if config.trust == McpTrust::Block || config.auth == McpAuth::OAuth2 {
        return McpState::DiscoverOnly;
    }
    let warning_prefix = format!("mcp server \"{}\"", config.name);
    if warnings
        .iter()
        .any(|warning| warning.contains(&warning_prefix))
    {
        return McpState::Stale;
    }
    match config.auth {
        McpAuth::ApiKey { .. } => McpState::AuthOk,
        McpAuth::None => McpState::Enabled,
        McpAuth::OAuth2 => McpState::DiscoverOnly,
    }
}

fn builtin_palette_entries() -> Vec<PaletteEntry> {
    let commands = [
        ("quit (q)", "quit Nosis Harness", PaletteAction::Quit),
        (
            "trust-dial (t)",
            "view session autonomy and policy rules",
            PaletteAction::TrustDial,
        ),
        (
            "timeline (l)",
            "view session receipts and answers",
            PaletteAction::Timeline,
        ),
        (
            "palette (?)",
            "find commands and tools",
            PaletteAction::Palette,
        ),
        (
            "scroll up (PageUp)",
            "scroll the transcript up",
            PaletteAction::ScrollUp,
        ),
        (
            "scroll down (PageDown)",
            "scroll the transcript down",
            PaletteAction::ScrollDown,
        ),
        (
            "scroll latest (End)",
            "return to the newest transcript line",
            PaletteAction::ScrollEnd,
        ),
        (
            "timeline restore (R)",
            RESTORE_DEFERRED,
            PaletteAction::Reserved,
        ),
    ];
    let mut entries: Vec<PaletteEntry> = commands
        .into_iter()
        .map(|(name, description, action)| PaletteEntry {
            kind: "command",
            name: name.into(),
            description: description.into(),
            state: None,
            action,
        })
        .collect();
    entries.extend(builtin_tools().into_iter().map(|tool| {
        let spec = tool.spec();
        PaletteEntry {
            kind: "tool",
            name: spec.name,
            description: spec.description,
            state: None,
            action: PaletteAction::Describe,
        }
    }));
    entries
}

fn trust_dial_lines(view: &PolicyView) -> Vec<String> {
    let autonomy = match view.autonomy {
        Autonomy::Ask => "ask",
        Autonomy::Auto => "auto",
    };
    let mut lines = vec![format!("session autonomy: {autonomy}")];
    append_rules(&mut lines, "auto-approve", &view.auto_paths);
    append_rules(&mut lines, "always-ask", &view.ask_paths);
    append_rules(&mut lines, "hard-block/protected", &view.block_paths);
    append_rules(&mut lines, "blocked command", &view.block_commands);
    lines
}

fn append_rules(lines: &mut Vec<String>, label: &str, rules: &[String]) {
    if rules.is_empty() {
        lines.push(format!("{label}: none"));
    } else {
        lines.extend(rules.iter().map(|rule| format!("{label}: {rule}")));
    }
}

impl PaletteEntry {
    fn line(&self) -> String {
        match self.state {
            Some(state) => format!(
                "{}: {} — {} [{}]",
                self.kind,
                self.name,
                self.description,
                state.as_str()
            ),
            None => format!("{}: {} — {}", self.kind, self.name, self.description),
        }
    }
}

fn short_text(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let head: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{head}…")
    } else {
        head
    }
}

fn outcome_name(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::Pass => "pass",
        Outcome::Fail => "fail",
        Outcome::Partial => "partial",
        Outcome::Skip => "skip",
        Outcome::Timeout => "timeout",
    }
}

fn failure_class_name(class: FailureClass) -> &'static str {
    match class {
        FailureClass::Context => "context",
        FailureClass::Constraint => "constraint",
        FailureClass::Verification => "verification",
        FailureClass::Planning => "planning",
    }
}

fn is_compaction_progress(line: &str) -> bool {
    let line = line.to_ascii_lowercase();
    line.contains("context ") && line.contains('%') && line.contains("compacted")
}

fn timeline_row(entry: &TimelineEntry) -> String {
    let (input, output, cached) = entry.tokens();
    let compacted = if entry.compacted { "  [compact]" } else { "" };
    format!(
        "#{}  {}  {input}/{output}/{cached}{compacted}",
        entry.turn,
        outcome_name(entry.outcome)
    )
}

fn timeline_detail_lines(entry: &TimelineEntry) -> Vec<String> {
    let (input, output, cached) = entry.tokens();
    let failure = entry
        .failure_class
        .map(failure_class_name)
        .unwrap_or("none");
    vec![
        format!("TURN #{}", entry.turn),
        format!("timestamp: {}", entry.ts_utc),
        format!("model: {}", entry.model_id),
        format!("task: {}", entry.task),
        format!("outcome: {}", outcome_name(entry.outcome)),
        format!("agent turns: {}", entry.turns),
        format!("tool calls: {}", entry.tool_calls),
        format!("failure class: {failure}"),
        format!("tokens: {input} in / {output} out / {cached} cached"),
        format!("compacted: {}", if entry.compacted { "yes" } else { "no" }),
        String::new(),
        format!("answer: {}", entry.answer),
    ]
}

/// Fold one worker event into application state.
pub fn apply_event(app: &mut App, event: AgentEvent) -> &Status {
    match event {
        AgentEvent::Progress(line) => {
            if is_compaction_progress(&line) {
                app.current_task_compacted = true;
            }
            app.push_line(&format!("  {line}"), TranscriptKind::Progress);
        }
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
        AgentEvent::TaskReceipt(summary) => {
            let turn = app.timeline.len().saturating_add(1);
            let compacted = std::mem::take(&mut app.current_task_compacted);
            app.timeline.push(TimelineEntry::from_receipt(
                turn,
                summary.receipt,
                summary.answer,
                compacted,
            ));
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

/// Build the short, scrubbed Telegram body for a state that needs attention.
pub fn notify_message(status: &Status, scrubber: &Scrubber) -> Option<String> {
    let raw = match status {
        Status::Waiting => "nosis: waiting on your approval".to_owned(),
        Status::Blocked(reason) => format!("nosis: blocked — {reason}"),
        Status::Idle | Status::Working => return None,
    };
    let safe = nh_vault::safe_line(scrubber, &raw);
    Some(short_text(&safe, MAX_NOTIFY_CHARS.saturating_sub(1)))
}

trait NotifySender: Send + Sync {
    fn send(&self, telegram: &TelegramNotifyConfig, body: &str) -> anyhow::Result<()>;
}

struct TelegramSender;

impl NotifySender for TelegramSender {
    fn send(&self, telegram: &TelegramNotifyConfig, body: &str) -> anyhow::Result<()> {
        let vault = EnvFallbackVault {
            inner: KeyringVault,
        };
        let token = vault
            .get("telegram")
            .map_err(|_| anyhow::anyhow!("telegram notify failed"))?;
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(TELEGRAM_CONNECT_TIMEOUT)
            .timeout(TELEGRAM_REQUEST_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| anyhow::anyhow!("telegram notify failed"))?;
        let response = client
            .post(format!(
                "https://api.telegram.org/bot{}/sendMessage",
                token.as_str()
            ))
            .form(&[("chat_id", telegram.chat_id.as_str()), ("text", body)])
            .send()
            .map_err(|_| anyhow::anyhow!("telegram notify failed"))?;
        if !response.status().is_success() {
            anyhow::bail!("telegram notify failed");
        }
        Ok(())
    }
}

struct Notifier {
    config: NotifyConfig,
    sender: Arc<dyn NotifySender>,
    failures: Receiver<()>,
    failure_tx: Sender<()>,
}

impl Notifier {
    fn new(config: NotifyConfig, sender: Arc<dyn NotifySender>) -> Self {
        let (failure_tx, failures) = mpsc::channel();
        Self {
            config,
            sender,
            failures,
            failure_tx,
        }
    }

    fn notify(&self, status: &Status, scrubber: &SharedScrubber) {
        if !self.config.telegram_enabled() {
            return;
        }
        let Some(telegram) = self.config.telegram.clone() else {
            return;
        };
        let body = match scrubber.read() {
            Ok(guard) => notify_message(status, &guard),
            Err(poisoned) => notify_message(status, &poisoned.into_inner()),
        };
        let Some(body) = body else {
            return;
        };
        let sender = Arc::clone(&self.sender);
        let failure_tx = self.failure_tx.clone();
        if thread::Builder::new()
            .name("nh-telegram".into())
            .spawn(move || {
                if sender.send(&telegram, &body).is_err() {
                    let _ = failure_tx.send(());
                }
            })
            .is_err()
        {
            let _ = self.failure_tx.send(());
        }
    }
}

fn drain_notify_failures(app: &mut App, notifier: &Notifier) {
    while notifier.failures.try_recv().is_ok() {
        app.push_line("telegram notify failed", TranscriptKind::Progress);
    }
}

fn entered_notify_state(previous: &Status, current: &Status) -> bool {
    (matches!(current, Status::Waiting) && !matches!(previous, Status::Waiting))
        || (matches!(current, Status::Blocked(_)) && !matches!(previous, Status::Blocked(_)))
}

/// Run the full-screen TUI until the user quits.
pub fn run(config: TuiConfig) -> anyhow::Result<()> {
    let route = config.resolver.resolve(&config.model_id)?;
    if route.class == RouteClass::Delegate {
        anyhow::bail!("delegate routes arrive in M4 — pick an api route");
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
    let policy_view = config.law.policy.view();
    let mut app = App::new(
        route.clone(),
        config.budget,
        Arc::clone(&scrubber),
        policy_view,
        config.palette_entries,
    );
    let notifier = Notifier::new(config.notify, Arc::new(TelegramSender));
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
    let result = ui_loop(&mut terminal, &mut app, &mut worker, &notifier);
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
                    let reason = safe_line(&scrubber, &error.to_string());
                    let _ = events.send(AgentEvent::Failed(reason.clone()));
                    let _ = events.send(AgentEvent::TaskReceipt(failed_timeline_summary(
                        &route.model_id,
                        &task,
                        &reason,
                    )));
                    continue;
                }
            }
        }
        match agent.run_with_history(&mut history, &task) {
            Ok((answer, receipt)) => {
                if let Some(usage) = &receipt.usage {
                    add_usage(&mut session_usage, usage);
                    let _ = events.send(AgentEvent::Usage(session_usage.clone()));
                }
                let _ = events.send(AgentEvent::Answer(answer.clone()));
                let _ = events.send(AgentEvent::TaskReceipt(TimelineSummary { receipt, answer }));
            }
            Err(error) => {
                let reason = safe_line(&scrubber, &error.to_string());
                let _ = events.send(AgentEvent::Failed(reason.clone()));
                let _ = events.send(AgentEvent::TaskReceipt(failed_timeline_summary(
                    &route.model_id,
                    &task,
                    &reason,
                )));
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

fn failed_timeline_summary(model_id: &str, task: &str, reason: &str) -> TimelineSummary {
    TimelineSummary {
        receipt: Receipt {
            ts_utc: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            model_id: model_id.to_owned(),
            task: task.to_owned(),
            turns: 0,
            tool_calls: 0,
            outcome: Outcome::Fail,
            failure_class: Some(FailureClass::Verification),
            usage: None,
        },
        answer: format!("error: {reason}"),
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
    notifier: &Notifier,
) -> anyhow::Result<()> {
    loop {
        loop {
            match worker.events.try_recv() {
                Ok(agent_event) => {
                    let previous = app.status.clone();
                    let ring = matches!(agent_event, AgentEvent::Approval(_))
                        && !matches!(app.status, Status::Waiting);
                    apply_event(app, agent_event);
                    if ring {
                        ring_bell();
                    }
                    if entered_notify_state(&previous, &app.status) {
                        notifier.notify(&app.status, &app.scrubber);
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    return Err(anyhow::anyhow!("agent stopped — retry the task"));
                }
            }
        }
        drain_notify_failures(app, notifier);

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

#[derive(Debug, PartialEq, Eq)]
enum UiAction {
    None,
    Dispatch(String),
    Quit,
}

fn handle_key(app: &mut App, worker: &mut Worker, key: KeyEvent) -> bool {
    match reduce_key(app, key) {
        UiAction::None => false,
        UiAction::Quit => true,
        UiAction::Dispatch(task) => {
            if worker.commands.send(WorkerCommand::Task(task)).is_err() {
                apply_event(
                    app,
                    AgentEvent::Failed("agent stopped — retry the task".into()),
                );
            }
            false
        }
    }
}

fn reduce_key(app: &mut App, key: KeyEvent) -> UiAction {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        if matches!(app.status, Status::Waiting) {
            app.answer_approval(false);
        }
        return UiAction::Quit;
    }
    if app.overlay != Overlay::None {
        return reduce_overlay_key(app, key);
    }
    if matches!(app.status, Status::Waiting) {
        app.answer_approval(matches!(key.code, KeyCode::Char('y' | 'Y')));
        return UiAction::None;
    }
    if matches!(app.status, Status::Working) {
        return UiAction::None;
    }
    match key.code {
        KeyCode::Char('q') if app.input.is_empty() => return UiAction::Quit,
        KeyCode::Char('t') if app.input.is_empty() => app.overlay = Overlay::TrustDial,
        KeyCode::Char('l') if app.input.is_empty() => {
            app.overlay = Overlay::Timeline {
                selected: app.timeline.len().saturating_sub(1),
                inspecting: false,
                note: None,
            };
        }
        KeyCode::Char('?') if app.input.is_empty() => {
            app.overlay = Overlay::Palette {
                filter: String::new(),
                selected: 0,
                detail: None,
            };
        }
        KeyCode::Char('R') if app.input.is_empty() => {}
        KeyCode::Enter => {
            if let Some(task) = app.dispatch() {
                return UiAction::Dispatch(task);
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
    UiAction::None
}

fn reduce_overlay_key(app: &mut App, key: KeyEvent) -> UiAction {
    if key.code == KeyCode::Esc {
        app.overlay = Overlay::None;
        return UiAction::None;
    }
    if matches!(app.overlay, Overlay::TrustDial) {
        if key.code == KeyCode::Char('t') {
            app.overlay = Overlay::None;
        }
        return UiAction::None;
    }

    let timeline_len = app.timeline.len();
    if let Overlay::Timeline {
        selected,
        inspecting,
        note,
    } = &mut app.overlay
    {
        timeline_key(timeline_len, selected, inspecting, note, key);
        return UiAction::None;
    }

    let activated = match &mut app.overlay {
        Overlay::Palette {
            filter,
            selected,
            detail,
        } => palette_key(&app.palette_entries, filter, selected, detail, key),
        Overlay::None | Overlay::TrustDial | Overlay::Timeline { .. } => None,
    };
    let Some(entry) = activated else {
        return UiAction::None;
    };
    activate_palette_entry(app, entry)
}

fn timeline_key(
    entry_count: usize,
    selected: &mut usize,
    inspecting: &mut bool,
    note: &mut Option<String>,
    key: KeyEvent,
) {
    match key.code {
        KeyCode::Up => {
            *selected = selected.saturating_sub(1);
            *inspecting = false;
            *note = None;
        }
        KeyCode::Down => {
            if entry_count > 0 {
                *selected = selected.saturating_add(1).min(entry_count - 1);
            }
            *inspecting = false;
            *note = None;
        }
        KeyCode::Enter if entry_count > 0 => {
            *selected = (*selected).min(entry_count - 1);
            *inspecting = true;
            *note = None;
        }
        KeyCode::Char('R') => {
            *note = Some(RESTORE_DEFERRED.into());
        }
        _ => {}
    }
}

fn palette_key(
    entries: &[PaletteEntry],
    filter: &mut String,
    selected: &mut usize,
    detail: &mut Option<String>,
    key: KeyEvent,
) -> Option<PaletteEntry> {
    match key.code {
        KeyCode::Backspace => {
            filter.pop();
            *selected = 0;
            *detail = None;
        }
        KeyCode::Up => {
            *selected = selected.saturating_sub(1);
            *detail = None;
        }
        KeyCode::Down => {
            let count = filter_palette(entries, filter).len();
            if count > 0 {
                *selected = selected.saturating_add(1).min(count - 1);
            }
            *detail = None;
        }
        KeyCode::Enter => {
            return filter_palette(entries, filter)
                .get(*selected)
                .map(|entry| (*entry).clone());
        }
        KeyCode::Char(character)
            if !character.is_control()
                && !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            filter.push(character);
            *selected = 0;
            *detail = None;
        }
        _ => {}
    }
    None
}

fn activate_palette_entry(app: &mut App, entry: PaletteEntry) -> UiAction {
    match entry.action {
        PaletteAction::Quit => UiAction::Quit,
        PaletteAction::TrustDial => {
            app.overlay = Overlay::TrustDial;
            UiAction::None
        }
        PaletteAction::Timeline => {
            app.overlay = Overlay::Timeline {
                selected: app.timeline.len().saturating_sub(1),
                inspecting: false,
                note: None,
            };
            UiAction::None
        }
        PaletteAction::Palette => {
            app.overlay = Overlay::Palette {
                filter: String::new(),
                selected: 0,
                detail: None,
            };
            UiAction::None
        }
        PaletteAction::ScrollUp => {
            app.scroll_back = app.scroll_back.saturating_add(5);
            app.overlay = Overlay::None;
            UiAction::None
        }
        PaletteAction::ScrollDown => {
            app.scroll_back = app.scroll_back.saturating_sub(5);
            app.overlay = Overlay::None;
            UiAction::None
        }
        PaletteAction::ScrollEnd => {
            app.scroll_back = 0;
            app.overlay = Overlay::None;
            UiAction::None
        }
        PaletteAction::Reserved | PaletteAction::Describe => {
            if let Overlay::Palette { detail, .. } = &mut app.overlay {
                *detail = Some(entry.description);
            }
            UiAction::None
        }
    }
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
    render_overlay(frame, app);
}

fn render_overlay(frame: &mut Frame<'_>, app: &App) {
    let area = inset(frame.area());
    match &app.overlay {
        Overlay::None => {}
        Overlay::TrustDial => render_trust_dial(frame, app, area),
        Overlay::Timeline {
            selected,
            inspecting,
            note,
        } => render_timeline(frame, app, area, *selected, *inspecting, note.as_deref()),
        Overlay::Palette {
            filter,
            selected,
            detail,
        } => render_palette(frame, app, area, filter, *selected, detail.as_deref()),
    }
}

fn render_trust_dial(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let mut raw = vec![
        "TRUST DIAL — read-only".to_owned(),
        "t or Esc: close".to_owned(),
        String::new(),
    ];
    raw.extend(trust_dial_lines(&app.policy_view));
    let lines: Vec<Line<'static>> = raw
        .into_iter()
        .map(|line| Line::from(safe_line(&app.scrubber, &line)))
        .collect();
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().fg(Color::White).bg(Color::Black)),
        area,
    );
}

fn render_timeline(
    frame: &mut Frame<'_>,
    app: &App,
    area: Rect,
    selected: usize,
    inspecting: bool,
    note: Option<&str>,
) {
    frame.render_widget(Clear, area);
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(area);
    let visible_rows = usize::from(columns[0].height.saturating_sub(3).max(1));
    let start = selected.saturating_add(1).saturating_sub(visible_rows);
    let mut rail = vec![
        Line::from(safe_line(&app.scrubber, "TIMELINE — newest at bottom")),
        Line::from(safe_line(
            &app.scrubber,
            "Up/Down: move | Enter: inspect | R: restore | Esc: close",
        )),
        Line::from(safe_line(&app.scrubber, "")),
    ];
    if app.timeline.is_empty() {
        rail.push(Line::from(safe_line(&app.scrubber, "no completed turns")));
    } else {
        rail.extend(
            app.timeline
                .iter()
                .enumerate()
                .skip(start)
                .take(visible_rows)
                .map(|(index, entry)| {
                    let marker = if index == selected { "> " } else { "  " };
                    let line =
                        safe_line(&app.scrubber, &format!("{marker}{}", timeline_row(entry)));
                    let style = if index == selected {
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    Line::from(Span::styled(line, style))
                }),
        );
    }

    let mut detail = Vec::new();
    if let Some(note) = note {
        detail.push(Line::from(Span::styled(
            safe_line(&app.scrubber, note),
            Style::default().fg(Color::DarkGray),
        )));
        detail.push(Line::from(safe_line(&app.scrubber, "")));
    }
    if let Some(entry) = app.timeline.get(selected) {
        let raw = if inspecting {
            timeline_detail_lines(entry)
        } else {
            vec![
                format!("selected: #{}", entry.turn),
                format!("task: {}", entry.task),
                "press Enter to inspect".into(),
            ]
        };
        detail.extend(
            raw.into_iter()
                .map(|line| Line::from(safe_line(&app.scrubber, &line))),
        );
    }

    let panel_style = Style::default().fg(Color::White).bg(Color::Black);
    frame.render_widget(Paragraph::new(rail).style(panel_style), columns[0]);
    frame.render_widget(Paragraph::new(detail).style(panel_style), columns[1]);
}

fn render_palette(
    frame: &mut Frame<'_>,
    app: &App,
    area: Rect,
    filter: &str,
    selected: usize,
    detail: Option<&str>,
) {
    let filtered = filter_palette(&app.palette_entries, filter);
    let visible_rows = usize::from(area.height.saturating_sub(5).max(1));
    let start = selected.saturating_add(1).saturating_sub(visible_rows);
    let mut lines = vec![
        Line::from(safe_line(&app.scrubber, "COMMANDS + TOOLS")),
        Line::from(safe_line(&app.scrubber, &format!("filter: {filter}"))),
        Line::from(safe_line(
            &app.scrubber,
            "Up/Down: move | Enter: select | Esc: close",
        )),
        Line::from(safe_line(&app.scrubber, "")),
    ];
    if filtered.is_empty() {
        lines.push(Line::from(safe_line(&app.scrubber, "no matches")));
    } else {
        lines.extend(
            filtered
                .iter()
                .enumerate()
                .skip(start)
                .take(visible_rows)
                .map(|(index, entry)| {
                    let marker = if index == selected { "> " } else { "  " };
                    let line = safe_line(&app.scrubber, &format!("{marker}{}", entry.line()));
                    let style = if index == selected {
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    Line::from(Span::styled(line, style))
                }),
        );
    }
    if let Some(detail) = detail {
        lines.push(Line::from(safe_line(
            &app.scrubber,
            &format!("selected: {detail}"),
        )));
    }
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().fg(Color::White).bg(Color::Black)),
        area,
    );
}

fn inset(area: Rect) -> Rect {
    let horizontal = u16::from(area.width > 4) * 2;
    let vertical = u16::from(area.height > 2);
    Rect::new(
        area.x.saturating_add(horizontal),
        area.y.saturating_add(vertical),
        area.width.saturating_sub(horizontal.saturating_mul(2)),
        area.height.saturating_sub(vertical.saturating_mul(2)),
    )
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
    use ratatui::backend::TestBackend;
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
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
            PolicyView {
                autonomy: Autonomy::Ask,
                auto_paths: Vec::new(),
                ask_paths: Vec::new(),
                block_paths: Vec::new(),
                block_commands: Vec::new(),
            },
            Vec::new(),
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

    fn receipt(task: &str, outcome: Outcome, usage: Option<Usage>) -> Receipt {
        Receipt {
            ts_utc: "2026-07-14T12:00:00Z".into(),
            model_id: "test-route".into(),
            task: task.into(),
            turns: 3,
            tool_calls: 2,
            outcome,
            failure_class: (outcome != Outcome::Pass).then_some(FailureClass::Constraint),
            usage,
        }
    }

    fn timeline_event(task: &str, answer: &str) -> AgentEvent {
        AgentEvent::TaskReceipt(TimelineSummary {
            receipt: receipt(
                task,
                Outcome::Pass,
                Some(Usage {
                    prompt_tokens: 10,
                    completion_tokens: 2,
                    cached_tokens: Some(4),
                }),
            ),
            answer: answer.into(),
        })
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
    fn timeline_entry_projects_receipt_outcome_and_tokens() {
        let entry = TimelineEntry::from_receipt(
            7,
            receipt(
                "summarize the workspace",
                Outcome::Partial,
                Some(Usage {
                    prompt_tokens: 120,
                    completion_tokens: 30,
                    cached_tokens: Some(50),
                }),
            ),
            "partial answer".into(),
            false,
        );

        assert_eq!(entry.turn, 7);
        assert_eq!(entry.outcome, Outcome::Partial);
        assert_eq!(entry.tokens(), (120, 30, 50));
        assert_eq!(entry.turns, 3);
        assert_eq!(entry.tool_calls, 2);
        assert_eq!(entry.answer, "partial answer");
        assert!(!entry.compacted);
    }

    #[test]
    fn compaction_progress_marks_only_the_current_timeline_turn() {
        let mut app = test_app(None);
        app.status = Status::Working;
        apply_event(
            &mut app,
            AgentEvent::Progress("context 73% — compacted 8 earlier messages".into()),
        );
        apply_event(&mut app, timeline_event("first", "one"));
        assert!(app.timeline[0].compacted);

        apply_event(&mut app, AgentEvent::Answer("one".into()));
        app.input = "second".into();
        assert_eq!(app.dispatch().as_deref(), Some("second"));
        apply_event(&mut app, timeline_event("second", "two"));
        assert!(!app.timeline[1].compacted);
    }

    #[test]
    fn timeline_reducer_scrubs_and_enter_inspects_the_selected_turn() {
        let mut app = test_app(None);
        apply_event(&mut app, timeline_event("first", "answer one"));
        apply_event(&mut app, timeline_event("second", "answer two"));

        assert_eq!(reduce_key(&mut app, char_key('l')), UiAction::None);
        match &app.overlay {
            Overlay::Timeline { selected, .. } => assert_eq!(*selected, 1),
            _ => panic!("timeline must open"),
        }
        reduce_key(&mut app, code_key(KeyCode::Up));
        reduce_key(&mut app, code_key(KeyCode::Enter));
        match &app.overlay {
            Overlay::Timeline {
                selected,
                inspecting,
                ..
            } => {
                assert_eq!(*selected, 0);
                assert!(*inspecting);
                assert_eq!(app.timeline[*selected].answer, "answer one");
            }
            _ => panic!("timeline must stay open"),
        }
        assert_eq!(app.timeline.len(), 2);
        assert!(app.input.is_empty());
    }

    #[test]
    fn timeline_restore_key_only_shows_the_deferral_note() {
        let mut app = test_app(None);
        apply_event(&mut app, timeline_event("first", "answer"));
        reduce_key(&mut app, char_key('l'));
        let before_task = app.timeline[0].task.clone();

        assert_eq!(reduce_key(&mut app, char_key('R')), UiAction::None);

        match &app.overlay {
            Overlay::Timeline { note, .. } => {
                assert_eq!(note.as_deref(), Some(RESTORE_DEFERRED));
            }
            _ => panic!("timeline must stay open"),
        }
        assert_eq!(app.timeline.len(), 1);
        assert_eq!(app.timeline[0].task, before_task);
        assert!(app.transcript.is_empty());
    }

    #[test]
    fn palette_filter_is_pure_and_finds_exec_shell() {
        let entries = builtin_palette_entries();
        let before = entries.clone();
        let filtered = filter_palette(&entries, "ex");

        assert!(
            filtered.iter().any(|entry| entry.name == "exec_shell"),
            "got: {:?}",
            filtered
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>()
        );
        assert_eq!(entries, before);
        assert!(filter_palette(&entries, "timeline")
            .iter()
            .any(|entry| entry.name == "timeline (l)"));
    }

    #[test]
    fn mcp_palette_state_uses_auth_trust_and_warnings() {
        let configs = vec![
            mcp_config("plain", McpAuth::None, McpTrust::Ask),
            mcp_config(
                "keyed",
                McpAuth::ApiKey {
                    vault_entry: "keyed".into(),
                },
                McpTrust::Auto,
            ),
            mcp_config("down", McpAuth::None, McpTrust::Ask),
            mcp_config("blocked", McpAuth::None, McpTrust::Block),
        ];
        let toolset = McpToolset {
            tools: Vec::new(),
            warnings: vec!["mcp server \"down\": connection refused".into()],
        };

        let entries = mcp_palette_entries(&configs, &toolset);
        let state = |name: &str| {
            entries
                .iter()
                .find(|entry| entry.name == name)
                .and_then(|entry| entry.state)
        };
        assert_eq!(state("plain"), Some(McpState::Enabled));
        assert_eq!(state("keyed"), Some(McpState::AuthOk));
        assert_eq!(state("down"), Some(McpState::Stale));
        assert_eq!(state("blocked"), Some(McpState::DiscoverOnly));
    }

    #[test]
    fn empty_and_broken_mcp_config_project_without_panicking() {
        let empty = McpToolset {
            tools: Vec::new(),
            warnings: Vec::new(),
        };
        assert!(mcp_palette_entries(&[], &empty).is_empty());

        let broken = McpToolset {
            tools: Vec::new(),
            warnings: vec!["could not parse .nosis/mcp.toml".into()],
        };
        let entries = mcp_palette_entries(&[], &broken);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].state, Some(McpState::Stale));
    }

    #[test]
    fn trust_dial_uses_plain_none_lines_for_empty_classes() {
        let lines = trust_dial_lines(&PolicyView {
            autonomy: Autonomy::Ask,
            auto_paths: Vec::new(),
            ask_paths: Vec::new(),
            block_paths: Vec::new(),
            block_commands: Vec::new(),
        });

        assert_eq!(
            lines,
            [
                "session autonomy: ask",
                "auto-approve: none",
                "always-ask: none",
                "hard-block/protected: none",
                "blocked command: none",
            ]
        );
        assert!(lines.iter().all(|line| !line.trim().is_empty()));
    }

    #[test]
    fn overlays_suppress_task_dispatch_and_escape_restores_base_view() {
        for opener in ['t', '?', 'l'] {
            let mut app = test_app(None);
            assert_eq!(reduce_key(&mut app, char_key(opener)), UiAction::None);
            assert_ne!(app.overlay, Overlay::None);

            for character in "work".chars() {
                assert_eq!(reduce_key(&mut app, char_key(character)), UiAction::None);
            }
            assert_eq!(
                reduce_key(&mut app, code_key(KeyCode::Enter)),
                UiAction::None
            );
            assert!(app.input.is_empty());
            assert!(app.transcript.is_empty());
            assert_eq!(app.status, Status::Idle);

            assert_eq!(reduce_key(&mut app, code_key(KeyCode::Esc)), UiAction::None);
            assert_eq!(app.overlay, Overlay::None);
        }
    }

    #[test]
    fn palette_enter_runs_commands_and_describes_tools() {
        let mut app = test_app(None);
        reduce_key(&mut app, char_key('?'));
        for character in "trust-dial".chars() {
            reduce_key(&mut app, char_key(character));
        }
        assert_eq!(
            reduce_key(&mut app, code_key(KeyCode::Enter)),
            UiAction::None
        );
        assert_eq!(app.overlay, Overlay::TrustDial);

        let mut app = test_app(None);
        reduce_key(&mut app, char_key('?'));
        for character in "quit".chars() {
            reduce_key(&mut app, char_key(character));
        }
        assert_eq!(
            reduce_key(&mut app, code_key(KeyCode::Enter)),
            UiAction::Quit
        );

        let mut app = test_app(None);
        reduce_key(&mut app, char_key('?'));
        for character in "exec_shell".chars() {
            reduce_key(&mut app, char_key(character));
        }
        assert_eq!(
            reduce_key(&mut app, code_key(KeyCode::Enter)),
            UiAction::None
        );
        match &app.overlay {
            Overlay::Palette { detail, .. } => {
                assert!(detail.as_deref().is_some_and(|line| !line.is_empty()));
            }
            _ => panic!("tool selection must keep the palette open"),
        }
    }

    fn mcp_config(name: &str, auth: McpAuth, trust: McpTrust) -> McpServerConfig {
        McpServerConfig {
            name: name.into(),
            url: "https://example.invalid/mcp".into(),
            spec: "2026-07-28".into(),
            auth,
            scopes: Vec::new(),
            default_mode: None,
            trust,
        }
    }

    fn char_key(character: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE)
    }

    fn code_key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
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
    fn notify_message_is_short_scrubbed_and_control_safe() {
        let secret = "fake-telegram-secret";
        let scrubber = Scrubber::new(vec![secret.into()]);
        let waiting = notify_message(&Status::Waiting, &scrubber).unwrap();
        assert_eq!(waiting, "nosis: waiting on your approval");

        let blocked = notify_message(
            &Status::Blocked(format!("reason={secret}\r\x1b[2K {}", "x".repeat(300))),
            &scrubber,
        )
        .unwrap();
        assert!(blocked.contains("[REDACTED]"), "got: {blocked}");
        assert!(!blocked.contains(secret), "got: {blocked}");
        assert!(!blocked.chars().any(char::is_control), "got: {blocked}");
        assert!(
            blocked.chars().count() <= MAX_NOTIFY_CHARS,
            "got: {blocked}"
        );
        assert!(notify_message(&Status::Idle, &scrubber).is_none());
    }

    #[test]
    fn notify_transition_fires_once_when_waiting_or_blocked_is_entered() {
        assert!(entered_notify_state(&Status::Working, &Status::Waiting));
        assert!(entered_notify_state(
            &Status::Working,
            &Status::Blocked("offline".into())
        ));
        assert!(!entered_notify_state(&Status::Waiting, &Status::Waiting));
        assert!(!entered_notify_state(
            &Status::Blocked("first".into()),
            &Status::Blocked("second".into())
        ));
        assert!(!entered_notify_state(&Status::Working, &Status::Idle));
    }

    struct MockNotifySender {
        calls: Arc<AtomicUsize>,
        attempted: Sender<()>,
        fail: bool,
    }

    impl NotifySender for MockNotifySender {
        fn send(&self, _telegram: &TelegramNotifyConfig, _body: &str) -> anyhow::Result<()> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let _ = self.attempted.send(());
            if self.fail {
                anyhow::bail!("injected notify failure");
            }
            Ok(())
        }
    }

    fn enabled_notify_config() -> NotifyConfig {
        NotifyConfig {
            telegram: Some(TelegramNotifyConfig {
                enabled: true,
                chat_id: "123456789".into(),
            }),
        }
    }

    #[test]
    fn disabled_notify_config_makes_no_send_attempt() {
        let calls = Arc::new(AtomicUsize::new(0));
        let (attempted, attempts) = mpsc::channel();
        let notifier = Notifier::new(
            NotifyConfig::default(),
            Arc::new(MockNotifySender {
                calls: Arc::clone(&calls),
                attempted,
                fail: false,
            }),
        );
        let scrubber = Arc::new(RwLock::new(Scrubber::new(Vec::new())));

        notifier.notify(&Status::Waiting, &scrubber);

        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(attempts.try_recv().is_err());
        assert!(notifier.failures.try_recv().is_err());
    }

    #[test]
    fn enabled_notify_uses_injected_sender_without_network() {
        let calls = Arc::new(AtomicUsize::new(0));
        let (attempted, attempts) = mpsc::channel();
        let notifier = Notifier::new(
            enabled_notify_config(),
            Arc::new(MockNotifySender {
                calls: Arc::clone(&calls),
                attempted,
                fail: false,
            }),
        );
        let scrubber = Arc::new(RwLock::new(Scrubber::new(Vec::new())));

        notifier.notify(&Status::Waiting, &scrubber);

        attempts.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(notifier.failures.try_recv().is_err());
    }

    #[test]
    fn failing_notify_adds_exactly_one_dim_warning_and_session_continues() {
        let calls = Arc::new(AtomicUsize::new(0));
        let (attempted, attempts) = mpsc::channel();
        let notifier = Notifier::new(
            enabled_notify_config(),
            Arc::new(MockNotifySender {
                calls: Arc::clone(&calls),
                attempted,
                fail: true,
            }),
        );
        let mut app = test_app(None);
        app.status = Status::Blocked("offline".into());

        notifier.notify(&app.status, &app.scrubber);
        attempts.recv_timeout(Duration::from_secs(1)).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        while app.transcript.is_empty() && std::time::Instant::now() < deadline {
            drain_notify_failures(&mut app, &notifier);
            thread::yield_now();
        }
        drain_notify_failures(&mut app, &notifier);

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(app.transcript.len(), 1);
        assert_eq!(app.transcript[0].text, "telegram notify failed");
        assert!(matches!(app.transcript[0].kind, TranscriptKind::Progress));
        assert_eq!(app.status, Status::Blocked("offline".into()));
    }

    #[test]
    fn rendered_line_is_redacted_and_has_no_control_characters() {
        let secret = "hunter2-fake-tui-secret";
        let scrubber = Arc::new(RwLock::new(Scrubber::new(vec![secret.into()])));
        let mut app = App::new(
            test_route(),
            None,
            scrubber,
            PolicyView {
                autonomy: Autonomy::Ask,
                auto_paths: Vec::new(),
                ask_paths: Vec::new(),
                block_paths: Vec::new(),
                block_commands: Vec::new(),
            },
            Vec::new(),
        );
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
    fn rendered_overlay_scrubs_descriptions_and_control_characters() {
        let secret = "hunter2-fake-overlay-secret";
        let scrubber = Arc::new(RwLock::new(Scrubber::new(vec![secret.into()])));
        let mut app = App::new(
            test_route(),
            None,
            scrubber,
            PolicyView {
                autonomy: Autonomy::Ask,
                auto_paths: Vec::new(),
                ask_paths: Vec::new(),
                block_paths: Vec::new(),
                block_commands: Vec::new(),
            },
            vec![PaletteEntry {
                kind: "tool",
                name: "secret-tool".into(),
                description: format!("value={secret}\r\x1b[2K"),
                state: Some(McpState::Enabled),
                action: PaletteAction::Describe,
            }],
        );
        app.overlay = Overlay::Palette {
            filter: "secret-tool".into(),
            selected: 0,
            detail: None,
        };
        let backend = TestBackend::new(100, 12);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|frame| render(frame, &app)).unwrap();
        let rendered: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();

        assert!(rendered.contains("[REDACTED]"), "got: {rendered}");
        assert!(!rendered.contains(secret), "got: {rendered}");
        assert!(!rendered.contains('\r'), "got: {rendered}");
        assert!(!rendered.contains('\x1b'), "got: {rendered}");
    }

    #[test]
    fn rendered_timeline_scrubs_every_receipt_and_answer_line() {
        let secret = "sk-timeline-00000000";
        let mut app = test_app(None);
        apply_event(
            &mut app,
            AgentEvent::TaskReceipt(TimelineSummary {
                receipt: receipt(
                    &format!("task value={secret}\r\x1b[2K"),
                    Outcome::Pass,
                    None,
                ),
                answer: format!("answer value={secret}\r\x1b[2K"),
            }),
        );
        app.overlay = Overlay::Timeline {
            selected: 0,
            inspecting: true,
            note: None,
        };
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|frame| render(frame, &app)).unwrap();
        let rendered: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();

        assert!(rendered.contains("[REDACTED]"), "got: {rendered}");
        assert!(!rendered.contains(secret), "got: {rendered}");
        assert!(!rendered.contains('\r'), "got: {rendered}");
        assert!(!rendered.contains('\x1b'), "got: {rendered}");
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
            let mut saw_answer = false;
            let mut saw_receipt = false;
            while !saw_answer || !saw_receipt {
                match worker.events.recv_timeout(Duration::from_secs(2)).unwrap() {
                    AgentEvent::Answer(answer) => {
                        assert_eq!(answer, "ok");
                        saw_answer = true;
                    }
                    AgentEvent::TaskReceipt(summary) => {
                        assert_eq!(summary.receipt.task, task);
                        assert_eq!(summary.receipt.outcome, Outcome::Pass);
                        assert_eq!(summary.answer, "ok");
                        saw_receipt = true;
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
        let message = "no key found for \"test\" — run `nh key add test`";
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
        match worker.events.recv_timeout(Duration::from_secs(2)).unwrap() {
            AgentEvent::TaskReceipt(summary) => {
                assert_eq!(summary.receipt.task, "hello");
                assert_eq!(summary.receipt.outcome, Outcome::Fail);
                assert!(summary.answer.starts_with("error: "));
            }
            _ => panic!("failed task must still produce one timeline receipt"),
        }
        worker.stop();
        if let Some(join) = worker.join.take() {
            join.join().unwrap();
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
