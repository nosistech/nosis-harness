//! Event reduction, timeline projection, and session cost accounting.

use crate::session::safe_line;
use crate::state::{AgentEvent, App, Status, TimelineEntry, TranscriptKind};
use crate::{APPROVAL_LEGEND, BUDGET_REASON};
use chrono::{DateTime, TimeZone, Utc};
use nh_core::agent::CompactionEvent;
use nh_core::receipt::{CompactionStats, FailureClass, Outcome};
use nh_core::wire::{cache_hit_pct, Usage};
use nh_routes::{
    cost_of, money, money_with_gloss, saved_pct, PriceConfidence, ResolvedRoute, RouteClass,
    RouteResolver, LOCAL_METER_COPY,
};

pub(super) fn outcome_name(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::Pass => "pass",
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

pub(super) fn timeline_row(entry: &TimelineEntry) -> String {
    let (input, output, cached) = entry.tokens();
    let compacted = if entry.compacted { "  [compact]" } else { "" };
    let mut tokens = format!("{input}/{output}");
    if let Some(cached) = cached {
        tokens.push_str(&format!("/{cached}"));
        if let Some(pct) = cache_hit_pct(input, Some(cached)) {
            tokens.push_str(&format!(" cache {pct:.0}%"));
        }
    }
    format!(
        "#{}  {}  {tokens}{compacted}",
        entry.turn,
        outcome_name(entry.outcome)
    )
}

pub(super) fn timeline_detail_lines(entry: &TimelineEntry) -> Vec<String> {
    let (input, output, cached) = entry.tokens();
    let failure = entry
        .failure_class
        .map(failure_class_name)
        .unwrap_or("none");
    let mut tokens = format!("tokens: {input} in / {output} out");
    if let Some(cached) = cached {
        tokens.push_str(&format!(" / {cached} cached"));
        if let Some(pct) = cache_hit_pct(input, Some(cached)) {
            tokens.push_str(&format!(" | cache {pct:.0}%"));
        }
    }
    let mut lines = vec![
        format!("TURN #{}", entry.turn),
        format!("timestamp: {}", entry.ts_utc),
        format!("model: {}", entry.model_id),
        format!("task: {}", entry.task),
        format!("outcome: {}", outcome_name(entry.outcome)),
        format!("agent turns: {}", entry.turns),
        format!("tool calls: {}", entry.tool_calls),
        format!("failure class: {failure}"),
        tokens,
        format!("compacted: {}", if entry.compacted { "yes" } else { "no" }),
    ];
    if let Some(detail) = &entry.compaction_detail {
        lines.push(detail.clone());
    }
    lines.push(String::new());
    lines.push(format!("answer: {}", entry.answer));
    lines
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
    let unpriced = |reason: &str| CompactionEffect {
        suffix: reason.to_owned(),
        hud: format!(
            "compact ~{}t · net not stated",
            stats.estimated_tokens_elided
        ),
    };

    if stats.events != 1 {
        return unpriced(" · aggregate money not stated - compactions affect separate next calls");
    }
    let Some(route) = route else {
        return unpriced(" · next-call money not stated - no price data");
    };
    if route.class() == RouteClass::Local {
        return unpriced(&format!(
            " · next-call money not stated - {LOCAL_METER_COPY}"
        ));
    }
    let Some(cached) = stats.preceding_cached_tokens else {
        return unpriced(
            " · next-call money not stated - exact preceding-call cached tokens unavailable",
        );
    };
    let Some(retained) = cached.checked_sub(stats.estimated_tokens_elided) else {
        return unpriced(
            " · next-call money not stated - measured cache does not cover the elided token estimate",
        );
    };
    let Some(at) = stats
        .occurred_at_unix_seconds
        .and_then(|seconds| Utc.timestamp_opt(seconds, 0).single())
    else {
        return unpriced(" · next-call money not stated - exact compaction time unavailable");
    };
    let Some(quote) = route.price_at(at) else {
        return unpriced(" · next-call money not stated - no price data");
    };
    let Some(saving) = cost_of(
        &quote,
        stats.estimated_tokens_elided,
        stats.estimated_tokens_elided,
        0,
    ) else {
        return unpriced(" · next-call money not stated - invalid compaction facts");
    };
    let (Some(cache_miss), Some(cache_hit)) = (
        cost_of(&quote, retained, 0, 0),
        cost_of(&quote, retained, retained, 0),
    ) else {
        return unpriced(" · next-call money not stated - invalid compaction facts");
    };
    if cache_miss < cache_hit {
        return unpriced(
            " · next-call money not stated - cache-miss price is below cache-hit price",
        );
    }

    let surcharge = cache_miss - cache_hit;
    let net = saving - surcharge;
    let is_break_even = net.abs() <= f64::EPSILON * saving.abs().max(surcharge.abs()).max(1.0);
    let (net_label, net_amount) = if is_break_even {
        ("net break-even", 0.0)
    } else if net < 0.0 {
        ("net loss", -net)
    } else {
        ("net saving", net)
    };
    let mut net_display = money_with_gloss(net_amount, quote.currency, resolver.fx(), at);
    let uncertain = quote.confidence == PriceConfidence::VerifyLive;
    if uncertain {
        net_display.push('*');
    }
    let mut suffix = format!(
        " · next-call estimate: cache-hit saving ~{} · cache-reset surcharge ~{} · {net_label} ~{net_display}",
        money(saving, quote.currency),
        money(surcharge, quote.currency),
    );
    if uncertain {
        suffix.push_str(" · *price verify_live");
    }
    CompactionEffect {
        suffix,
        hud: format!(
            "compact ~{}t · next-call {net_label} ~{net_display}",
            stats.estimated_tokens_elided
        ),
    }
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
        AgentEvent::ModelFinished { route } => {
            if app
                .active_model
                .as_ref()
                .is_some_and(|request| request.route == route)
            {
                app.active_model = None;
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
            if app.session_allow.contains(&request.prompt) {
                let _ = request.reply.send(true);
                app.push_line(
                    &format!("auto-approved (session rule): {}", request.prompt),
                    TranscriptKind::Progress,
                );
                app.set_status(Status::Working, Utc::now());
            } else {
                let line = format!("approve: {}   {APPROVAL_LEGEND}", request.prompt);
                app.push_approval_line(&line);
                app.pending_approval = Some(request);
                app.set_status(Status::Waiting, Utc::now());
            }
        }
        AgentEvent::Usage(usage) => {
            app.usage = usage;
            if app.budget_reached() {
                app.set_status(Status::Blocked(BUDGET_REASON.into()), Utc::now());
            }
        }
        AgentEvent::TaskReceipt(summary) => {
            let receipt_route_is_current = summary.route_id == app.route.id();
            let receipt_route = app.resolver.resolve(&summary.route_id).ok();
            let receipt_at = DateTime::parse_from_rfc3339(&summary.receipt.ts_utc)
                .ok()
                .map(|at| at.with_timezone(&Utc));
            if let Some(route) = &receipt_route {
                if route.class() == RouteClass::Local {
                    app.push_line(LOCAL_METER_COPY, TranscriptKind::Progress);
                } else if let (Some(usage), Some(at)) = (summary.receipt.usage.as_ref(), receipt_at)
                {
                    record_route_turn_cost(app, route, usage, at, true);
                }
            } else {
                app.has_failed_turn = true;
            }
            let turn = app.timeline.len().saturating_add(1);
            let live_compaction = std::mem::take(&mut app.current_task_compaction);
            let mut entry =
                TimelineEntry::from_receipt(turn, summary.receipt, summary.answer, live_compaction);
            if !entry.compaction.is_empty() {
                let effect =
                    compaction_effect(&app.resolver, receipt_route.as_ref(), entry.compaction);
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
        AgentEvent::Answer(answer) => {
            app.active_model = None;
            app.active_tool = None;
            app.push_text("", &answer, TranscriptKind::Answer);
            let status = if app.budget_reached() {
                Status::Blocked(BUDGET_REASON.into())
            } else {
                Status::Idle
            };
            app.set_status(status, Utc::now());
        }
        AgentEvent::MeterIncomplete => app.has_failed_turn = true,
        AgentEvent::Failed(reason) => {
            app.active_model = None;
            app.active_tool = None;
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
            app.set_status(Status::Blocked(status_reason), Utc::now());
        }
    }
    &app.status
}

#[cfg(test)]
pub(super) fn record_turn_cost(app: &mut App, usage: &Usage, at: DateTime<Utc>) {
    let route = app.route.clone();
    record_route_turn_cost(app, &route, usage, at, true);
}

pub(super) fn record_restored_turn_cost(
    app: &mut App,
    route: &ResolvedRoute,
    usage: &Usage,
    at: DateTime<Utc>,
) {
    record_route_turn_cost(app, route, usage, at, false);
}

fn record_route_turn_cost(
    app: &mut App,
    route: &ResolvedRoute,
    usage: &Usage,
    at: DateTime<Utc>,
    show_details: bool,
) {
    if route.class() == RouteClass::Local {
        if show_details {
            app.push_line(LOCAL_METER_COPY, TranscriptKind::Progress);
        }
        return;
    }
    let Some(quote) = route.price_at(at) else {
        return;
    };
    let cached = usage.cached_tokens.unwrap_or(0);
    let Some(actual) = cost_of(&quote, usage.prompt_tokens, cached, usage.completion_tokens) else {
        let _ = apply_event(app, AgentEvent::MeterIncomplete);
        return;
    };
    let uncertain = quote.confidence == PriceConfidence::VerifyLive;
    app.add_session_cost(quote.currency, actual, uncertain);
    if show_details {
        for line in savings_lines(&app.resolver, route, usage, at) {
            app.push_line(&line, TranscriptKind::Progress);
        }
    }
}

pub(super) fn savings_lines(
    resolver: &RouteResolver,
    route: &ResolvedRoute,
    usage: &Usage,
    at: DateTime<Utc>,
) -> Vec<String> {
    if route.class() == RouteClass::Local {
        return vec![LOCAL_METER_COPY.to_owned()];
    }
    let Some(quote) = route.price_at(at) else {
        return Vec::new();
    };
    let cached = usage.cached_tokens.unwrap_or(0);
    let Some(actual) = cost_of(&quote, usage.prompt_tokens, cached, usage.completion_tokens) else {
        return vec!["cost unpriced - invalid usage; meter incomplete".into()];
    };
    let mut paid = money_with_gloss(actual, quote.currency, resolver.fx(), at);
    let uncertain = quote.confidence == PriceConfidence::VerifyLive;
    if uncertain {
        paid.push('*');
    }
    let mut headline = format!("cost {paid}");
    let naive = resolver.naive_cost(
        route,
        usage.prompt_tokens,
        cached,
        usage.completion_tokens,
        at,
    );
    if let Some(percent) = naive
        .as_ref()
        .and_then(|costs| saved_pct(actual, costs.no_cache))
    {
        headline.push_str(&format!(" - saved {percent}% vs no-cache"));
    }
    let mut lines = vec![headline];
    if let Some(costs) = naive {
        lines.push(format!(
            "naive: peak {} · no-cache {} · top-tier {}",
            money(costs.peak, costs.currency),
            money(costs.no_cache, costs.currency),
            money(costs.top_tier, costs.currency)
        ));
    }
    if uncertain {
        lines.push("*price verify_live".into());
    }
    lines
}
