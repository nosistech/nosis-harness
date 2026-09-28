//! Event reduction, timeline projection, and session cost accounting.

use crate::session::safe_line;
use crate::state::{
    AgentEvent, App, RecoveryHint, Status, TimelineEntry, TimelineReview, TranscriptKind,
    MAX_SESSION_REVIEW_BYTES,
};
use crate::{APPROVAL_LEGEND, APPROVAL_ONCE_LEGEND};
use chrono::{DateTime, TimeZone, Utc};
use nh_core::agent::{result_notice, CompactionEvent};
use nh_core::cost::{
    compaction_cost, turn_cost, CompactionCostVerdict, TurnCostVerdict, PRICE_VERIFY_LIVE,
};
use nh_core::receipt::{CompactionStats, FailureClass, Outcome, ReceiptKind};
use nh_core::terminal_capability::TerminalCapability;
use nh_core::wire::{cache_hit_pct, ProviderFailureKind, Usage, UsageEvidence};
use nh_routes::{ResolvedRoute, RouteClass, RouteResolver, LOCAL_METER_COPY};
use nh_tools::{CommandOutcome, FileChangeKind, ReviewText, ToolReviewItem};

pub(super) fn outcome_name(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::Pass => "completed",
        Outcome::Fail => "fail",
        Outcome::Partial => "partial",
        Outcome::Skip => "skip",
        Outcome::Timeout => "timeout",
    }
}

pub(super) fn failure_class_name(class: FailureClass) -> &'static str {
    match class {
        FailureClass::Context => "context",
        FailureClass::Constraint => "constraint",
        FailureClass::Filtered => "filtered",
        FailureClass::Verification => "verification",
        FailureClass::Planning => "planning",
        FailureClass::Unreceipted => "unreceipted",
    }
}

pub(super) fn measured_duration(duration_ms: u64) -> String {
    if duration_ms < 1_000 {
        return format!("{duration_ms}ms");
    }
    let seconds = duration_ms / 1_000;
    let milliseconds = duration_ms % 1_000;
    if milliseconds == 0 {
        format!("{seconds}s")
    } else {
        format!("{seconds}.{milliseconds:03}s")
    }
}

#[cfg(test)]
pub(super) fn timeline_row(entry: &TimelineEntry) -> String {
    timeline_row_for(TerminalCapability::Unicode, entry)
}

pub(super) fn timeline_row_for(
    terminal_capability: TerminalCapability,
    entry: &TimelineEntry,
) -> String {
    let compacted = if entry.compacted { "  [compact]" } else { "" };
    let duration = entry
        .duration_ms
        .map(|duration_ms| format!("  {}", measured_duration(duration_ms)))
        .unwrap_or_default();
    let tokens = match (entry.usage.as_ref(), entry.tokens()) {
        (Some(usage), Some((input, output, _))) if usage.evidence == UsageEvidence::Partial => {
            format!("~{input}/~{output} lower bound")
        }
        (Some(usage), Some((input, output, cached))) => {
            let mut tokens = format!("{input}/{output}");
            if usage.evidence.is_measured() {
                if let Some(cached) = cached {
                    tokens.push_str(&format!("/{cached}"));
                    if let Some(pct) = cache_hit_pct(input, Some(cached)) {
                        tokens.push_str(&format!(" cache {pct:.0}%"));
                    }
                }
            }
            tokens
        }
        (Some(_), None) => "usage unknown".into(),
        (None, _) => "usage unreported".into(),
    };
    let line = format!(
        "#{}  {}{duration}  {tokens}{compacted}",
        entry.turn,
        if entry.kind == ReceiptKind::CancelledTurn {
            "cancelled"
        } else {
            outcome_name(entry.outcome)
        }
    );
    terminal_capability.render_text(&line).into_owned()
}

#[cfg(test)]
pub(super) fn timeline_detail_lines(entry: &TimelineEntry) -> Vec<String> {
    timeline_detail_lines_for(TerminalCapability::Unicode, entry)
}

