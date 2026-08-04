//! TUI domain state, immutable configuration, and application state transitions.

use crate::palette::{builtin_palette_entries, short_text};
use crate::render::budget_bar;
use crate::session::{effort_for, effort_name, safe_line, scrub_full_line};
use crate::worker::ApprovalRequest;
use crate::{SharedScrubber, BUDGET_REASON};
use chrono::{DateTime, FixedOffset, Utc};
use nh_core::agent::CompactionEvent;
use nh_core::receipt::{CompactionStats, FailureClass, Outcome, Receipt};
use nh_core::session_ledger::RestoredSession;
use nh_core::wire::{cache_hit_pct, ThinkingEffort, Usage, UsageEvidence};
use nh_law::{Law, PolicyView};
use nh_routes::{
    money, money_with_gloss, Currency, PriceConfidence, Profiles, ResolvedRoute, RouteClass,
    RouteResolver,
};
use std::cell::Cell;
use std::path::PathBuf;

/// The single status shown by the semáforo.
#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Idle,
    Working,
    Waiting,
    Blocked(String),
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
    Answer(String),
    Failed(String),
}

/// Resolved inputs for one TUI session.
pub struct TuiConfig {
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

pub(super) struct UiDiscovery {
    pub(super) palette_entries: Vec<PaletteEntry>,
    pub(super) credentialed_providers: Vec<String>,
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

/// Unit-testable state for the renderer.
pub struct App {
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
    pub(super) input: String,
    pub(super) budget: Option<u64>,
    pub(super) scroll_back: u16,
    pub(super) max_scroll: Cell<u16>,
    pub(super) scrubber: SharedScrubber,
    pub(super) local_offset: FixedOffset,
    pub(super) policy_view: PolicyView,
    pub(super) palette_entries: Vec<PaletteEntry>,
    pub(super) credentialed_providers: Vec<String>,
    pub(super) overlay: Overlay,
    pub(super) timeline: Vec<TimelineEntry>,
    pub(super) current_task_compaction: CompactionStats,
    pub(super) last_compaction_hud: Option<String>,
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
        discovery: UiDiscovery,
        profile_config: (Profiles, String),
    ) -> Self {
        let (profiles, active_profile) = profile_config;
        let UiDiscovery {
            palette_entries: mcp_entries,
            credentialed_providers,
        } = discovery;
        let mut palette_entries = builtin_palette_entries();
        palette_entries.extend(mcp_entries);
        let execution_policy = profiles.effective(&active_profile, &route);
        Self {
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
            input: String::new(),
            budget,
            scroll_back: 0,
            max_scroll: Cell::new(0),
            scrubber,
            local_offset: *chrono::Local::now().offset(),
            policy_view,
            palette_entries,
            credentialed_providers,
            overlay: Overlay::None,
            timeline: Vec::new(),
            current_task_compaction: CompactionStats::default(),
            last_compaction_hud: None,
            session_cost: Vec::new(),
            session_cost_incomplete: false,
            session_allow: Vec::new(),
            resumed: false,
        }
    }

    pub(super) fn push_line(&mut self, text: &str, kind: TranscriptKind) {
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
            self.push_line(&format!("{prefix}{line}"), kind);
        }
        if !saw_line {
            self.push_line(prefix, kind);
        }
    }

    pub(super) fn push_approval_line(&mut self, text: &str) {
        self.transcript.push(TranscriptLine {
            text: scrub_full_line(&self.scrubber, text),
            kind: TranscriptKind::Approval,
        });
        self.scroll_back = 0;
    }

    pub(super) fn set_status(&mut self, status: Status, now: DateTime<Utc>) {
        let entering_work =
            matches!(status, Status::Working) && !matches!(self.status, Status::Working);
        self.status = status;
        if entering_work {
            self.working_since.get_or_insert(now);
        } else if !matches!(self.status, Status::Working | Status::Waiting) {
            self.working_since = None;
        }
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

    pub(super) fn dispatch(&mut self) -> Option<String> {
        if matches!(self.status, Status::Working | Status::Waiting) || self.budget_reached() {
            return None;
        }
        let task = self.input.trim().to_owned();
        if task.is_empty() {
            return None;
        }
        self.input.clear();
        self.current_task_compaction = CompactionStats::default();
        self.last_compaction_hud = None;
        self.active_model = None;
        self.active_tool = None;
        self.push_line(&task, TranscriptKind::Task);
        self.set_status(Status::Working, Utc::now());
        Some(task)
    }

    pub(super) fn switch_route(&mut self, route: ResolvedRoute) {
        self.last_compaction_hud = None;
        let policy = self.profiles.effective(&self.active_profile, &route);
        self.effort = effort_for(policy.posture, route.thinking_dialect(), route.wire());
        self.active_profile = policy.profile;
        self.route = route;
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

    pub(super) fn add_session_cost(&mut self, currency: Currency, amount: f64, uncertain: bool) {
        if let Some(total) = self
            .session_cost
            .iter_mut()
            .find(|total| total.currency == currency)
        {
            total.amount += amount;
            total.uncertain |= uncertain;
        } else {
            self.session_cost.push(SessionCost {
                currency,
                amount,
                uncertain,
            });
        }
    }

    pub(super) fn mark_session_cost_incomplete(&mut self) {
        self.session_cost_incomplete = true;
    }

    pub(super) fn session_money(&self, now: DateTime<Utc>) -> String {
        if self.session_cost_incomplete
            && (self.session_cost.is_empty()
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
                            if self.session_cost_incomplete {
                                display.insert(0, '~');
                            }
                            display
                        })
                })
                .collect::<Vec<_>>()
                .join(" · ")
        };
        if self.session_cost_incomplete {
            format!("{display} - subtotal; meter incomplete")
        } else {
            display
        }
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
        if let Some(compaction) = &self.last_compaction_hud {
            line.push_str(" · ");
            line.push_str(compaction);
        }
        line.push_str(&format!(
            " · {}",
            self.route.peak_status(now, self.local_offset)
        ));
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
        safe_line(&self.scrubber, &line)
    }
}

fn budget_pct(used: u64, limit: u64) -> u64 {
    if limit == 0 {
        100
    } else {
        used.saturating_mul(100).checked_div(limit).unwrap_or(100)
    }
    .min(100)
}

impl Drop for App {
    fn drop(&mut self) {
        self.close_pending_approval();
    }
}
