//! Receipt-to-terminal cost and usage projection for `run` and `chat`.

use chrono::{DateTime, Utc};
use nh_core::agent::CompactionEvent;
use nh_core::cost::{compaction_cost, turn_cost, TurnCostVerdict, PRICE_VERIFY_LIVE};
use nh_core::receipt::CompactionStats;
use nh_core::terminal_capability::TerminalCapability;
use nh_core::wire::{cache_hit_pct, Usage, UsageEvidence};
use nh_routes::{
    format_context_percent, ResolvedRoute, RouteClass, RouteResolver, LOCAL_METER_COPY,
};

#[derive(Clone, Copy)]
pub(super) struct RunTiming {
    pub started: DateTime<Utc>,
    pub ended: DateTime<Utc>,
}

#[derive(Clone, Copy)]
pub(super) struct RunUsage<'a> {
    total: Option<&'a Usage>,
    latest_request: Option<&'a Usage>,
}

impl<'a> RunUsage<'a> {
    pub(super) fn new(total: Option<&'a Usage>, latest_request: Option<&'a Usage>) -> Self {
        Self {
            total,
            latest_request,
        }
    }
}

pub(super) fn terminal_meter_lines(
    terminal_capability: TerminalCapability,
    lines: Vec<String>,
) -> Vec<String> {
    lines
        .into_iter()
        .map(|line| terminal_capability.render_text(&line).into_owned())
        .collect()
}

pub(super) fn run_meter_lines(
    resolver: &RouteResolver,
    route: &ResolvedRoute,
    usage: RunUsage<'_>,
    compaction: &CompactionStats,
    turns: u32,
    tool_calls: u32,
    timing: RunTiming,
) -> Vec<String> {
    let context_usage = usage.latest_request;
    let usage = usage.total;
    let usage_unknown = usage.is_none_or(|usage| usage.evidence == UsageEvidence::Unknown);
    let mut token_summary = usage_token_summary(usage);
    if let Some(context) = context_window_summary(route, context_usage) {
        token_summary.push_str(" | ");
        token_summary.push_str(&context);
    }
    if usage_unknown {
        let mut lines = vec![format!(
            "turns {turns} | tool calls {tool_calls} | {token_summary} - cost unknown"
        )];
        if route.class() == RouteClass::Local {
            lines.push(LOCAL_METER_COPY.to_owned());
        }
        if let Some(line) = compaction_meter_line(resolver, route, compaction) {
            lines.push(line);
        }
        return lines;
    }
    let usage = usage.expect("known usage checked above");
    let mut lines = vec![format!(
        "turns {turns} | tool calls {tool_calls} | {token_summary}"
    )];
    if let Some(line) = turn_cost_line_for_run(resolver, route, usage, timing.started, timing.ended)
    {
        lines.push(line);
    }
    if let Some(line) = compaction_meter_line(resolver, route, compaction) {
        lines.push(line);
    }
    lines
}

pub(crate) fn usage_token_summary(usage: Option<&Usage>) -> String {
    let Some(usage) = usage.filter(|usage| usage.evidence != UsageEvidence::Unknown) else {
        return "tokens: not reported by provider".into();
    };
    let partial = usage.evidence == UsageEvidence::Partial;
    let marker = if partial { "~" } else { "" };
    let mut summary = format!(
        "tokens {marker}{} in / {marker}{} out",
        usage.prompt_tokens, usage.completion_tokens
    );
    if partial {
        if let Some(cached) = usage.cached_tokens {
            summary.push_str(&format!(" / ~{cached} cached"));
        }
        summary.push_str(" (lower bound)");
    } else if let (Some(cached), Some(pct)) = (
        usage.cached_tokens,
        cache_hit_pct(usage.prompt_tokens, usage.cached_tokens),
    ) {
        summary.push_str(&format!(" / {cached} cached | cache {pct:.0}%"));
    }
    summary
}

pub(crate) fn context_window_summary(
    route: &ResolvedRoute,
    usage: Option<&Usage>,
) -> Option<String> {
    let window = route.context()?;
    let usage = usage.filter(|usage| usage.evidence != UsageEvidence::Unknown)?;
    let marker = if usage.evidence == UsageEvidence::Partial {
        "~"
    } else {
        ""
    };
    let percent = format_context_percent(usage.prompt_tokens, window);
    Some(format!("ctx {marker}{percent}"))
}