pub(super) fn timeline_detail_lines_for(
    terminal_capability: TerminalCapability,
    entry: &TimelineEntry,
) -> Vec<String> {
    let failure = entry
        .failure_class
        .map(failure_class_name)
        .unwrap_or("none");
    let tokens = match (entry.usage.as_ref(), entry.tokens()) {
        (Some(usage), Some((input, output, _))) if usage.evidence == UsageEvidence::Partial => {
            format!("tokens: ~{input} in / ~{output} out - lower bound")
        }
        (Some(usage), Some((input, output, cached))) => {
            let mut tokens = format!("tokens: {input} in / {output} out");
            if usage.evidence.is_measured() {
                if let Some(cached) = cached {
                    tokens.push_str(&format!(" / {cached} cached"));
                    if let Some(pct) = cache_hit_pct(input, Some(cached)) {
                        tokens.push_str(&format!(" | cache {pct:.0}%"));
                    }
                }
            }
            tokens
        }
        (Some(_), None) => "tokens: unavailable - usage unknown".into(),
        (None, _) => "tokens: unavailable - usage unreported".into(),
    };
    let verification = match &entry.review {
        TimelineReview::Live(_) => {
            "verification: live tool evidence below; correctness not independently verified"
        }
        TimelineReview::Unavailable | TimelineReview::Expired { .. } => {
            "verification: not recorded"
        }
    };
    let mut lines = vec![
        format!("TURN #{}", entry.turn),
        format!("timestamp: {}", entry.ts_utc),
        format!("model: {}", entry.model_id),
        format!("task: {}", entry.task),
        format!(
            "kind: {}",
            if entry.kind == ReceiptKind::CancelledTurn {
                "cancelled_turn"
            } else {
                "task"
            }
        ),
        format!("execution outcome: {}", outcome_name(entry.outcome)),
        verification.to_owned(),
        format!("agent turns: {}", entry.turns),
        format!("tool calls: {}", entry.tool_calls),
    ];
    if let Some(duration_ms) = entry.duration_ms {
        lines.push(format!("duration: {}", measured_duration(duration_ms)));
    }
    lines.extend([
        format!("failure class: {failure}"),
        tokens,
        format!("compacted: {}", if entry.compacted { "yes" } else { "no" }),
    ]);
    if let Some(detail) = &entry.compaction_detail {
        lines.push(detail.clone());
    }
    append_review_lines(&mut lines, &entry.review);
    lines.push(String::new());
    lines.push(format!("answer: {}", entry.answer));
    lines
        .into_iter()
        .map(|line| terminal_capability.render_text(&line).into_owned())
        .collect()
}

fn append_review_lines(lines: &mut Vec<String>, review: &TimelineReview) {
    lines.push(String::new());
    lines.push("OBSERVED BUILT-IN TOOL WORK".to_owned());
    match review {
        TimelineReview::Unavailable => {
            lines.push(
                "details unavailable - review evidence is live-session only and is not stored in receipts"
                    .to_owned(),
            );
        }
        TimelineReview::Expired { observed_items } => {
            lines.push(format!(
                "{observed_items} observed items no longer retained - the live-session review cap was reached"
            ));
        }
        TimelineReview::Live(review) => {
            lines.push(
                "live session only - built-in tool fragments observed at completion; current files and other tool effects may differ"
                    .to_owned(),
            );
            if review.incomplete {
                lines.push(
                    "review evidence incomplete - a built-in tool failed before an outcome could be retained"
                        .to_owned(),
                );
            }
            if review.items.is_empty() {
                lines.push(
                    "no built-in file publication or shell command outcome was retained".to_owned(),
                );
            }
            for item in &review.items {
                match item {
                    ToolReviewItem::FileChange {
                        kind,
                        path,
                        before,
                        after,
                    } => {
                        lines.push(String::new());
                        lines.push(format!(
                            "file {}: {}",
                            match kind {
                                FileChangeKind::Created => "created",
                                FileChangeKind::Edited => "edited",
                            },
                            path.text
                        ));
                        if path.truncated {
                            lines.push("  ... [path truncated]".to_owned());
                        }
                        if let Some(before) = before {
                            append_review_fragment(lines, "- ", before);
                        }
                        append_review_fragment(lines, "+ ", after);
                    }
                    ToolReviewItem::Command { command, outcome } => {
                        lines.push(String::new());
                        lines.push(format!("command: {}", command.text));
                        if command.truncated {
                            lines.push("  ... [command truncated]".to_owned());
                        }
                        lines.push(format!("outcome: {}", command_outcome(*outcome)));
                    }
                }
            }
            if review.omitted > 0 {
                lines.push(format!(
                    "{} additional review items omitted by the per-task cap",
                    review.omitted
                ));
            }
            lines.push(final_state_check(review));
        }
    }
}

