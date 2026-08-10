//! TUI domain state, immutable configuration, and application state transitions.

use crate::palette::{builtin_palette_entries, short_text, ColorMode};
use crate::render::budget_bar;
use crate::session::{effort_for, effort_name, safe_line, scrub_full_line};
use crate::worker::ApprovalRequest;
use crate::{SharedScrubber, BUDGET_REASON, BUDGET_WARN_FRACTION};
use chrono::{DateTime, FixedOffset, Utc};
use nh_core::agent::{estimate_message_tokens, CompactionEvent};
use nh_core::receipt::{CompactionStats, FailureClass, Outcome, Receipt, ReceiptKind};
use nh_core::session_ledger::RestoredSession;
use nh_core::terminal_capability::TerminalCapability;
use nh_core::wire::{cache_hit_pct, ChatMessage, ThinkingEffort, Usage, UsageEvidence};
use nh_law::{Law, PolicyView};
use nh_routes::{
    cost_of, format_context_percent, money, money_with_gloss, Currency, PriceConfidence, Profiles,
    ResolvedRoute, RouteClass, RouteResolver,
};
use std::cell::Cell;
use std::collections::{btree_map::Entry, BTreeMap};
use std::ops::Range;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

pub(super) const MIN_TYPICAL_DURATION_SAMPLES: usize = 5;
pub(super) const PROMPT_HISTORY_CAPACITY: usize = 100;
pub(super) const PROMPT_ESTIMATE_UNAVAILABLE: u64 = u64::MAX;

/// The single status shown by the semáforo.
#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Idle,
    Working,
    FinishingInterrupted,
    Waiting,
    Blocked(String),
}

impl Status {
    pub(super) fn esc_interrupts_turn(&self) -> bool {
        matches!(self, Self::Working | Self::Waiting)
    }
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
    pub duration_ms: Option<u64>,
    pub kind: ReceiptKind,
    pub outcome: Outcome,
    pub failure_class: Option<FailureClass>,
    pub usage: Option<Usage>,
    pub answer: String,
    pub compacted: bool,
    pub compaction: CompactionStats,
    pub(super) compaction_detail: Option<String>,
    pub(super) compaction_hud: Option<String>,
}

impl TimelineEntry {
    /// Build a timeline row without mutating the source receipt.
    pub fn from_receipt(
        turn: usize,
        receipt: Receipt,
        answer: String,
        live_compaction: CompactionStats,
    ) -> Self {
        let compaction = if receipt.compaction.is_empty() {
            live_compaction
        } else {
            *receipt.compaction
        };
        Self {
            turn,
            ts_utc: receipt.ts_utc,
            model_id: receipt.model_id,
            task: short_text(&receipt.task, 120),
            turns: receipt.turns,
            tool_calls: receipt.tool_calls,
            duration_ms: receipt.duration_ms,
            kind: receipt.kind,
            outcome: receipt.outcome,
            failure_class: receipt.failure_class,
            usage: receipt.usage,
            answer,
            compacted: !compaction.is_empty(),
            compaction,
            compaction_detail: None,
            compaction_hud: None,
        }
    }

    pub(super) fn tokens(&self) -> Option<(u64, u64, Option<u64>)> {
        self.usage.as_ref().and_then(|usage| {
            usage.evidence.has_reported_counters().then_some((
                usage.prompt_tokens,
                usage.completion_tokens,
                usage.cached_tokens,
            ))
        })
    }
}

/// Additive worker payload carrying the receipt alongside its existing answer event.
pub struct TimelineSummary {
    pub route_id: String,
    pub receipt: Receipt,
    pub answer: String,
}

