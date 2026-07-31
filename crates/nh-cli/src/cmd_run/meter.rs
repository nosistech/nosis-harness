//! Receipt-to-terminal cost and usage projection for `run` and `chat`.

use chrono::{DateTime, Utc};
use nh_core::wire::{cache_hit_pct, Usage};
use nh_routes::{
    cost_of, money, money_with_gloss, saved_pct, PriceConfidence, ResolvedRoute, RouteClass,
    RouteResolver, LOCAL_METER_COPY,
};

pub(super) fn run_meter_lines(
    resolver: &RouteResolver,
    route: &ResolvedRoute,
    usage: Option<&Usage>,
    turns: u32,
    tool_calls: u32,
    started: DateTime<Utc>,
    ended: DateTime<Utc>,
) -> Vec<String> {
    let Some(usage) = usage else {
        let mut lines = vec![format!(
            "turns {turns} | tool calls {tool_calls} | tokens: not reported by provider — cost unknown"
        )];
        if route.class() == RouteClass::Local {
            lines.push(LOCAL_METER_COPY.to_owned());
        }
        return lines;
    };
    let mut token_line = format!(
        "turns {turns} | tool calls {tool_calls} | tokens {} in / {} out",
        usage.prompt_tokens, usage.completion_tokens
    );
    if let (Some(cached), Some(pct)) = (
        usage.cached_tokens,
        cache_hit_pct(usage.prompt_tokens, usage.cached_tokens),
    ) {
        token_line.push_str(&format!(" / {cached} cached | cache {pct:.0}%"));
    }
    let mut lines = vec![token_line];
    if let Some(line) = turn_cost_line_for_run(resolver, route, usage, started, ended) {
        lines.push(line);
    }
    lines
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
    if crossed && !line.starts_with("cost unpriced") {
        line.push_str(" · *priced at run end — spans a peak boundary");
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
    let quote = route.price_at(at)?;
    let cached = usage.cached_tokens.unwrap_or(0);
    let Some(actual) = cost_of(&quote, usage.prompt_tokens, cached, usage.completion_tokens) else {
        return Some("cost unpriced — invalid usage; meter incomplete".into());
    };
    let mut paid = money_with_gloss(actual, quote.currency, resolver.fx(), at);
    if quote.confidence == PriceConfidence::VerifyLive {
        paid.push('*');
    }
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
        line.push_str(&format!(" — saved {percent}% vs no-cache"));
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