fn append_review_fragment(lines: &mut Vec<String>, prefix: &str, text: &ReviewText) {
    if text.text.is_empty() {
        lines.push(format!("{prefix}<empty>"));
    } else {
        lines.extend(text.text.split('\n').map(|line| format!("{prefix}{line}")));
    }
    if text.truncated {
        lines.push(format!("{prefix}... [fragment truncated]"));
    }
}

fn command_outcome(outcome: CommandOutcome) -> String {
    match outcome {
        CommandOutcome::Exited(Some(code)) => {
            format!("exited {code} - execution only; result not independently verified")
        }
        CommandOutcome::Exited(None) => {
            "exit status unavailable - execution only; result not independently verified".to_owned()
        }
        CommandOutcome::Blocked => "blocked before execution".to_owned(),
        CommandOutcome::Denied => "denied before execution".to_owned(),
        CommandOutcome::CancelledBeforeStart => "cancelled before execution".to_owned(),
        CommandOutcome::Cancelled {
            termination_complete: true,
        } => "cancelled".to_owned(),
        CommandOutcome::Cancelled {
            termination_complete: false,
        } => "cancelled; process-tree termination incomplete".to_owned(),
        CommandOutcome::TimedOut {
            termination_complete: true,
        } => "timed out".to_owned(),
        CommandOutcome::TimedOut {
            termination_complete: false,
        } => "timed out; process-tree termination incomplete".to_owned(),
    }
}

fn final_state_check(review: &crate::state::TaskReview) -> String {
    if review.incomplete {
        return "final-state check: evidence is incomplete because a built-in tool failed before an outcome could be retained; no final-state inference"
            .to_owned();
    }
    if review.omitted > 0 {
        return "final-state check: evidence is incomplete because later items may have been omitted; no final-state inference"
            .to_owned();
    }
    if review.items.iter().any(|item| {
        matches!(
            item,
            ToolReviewItem::Command {
                outcome: CommandOutcome::Cancelled {
                    termination_complete: false,
                } | CommandOutcome::TimedOut {
                    termination_complete: false,
                },
                ..
            }
        )
    }) {
        return "final-state check: process-tree termination was incomplete during this task; no final-state inference"
            .to_owned();
    }
    let Some(last_change) = review
        .items
        .iter()
        .rposition(|item| matches!(item, ToolReviewItem::FileChange { .. }))
    else {
        return "final-state check: no built-in file publication was retained; other tools may still have changed files"
            .to_owned();
    };
    if review.items.iter().skip(last_change + 1).any(|item| {
        matches!(
            item,
            ToolReviewItem::Command {
                outcome: CommandOutcome::Cancelled { .. } | CommandOutcome::TimedOut { .. },
                ..
            }
        )
    }) {
        return "final-state check: a later command started but did not complete; no final-state inference"
            .to_owned();
    }
    let completed = review
        .items
        .iter()
        .skip(last_change + 1)
        .rev()
        .find_map(|item| {
            let ToolReviewItem::Command { outcome, .. } = item else {
                return None;
            };
            matches!(outcome, CommandOutcome::Exited(_)).then_some(*outcome)
        });
    match completed {
        Some(CommandOutcome::Exited(Some(0))) => {
            "final-state check: a later command exited 0; what it checked is not independently verified"
                .to_owned()
        }
        Some(CommandOutcome::Exited(Some(code))) => {
            format!("final-state check: a later command exited {code}")
        }
        Some(CommandOutcome::Exited(None)) => {
            "final-state check: a later command completed with unknown exit status".to_owned()
        }
        _ => "final-state check: no completed command was recorded after the last built-in file publication"
            .to_owned(),
    }
}