/// Render one progress callback. Ordinary core progress remains byte-for-byte
/// unchanged; compaction facts are recognized and priced by this surface.
pub(crate) fn progress_meter_line(
    resolver: &RouteResolver,
    route: &ResolvedRoute,
    core_line: &str,
) -> String {
    let Some(event) = CompactionEvent::parse(core_line) else {
        return core_line.to_owned();
    };
    let mut line = format!(
        "context ~{}% - compacted {} earlier messages · ~{} tokens elided",
        event.context_percent, event.messages_elided, event.estimated_tokens_elided
    );
    append_compaction_effect(
        &mut line,
        resolver,
        route,
        1,
        event.estimated_tokens_elided,
        event.preceding_cached_tokens,
        compaction_time(event.occurred_at_unix_seconds),
    );
    line
}

pub(crate) fn terminal_progress_meter_line(
    terminal_capability: TerminalCapability,
    resolver: &RouteResolver,
    route: &ResolvedRoute,
    core_line: &str,
) -> String {
    terminal_capability
        .render_text(&progress_meter_line(resolver, route, core_line))
        .into_owned()
}

pub(crate) fn compaction_meter_line(
    resolver: &RouteResolver,
    route: &ResolvedRoute,
    stats: &CompactionStats,
) -> Option<String> {
    if stats.is_empty() {
        return None;
    }
    let event_noun = if stats.events == 1 { "event" } else { "events" };
    let message_noun = if stats.messages_elided == 1 {
        "message"
    } else {
        "messages"
    };
    let mut line = format!(
        "compaction {} {event_noun} · {} {message_noun} elided · ~{} tokens elided",
        stats.events, stats.messages_elided, stats.estimated_tokens_elided
    );
    append_compaction_effect(
        &mut line,
        resolver,
        route,
        stats.events,
        stats.estimated_tokens_elided,
        stats.preceding_cached_tokens,
        compaction_time(stats.occurred_at_unix_seconds),
    );
    Some(line)
}

fn append_compaction_effect(
    line: &mut String,
    resolver: &RouteResolver,
    route: &ResolvedRoute,
    events: u32,
    estimated_tokens_elided: u64,
    preceding_cached_tokens: Option<u64>,
    occurred_at: Option<DateTime<Utc>>,
) {
    let cost = compaction_cost(
        resolver,
        Some(route),
        events,
        estimated_tokens_elided,
        preceding_cached_tokens,
        occurred_at,
    );
    line.push_str(&cost.suffix());
}

fn compaction_time(unix_seconds: Option<i64>) -> Option<DateTime<Utc>> {
    unix_seconds.and_then(|seconds| DateTime::<Utc>::from_timestamp(seconds, 0))
}

pub(super) fn turn_cost_line_for_run(
    resolver: &RouteResolver,
    route: &ResolvedRoute,
    usage: &Usage,
    started: DateTime<Utc>,
    ended: DateTime<Utc>,
) -> Option<String> {
    let mut line = turn_cost_line(resolver, route, usage, ended)?;
    if route.class() == RouteClass::Local {
        return Some(line);
    }
    let crossed = matches!(
        (route.price_at(started), route.price_at(ended)),
        (Some(start), Some(end)) if start.peak != end.peak
    );
    if crossed
        && usage.evidence.is_measured()
        && usage.cached_tokens.is_some()
        && !line.starts_with("cost unpriced")
    {
        line.push_str(" · *priced at run end - spans a peak boundary");
    }
    Some(line)
}

pub(crate) fn turn_cost_line(
    resolver: &RouteResolver,
    route: &ResolvedRoute,
    usage: &Usage,
    at: DateTime<Utc>,
) -> Option<String> {
    let line = match turn_cost(resolver, route, Some(usage), Some(at)) {
        TurnCostVerdict::Local => LOCAL_METER_COPY.to_owned(),
        TurnCostVerdict::NotStated(reason) => reason.to_owned(),
        TurnCostVerdict::Priced(cost) => {
            let mut line = cost.headline().to_owned();
            if !cost.counterfactuals().is_empty() {
                line.push_str(&format!("   ({})", cost.counterfactuals().join(" · ")));
            }
            if cost.uncertain {
                line.push_str(" · ");
                line.push_str(PRICE_VERIFY_LIVE);
            }
            line
        }
    };
    Some(line)
}
