//! M3 TUI: one status, one worker, and small Windows-safe views.

use std::cell::Cell;
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
    event::{
        self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers,
    },
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
    widgets::{Block, BorderType, Borders, Clear, Padding, Paragraph, Wrap},
    Frame, Terminal,
};

type SharedScrubber = Arc<RwLock<Scrubber>>;
type ConnectFn =
    Box<dyn Fn(&ResolvedRoute) -> anyhow::Result<(Box<dyn ChatClient>, String)> + Send + Sync>;

const EVENT_POLL: Duration = Duration::from_millis(50);
const BUDGET_REASON: &str = "budget reached";
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
    Prefill(&'static str),
    Describe,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Overlay {
    None,
    CommandMenu {
        selected: usize,
    },
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

#[derive(Clone, Copy, PartialEq, Eq)]
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
    resolver: RouteResolver,
    route: ResolvedRoute,
    effort: ThinkingEffort,
    transcript: Vec<TranscriptLine>,
    pending_approval: Option<ApprovalRequest>,
    usage: Usage,
    input: String,
    budget: Option<u64>,
    scroll_back: u16,
    max_scroll: Cell<u16>,
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
        resolver: RouteResolver,
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
            effort: effort_for(route.thinking_dialect),
            resolver,
            route,
            transcript: Vec::new(),
            pending_approval: None,
            usage: Usage::default(),
            input: String::new(),
            budget,
            scroll_back: 0,
            max_scroll: Cell::new(0),
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
        self.push_line(&task, TranscriptKind::Task);
        self.status = Status::Working;
        Some(task)
    }

    fn switch_route(&mut self, route: ResolvedRoute) {
        self.effort = effort_for(route.thinking_dialect);
        self.route = route;
        self.push_line(
            &format!("switched to {} - context kept, cache resets", self.route.id),
            TranscriptKind::Progress,
        );
    }

    fn set_effort(&mut self, effort: ThinkingEffort) {
        self.effort = effort;
        self.push_line(
            &format!("reasoning effort set to {}", effort_name(effort)),
            TranscriptKind::Progress,
        );
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
            "in {} · out {} · cached {}",
            self.usage.prompt_tokens, self.usage.completion_tokens, cached
        );
        if let Some(pct) = cache_hit_pct(self.usage.prompt_tokens, cached) {
            line.push_str(&format!(" · cache {pct:.0}%"));
        }
        line.push_str(&format!(
            " · {}",
            self.route.peak_status(now, self.local_offset)
        ));
        if let Some(limit) = self.budget {
            let used = self.used_tokens();
            let pct = if limit == 0 {
                100
            } else {
                used.saturating_mul(100).checked_div(limit).unwrap_or(100)
            }
            .min(100);
            line.push_str(&format!(
                " · {} {pct}% {used}/{limit}",
                budget_bar(used, limit)
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
        (
            "/help",
            "show commands, tools, and MCP state",
            PaletteAction::Palette,
        ),
        ("/?", "alias for /help", PaletteAction::Palette),
        (
            "/trust",
            "view session autonomy and policy rules",
            PaletteAction::TrustDial,
        ),
        (
            "/timeline",
            "view session receipts and answers",
            PaletteAction::Timeline,
        ),
        (
            "/model <id>",
            "switch model route and keep context",
            PaletteAction::Prefill("/model "),
        ),
        (
            "/provider <name>",
            "switch to a provider's default route",
            PaletteAction::Prefill("/provider "),
        ),
        (
            "/effort <none|low|high|max>",
            "set reasoning effort for subsequent turns",
            PaletteAction::Prefill("/effort "),
        ),
        ("/quit", "quit Nosis Harness", PaletteAction::Quit),
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
                "{}: {} - {} [{}]",
                self.kind,
                self.name,
                self.description,
                state.as_str()
            ),
            None => format!("{}: {} - {}", self.kind, self.name, self.description),
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
            app.push_line(&line, TranscriptKind::Progress);
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
            app.push_text("", &answer, TranscriptKind::Answer);
            app.status = if app.budget_reached() {
                Status::Blocked(BUDGET_REASON.into())
            } else {
                Status::Idle
            };
        }
        AgentEvent::Failed(reason) => {
            let status_reason = safe_line(&app.scrubber, &reason);
            let what = reason
                .lines()
                .next()
                .filter(|line| !line.trim().is_empty())
                .unwrap_or("the task could not finish");
            let what = safe_line(&app.scrubber, what);
            app.push_line(
                &format!("! {what} - retry the task or type /help"),
                TranscriptKind::Error,
            );
            app.status = Status::Blocked(status_reason);
        }
    }
    &app.status
}