struct CompactionEffect {
    suffix: String,
    hud: String,
}

fn compaction_effect(
    resolver: &RouteResolver,
    route: Option<&ResolvedRoute>,
    stats: CompactionStats,
) -> CompactionEffect {
    let occurred_at = stats
        .occurred_at_unix_seconds
        .and_then(|seconds| Utc.timestamp_opt(seconds, 0).single());
    let cost = compaction_cost(
        resolver,
        route,
        stats.events,
        stats.estimated_tokens_elided,
        stats.preceding_cached_tokens,
        occurred_at,
    );
    let suffix = cost.suffix();
    let hud = match cost {
        CompactionCostVerdict::NotStated(_) => format!(
            "compact ~{}t · net not stated",
            stats.estimated_tokens_elided
        ),
        CompactionCostVerdict::Priced(cost) => format!(
            "compact ~{}t · next-call {} {}",
            stats.estimated_tokens_elided, cost.net_label, cost.net_display
        ),
    };
    CompactionEffect { suffix, hud }
}

fn record_compaction_event(stats: &mut CompactionStats, event: &CompactionEvent) {
    if let Some(unix_seconds) = event.occurred_at_unix_seconds {
        stats.record_at(
            event.messages_elided,
            event.estimated_tokens_elided,
            event.preceding_cached_tokens,
            unix_seconds,
        );
    } else {
        stats.record(
            event.messages_elided,
            event.estimated_tokens_elided,
            event.preceding_cached_tokens,
        );
    }
}

fn compaction_fact_line(stats: CompactionStats) -> String {
    let events = if stats.events == 1 { "event" } else { "events" };
    let messages = if stats.messages_elided == 1 {
        "message"
    } else {
        "messages"
    };
    format!(
        "compaction {} {events} · {} {messages} elided · ~{} tokens elided",
        stats.events, stats.messages_elided, stats.estimated_tokens_elided
    )
}