/// One immutable row in the discoverability palette.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteEntry {
    pub(super) kind: &'static str,
    pub(super) name: String,
    pub(super) description: String,
    pub(super) state: Option<McpState>,
    pub(super) action: PaletteAction,
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
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Enabled => "enabled",
            Self::AuthOk => "auth-ok",
            Self::Stale => "stale",
            Self::DiscoverOnly => "discover-only",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PaletteAction {
    Quit,
    Search,
    TrustDial,
    Timeline,
    Why,
    Palette,
    Prefill(&'static str),
    Describe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PickerKind {
    Model,
    Provider,
    Profile,
}

impl PickerKind {
    pub(super) fn title(self) -> &'static str {
        match self {
            Self::Model => " Select model ",
            Self::Provider => " Select provider ",
            Self::Profile => " Select profile ",
        }
    }

    pub(super) fn empty_message(self) -> &'static str {
        match self {
            Self::Model => "no catalog routes",
            Self::Provider => "no providers with usable credentials",
            Self::Profile => "no profiles",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PickerRow {
    pub(super) value: String,
    pub(super) label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Overlay {
    None,
    Search {
        query: String,
        selected: usize,
        original_scroll: usize,
    },
    CommandMenu {
        selected: usize,
    },
    Help,
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
    Picker {
        kind: PickerKind,
        selected: usize,
        rows: Vec<PickerRow>,
    },
}

/// Everything the render loop learns from the worker.
pub enum AgentEvent {
    Progress(String),
    Compaction(CompactionEvent),
    ModelStarted {
        route: String,
        started_at: DateTime<Utc>,
    },
    ModelFinished {
        route: String,
        usage: Option<Usage>,
    },
    ToolStarted {
        name: String,
        started_at: DateTime<Utc>,
    },
    ToolFinished {
        name: String,
    },
    Approval(ApprovalRequest),
    Usage(Usage),
    TaskReceipt(TimelineSummary),
    CancelledTurn(TimelineSummary),
    Answer(String),
    Failed(String),
}

/// Resolved inputs for one TUI session.
pub struct TuiConfig {
    pub terminal_capability: TerminalCapability,
    pub resolver: RouteResolver,
    pub model_id: String,
    pub profiles: Profiles,
    pub profile: String,
    pub law: Law,
    pub budget: Option<u64>,
    pub repo_root: PathBuf,
    pub workdir: PathBuf,
    pub palette_entries: Vec<PaletteEntry>,
    pub credentialed_providers: Vec<String>,
    pub resume: Option<RestoredSession>,
}

pub(super) struct UiInputs {
    pub(super) terminal_capability: TerminalCapability,
    pub(super) palette_entries: Vec<PaletteEntry>,
    pub(super) credentialed_providers: Vec<String>,
    pub(super) color_mode: ColorMode,
    pub(super) route_timing_history: RouteTimingHistory,
    pub(super) prompt_base_tokens: Arc<AtomicU64>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum TranscriptKind {
    Task,
    Answer,
    Progress,
    Approval,
    Error,
}

pub(super) struct TranscriptLine {
    pub(super) text: String,
    pub(super) kind: TranscriptKind,
}

#[derive(Debug, Clone)]
pub(super) struct SessionCost {
    pub(super) currency: Currency,
    pub(super) amount: f64,
    pub(super) uncertain: bool,
    pub(super) upper_bound: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ActiveTool {
    pub(super) name: String,
    pub(super) started_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ActiveModel {
    pub(super) route: String,
    pub(super) started_at: DateTime<Utc>,
}

#[derive(Debug, Default)]
pub(super) struct RouteTimingHistory {
    durations_by_route: BTreeMap<String, Vec<u64>>,
}

impl RouteTimingHistory {
    pub(super) fn from_receipts(
        resolver: &RouteResolver,
        receipts: impl IntoIterator<Item = Receipt>,
    ) -> Self {
        let mut unique_route_by_model = BTreeMap::<String, Option<String>>::new();
        for route_id in resolver.available() {
            let Ok(route) = resolver.resolve(&route_id) else {
                continue;
            };
            match unique_route_by_model.entry(route.model_id().to_owned()) {
                Entry::Vacant(entry) => {
                    entry.insert(Some(route.id().to_owned()));
                }
                Entry::Occupied(mut entry) => {
                    entry.insert(None);
                }
            }
        }

        let mut history = Self::default();
        for receipt in receipts {
            let Some(Some(route_id)) = unique_route_by_model.get(&receipt.model_id) else {
                continue;
            };
            history.record(route_id, &receipt);
        }
        history
    }

    pub(super) fn record(&mut self, route_id: &str, receipt: &Receipt) {
        if receipt.kind != ReceiptKind::Task || receipt.outcome != Outcome::Pass {
            return;
        }
        let Some(duration_ms) = receipt.duration_ms else {
            return;
        };
        let durations = self
            .durations_by_route
            .entry(route_id.to_owned())
            .or_default();
        let index = durations.partition_point(|duration| *duration <= duration_ms);
        durations.insert(index, duration_ms);
    }

    pub(super) fn typical_for(&self, route_id: &str) -> Option<u64> {
        let durations = self.durations_by_route.get(route_id)?;
        if durations.len() < MIN_TYPICAL_DURATION_SAMPLES {
            return None;
        }
        let upper = durations[durations.len() / 2];
        if durations.len() % 2 == 1 {
            Some(upper)
        } else {
            let lower = durations[durations.len() / 2 - 1];
            Some(lower.saturating_add(upper.saturating_sub(lower) / 2))
        }
    }
}

/// Unit-testable state for the renderer.
pub struct App {
    pub(super) terminal_capability: TerminalCapability,
    pub(super) status: Status,
    pub(super) working_since: Option<DateTime<Utc>>,
    pub(super) active_model: Option<ActiveModel>,
    pub(super) active_tool: Option<ActiveTool>,
    pub(super) resolver: RouteResolver,
    pub(super) route: ResolvedRoute,
    pub(super) profiles: Profiles,
    pub(super) active_profile: String,
    pub(super) effort: ThinkingEffort,
    pub(super) transcript: Vec<TranscriptLine>,
    pub(super) pending_approval: Option<ApprovalRequest>,
    pub(super) usage: Option<Usage>,
    pub(super) last_request_usage: Option<Usage>,
    pub(super) input: String,
    pub(super) pending_send: bool,
    pub(super) prompt_history: Vec<String>,
    pub(super) prompt_history_index: Option<usize>,
    pub(super) prompt_history_draft: Option<String>,
    pub(super) last_ctrl_c: Option<Instant>,
    pub(super) budget: Option<u64>,
    pub(super) budget_warned: bool,
    pub(super) color_mode: ColorMode,
    pub(super) scroll_back: usize,
    pub(super) max_scroll: Cell<usize>,
    pub(super) search_match_scroll: Cell<usize>,
    pub(super) scrubber: SharedScrubber,
    pub(super) local_offset: FixedOffset,
    pub(super) policy_view: PolicyView,
    pub(super) palette_entries: Vec<PaletteEntry>,
    pub(super) credentialed_providers: Vec<String>,
    pub(super) overlay: Overlay,
    pub(super) timeline: Vec<TimelineEntry>,
    pub(super) current_task_compaction: CompactionStats,
    pub(super) last_compaction_hud: Option<String>,
    pub(super) route_timing_history: RouteTimingHistory,
    pub(super) typical_duration_ms: Option<u64>,
    pub(super) prompt_base_tokens: Arc<AtomicU64>,
    pub(super) session_cost: Vec<SessionCost>,
    pub(super) session_cost_incomplete: bool,
    pub(super) session_allow: Vec<String>,
    pub(super) resumed: bool,
}

impl App {
    pub(super) fn new(
        resolver: RouteResolver,
        route: ResolvedRoute,
        budget: Option<u64>,
        scrubber: SharedScrubber,
        policy_view: PolicyView,
        inputs: UiInputs,
        profile_config: (Profiles, String),
    ) -> Self {
        let (profiles, active_profile) = profile_config;
        let UiInputs {
            terminal_capability,
            palette_entries: mcp_entries,
            credentialed_providers,
            color_mode,
            route_timing_history,
            prompt_base_tokens,
        } = inputs;
        let mut palette_entries = builtin_palette_entries();
        palette_entries.extend(mcp_entries);
        let execution_policy = profiles.effective(&active_profile, &route);
        let typical_duration_ms = route_timing_history.typical_for(route.id());
        Self {
            terminal_capability,
            status: if budget == Some(0) {
                Status::Blocked(BUDGET_REASON.into())
            } else {
                Status::Idle
            },
            working_since: None,
            active_model: None,
            active_tool: None,
            effort: effort_for(
                execution_policy.posture,
                route.thinking_dialect(),
                route.wire(),
            ),
            resolver,
            route,
            profiles,
            active_profile: execution_policy.profile,
            transcript: Vec::new(),
            pending_approval: None,
            usage: None,
            last_request_usage: None,
            input: String::new(),
            pending_send: false,
            prompt_history: Vec::new(),
            prompt_history_index: None,
            prompt_history_draft: None,
            last_ctrl_c: None,
            budget,
            budget_warned: false,
            color_mode,
            scroll_back: 0,
            max_scroll: Cell::new(0),
            search_match_scroll: Cell::new(0),
            scrubber,
            local_offset: *chrono::Local::now().offset(),
            policy_view,
            palette_entries,
            credentialed_providers,
            overlay: Overlay::None,
            timeline: Vec::new(),
            current_task_compaction: CompactionStats::default(),
            last_compaction_hud: None,
            route_timing_history,
            typical_duration_ms,
            prompt_base_tokens,
            session_cost: Vec::new(),
            session_cost_incomplete: false,
            session_allow: Vec::new(),
            resumed: false,
        }
    }

    pub(super) fn push_line(&mut self, text: &str, kind: TranscriptKind) {
        let text = self.terminal_capability.render_text(text);
        self.push_content_line(&text, kind);
    }

    pub(super) fn push_content_line(&mut self, text: &str, kind: TranscriptKind) {
        self.transcript.push(TranscriptLine {
            text: safe_line(&self.scrubber, text),
            kind,
        });
        self.scroll_back = 0;
    }

    pub(super) fn push_text(&mut self, prefix: &str, text: &str, kind: TranscriptKind) {
        let mut saw_line = false;
        for line in text.lines() {
            saw_line = true;
            self.push_content_line(&format!("{prefix}{line}"), kind);
        }
        if !saw_line {
            self.push_content_line(prefix, kind);
        }
    }

    pub(super) fn push_approval_line(&mut self, text: &str) {
        self.transcript.push(TranscriptLine {
            text: scrub_full_line(&self.scrubber, text),
            kind: TranscriptKind::Approval,
        });
        self.scroll_back = 0;
    }

    pub(super) fn open_search(&mut self) {
        let original_scroll = match &self.overlay {
            Overlay::Search {
                original_scroll, ..
            } => *original_scroll,
            _ => self.scroll_back,
        };
        self.search_match_scroll.set(self.scroll_back);
        self.overlay = Overlay::Search {
            query: String::new(),
            selected: 0,
            original_scroll,
        };
    }

    pub(super) fn set_status(&mut self, status: Status, now: DateTime<Utc>) {
        let entering_work = matches!(status, Status::Working)
            && !matches!(self.status, Status::Working | Status::FinishingInterrupted);
        self.status = status;
        if entering_work {
            self.working_since.get_or_insert(now);
        } else if !matches!(
            self.status,
            Status::Working | Status::FinishingInterrupted | Status::Waiting
        ) {
            self.working_since = None;
        }
    }

    pub(super) fn interrupt_turn(&mut self) {
        self.active_model = None;
        self.active_tool = None;
        self.set_status(Status::FinishingInterrupted, Utc::now());
    }

    pub(super) fn used_tokens(&self) -> Option<u64> {
        match self.usage.as_ref() {
            None => Some(0),
            Some(usage) if usage.evidence.has_reported_counters() => {
                Some(usage.prompt_tokens.saturating_add(usage.completion_tokens))
            }
            Some(_) => None,
        }
    }

    pub(super) fn budget_reached(&self) -> bool {
        self.budget
            .zip(self.used_tokens())
            .is_some_and(|(limit, used)| used >= limit)
    }

    pub(super) fn warn_before_budget(&mut self) {
        if self.budget_warned || self.budget_reached() {
            return;
        }
        let Some(usage) = self.usage.as_ref() else {
            return;
        };
        if !usage.evidence.is_measured() {
            return;
        }
        let Some((limit, used)) = self.budget.zip(self.used_tokens()) else {
            return;
        };
        if used < budget_warning_threshold(limit) {
            return;
        }

        self.budget_warned = true;
        self.push_line(
            &format!(
                "budget warning: {used} tokens used of {limit} budget - session will stop at the budget"
            ),
            TranscriptKind::Progress,
        );
    }

    pub(super) fn dispatch(&mut self) -> Option<String> {
        if matches!(
            self.status,
            Status::Working | Status::FinishingInterrupted | Status::Waiting
        ) || self.budget_reached()
        {
            return None;
        }
        let task = self.input.trim().to_owned();
        if task.is_empty() {
            self.pending_send = false;
            return None;
        }
        self.remember_prompt(&task);
        self.input.clear();
        self.pending_send = false;
        self.current_task_compaction = CompactionStats::default();
        self.last_compaction_hud = None;
        self.active_model = None;
        self.active_tool = None;
        self.push_content_line(&task, TranscriptKind::Task);
        self.set_status(Status::Working, Utc::now());
        Some(task)
    }

    fn remember_prompt(&mut self, prompt: &str) {
        self.end_prompt_history_recall();
        if self
            .prompt_history
            .last()
            .is_some_and(|previous| previous == prompt)
        {
            return;
        }
        if self.prompt_history.len() == PROMPT_HISTORY_CAPACITY {
            self.prompt_history.remove(0);
        }
        self.prompt_history.push(prompt.to_owned());
    }

    pub(super) fn recall_previous_prompt(&mut self) {
        if self.prompt_history.is_empty() {
            return;
        }
        let index = self.prompt_history_index.map_or_else(
            || {
                self.prompt_history_draft = Some(self.input.clone());
                self.prompt_history.len() - 1
            },
            |index| index.saturating_sub(1),
        );
        self.prompt_history_index = Some(index);
        self.input.clone_from(&self.prompt_history[index]);
    }

    pub(super) fn recall_next_prompt(&mut self) {
        let Some(index) = self.prompt_history_index else {
            return;
        };
        if index.saturating_add(1) < self.prompt_history.len() {
            let next = index + 1;
            self.prompt_history_index = Some(next);
            self.input.clone_from(&self.prompt_history[next]);
            return;
        }
        self.prompt_history_index = None;
        self.input = self.prompt_history_draft.take().unwrap_or_default();
        if self.input.trim().is_empty() {
            self.pending_send = false;
        }
    }

    pub(super) fn end_prompt_history_recall(&mut self) {
        self.prompt_history_index = None;
        self.prompt_history_draft = None;
    }

    pub(super) fn switch_route(&mut self, route: ResolvedRoute) {
        self.last_compaction_hud = None;
        self.last_request_usage = None;
        let policy = self.profiles.effective(&self.active_profile, &route);
        self.effort = effort_for(policy.posture, route.thinking_dialect(), route.wire());
        self.active_profile = policy.profile;
        self.route = route;
        self.refresh_typical_duration();
        self.invalidate_prompt_estimate();
        self.push_line(
            &format!(
                "switched to {} - context kept, cache resets",
                self.route.id()
            ),
            TranscriptKind::Progress,
        );
    }

    pub(super) fn set_effort(&mut self, effort: ThinkingEffort) {
        self.effort = effort;
        self.invalidate_prompt_estimate();
        self.push_line(
            &format!("reasoning effort set to {}", effort_name(effort)),
            TranscriptKind::Progress,
        );
    }

    pub(super) fn answer_approval(&mut self, approved: bool) {
        self.answer_approval_with_rule(approved, false);
    }

    pub(super) fn answer_approval_with_rule(&mut self, approved: bool, always: bool) {
        if let Some(request) = self.pending_approval.take() {
            if approved && always && !self.session_allow.contains(&request.prompt) {
                self.session_allow.push(request.prompt.clone());
            }
            let _ = request.reply.send(approved);
            self.push_line(
                if approved && always {
                    "approval: yes, always this session"
                } else if approved {
                    "approval: yes"
                } else {
                    "approval: no"
                },
                TranscriptKind::Progress,
            );
            self.set_status(Status::Working, Utc::now());
        }
    }

    pub(super) fn close_pending_approval(&mut self) {
        drop(self.pending_approval.take());
    }

    pub(super) fn add_session_cost(
        &mut self,
        currency: Currency,
        amount: f64,
        uncertain: bool,
        upper_bound: bool,
    ) {
        if let Some(total) = self
            .session_cost
            .iter_mut()
            .find(|total| total.currency == currency)
        {
            total.amount += amount;
            total.uncertain |= uncertain;
            total.upper_bound |= upper_bound;
        } else {
            self.session_cost.push(SessionCost {
                currency,
                amount,
                uncertain,
                upper_bound,
            });
        }
    }

    pub(super) fn mark_session_cost_incomplete(&mut self) {
        self.session_cost_incomplete = true;
    }

    pub(super) fn record_route_duration(&mut self, route_id: &str, receipt: &Receipt) {
        let route_matches_receipt = self
            .resolver
            .resolve(route_id)
            .ok()
            .is_some_and(|route| route.model_id() == receipt.model_id);
        if !route_matches_receipt {
            return;
        }
        self.route_timing_history.record(route_id, receipt);
        if route_id == self.route.id() {
            self.refresh_typical_duration();
        }
    }

    fn refresh_typical_duration(&mut self) {
        self.typical_duration_ms = self.route_timing_history.typical_for(self.route.id());
    }

    pub(super) fn invalidate_prompt_estimate(&self) {
        self.prompt_base_tokens
            .store(PROMPT_ESTIMATE_UNAVAILABLE, Ordering::Release);
    }

    pub(super) fn prompt_cost_preview(&self, now: DateTime<Utc>) -> Option<String> {
        if !matches!(self.status, Status::Idle | Status::Blocked(_)) || self.pending_send {
            return None;
        }
        let task = self.input.trim();
        if task.is_empty() || task.starts_with('/') || self.route.class() != RouteClass::Api {
            return None;
        }
        let base_tokens = self.prompt_base_tokens.load(Ordering::Acquire);
        if base_tokens == PROMPT_ESTIMATE_UNAVAILABLE {
            return None;
        }
        let message = ChatMessage {
            role: "user".into(),
            content: Some(task.to_owned()),
            parts: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
        };
        let prompt_tokens = base_tokens.saturating_add(estimate_message_tokens(&[message], false));
        let quote = self.route.price_at(now)?;
        let amount = cost_of(&quote, prompt_tokens, 0, 0)?;
        let display = money_with_gloss(amount, quote.currency, self.resolver.fx(), now);
        Some(format!(
            "prompt ~{display} estimated if uncached; output unknown"
        ))
    }

    pub(super) fn session_money(&self, now: DateTime<Utc>) -> String {
        if self.session_cost_incomplete
            && (self.session_cost.iter().any(|total| total.upper_bound)
                || self.session_cost.is_empty()
                || self
                    .session_cost
                    .iter()
                    .all(|total| total.amount.abs() <= f64::EPSILON))
        {
            return "unavailable - meter incomplete".into();
        }
        let display = if self.session_cost.is_empty() {
            if self.route.class() == RouteClass::Local {
                "no billed tokens".into()
            } else {
                self.route.price_at(now).map_or_else(
                    || "-".into(),
                    |quote| {
                        let mut display =
                            money_with_gloss(0.0, quote.currency, self.resolver.fx(), now);
                        if quote.confidence == PriceConfidence::VerifyLive {
                            display.push('*');
                        }
                        display
                    },
                )
            }
        } else {
            let visible_totals = self
                .session_cost
                .iter()
                .filter(|total| !self.session_cost_incomplete || total.amount.abs() > f64::EPSILON)
                .count();
            let mixed = visible_totals > 1;
            [Currency::Cny, Currency::Usd]
                .into_iter()
                .filter_map(|currency| {
                    self.session_cost
                        .iter()
                        .find(|total| total.currency == currency)
                        .filter(|total| {
                            !self.session_cost_incomplete || total.amount.abs() > f64::EPSILON
                        })
                        .map(|total| {
                            let mut display = if mixed {
                                money(total.amount, total.currency)
                            } else {
                                money_with_gloss(
                                    total.amount,
                                    total.currency,
                                    self.resolver.fx(),
                                    now,
                                )
                            };
                            if total.uncertain {
                                display.push('*');
                            }
                            if total.upper_bound {
                                display.insert_str(0, "at most ");
                            }
                            if self.session_cost_incomplete {
                                display.insert(0, '~');
                            }
                            display
                        })
                })
                .collect::<Vec<_>>()
                .join(" · ")
        };
        let display = if self.session_cost_incomplete {
            format!("{display} - subtotal; meter incomplete")
        } else {
            display
        };
        self.terminal_capability.render_text(&display).into_owned()
    }

    pub(super) fn hud_line(&self, now: DateTime<Utc>) -> String {
        let mut line = format!("session {}", self.session_money(now));
        match self.usage.as_ref() {
            None => line.push_str(" · no usage yet"),
            Some(usage) if usage.evidence == UsageEvidence::Measured => {
                line.push_str(&format!(
                    " · in {} · out {}",
                    usage.prompt_tokens, usage.completion_tokens
                ));
                if let Some(pct) = cache_hit_pct(usage.prompt_tokens, usage.cached_tokens) {
                    line.push_str(&format!(" · cache {pct:.0}%"));
                }
            }
            Some(usage) if usage.evidence == UsageEvidence::Partial => {
                line.push_str(&format!(
                    " · in ~{} · out ~{} · token lower bound",
                    usage.prompt_tokens, usage.completion_tokens
                ));
            }
            Some(_) => line.push_str(" · tokens unavailable - usage unknown"),
        }
        if let (Some(context), Some(usage)) =
            (self.route.context(), self.last_request_usage.as_ref())
        {
            match usage.evidence {
                UsageEvidence::Measured => {
                    let percent = format_context_percent(usage.prompt_tokens, context);
                    line.push_str(&format!(" · ctx {percent}"));
                }
                UsageEvidence::Partial => {
                    let percent = format_context_percent(usage.prompt_tokens, context);
                    line.push_str(&format!(" · ctx ~{percent}"));
                }
                UsageEvidence::Unknown => {}
            }
        }
        if let Some(compaction) = &self.last_compaction_hud {
            line.push_str(" · ");
            line.push_str(compaction);
        }
        if let Some(preview) = self.prompt_cost_preview(now) {
            line.push_str(" · ");
            line.push_str(&preview);
        }
        if let Some(peak_status) = self.route.peak_status(now, self.local_offset) {
            line.push_str(" · ");
            line.push_str(&peak_status);
        }
        line.push_str(&format!(" · profile {}", self.active_profile));
        if self.resumed {
            line.push_str(" · resumed");
        }
        if let Some(limit) = self.budget {
            match (self.used_tokens(), self.usage.as_ref()) {
                (Some(used), Some(usage)) if usage.evidence == UsageEvidence::Partial => {
                    let pct = budget_pct(used, limit);
                    line.push_str(&format!(
                        " · {} ~{pct}% ~{used}/{limit} lower bound",
                        budget_bar(used, limit)
                    ));
                }
                (Some(used), _) => {
                    let pct = budget_pct(used, limit);
                    line.push_str(&format!(
                        " · {} {pct}% {used}/{limit}",
                        budget_bar(used, limit)
                    ));
                }
                (None, _) => {
                    line.push_str(&format!(" · budget usage unavailable/{limit}"));
                }
            }
        }
        if let Some(quote) = self.route.price_at(now) {
            if quote.confidence == PriceConfidence::VerifyLive {
                line.push_str(" · *price verify_live");
            }
        }
        let line = self.terminal_capability.render_text(&line);
        safe_line(&self.scrubber, &line)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SearchMatchLine {
    pub(super) line_index: usize,
    pub(super) ranges: Vec<Range<usize>>,
}

/// Search only the scrubbed, escaped transcript projection retained by the UI.
pub(super) fn search_match_lines(
    transcript: &[TranscriptLine],
    query: &str,
) -> Vec<SearchMatchLine> {
    if query.is_empty() {
        return Vec::new();
    }
    transcript
        .iter()
        .enumerate()
        .filter_map(|(line_index, line)| {
            let ranges = search_match_ranges(&line.text, query);
            (!ranges.is_empty()).then_some(SearchMatchLine { line_index, ranges })
        })
        .collect()
}

pub(super) fn search_match_count(matches: &[SearchMatchLine]) -> usize {
    matches.iter().fold(0, |count, matched| {
        count.saturating_add(matched.ranges.len())
    })
}

pub(super) fn search_match_position(
    matches: &[SearchMatchLine],
    selected: usize,
) -> Option<(usize, usize)> {
    let mut remaining = selected.min(search_match_count(matches).saturating_sub(1));
    for matched in matches {
        if remaining < matched.ranges.len() {
            return Some((matched.line_index, remaining));
        }
        remaining = remaining.saturating_sub(matched.ranges.len());
    }
    None
}

fn search_match_ranges(text: &str, query: &str) -> Vec<Range<usize>> {
    let text_bytes = text.as_bytes();
    let query_bytes = query.as_bytes();
    let mut ranges = Vec::new();
    let mut offset = 0;

    // ASCII-only case folding is byte-length preserving, so these byte ranges map exactly
    // onto the displayed UTF-8 text. Non-ASCII text is matched literally and case-sensitively.
    while query_bytes.len() <= text_bytes.len().saturating_sub(offset) {
        let Some(relative) = text_bytes[offset..]
            .windows(query_bytes.len())
            .position(|candidate| candidate.eq_ignore_ascii_case(query_bytes))
        else {
            break;
        };
        let start = offset.saturating_add(relative);
        let end = start.saturating_add(query_bytes.len());
        if text.is_char_boundary(start) && text.is_char_boundary(end) {
            ranges.push(start..end);
            offset = end;
        } else {
            offset = start.saturating_add(1);
        }
    }
    ranges
}

fn budget_pct(used: u64, limit: u64) -> u64 {
    if limit == 0 {
        100
    } else {
        used.saturating_mul(100).checked_div(limit).unwrap_or(100)
    }
    .min(100)
}

fn budget_warning_threshold(limit: u64) -> u64 {
    let (numerator, denominator) = BUDGET_WARN_FRACTION;
    let whole = limit / denominator * numerator;
    let remainder = limit % denominator * numerator;
    whole.saturating_add(remainder.div_ceil(denominator))
}

impl Drop for App {
    fn drop(&mut self) {
        self.close_pending_approval();
    }
}