/// Build the short, scrubbed Telegram body for a state that needs attention.
pub fn notify_message(status: &Status, scrubber: &Scrubber) -> Option<String> {
    let raw = match status {
        Status::Waiting => "nosis: waiting on your approval".to_owned(),
        Status::Blocked(reason) => format!("nosis: blocked - {reason}"),
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
    let TuiConfig {
        resolver,
        model_id: _,
        law,
        budget,
        repo_root,
        workdir,
        palette_entries,
        notify,
    } = config;
    let scrubber = Arc::new(RwLock::new(Scrubber::new(Vec::new())));
    let initial = connect(&route);
    if let Ok((_, literal)) = &initial {
        install_literal(&scrubber, &mut Vec::new(), literal.clone());
    }
    let policy_view = law.policy.view();
    let mut app = App::new(
        resolver,
        route.clone(),
        budget,
        Arc::clone(&scrubber),
        policy_view,
        palette_entries,
    );
    let notifier = Notifier::new(notify, Arc::new(TelegramSender));
    let mut worker = spawn_worker(WorkerConfig {
        route,
        law,
        repo_root,
        workdir,
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
    SwitchRoute(Box<ResolvedRoute>),
    SetEffort(ThinkingEffort),
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
        mut route,
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
    let law_constitution = law.constitution;
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
        constitution: Some(identity_constitution(&law_constitution, &route)),
        context_limit: route.context,
        on_event: Some(Box::new(move |line| {
            let _ = progress_events.send(AgentEvent::Progress(safe_line(&event_scrubber, line)));
        })),
    };

    let mut history: Vec<ChatMessage> = Vec::new();
    let mut session_usage = Usage::default();
    while let Ok(command) = commands.recv() {
        let task = match command {
            WorkerCommand::Task(task) => task,
            WorkerCommand::SwitchRoute(next_route) => {
                let connection = connect(&next_route);
                match connection {
                    Ok((client, literal)) => {
                        install_literal(&scrubber, &mut key_literals, literal);
                        agent.receipts.scrubber = Scrubber::new(key_literals.clone());
                        agent.client = client;
                        connected = true;
                    }
                    Err(error) => {
                        agent.client = Box::new(NotConnected {
                            message: error.to_string(),
                        });
                        connected = false;
                    }
                }
                agent.model_id = next_route.model_id.clone();
                agent.thinking = effort_for(next_route.thinking_dialect);
                let constitution = identity_constitution(&law_constitution, &next_route);
                agent.constitution = Some(constitution.clone());
                replace_system_message(&mut history, constitution);
                agent.context_limit = next_route.context;
                route = *next_route;
                continue;
            }
            WorkerCommand::SetEffort(effort) => {
                agent.thinking = effort;
                continue;
            }
            WorkerCommand::Stop => break,
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

fn replace_system_message(history: &mut [ChatMessage], constitution: String) {
    if let Some(system) = history
        .first_mut()
        .filter(|message| message.role == "system")
    {
        system.content = Some(constitution);
        system.tool_calls = None;
        system.tool_call_id = None;
        system.reasoning_content = None;
    }
}

/// The honest-identity system prompt: names the real route + provider and forbids
/// claiming to be Claude/GPT, then appends the law constitution. Shared with the CLI
/// `run`/`chat` paths so every agent surface - not just the TUI - is honest.
pub fn identity_constitution(law_constitution: &str, route: &ResolvedRoute) -> String {
    format!(
        "You are nosis, an autonomous coding harness. You are running on the model route '{}' via {}. If asked what model or assistant you are, answer 'nosis on {}'; never claim to be Claude, GPT, or any other assistant.\n\n{}",
        route.id, route.provider, route.id, law_constitution
    )
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

fn effort_name(effort: ThinkingEffort) -> &'static str {
    match effort {
        ThinkingEffort::None => "none",
        ThinkingEffort::Low => "low",
        ThinkingEffort::High => "high",
        ThinkingEffort::Max => "max",
    }
}

fn parse_effort(value: &str) -> Option<ThinkingEffort> {
    match value.trim().to_ascii_lowercase().as_str() {
        "none" => Some(ThinkingEffort::None),
        "low" => Some(ThinkingEffort::Low),
        "high" => Some(ThinkingEffort::High),
        "max" => Some(ThinkingEffort::Max),
        _ => None,
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
                    return Err(anyhow::anyhow!("agent stopped - retry the task"));
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
        let input = event::read().context("could not read terminal input")?;
        if handle_input_event(app, worker, input) {
            worker.stop();
            return Ok(());
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum UiAction {
    None,
    Dispatch(String),
    SwitchRoute(String),
    SetEffort(ThinkingEffort),
    Quit,
}

fn handle_input_event(app: &mut App, worker: &mut Worker, input: Event) -> bool {
    let action = reduce_input_event(app, input);
    handle_action(app, worker, action)
}

fn handle_action(app: &mut App, worker: &mut Worker, action: UiAction) -> bool {
    match action {
        UiAction::None => false,
        UiAction::Quit => true,
        UiAction::Dispatch(task) => {
            if worker.commands.send(WorkerCommand::Task(task)).is_err() {
                apply_event(
                    app,
                    AgentEvent::Failed("agent stopped - retry the task".into()),
                );
            }
            false
        }
        UiAction::SwitchRoute(route_id) => {
            let route = app
                .resolver
                .resolve(&route_id)
                .expect("a command action only carries a resolved route");
            if worker
                .commands
                .send(WorkerCommand::SwitchRoute(Box::new(route.clone())))
                .is_err()
            {
                apply_event(
                    app,
                    AgentEvent::Failed("agent stopped - retry the task".into()),
                );
            } else {
                app.switch_route(route);
            }
            false
        }
        UiAction::SetEffort(effort) => {
            if worker
                .commands
                .send(WorkerCommand::SetEffort(effort))
                .is_err()
            {
                apply_event(
                    app,
                    AgentEvent::Failed("agent stopped - retry the task".into()),
                );
            } else {
                app.set_effort(effort);
            }
            false
        }
    }
}

#[cfg(test)]
fn handle_key(app: &mut App, worker: &mut Worker, key: KeyEvent) -> bool {
    let action = reduce_key(app, key);
    handle_action(app, worker, action)
}

fn reduce_input_event(app: &mut App, input: Event) -> UiAction {
    match input {
        Event::Key(key) if key.kind == KeyEventKind::Press => reduce_key(app, key),
        Event::Paste(text) => reduce_paste(app, &text),
        _ => UiAction::None,
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
        KeyCode::Enter => {
            if let Some(task) = app.dispatch() {
                return UiAction::Dispatch(task);
            }
        }
        KeyCode::Backspace => {
            app.input.pop();
        }
        KeyCode::Up => scroll_transcript(app, 1, true),
        KeyCode::Down => scroll_transcript(app, 1, false),
        KeyCode::PageUp => scroll_transcript(app, 5, true),
        KeyCode::PageDown => scroll_transcript(app, 5, false),
        KeyCode::End => app.scroll_back = 0,
        KeyCode::Char(character)
            if !character.is_control()
                && !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            app.input.push(character);
            if app.input.starts_with('/') {
                app.overlay = Overlay::CommandMenu { selected: 0 };
            }
        }
        _ => {}
    }
    UiAction::None
}

fn reduce_paste(app: &mut App, text: &str) -> UiAction {
    if matches!(app.status, Status::Working | Status::Waiting)
        || !matches!(app.overlay, Overlay::None | Overlay::CommandMenu { .. })
    {
        return UiAction::None;
    }

    app.input
        .extend(text.chars().filter_map(|character| match character {
            '\n' | '\r' | '\t' => Some(' '),
            character if character.is_control() => None,
            character => Some(character),
        }));

    if app.input.starts_with('/') {
        if let Overlay::CommandMenu { selected } = &mut app.overlay {
            *selected = 0;
        } else {
            app.overlay = Overlay::CommandMenu { selected: 0 };
        }
    } else if matches!(app.overlay, Overlay::CommandMenu { .. }) {
        app.overlay = Overlay::None;
    }

    UiAction::None
}

fn scroll_transcript(app: &mut App, amount: u16, toward_older: bool) {
    let max_scroll = app.max_scroll.get();
    let current = app.scroll_back.min(max_scroll);
    app.scroll_back = if toward_older {
        current.saturating_add(amount).min(max_scroll)
    } else {
        current.saturating_sub(amount)
    };
}

fn reduce_overlay_key(app: &mut App, key: KeyEvent) -> UiAction {
    if matches!(app.overlay, Overlay::CommandMenu { .. }) {
        return reduce_command_menu_key(app, key);
    }
    if key.code == KeyCode::Esc {
        app.overlay = Overlay::None;
        return UiAction::None;
    }
    if matches!(app.overlay, Overlay::TrustDial) {
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
        Overlay::None
        | Overlay::CommandMenu { .. }
        | Overlay::TrustDial
        | Overlay::Timeline { .. } => None,
    };
    let Some(entry) = activated else {
        return UiAction::None;
    };
    activate_palette_entry(app, entry)
}

fn reduce_command_menu_key(app: &mut App, key: KeyEvent) -> UiAction {
    match key.code {
        KeyCode::Esc => {
            app.input.clear();
            app.overlay = Overlay::None;
        }
        KeyCode::Backspace => {
            app.input.pop();
            if app.input.is_empty() {
                app.overlay = Overlay::None;
            } else if let Overlay::CommandMenu { selected } = &mut app.overlay {
                *selected = 0;
            }
        }
        KeyCode::Up => {
            if let Overlay::CommandMenu { selected } = &mut app.overlay {
                *selected = selected.saturating_sub(1);
            }
        }
        KeyCode::Down => {
            let count = command_matches(app).len();
            if let Overlay::CommandMenu { selected } = &mut app.overlay {
                if count > 0 {
                    *selected = selected.saturating_add(1).min(count - 1);
                }
            }
        }
        KeyCode::Enter => return execute_command_menu(app),
        KeyCode::Char(character)
            if !character.is_control()
                && !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            app.input.push(character);
            if let Overlay::CommandMenu { selected } = &mut app.overlay {
                *selected = 0;
            }
        }
        _ => {}
    }
    UiAction::None
}

fn execute_command_menu(app: &mut App) -> UiAction {
    let command_text = app.input.strip_prefix('/').unwrap_or("");
    let typed = command_text.split_whitespace().next().unwrap_or("");
    let expected = format!("/{typed}");
    let exact = app.palette_entries.iter().any(|entry| {
        entry.kind == "command" && entry.name.split_whitespace().next().unwrap_or("") == expected
    });
    if !command_text.chars().any(char::is_whitespace) && (typed.is_empty() || !exact) {
        let selected = match app.overlay {
            Overlay::CommandMenu { selected } => selected,
            _ => 0,
        };
        if let Some(entry) = command_matches(app).get(selected).copied().cloned() {
            return activate_palette_entry(app, entry);
        }
    }
    execute_command(app)
}

fn command_matches(app: &App) -> Vec<&PaletteEntry> {
    let query = app
        .input
        .strip_prefix('/')
        .unwrap_or("")
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_lowercase();
    app.palette_entries
        .iter()
        .filter(|entry| {
            entry.kind == "command"
                && (query.is_empty()
                    || entry.name.to_lowercase().contains(&query)
                    || entry.description.to_lowercase().contains(&query))
        })
        .collect()
}

fn execute_command(app: &mut App) -> UiAction {
    let input = std::mem::take(&mut app.input);
    app.overlay = Overlay::None;
    let mut parts = input.strip_prefix('/').unwrap_or("").split_whitespace();
    let name = parts.next().unwrap_or("");
    let arg = parts.next();
    match (name, arg) {
        ("help" | "?", _) => {
            app.overlay = Overlay::Palette {
                filter: String::new(),
                selected: 0,
                detail: None,
            };
            UiAction::None
        }
        ("trust", _) => {
            app.overlay = Overlay::TrustDial;
            UiAction::None
        }
        ("timeline", _) => {
            app.overlay = Overlay::Timeline {
                selected: app.timeline.len().saturating_sub(1),
                inspecting: false,
                note: None,
            };
            UiAction::None
        }
        ("model", Some(id)) => resolved_route_action(app, app.resolver.resolve(id)),
        ("model", None) => command_error(app, "usage: /model <id>"),
        ("provider", Some(provider)) => {
            resolved_route_action(app, app.resolver.provider_default(provider))
        }
        ("provider", None) => command_error(app, "usage: /provider <name>"),
        ("effort", Some(value)) => match parse_effort(value) {
            Some(effort) => UiAction::SetEffort(effort),
            None => command_error(app, "usage: /effort <none|low|high|max>"),
        },
        ("effort", None) => command_error(app, "usage: /effort <none|low|high|max>"),
        ("quit", _) => UiAction::Quit,
        _ => command_error(app, "unknown command - type / to see all"),
    }
}

fn resolved_route_action(app: &mut App, resolved: anyhow::Result<ResolvedRoute>) -> UiAction {
    match resolved {
        Ok(route) if route.class == RouteClass::Delegate => {
            command_error(app, "delegate routes arrive in M4 - pick an api route")
        }
        Ok(route) => UiAction::SwitchRoute(route.id),
        Err(error) => command_error(app, &error.to_string()),
    }
}

fn command_error(app: &mut App, message: &str) -> UiAction {
    app.push_line(message, TranscriptKind::Error);
    UiAction::None
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
        PaletteAction::Prefill(command) => {
            app.input = command.into();
            app.overlay = Overlay::CommandMenu { selected: 0 };
            UiAction::None
        }
        PaletteAction::Describe => {
            if let Overlay::Palette { detail, .. } = &mut app.overlay {
                *detail = Some(entry.description);
            }
            UiAction::None
        }
    }
}

fn render(frame: &mut Frame<'_>, app: &App) {
    let area = frame.area();
    let outer = main_block(app);
    let inner = outer.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(outer, area);
    let regions = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);
    render_transcript(frame, app, regions[0]);
    render_key_hints(frame, app, regions[1]);
    render_separator(frame, app, regions[2]);
    render_input(frame, app, regions[3]);
    render_hud(frame, app, regions[4]);
    render_overlay(frame, app);
}

fn main_block(app: &App) -> Block<'static> {
    let (status, status_style) = status_chip(&app.status);
    let left_title = Line::from(vec![
        Span::styled(
            safe_line(&app.scrubber, " nosis "),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            safe_line(&app.scrubber, "· "),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(safe_line(&app.scrubber, &status), status_style),
        Span::raw(safe_line(&app.scrubber, " ")),
    ]);
    let route_title = Line::from(Span::styled(
        safe_line(
            &app.scrubber,
            &format!(" {} · effort: {} ", app.route.id, effort_name(app.effort)),
        ),
        Style::default().fg(Color::Cyan),
    ))
    .right_aligned();

    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(Color::DarkGray))
        .style(Style::default().bg(Color::Black))
        .title_top(left_title)
        .title_top(route_title)
}

fn render_key_hints(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let hints = safe_line(
        &app.scrubber,
        "/ commands   ↑↓ scroll   Enter send   Ctrl+C quit",
    );
    frame.render_widget(
        Paragraph::new(hints).style(
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::DIM),
        ),
        area,
    );
}

fn render_separator(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let rule = safe_line(&app.scrubber, &"─".repeat(usize::from(area.width)));
    frame.render_widget(
        Paragraph::new(rule).style(
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::DIM),
        ),
        area,
    );
}

fn render_input(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let prompt = safe_line(&app.scrubber, "❯ ");
    let input = safe_line(&app.scrubber, &app.input);
    let mut spans = vec![Span::styled(
        prompt.clone(),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )];
    if app.input.is_empty() {
        spans.push(Span::styled(
            safe_line(&app.scrubber, "type a task and press Enter…"),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::DIM),
        ));
    } else {
        spans.push(Span::styled(
            input.clone(),
            Style::default().fg(Color::White),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);

    if app.overlay == Overlay::None && area.width > 0 && area.height > 0 {
        let cursor_width = Line::from(format!("{prompt}{input}")).width();
        let cursor_x = area.x.saturating_add(
            u16::try_from(cursor_width)
                .unwrap_or(u16::MAX)
                .min(area.width.saturating_sub(1)),
        );
        frame.set_cursor_position((cursor_x, area.y));
    }
}

fn render_hud(frame: &mut Frame<'_>, app: &App, area: Rect) {
    frame.render_widget(
        Paragraph::new(app.hud_line(Utc::now())).style(Style::default().fg(Color::Gray)),
        area,
    );
}

fn render_overlay(frame: &mut Frame<'_>, app: &App) {
    match &app.overlay {
        Overlay::None => {}
        Overlay::CommandMenu { selected } => {
            render_command_menu(frame, app, modal_area(frame.area(), 14), *selected)
        }
        Overlay::TrustDial => {
            let desired = u16::try_from(trust_dial_lines(&app.policy_view).len())
                .unwrap_or(u16::MAX)
                .saturating_add(3)
                .max(8);
            render_trust_dial(frame, app, modal_area(frame.area(), desired));
        }
        Overlay::Timeline {
            selected,
            inspecting,
            note,
        } => render_timeline(
            frame,
            app,
            modal_area(frame.area(), 20),
            *selected,
            *inspecting,
            note.as_deref(),
        ),
        Overlay::Palette {
            filter,
            selected,
            detail,
        } => render_palette(
            frame,
            app,
            modal_area(frame.area(), 18),
            filter,
            *selected,
            detail.as_deref(),
        ),
    }
}

fn render_trust_dial(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let body = render_modal_shell(
        frame,
        app,
        area,
        " Trust Dial · read-only ",
        "Read-only policy view · Esc close",
    );
    let lines: Vec<Line<'static>> = trust_dial_lines(&app.policy_view)
        .into_iter()
        .map(|line| Line::from(safe_line(&app.scrubber, &line)))
        .collect();
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().fg(Color::White).bg(Color::Black)),
        body,
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
    let body = render_modal_shell(
        frame,
        app,
        area,
        " Timeline ",
        "↑/↓ move · Enter inspect · Esc close",
    );
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(body);
    let visible_rows = usize::from(columns[0].height.max(1));
    let start = selected.saturating_add(1).saturating_sub(visible_rows);
    let mut rail = Vec::new();
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
                    let marker = if index == selected { "❯ " } else { "  " };
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

fn render_command_menu(frame: &mut Frame<'_>, app: &App, area: Rect, selected: usize) {
    let body = render_modal_shell(
        frame,
        app,
        area,
        " Commands ",
        "Type a command · ↑/↓ browse · Enter run · Esc close",
    );
    let filtered = command_matches(app);
    let visible_rows = usize::from(body.height.saturating_sub(2).max(1));
    let selected = selected.min(filtered.len().saturating_sub(1));
    let start = selected.saturating_add(1).saturating_sub(visible_rows);
    let query = app.input.strip_prefix('/').unwrap_or("");
    let mut lines = vec![
        Line::from(safe_line(&app.scrubber, &format!("command: /{query}"))),
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
                    let marker = if index == selected { "❯ " } else { "  " };
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
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().fg(Color::White).bg(Color::Black)),
        body,
    );
}

fn render_palette(
    frame: &mut Frame<'_>,
    app: &App,
    area: Rect,
    filter: &str,
    selected: usize,
    detail: Option<&str>,
) {
    let body = render_modal_shell(
        frame,
        app,
        area,
        " Commands + Tools ",
        "Type to filter · ↑/↓ move · Enter select · Esc close",
    );
    let filtered = filter_palette(&app.palette_entries, filter);
    let visible_rows = usize::from(body.height.saturating_sub(2).max(1));
    let start = selected.saturating_add(1).saturating_sub(visible_rows);
    let mut lines = vec![
        Line::from(safe_line(&app.scrubber, &format!("filter: {filter}"))),
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
                    let marker = if index == selected { "❯ " } else { "  " };
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
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().fg(Color::White).bg(Color::Black)),
        body,
    );
}

