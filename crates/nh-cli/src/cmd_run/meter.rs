//! Receipt-to-terminal cost and usage projection for `run` and `chat`.

use chrono::{DateTime, Utc};
use nh_core::agent::CompactionEvent;
use nh_core::receipt::CompactionStats;
use nh_core::wire::{cache_hit_pct, Usage, UsageEvidence};
use nh_routes::{
    cache_split_cost_upper_bound, cost_of, money, money_with_gloss, saved_pct, PriceConfidence,
    ResolvedRoute, RouteClass, RouteResolver, LOCAL_METER_COPY,
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
    // Keep ratios above 100% (and non-finite ratios) visible: they can expose
    // an incorrect catalog window and must not be disguised by clamping.
    let percent = usage.prompt_tokens as f64 / window as f64 * 100.0;
    Some(format!("ctx {marker}{percent:.0}%"))
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
    if events != 1 {
        line.push_str(" · aggregate money not stated - compactions affect separate next calls");
        return;
    }
    if route.class() == RouteClass::Local {
        line.push_str(" · next-call money not stated - ");
        line.push_str(LOCAL_METER_COPY);
        return;
    }
    let Some(cached) = preceding_cached_tokens else {
        line.push_str(
            " · next-call money not stated - exact preceding-call cached tokens unavailable",
        );
        return;
    };
    let Some(retained) = cached.checked_sub(estimated_tokens_elided) else {
        line.push_str(
            " · next-call money not stated - measured cache does not cover the elided token estimate",
        );
        return;
    };
    let Some(at) = occurred_at else {
        line.push_str(" · next-call money not stated - exact compaction time unavailable");
        return;
    };
    let Some(quote) = route.price_at(at) else {
        line.push_str(" · next-call money not stated - no price data");
        return;
    };
    let Some(saving) = cost_of(&quote, estimated_tokens_elided, estimated_tokens_elided, 0) else {
        line.push_str(" · next-call money not stated - invalid compaction facts");
        return;
    };
    let (Some(retained_miss), Some(retained_hit)) = (
        cost_of(&quote, retained, 0, 0),
        cost_of(&quote, retained, retained, 0),
    ) else {
        line.push_str(" · next-call money not stated - invalid compaction facts");
        return;
    };
    if retained_miss < retained_hit {
        line.push_str(" · next-call money not stated - cache-miss price is below cache-hit price");
        return;
    }
    let surcharge = retained_miss - retained_hit;
    let net = saving - surcharge;
    let net_is_zero = net.abs() <= f64::EPSILON * saving.abs().max(surcharge.abs()).max(1.0);
    let saving = format!("~{}", money(saving, quote.currency));
    let surcharge = format!("~{}", money(surcharge, quote.currency));
    let (net_label, net_amount) = if net_is_zero {
        ("net break-even", 0.0)
    } else if net < 0.0 {
        ("net loss", -net)
    } else {
        ("net saving", net)
    };
    let mut net = format!(
        "~{}",
        money_with_gloss(net_amount, quote.currency, resolver.fx(), at)
    );
    if quote.confidence == PriceConfidence::VerifyLive {
        net.push('*');
    }
    line.push_str(&format!(
        " · next-call estimate: cache-hit saving {saving} · cache-reset surcharge {surcharge} · {net_label} {net}"
    ));
    if quote.confidence == PriceConfidence::VerifyLive {
        line.push_str(" · *price verify_live");
    }
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
    if route.class() == RouteClass::Local {
        return Some(LOCAL_METER_COPY.to_owned());
    }
    match usage.evidence {
        UsageEvidence::Measured => {}
        UsageEvidence::Partial => {
            return Some("cost unknown - usage is a lower bound".into());
        }
        UsageEvidence::Unknown => return Some("cost unknown - usage unknown".into()),
    }
    let Some(quote) = route.price_at(at) else {
        return Some("cost unpriced - no price data".into());
    };
    let actual = usage.cached_tokens.map_or_else(
        || cache_split_cost_upper_bound(&quote, usage.prompt_tokens, usage.completion_tokens),
        |cached| cost_of(&quote, usage.prompt_tokens, cached, usage.completion_tokens),
    );
    let Some(actual) = actual else {
        return Some("cost unpriced - invalid usage; meter incomplete".into());
    };
    let mut paid = money_with_gloss(actual, quote.currency, resolver.fx(), at);
    if quote.confidence == PriceConfidence::VerifyLive {
        paid.push('*');
    }
    if usage.cached_tokens.is_none() {
        let mut line = format!("cost at most {paid} - cache split not reported by provider");
        if quote.confidence == PriceConfidence::VerifyLive {
            line.push_str(" · *price verify_live");
        }
        return Some(line);
    }
    let cached = usage
        .cached_tokens
        .expect("cache evidence checked before exact-cost comparison");
    let mut line = format!("cost {paid}");
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
        line.push_str(&format!(" - saved {percent}% vs no-cache"));
    }
    if let Some(costs) = naive {
        line.push_str(&format!(
            "   (peak {} · no-cache {} · top-tier {})",
            money(costs.peak, costs.currency),
            money(costs.no_cache, costs.currency),
            money(costs.top_tier, costs.currency)
        ));
    }
    if quote.confidence == PriceConfidence::VerifyLive {
        line.push_str(" · *price verify_live");
    }
    Some(line)
}