/// Fold one worker event into application state.
pub fn apply_event(app: &mut App, event: AgentEvent) -> &Status {
    match event {
        AgentEvent::Progress(line) => {
            app.push_line(&line, TranscriptKind::Progress);
        }
        AgentEvent::ToolReview(item) => {
            app.current_task_review.record(item);
        }
        AgentEvent::ToolReviewIncomplete => {
            app.current_task_review.mark_incomplete();
        }
        AgentEvent::Compaction(event) => {
            record_compaction_event(&mut app.current_task_compaction, &event);
            let mut stats = CompactionStats::default();
            record_compaction_event(&mut stats, &event);
            let effect = compaction_effect(&app.resolver, Some(&app.route), stats);
            app.push_line(
                &format!(
                    "context ~{}% - compacted {} earlier messages · ~{} tokens elided{}",
                    event.context_percent,
                    event.messages_elided,
                    event.estimated_tokens_elided,
                    effect.suffix
                ),
                TranscriptKind::Progress,
            );
        }
        AgentEvent::ModelStarted { route, started_at } => {
            app.active_model = Some(crate::state::ActiveModel { route, started_at });
        }
        AgentEvent::ModelFinished { route, usage } => {
            if app
                .active_model
                .as_ref()
                .is_some_and(|request| request.route == route)
            {
                app.active_model = None;
            }
            if app.route.id() == route {
                app.last_request_usage = usage;
            }
        }
        AgentEvent::ToolStarted { name, started_at } => {
            app.active_model = None;
            app.active_tool = Some(crate::state::ActiveTool { name, started_at });
        }
        AgentEvent::ToolFinished { name } => {
            if app
                .active_tool
                .as_ref()
                .is_some_and(|tool| tool.name == name)
            {
                app.active_tool = None;
            }
        }
        AgentEvent::Approval(request) => {
            if request
                .repeat_key
                .as_ref()
                .is_some_and(|key| app.session_allow.contains(key))
            {
                let _ = request.reply.send(true);
                app.push_content_line(
                    &format!("auto-approved (session rule): {}", request.prompt),
                    TranscriptKind::Progress,
                );
                app.set_status(Status::Working, Utc::now());
            } else {
                let legend = if request.repeat_key.is_some() {
                    APPROVAL_LEGEND
                } else {
                    APPROVAL_ONCE_LEGEND
                };
                let line = format!("approve: {}   {legend}", request.prompt);
                app.push_approval_line(&line);
                app.pending_approval = Some(request);
                app.set_status(Status::Waiting, Utc::now());
            }
        }
        AgentEvent::Usage(mut usage) => {
            let was_unavailable = app.budget_usage_unavailable();
            if app
                .usage
                .as_ref()
                .is_some_and(|prior| !prior.evidence.is_measured())
                && usage.evidence.is_measured()
            {
                usage.mark_unreported_component();
            }
            if app.route.class() == RouteClass::Api
                && (!usage.evidence.is_measured() || app.route.price_at(Utc::now()).is_none())
            {
                app.mark_session_cost_incomplete();
            }
            app.usage = Some(usage);
            app.warn_before_budget();
            if !was_unavailable && app.budget_usage_unavailable() {
                app.push_line(
                    "budget usage is incomplete or unavailable - no more tasks will be sent in this budgeted session",
                    TranscriptKind::Progress,
                );
            }
            if let Some(reason) = app.budget_block_reason() {
                app.set_status(Status::Blocked(reason.into()), Utc::now());
            }
        }
        AgentEvent::TaskReceipt(summary) => {
            record_timeline_summary(app, summary);
        }
        AgentEvent::CancelledTurn(summary) => {
            app.active_model = None;
            app.active_tool = None;
            app.push_line(
                "turn cancelled - the provider may still bill the completed request",
                TranscriptKind::Progress,
            );
            app.push_line(
                "next: inspect /review if tools ran; edit the queued draft or type a task, then press Enter when ready. Nothing was sent automatically.",
                TranscriptKind::Progress,
            );
            record_timeline_summary(app, summary);
            let status = if let Some(reason) = app.budget_block_reason() {
                Status::Blocked(reason.into())
            } else {
                Status::Idle
            };
            app.set_status(status, Utc::now());
        }
        AgentEvent::Answer(answer) => {
            app.active_model = None;
            app.active_tool = None;
            app.push_text("", &answer, TranscriptKind::Answer);
            app.push_line(result_notice(false), TranscriptKind::Progress);
            let status = if let Some(reason) = app.budget_block_reason() {
                Status::Blocked(reason.into())
            } else {
                Status::Idle
            };
            app.set_status(status, Utc::now());
        }
        AgentEvent::Failed(reason) => {
            apply_failure(app, &reason, None);
        }
        AgentEvent::RecoverableFailure {
            reason,
            recovery,
            retry_task,
        } => {
            if app.input.is_empty() {
                if let Some(task) = retry_task {
                    app.input = task;
                    app.input_cursor = None;
                    app.pending_send = false;
                }
            }
            apply_failure(app, &reason, Some(&recovery));
        }
    }
    &app.status
}