fn modal_area(area: Rect, desired_height: u16) -> Rect {
    let width = if area.width > 4 {
        area.width.saturating_sub(4).min(76)
    } else {
        area.width
    };
    let max_height = if area.height > 2 {
        area.height.saturating_sub(2)
    } else {
        area.height
    };
    let height = desired_height.clamp(6, 20).min(max_height);
    Rect::new(
        area.x.saturating_add(area.width.saturating_sub(width) / 2),
        area.y
            .saturating_add(area.height.saturating_sub(height) / 2),
        width,
        height,
    )
}

fn render_modal_shell(
    frame: &mut Frame<'_>,
    app: &App,
    area: Rect,
    title: &str,
    help: &str,
) -> Rect {
    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(Color::DarkGray))
        .style(Style::default().fg(Color::White).bg(Color::Black))
        .padding(Padding::horizontal(1))
        .title_top(Line::from(Span::styled(
            safe_line(&app.scrubber, title),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(inner);
    frame.render_widget(
        Paragraph::new(safe_line(&app.scrubber, help)).style(
            Style::default()
                .fg(Color::DarkGray)
                .bg(Color::Black)
                .add_modifier(Modifier::DIM),
        ),
        rows[0],
    );
    rows[1]
}

fn render_transcript(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if app.transcript.is_empty() {
        render_empty_state(frame, app, area);
        return;
    }
    let lines = chat_lines(app);
    let (scroll, max_scroll, overflow) = transcript_scroll_state(&lines, area, app.scroll_back);
    app.max_scroll.set(max_scroll);
    let transcript = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    frame.render_widget(transcript, area);
    render_overflow_markers(frame, app, area, overflow);
}

fn render_empty_state(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let height = area.height.min(4);
    let welcome_area = Rect::new(
        area.x,
        area.y
            .saturating_add(area.height.saturating_sub(height) / 2),
        area.width,
        height,
    );
    let lines = vec![
        Line::from(Span::styled(
            safe_line(&app.scrubber, "Welcome to nosis."),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            safe_line(&app.scrubber, "Type a task and press Enter."),
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            safe_line(&app.scrubber, "e.g. \"fix the failing test in this repo\""),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::DIM),
        )),
        Line::from(Span::styled(
            safe_line(&app.scrubber, "Type / to see everything nosis can do."),
            Style::default().fg(Color::Gray),
        )),
    ];
    frame.render_widget(
        Paragraph::new(lines).alignment(Alignment::Center),
        welcome_area,
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TranscriptOverflow {
    above: bool,
    below: bool,
}

fn transcript_scroll_state(
    lines: &[Line<'_>],
    area: Rect,
    scroll_back: u16,
) -> (u16, u16, TranscriptOverflow) {
    let rows = wrapped_rows(lines, area.width.max(1));
    let max_scroll = rows.saturating_sub(area.height);
    let scroll = max_scroll.saturating_sub(scroll_back.min(max_scroll));
    (
        scroll,
        max_scroll,
        TranscriptOverflow {
            above: scroll > 0,
            below: scroll < max_scroll,
        },
    )
}

fn render_overflow_markers(
    frame: &mut Frame<'_>,
    app: &App,
    area: Rect,
    overflow: TranscriptOverflow,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let marker_width = area.width.min(6);
    let marker_x = area.right().saturating_sub(marker_width);
    let style = Style::default()
        .fg(Color::DarkGray)
        .bg(Color::Black)
        .add_modifier(Modifier::DIM);
    if overflow.above {
        let marker = safe_line(&app.scrubber, "↑ more");
        frame.render_widget(
            Paragraph::new(marker)
                .style(style)
                .alignment(Alignment::Right),
            Rect::new(marker_x, area.y, marker_width, 1),
        );
    }
    if overflow.below {
        let marker = safe_line(&app.scrubber, "↓ more");
        frame.render_widget(
            Paragraph::new(marker)
                .style(style)
                .alignment(Alignment::Right),
            Rect::new(marker_x, area.bottom().saturating_sub(1), marker_width, 1),
        );
    }
}

fn chat_lines(app: &App) -> Vec<Line<'static>> {
    let mut rendered = Vec::new();
    let mut previous = None;
    for item in &app.transcript {
        let starts_group = previous != Some(item.kind);
        if starts_group && !rendered.is_empty() {
            rendered.push(Line::from(safe_line(&app.scrubber, "")));
        }
        match item.kind {
            TranscriptKind::Task => {
                if starts_group {
                    rendered.push(Line::from(Span::styled(
                        safe_line(&app.scrubber, "❯ you"),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    )));
                }
                rendered.push(Line::from(Span::styled(
                    safe_line(&app.scrubber, &format!("   {}", item.text)),
                    Style::default().fg(Color::White),
                )));
            }
            TranscriptKind::Answer => {
                if starts_group {
                    rendered.push(Line::from(Span::styled(
                        safe_line(&app.scrubber, "◆ nosis"),
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    )));
                }
                rendered.push(Line::from(Span::styled(
                    safe_line(&app.scrubber, &format!("   {}", item.text)),
                    Style::default().fg(Color::White),
                )));
            }
            TranscriptKind::Progress => rendered.push(Line::from(Span::styled(
                safe_line(&app.scrubber, &format!("· {}", item.text)),
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::DIM),
            ))),
            TranscriptKind::Approval => rendered.push(Line::from(Span::styled(
                safe_line(&app.scrubber, &format!(" {} ", item.text)),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED),
            ))),
            TranscriptKind::Error => rendered.push(Line::from(Span::styled(
                safe_line(&app.scrubber, &item.text),
                Style::default().fg(Color::Red),
            ))),
        }
        previous = Some(item.kind);
    }
    rendered
}

fn wrapped_rows(lines: &[Line<'_>], width: u16) -> u16 {
    let width = usize::from(width.max(1));
    lines.iter().fold(0_u16, |rows, line| {
        let cells = line.width().max(1);
        let line_rows = cells.div_ceil(width).min(usize::from(u16::MAX)) as u16;
        rows.saturating_add(line_rows)
    })
}

fn status_chip(status: &Status) -> (String, Style) {
    match status {
        Status::Idle => (
            "○ IDLE".into(),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::DIM),
        ),
        Status::Working => (
            "● WORKING".into(),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Status::Waiting => (
            "● WAITING ON YOU".into(),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Status::Blocked(_) => (
            "● BLOCKED".into(),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
    }
}

fn budget_bar(used: u64, limit: u64) -> String {
    const WIDTH: u64 = 7;
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
        if let Err(error) = write_setup_commands(&mut stdout) {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SetupCommand {
    EnterScreen,
    EnablePaste,
    HideCursor,
}

fn run_setup_sequence(mut run: impl FnMut(SetupCommand) -> io::Result<()>) -> io::Result<()> {
    for command in [
        SetupCommand::EnterScreen,
        SetupCommand::EnablePaste,
        SetupCommand::HideCursor,
    ] {
        run(command)?;
    }
    Ok(())
}

fn write_setup_commands(writer: &mut impl Write) -> io::Result<()> {
    run_setup_sequence(|command| match command {
        SetupCommand::EnterScreen => execute!(writer, EnterAlternateScreen),
        SetupCommand::EnablePaste => execute!(writer, EnableBracketedPaste),
        SetupCommand::HideCursor => execute!(writer, Hide),
    })
}

fn restore_terminal() {
    let _ = disable_raw_mode();
    let mut stdout = io::stdout();
    let _ = write_restore_commands(&mut stdout);
    let _ = stdout.flush();
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RestoreCommand {
    DisablePaste,
    ShowCursor,
    LeaveScreen,
}

fn run_restore_sequence(mut run: impl FnMut(RestoreCommand) -> io::Result<()>) -> io::Result<()> {
    for command in [
        RestoreCommand::DisablePaste,
        RestoreCommand::ShowCursor,
        RestoreCommand::LeaveScreen,
    ] {
        run(command)?;
    }
    Ok(())
}

fn write_restore_commands(writer: &mut impl Write) -> io::Result<()> {
    run_restore_sequence(|command| match command {
        RestoreCommand::DisablePaste => execute!(writer, DisableBracketedPaste),
        RestoreCommand::ShowCursor => execute!(writer, Show),
        RestoreCommand::LeaveScreen => execute!(writer, LeaveAlternateScreen),
    })
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

        [routes.other-route]
        provider = "other"
        model_id = "other-route"
        base_url = "https://example.invalid"
        wire = "openai"
        vault_entry = "other"
        context = 2000
    "#;

    fn test_resolver() -> RouteResolver {
        RouteResolver::from_toml(TEST_CATALOG).expect("test catalog parses")
    }

    fn test_route() -> ResolvedRoute {
        test_resolver().resolve("test-route").unwrap()
    }

    fn test_app(budget: Option<u64>) -> App {
        App::new(
            test_resolver(),
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

    fn render_buffer(app: &App, width: u16, height: u16) -> ratatui::buffer::Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(frame, app)).unwrap();
        terminal.backend().buffer().clone()
    }

    fn buffer_rows(buffer: &ratatui::buffer::Buffer) -> Vec<String> {
        let width = usize::from(buffer.area.width);
        buffer
            .content
            .chunks(width)
            .map(|row| row.iter().map(|cell| cell.symbol()).collect())
            .collect()
    }

    fn buffer_text(buffer: &ratatui::buffer::Buffer) -> String {
        buffer_rows(buffer).join("\n")
    }

    fn find_ascii_text(buffer: &ratatui::buffer::Buffer, needle: &str) -> (u16, u16) {
        for (y, row) in buffer_rows(buffer).iter().enumerate() {
            if let Some(x) = row.find(needle) {
                return (u16::try_from(x).unwrap(), u16::try_from(y).unwrap());
            }
        }
        panic!("could not find {needle:?} in {}", buffer_text(buffer));
    }

    fn assert_plain_modal_ring(buffer: &ratatui::buffer::Buffer, area: Rect) {
        let right = area.right().saturating_sub(1);
        let bottom = area.bottom().saturating_sub(1);
        assert_eq!(buffer[(area.x, area.y)].symbol(), "┌");
        assert_eq!(buffer[(right, area.y)].symbol(), "┐");
        assert_eq!(buffer[(area.x, bottom)].symbol(), "└");
        assert_eq!(buffer[(right, bottom)].symbol(), "┘");
        for y in area.y.saturating_add(1)..bottom {
            assert_eq!(buffer[(area.x, y)].symbol(), "│", "left edge at y={y}");
            assert_eq!(buffer[(right, y)].symbol(), "│", "right edge at y={y}");
        }
        for x in area.x.saturating_add(1)..right {
            assert_eq!(buffer[(x, bottom)].symbol(), "─", "bottom edge at x={x}");
        }
    }

    fn modal_text(buffer: &ratatui::buffer::Buffer, area: Rect) -> String {
        let mut text = String::new();
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                text.push_str(buffer[(x, y)].symbol());
            }
            text.push('\n');
        }
        text
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
    fn outer_frame_and_each_status_word_render() {
        let cases = [
            (Status::Idle, "○ IDLE"),
            (Status::Working, "● WORKING"),
            (Status::Waiting, "● WAITING ON YOU"),
            (Status::Blocked("offline".into()), "● BLOCKED"),
        ];
        for (status, label) in cases {
            let mut app = test_app(None);
            app.status = status;
            let buffer = render_buffer(&app, 90, 20);
            let text = buffer_text(&buffer);

            assert_eq!(buffer[(0, 0)].symbol(), "┌");
            assert_eq!(buffer[(89, 0)].symbol(), "┐");
            assert_eq!(buffer[(0, 19)].symbol(), "└");
            assert_eq!(buffer[(89, 19)].symbol(), "┘");
            assert!(text.contains("nosis"), "got: {text}");
            assert!(text.contains("test-route"), "got: {text}");
            assert!(text.contains(label), "got: {text}");
        }
    }

    #[test]
    fn chat_roles_label_indented_turns_and_leave_a_visual_gap() {
        let mut app = test_app(None);
        app.input = "fix this test".into();
        assert_eq!(app.dispatch().as_deref(), Some("fix this test"));
        apply_event(&mut app, AgentEvent::Answer("done cleanly".into()));

        let rows = buffer_rows(&render_buffer(&app, 90, 20));
        let user_row = rows.iter().position(|row| row.contains("❯ you")).unwrap();
        let task_row = rows
            .iter()
            .position(|row| row.contains("   fix this test"))
            .unwrap();
        let nosis_row = rows.iter().position(|row| row.contains("◆ nosis")).unwrap();
        let answer_row = rows
            .iter()
            .position(|row| row.contains("   done cleanly"))
            .unwrap();

        assert_eq!(task_row, user_row + 1);
        assert_eq!(answer_row, nosis_row + 1);
        assert!(nosis_row > task_row + 1);
        assert!(rows[task_row + 1].trim_matches(['│', ' ']).is_empty());
    }

    #[test]
    fn empty_state_and_key_strip_are_self_teaching_then_conversation_replaces_welcome() {
        let mut app = test_app(None);
        let fresh = buffer_text(&render_buffer(&app, 90, 20));
        assert!(fresh.contains("Welcome to nosis."), "got: {fresh}");
        assert!(
            fresh.contains("Type a task and press Enter."),
            "got: {fresh}"
        );
        assert!(
            fresh.contains("e.g. \"fix the failing test in this repo\""),
            "got: {fresh}"
        );
        assert!(
            fresh.contains("Type / to see everything nosis can do."),
            "got: {fresh}"
        );
        assert!(
            fresh.contains("/ commands   ↑↓ scroll   Enter send   Ctrl+C quit"),
            "got: {fresh}"
        );

        app.input = "start".into();
        app.dispatch().unwrap();
        let active = buffer_text(&render_buffer(&app, 90, 20));
        assert!(!active.contains("Welcome to nosis."), "got: {active}");
        assert!(active.contains("❯ you"), "got: {active}");
        assert!(active.contains("   start"), "got: {active}");
        assert!(
            active.contains("/ commands   ↑↓ scroll   Enter send   Ctrl+C quit"),
            "got: {active}"
        );
    }

    #[test]
    fn centered_modal_frames_clear_transcript_for_every_overlay() {
        let terminal = Rect::new(0, 0, 100, 30);
        let cases = [
            (
                Overlay::CommandMenu { selected: 0 },
                modal_area(terminal, 14),
                "Commands",
            ),
            (
                Overlay::TrustDial,
                modal_area(terminal, 8),
                "Trust Dial · read-only",
            ),
            (
                Overlay::Palette {
                    filter: String::new(),
                    selected: 0,
                    detail: None,
                },
                modal_area(terminal, 18),
                "Commands + Tools",
            ),
            (
                Overlay::Timeline {
                    selected: 0,
                    inspecting: false,
                    note: None,
                },
                modal_area(terminal, 20),
                "Timeline",
            ),
        ];

        for (overlay, area, title) in cases {
            let mut app = test_app(None);
            for _ in 0..40 {
                app.push_line(
                    "ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ",
                    TranscriptKind::Progress,
                );
            }
            apply_event(&mut app, timeline_event("safe task", "safe answer"));
            app.overlay = overlay;

            let buffer = render_buffer(&app, terminal.width, terminal.height);
            let modal = modal_text(&buffer, area);
            assert_plain_modal_ring(&buffer, area);
            assert!(modal.contains(title), "got: {modal}");
            assert!(!modal.contains('Z'), "transcript bled into modal: {modal}");
        }
    }

    #[test]
    fn every_new_surface_scrubs_literals_and_control_characters() {
        let transcript_secret = "fake-key-transcript";
        let modal_secret = "fake-key-modal";
        let empty_literal = "Type a task and press Enter.";
        let hud_literal = "no price data";
        let scrubber = Arc::new(RwLock::new(Scrubber::new(vec![
            transcript_secret.into(),
            modal_secret.into(),
            empty_literal.into(),
            hud_literal.into(),
        ])));
        let mut app = App::new(
            test_resolver(),
            test_route(),
            None,
            scrubber,
            PolicyView {
                autonomy: Autonomy::Ask,
                auto_paths: Vec::new(),
                ask_paths: Vec::new(),
                block_paths: vec![format!("{modal_secret}\r\x1b[2K")],
                block_commands: Vec::new(),
            },
            Vec::new(),
        );

        let empty = buffer_text(&render_buffer(&app, 100, 24));
        assert!(empty.matches("[REDACTED]").count() >= 2, "got: {empty}");
        assert!(!empty.contains(empty_literal), "got: {empty}");
        assert!(!empty.contains(hud_literal), "got: {empty}");

        apply_event(
            &mut app,
            AgentEvent::Progress(format!("value={transcript_secret}\r\x1b[2K")),
        );
        let transcript = buffer_text(&render_buffer(&app, 100, 24));
        assert!(transcript.contains("[REDACTED]"), "got: {transcript}");
        assert!(!transcript.contains(transcript_secret), "got: {transcript}");
        assert!(!transcript.contains('\r'), "got: {transcript}");
        assert!(!transcript.contains('\x1b'), "got: {transcript}");

        app.overlay = Overlay::TrustDial;
        let modal = buffer_text(&render_buffer(&app, 100, 24));
        assert!(modal.contains("[REDACTED]"), "got: {modal}");
        assert!(!modal.contains(modal_secret), "got: {modal}");
        assert!(!modal.contains('\r'), "got: {modal}");
        assert!(!modal.contains('\x1b'), "got: {modal}");
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
    fn approval_row_names_the_command_and_is_amber_reversed() {
        let mut app = test_app(None);
        app.status = Status::Working;
        let (event, _answer) = approval("cargo test --workspace");
        apply_event(&mut app, event);

        let buffer = render_buffer(&app, 100, 20);
        let text = buffer_text(&buffer);
        let (x, y) = find_ascii_text(&buffer, "approve?");
        let cell = &buffer[(x, y)];
        assert!(
            text.contains("approve? cargo test --workspace  [y/N]"),
            "got: {text}"
        );
        assert_eq!(cell.fg, Color::Yellow);
        assert!(cell.modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn failed_event_renders_one_friendly_line_without_a_trace() {
        let mut app = test_app(None);
        app.status = Status::Working;
        apply_event(
            &mut app,
            AgentEvent::Failed("network unavailable\nstack backtrace:\n0: internal frame".into()),
        );

        assert!(matches!(
            &app.status,
            Status::Blocked(reason)
                if reason.starts_with("network unavailable") && reason.contains("backtrace")
        ));
        assert_eq!(app.transcript.len(), 1);
        assert_eq!(
            app.transcript[0].text,
            "! network unavailable - retry the task or type /help"
        );
        let text = buffer_text(&render_buffer(&app, 100, 20));
        assert!(text.contains("! network unavailable - retry the task"));
        assert!(!text.contains("backtrace"), "got: {text}");
        assert!(!text.contains("internal frame"), "got: {text}");
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
            AgentEvent::Progress("context 73% - compacted 8 earlier messages".into()),
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

        type_text(&mut app, "/timeline");
        assert_eq!(
            reduce_key(&mut app, code_key(KeyCode::Enter)),
            UiAction::None
        );
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
    fn printable_words_type_freely_without_opening_overlays() {
        for word in ["list", "trust", "quit", "?", "R"] {
            let mut app = test_app(None);
            type_text(&mut app, word);

            assert_eq!(app.input, word);
            assert_eq!(app.overlay, Overlay::None);
            assert!(app.transcript.is_empty());
        }
    }

    #[test]
    fn slash_opens_live_command_menu_and_mod_filter_surfaces_model() {
        let mut app = test_app(None);
        type_text(&mut app, "/mod");

        assert!(matches!(app.overlay, Overlay::CommandMenu { .. }));
        let matches = command_matches(&app);
        assert!(matches.iter().any(|entry| entry.name == "/model <id>"));
        let rendered = buffer_text(&render_buffer(&app, 90, 22));
        assert!(rendered.contains("Commands"), "got: {rendered}");
        assert!(rendered.contains("/model <id>"), "got: {rendered}");

        assert_eq!(
            reduce_key(&mut app, code_key(KeyCode::Enter)),
            UiAction::None
        );
        assert_eq!(app.input, "/model ");
        assert!(matches!(app.overlay, Overlay::CommandMenu { .. }));

        assert_eq!(reduce_key(&mut app, code_key(KeyCode::Esc)), UiAction::None);
        assert_eq!(app.overlay, Overlay::None);
        assert!(app.input.is_empty());
    }

    #[test]
    fn paste_appends_to_input_without_dispatching() {
        let mut app = test_app(None);

        let action = reduce_input_event(&mut app, Event::Paste("foo bar".into()));

        assert_eq!(action, UiAction::None);
        assert_eq!(app.input, "foo bar");
        assert!(app.transcript.is_empty());
        assert_eq!(app.status, Status::Idle);
    }

    #[test]
    fn multiline_paste_becomes_one_input_line_without_dispatching() {
        let mut app = test_app(None);

        let action = reduce_input_event(&mut app, Event::Paste("line1\nline2".into()));

        assert_eq!(action, UiAction::None);
        assert_eq!(app.input, "line1 line2");
        assert!(app.transcript.is_empty());
        assert_eq!(app.status, Status::Idle);
    }

    #[test]
    fn paste_updates_the_open_slash_menu_without_dispatching() {
        let mut app = test_app(None);
        type_text(&mut app, "/");
        assert!(matches!(app.overlay, Overlay::CommandMenu { .. }));

        let action = reduce_input_event(&mut app, Event::Paste("mod".into()));

        assert_eq!(action, UiAction::None);
        assert_eq!(app.input, "/mod");
        assert!(matches!(app.overlay, Overlay::CommandMenu { selected: 0 }));
        assert!(command_matches(&app)
            .iter()
            .any(|entry| entry.name == "/model <id>"));
        assert!(app.transcript.is_empty());
    }

    #[test]
    fn bad_model_command_is_one_friendly_line_and_keeps_route() {
        let mut app = test_app(None);
        let original = app.route.id.clone();
        type_text(&mut app, "/model missing-route");

        assert_eq!(
            reduce_key(&mut app, code_key(KeyCode::Enter)),
            UiAction::None
        );

        assert_eq!(app.route.id, original);
        assert_eq!(app.transcript.len(), 1);
        assert!(
            app.transcript[0]
                .text
                .contains("unknown model id 'missing-route'"),
            "got: {}",
            app.transcript[0].text
        );
        assert_eq!(app.overlay, Overlay::None);
        assert!(app.input.is_empty());
    }

    #[test]
    fn effort_command_sets_header_and_invalid_value_shows_usage() {
        let mut app = test_app(None);
        type_text(&mut app, "/effort High");
        assert_eq!(
            reduce_key(&mut app, code_key(KeyCode::Enter)),
            UiAction::SetEffort(ThinkingEffort::High)
        );
        app.set_effort(ThinkingEffort::High);
        let rendered = buffer_text(&render_buffer(&app, 90, 20));
        assert!(rendered.contains("effort: high"), "got: {rendered}");

        type_text(&mut app, "/effort MAX");
        assert_eq!(
            reduce_key(&mut app, code_key(KeyCode::Enter)),
            UiAction::SetEffort(ThinkingEffort::Max)
        );
        app.set_effort(ThinkingEffort::Max);

        let before = app.transcript.len();
        type_text(&mut app, "/effort extreme");
        assert_eq!(
            reduce_key(&mut app, code_key(KeyCode::Enter)),
            UiAction::None
        );
        assert_eq!(app.transcript.len(), before + 1);
        assert_eq!(
            app.transcript.last().map(|line| line.text.as_str()),
            Some("usage: /effort <none|low|high|max>")
        );
        assert_eq!(app.effort, ThinkingEffort::Max);
    }

    #[test]
    fn slash_lines_are_commands_never_dispatched_as_tasks() {
        let mut app = test_app(None);
        type_text(&mut app, "/typo");

        assert_eq!(
            reduce_key(&mut app, code_key(KeyCode::Enter)),
            UiAction::None
        );
        assert_eq!(app.transcript.len(), 1);
        assert_eq!(
            app.transcript[0].text,
            "unknown command - type / to see all"
        );
        assert!(app
            .transcript
            .iter()
            .all(|line| line.kind != TranscriptKind::Task));
    }

    #[test]
    fn keyboard_arrows_pages_and_end_control_transcript_scroll() {
        let mut app = test_app(None);
        for index in 0..20 {
            app.push_line(&format!("line {index}"), TranscriptKind::Progress);
        }
        let _ = render_buffer(&app, 80, 16);
        assert!(app.max_scroll.get() > 0);
        assert_eq!(app.scroll_back, 0);

        reduce_key(&mut app, code_key(KeyCode::Up));
        assert_eq!(app.scroll_back, 1);
        reduce_key(&mut app, code_key(KeyCode::Down));
        assert_eq!(app.scroll_back, 0);

        reduce_key(&mut app, code_key(KeyCode::PageUp));
        assert_eq!(app.scroll_back, 5);
        reduce_key(&mut app, code_key(KeyCode::PageDown));
        assert_eq!(app.scroll_back, 0);

        app.scroll_back = 9;
        reduce_key(&mut app, code_key(KeyCode::End));
        assert_eq!(app.scroll_back, 0);
    }

    #[test]
    fn more_markers_render_only_for_overflow_and_track_both_directions() {
        let mut app = test_app(None);
        app.push_line("short", TranscriptKind::Progress);
        let short = buffer_text(&render_buffer(&app, 80, 16));
        assert!(!short.contains("↑ more"), "got: {short}");
        assert!(!short.contains("↓ more"), "got: {short}");

        for index in 0..30 {
            app.push_line(&format!("output line {index}"), TranscriptKind::Progress);
        }
        let newest = buffer_text(&render_buffer(&app, 80, 16));
        assert!(newest.contains("↑ more"), "got: {newest}");
        assert!(!newest.contains("↓ more"), "got: {newest}");

        app.scroll_back = 3;
        let middle = buffer_text(&render_buffer(&app, 80, 16));
        assert!(middle.contains("↑ more"), "got: {middle}");
        assert!(middle.contains("↓ more"), "got: {middle}");

        app.scroll_back = u16::MAX;
        let oldest = buffer_text(&render_buffer(&app, 80, 16));
        assert!(!oldest.contains("↑ more"), "got: {oldest}");
        assert!(oldest.contains("↓ more"), "got: {oldest}");
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
            .any(|entry| entry.name == "/timeline"));
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
        for command in ["/trust", "/help", "/timeline"] {
            let mut app = test_app(None);
            type_text(&mut app, command);
            assert_eq!(
                reduce_key(&mut app, code_key(KeyCode::Enter)),
                UiAction::None
            );
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
        type_text(&mut app, "/help");
        reduce_key(&mut app, code_key(KeyCode::Enter));
        type_text(&mut app, "trust");
        assert_eq!(
            reduce_key(&mut app, code_key(KeyCode::Enter)),
            UiAction::None
        );
        assert_eq!(app.overlay, Overlay::TrustDial);

        let mut app = test_app(None);
        type_text(&mut app, "/help");
        reduce_key(&mut app, code_key(KeyCode::Enter));
        type_text(&mut app, "quit");
        assert_eq!(
            reduce_key(&mut app, code_key(KeyCode::Enter)),
            UiAction::Quit
        );

        let mut app = test_app(None);
        type_text(&mut app, "/help");
        reduce_key(&mut app, code_key(KeyCode::Enter));
        type_text(&mut app, "exec_shell");
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

    fn type_text(app: &mut App, text: &str) {
        for character in text.chars() {
            assert_eq!(reduce_key(app, char_key(character)), UiAction::None);
        }
    }

    fn code_key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn cost_hud_omits_cache_before_usage_and_shows_it_after() {
        let mut app = test_app(None);
        let before = app.hud_line(Utc::now());
        assert!(before.contains("in 0 · out 0 · cached 0"), "got: {before}");
        assert!(before.contains("no price data"), "got: {before}");
        assert!(!before.contains("| cache "), "got: {before}");
        assert!(!before.contains("· cache "), "got: {before}");
        apply_event(
            &mut app,
            AgentEvent::Usage(Usage {
                prompt_tokens: 100,
                completion_tokens: 20,
                cached_tokens: Some(25),
            }),
        );
        let after = app.hud_line(Utc::now());
        assert!(
            after.contains("in 100 · out 20 · cached 25"),
            "got: {after}"
        );
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
        let hud = app.hud_line(Utc::now());
        assert!(hud.contains("[#######] 100%"), "got: {hud}");
        assert!(hud.contains("100/100"), "got: {hud}");
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
            test_resolver(),
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
            test_resolver(),
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

    #[test]
    fn terminal_setup_enables_bracketed_paste_and_native_selection() {
        let mut commands = Vec::new();

        run_setup_sequence(|command| {
            commands.push(command);
            Ok(())
        })
        .unwrap();

        assert_eq!(
            commands,
            [
                SetupCommand::EnterScreen,
                SetupCommand::EnablePaste,
                SetupCommand::HideCursor,
            ],
            "setup must enable bracketed paste while preserving native terminal selection"
        );
    }

    #[test]
    fn terminal_guard_teardown_disables_bracketed_paste() {
        let commands = Arc::new(Mutex::new(Vec::new()));
        {
            let commands_for_guard = Arc::clone(&commands);
            let _guard = TerminalGuard::with_restore(move || {
                run_restore_sequence(|command| {
                    commands_for_guard.lock().unwrap().push(command);
                    Ok(())
                })
                .unwrap();
            });
        }
        assert_eq!(
            *commands.lock().unwrap(),
            [
                RestoreCommand::DisablePaste,
                RestoreCommand::ShowCursor,
                RestoreCommand::LeaveScreen,
            ],
            "bracketed paste must be disabled before cursor/screen restoration"
        );
    }

    #[test]
    fn identity_constitution_is_stable_and_names_the_route_honestly() {
        let route = test_route();
        let first = identity_constitution("law bytes", &route);
        let second = identity_constitution("law bytes", &route);

        assert_eq!(first, second);
        assert!(first.contains("test-route"), "got: {first}");
        assert!(first.contains("via test"), "got: {first}");
        assert!(first.contains("never claim to be Claude"), "got: {first}");
        assert!(first.ends_with("law bytes"), "got: {first}");
    }

    struct MockClient {
        request_lengths: Arc<Mutex<Vec<usize>>>,
    }

    #[derive(Debug)]
    struct RecordedRequest {
        model: String,
        message_count: usize,
        system: String,
        effort: ThinkingEffort,
    }

    struct RecordingClient {
        requests: Arc<Mutex<Vec<RecordedRequest>>>,
    }

    impl ChatClient for RecordingClient {
        fn complete(&self, request: &ChatRequest) -> anyhow::Result<ChatResponse> {
            self.requests.lock().unwrap().push(RecordedRequest {
                model: request.model.clone(),
                message_count: request.messages.len(),
                system: request.messages[0].content.clone().unwrap_or_default(),
                effort: request.thinking,
            });
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

    fn receive_completed_task(worker: &Worker, app: &mut App) {
        let mut saw_answer = false;
        let mut saw_receipt = false;
        while !saw_answer || !saw_receipt {
            let event = worker
                .events
                .recv_timeout(Duration::from_secs(2))
                .expect("worker completes the task");
            saw_answer |= matches!(&event, AgentEvent::Answer(_));
            saw_receipt |= matches!(&event, AgentEvent::TaskReceipt(_));
            match event {
                AgentEvent::Approval(_) => panic!("mock never asks for approval"),
                AgentEvent::Failed(reason) => panic!("worker failed: {reason}"),
                event => {
                    apply_event(app, event);
                }
            }
        }
    }

    #[test]
    fn model_switch_keeps_worker_history_transcript_and_updates_route_identity() {
        let root = temp_dir();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_for_connect = Arc::clone(&requests);
        let connect: ConnectFn = Box::new(move |route| {
            Ok((
                Box::new(RecordingClient {
                    requests: Arc::clone(&requests_for_connect),
                }),
                format!("fake-key-{}", route.vault_entry),
            ))
        });
        let law = nh_law::load(&root, &nh_law::LoadOptions { cli_autonomy: None });
        let mut worker = spawn_worker(WorkerConfig {
            route: test_route(),
            law,
            repo_root: root.clone(),
            workdir: root.clone(),
            scrubber: Arc::new(RwLock::new(Scrubber::new(Vec::new()))),
            connect,
            initial: None,
        })
        .unwrap();
        let mut app = test_app(None);

        app.input = "first task".into();
        assert!(!handle_key(&mut app, &mut worker, code_key(KeyCode::Enter)));
        receive_completed_task(&worker, &mut app);
        let retained: Vec<_> = app
            .transcript
            .iter()
            .map(|line| line.text.clone())
            .collect();

        type_text(&mut app, "/model other-route");
        assert!(!handle_key(&mut app, &mut worker, code_key(KeyCode::Enter)));
        assert_eq!(app.route.id, "other-route");
        assert_eq!(app.timeline.len(), 1);
        assert_eq!(
            app.transcript
                .iter()
                .take(retained.len())
                .map(|line| line.text.clone())
                .collect::<Vec<_>>(),
            retained
        );
        assert_eq!(
            app.transcript.last().map(|line| line.text.as_str()),
            Some("switched to other-route - context kept, cache resets")
        );

        type_text(&mut app, "/effort high");
        assert!(!handle_key(&mut app, &mut worker, code_key(KeyCode::Enter)));
        assert_eq!(app.effort, ThinkingEffort::High);

        app.input = "second task".into();
        assert!(!handle_key(&mut app, &mut worker, code_key(KeyCode::Enter)));
        receive_completed_task(&worker, &mut app);
        assert_eq!(app.timeline.len(), 2);

        worker.stop();
        if let Some(join) = worker.join.take() {
            join.join().unwrap();
        }
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 2, "requests: {requests:#?}");
        assert_eq!(requests[0].model, "test-route");
        assert_eq!(requests[0].message_count, 2);
        assert!(requests[0].system.contains("nosis on test-route"));
        assert_eq!(requests[0].effort, ThinkingEffort::None);
        assert_eq!(requests[1].model, "other-route");
        assert_eq!(requests[1].message_count, 4, "history was not kept");
        assert!(requests[1].system.contains("nosis on other-route"));
        assert!(requests[1].system.contains("never claim to be Claude"));
        assert_eq!(requests[1].effort, ThinkingEffort::High);
        drop(requests);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn keyless_switch_accepts_route_then_next_task_surfaces_add_key_line() {
        let root = temp_dir();
        let request_lengths = Arc::new(Mutex::new(Vec::new()));
        let lengths_for_connect = Arc::clone(&request_lengths);
        let connect: ConnectFn = Box::new(move |route| {
            if route.id == "other-route" {
                anyhow::bail!("no key found for \"other\" - run `nh key add other`");
            }
            Ok((
                Box::new(MockClient {
                    request_lengths: Arc::clone(&lengths_for_connect),
                }),
                "fake-worker-secret".into(),
            ))
        });
        let law = nh_law::load(&root, &nh_law::LoadOptions { cli_autonomy: None });
        let mut worker = spawn_worker(WorkerConfig {
            route: test_route(),
            law,
            repo_root: root.clone(),
            workdir: root.clone(),
            scrubber: Arc::new(RwLock::new(Scrubber::new(Vec::new()))),
            connect,
            initial: None,
        })
        .unwrap();
        worker
            .commands
            .send(WorkerCommand::SwitchRoute(Box::new(
                test_resolver().resolve("other-route").unwrap(),
            )))
            .unwrap();
        worker
            .commands
            .send(WorkerCommand::Task("hello".into()))
            .unwrap();

        match worker.events.recv_timeout(Duration::from_secs(2)).unwrap() {
            AgentEvent::Failed(reason) => {
                assert!(reason.contains("nh key add other"), "got: {reason}");
            }
            _ => panic!("keyless switched task must fail with one friendly line"),
        }
        match worker.events.recv_timeout(Duration::from_secs(2)).unwrap() {
            AgentEvent::TaskReceipt(summary) => {
                assert_eq!(summary.receipt.model_id, "other-route");
                assert_eq!(summary.receipt.task, "hello");
            }
            _ => panic!("failed switched task must produce a timeline receipt"),
        }
        assert!(request_lengths.lock().unwrap().is_empty());
        worker.stop();
        if let Some(join) = worker.join.take() {
            join.join().unwrap();
        }
        std::fs::remove_dir_all(root).unwrap();
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