fn apply_failure(app: &mut App, reason: &str, hint: Option<&RecoveryHint>) {
    app.active_model = None;
    app.active_tool = None;
    let status_reason = safe_line(&app.scrubber, reason);
    let budget_reason = app.budget_block_reason();
    let what = reason
        .lines()
        .next()
        .filter(|line| !line.trim().is_empty())
        .unwrap_or("the task could not finish");
    let what = safe_line(&app.scrubber, what);
    let recovery = budget_reason.map_or_else(
        || hint.map_or_else(|| "retry the task or type /help".to_owned(), recovery_line),
        |reason| format!("{reason}; use /help or start a new session"),
    );
    if hint.is_some() {
        app.push_line(&format!("! {what}"), TranscriptKind::Error);
        app.push_line(&format!("next: {recovery}"), TranscriptKind::Progress);
    } else {
        app.push_line(&format!("! {what} - {recovery}"), TranscriptKind::Error);
    }
    let status_reason = budget_reason.unwrap_or(status_reason.as_str()).to_owned();
    app.set_status(Status::Blocked(status_reason), Utc::now());
}

fn recovery_line(hint: &RecoveryHint) -> String {
    let action = match hint {
        RecoveryHint::Connection { key_entry } => format!(
            "in another terminal, run nh doctor; if the {key_entry} key is missing or rejected, run nh key add {key_entry}. Re-enter or edit the task shown above if it was not restored"
        ),
        RecoveryHint::Provider {
            kind: ProviderFailureKind::Authentication,
            key_entry,
        } => format!(
            "in another terminal, run nh doctor and verify the {key_entry} credential and provider access; use nh key add {key_entry} only if the key is missing or rejected"
        ),
        RecoveryHint::Provider {
            kind: ProviderFailureKind::RateLimited,
            ..
        } => "check provider limits and credit; wait before retrying if the limit is temporary"
            .to_owned(),
        RecoveryHint::Provider {
            kind: ProviderFailureKind::Timeout,
            ..
        } => "check network and provider status before retrying; the request may still be billed"
            .to_owned(),
        RecoveryHint::Provider {
            kind: ProviderFailureKind::Network,
            ..
        } => "in another terminal, run nh doctor for local configuration; check the network and provider status separately before retrying"
            .to_owned(),
        RecoveryHint::Provider {
            kind: ProviderFailureKind::Unavailable,
            ..
        } => "check provider status and wait before retrying".to_owned(),
        RecoveryHint::Provider {
            kind: ProviderFailureKind::Rejected,
            ..
        } => "review the error and model settings, then edit the task before retrying".to_owned(),
        RecoveryHint::Provider {
            kind: ProviderFailureKind::InvalidResponse,
            ..
        } => "check provider status before retrying; the unusable response may still be billed"
            .to_owned(),
    };
    format!("{action}. No new task was sent automatically")
}

fn record_timeline_summary(app: &mut App, summary: crate::state::TimelineSummary) {
    app.record_route_duration(&summary.route_id, &summary.receipt);
    let receipt_route_is_current = summary.route_id == app.route.id();
    let receipt_route = app.resolver.resolve(&summary.route_id).ok();
    let receipt_at = DateTime::parse_from_rfc3339(&summary.receipt.ts_utc)
        .ok()
        .map(|at| at.with_timezone(&Utc));
    if let Some(route) = &receipt_route {
        record_route_turn_cost(app, route, summary.receipt.usage.as_ref(), receipt_at, true);
    } else {
        app.mark_session_cost_incomplete();
        app.push_line(
            "cost unknown - receipt route is not in the catalog",
            TranscriptKind::Progress,
        );
    }
    let turn = app.timeline.len().saturating_add(1);
    let live_compaction = std::mem::take(&mut app.current_task_compaction);
    let mut entry =
        TimelineEntry::from_receipt(turn, summary.receipt, summary.answer, live_compaction);
    let review = std::mem::take(&mut app.current_task_review);
    retain_review(app, &mut entry, review);
    if !entry.compaction.is_empty() {
        let effect = compaction_effect(&app.resolver, receipt_route.as_ref(), entry.compaction);
        entry.compaction_detail = Some(format!(
            "{}{}",
            compaction_fact_line(entry.compaction),
            effect.suffix
        ));
        entry.compaction_hud = Some(effect.hud);
    }
    app.last_compaction_hud = if receipt_route_is_current {
        entry.compaction_hud.clone()
    } else {
        None
    };
    app.timeline.push(entry);
}

fn retain_review(app: &mut App, entry: &mut TimelineEntry, review: crate::state::TaskReview) {
    let bytes = review.retained_bytes();
    while app.timeline_review_bytes.saturating_add(bytes) > MAX_SESSION_REVIEW_BYTES {
        let Some(index) = app.timeline.iter().position(|prior| {
            matches!(&prior.review, TimelineReview::Live(review) if review.retained_bytes() > 0)
        }) else {
            break;
        };
        let (released, observed_items) = match &app.timeline[index].review {
            TimelineReview::Live(review) => (review.retained_bytes(), review.observed_items()),
            TimelineReview::Unavailable | TimelineReview::Expired { .. } => (0, 0),
        };
        app.timeline_review_bytes = app.timeline_review_bytes.saturating_sub(released);
        app.timeline[index].review = TimelineReview::Expired { observed_items };
    }
    app.timeline_review_bytes = app.timeline_review_bytes.saturating_add(bytes);
    entry.review = TimelineReview::Live(review);
}

#[cfg(test)]
pub(super) fn record_turn_cost(app: &mut App, usage: &Usage, at: DateTime<Utc>) {
    let route = app.route.clone();
    record_route_turn_cost(app, &route, Some(usage), Some(at), true);
}

pub(super) fn record_restored_turn_cost(
    app: &mut App,
    route: &ResolvedRoute,
    usage: Option<&Usage>,
    at: DateTime<Utc>,
) {
    record_route_turn_cost(app, route, usage, Some(at), false);
}

fn record_route_turn_cost(
    app: &mut App,
    route: &ResolvedRoute,
    usage: Option<&Usage>,
    at: Option<DateTime<Utc>>,
    show_details: bool,
) {
    let cost = turn_cost(&app.resolver, route, usage, at);
    match &cost {
        TurnCostVerdict::Local => app.mark_session_cost_incomplete(),
        TurnCostVerdict::NotStated(_) => app.mark_session_cost_incomplete(),
        TurnCostVerdict::Priced(cost) => app.add_session_cost(
            cost.currency,
            cost.amount,
            cost.uncertain,
            !cost.cache_split_reported,
        ),
    }
    if show_details {
        for line in savings_lines_for(&cost) {
            app.push_line(&line, TranscriptKind::Progress);
        }
    }
}

#[cfg(test)]
pub(super) fn savings_lines(
    resolver: &RouteResolver,
    route: &ResolvedRoute,
    usage: &Usage,
    at: DateTime<Utc>,
) -> Vec<String> {
    let cost = turn_cost(resolver, route, Some(usage), Some(at));
    savings_lines_for(&cost)
}

fn savings_lines_for(cost: &TurnCostVerdict) -> Vec<String> {
    match cost {
        TurnCostVerdict::Local => vec![LOCAL_METER_COPY.to_owned()],
        TurnCostVerdict::NotStated(reason) => vec![(*reason).to_owned()],
        TurnCostVerdict::Priced(cost) => {
            let mut lines = vec![cost.headline().to_owned()];
            if !cost.counterfactuals().is_empty() {
                lines.push(format!("naive: {}", cost.counterfactuals().join(" · ")));
            }
            if cost.uncertain {
                lines.push(PRICE_VERIFY_LIVE.to_owned());
            }
            lines
        }
    }
}
