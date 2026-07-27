//! Route resolution, selection explanation, and cost MCP handlers.

use super::*;

#[derive(Deserialize)]
struct RouteResolveArgs {
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    prefer_offpeak: Option<bool>,
}

pub(super) fn route_resolve(arguments: &Value, runtime: &Runtime) -> Value {
    let args: RouteResolveArgs = match serde_json::from_value(arguments.clone()) {
        Ok(args) => args,
        Err(error) => return tool_error(runtime, &error.to_string()),
    };
    let resolver = match nh_routes::RouteResolver::from_toml(&runtime.config.catalog) {
        Ok(resolver) => resolver,
        Err(error) => return tool_error(runtime, &error.to_string()),
    };
    let route = match resolver.resolve(
        args.model
            .as_deref()
            .unwrap_or(&runtime.config.default_route),
    ) {
        Ok(route) => route,
        Err(error) => return tool_error(runtime, &error.to_string()),
    };
    let now = Utc::now();
    let local = *Local::now().offset();
    let mut text = format!(
        "route {} · {} · {} thinking · {}",
        route.id(),
        route.provider(),
        route.thinking_dialect().as_str(),
        route.peak_status(now, local)
    );
    if args.prefer_offpeak == Some(true)
        && route.price_at(now).map(|quote| quote.peak) == Some(true)
    {
        text.push_str(" · would park until off-peak");
    }
    let would_park_offpeak = args.prefer_offpeak == Some(true)
        && route.price_at(now).map(|quote| quote.peak) == Some(true);
    let structured = json!({
        "route": {
            "id": route.id(),
            "provider": route.provider(),
            "thinking": route.thinking_dialect().as_str(),
            "peak_status": route.peak_status(now, local)
        },
        "would_park_offpeak": would_park_offpeak
    });
    tool_result(runtime, &text, structured, false)
}

#[derive(Deserialize)]
struct WhyArgs {
    #[serde(default, rename = "task")]
    _task: Option<String>,
    prompt_tokens: u64,
    output_tokens: u64,
    #[serde(default)]
    allowed: Option<Vec<String>>,
    #[serde(default, rename = "prefer_offpeak")]
    _prefer_offpeak: Option<bool>,
}

pub(super) fn why(arguments: &Value, runtime: &Runtime) -> Value {
    why_at(arguments, runtime, Utc::now())
}

pub(super) fn why_at(arguments: &Value, runtime: &Runtime, at: chrono::DateTime<Utc>) -> Value {
    let args: WhyArgs = match serde_json::from_value(arguments.clone()) {
        Ok(args) => args,
        Err(error) => return tool_error(runtime, &error.to_string()),
    };
    let resolver = match nh_routes::RouteResolver::from_toml(&runtime.config.catalog) {
        Ok(resolver) => resolver,
        Err(error) => return tool_error(runtime, &error.to_string()),
    };
    let allowed = match args.allowed {
        Some(allowed) if !allowed.is_empty() => allowed,
        _ => resolver
            .available()
            .into_iter()
            .filter(|id| {
                resolver
                    .resolve(id)
                    .is_ok_and(|route| route.class() == nh_routes::RouteClass::Api)
            })
            .collect(),
    };
    let allowed_refs: Vec<&str> = allowed.iter().map(String::as_str).collect();
    let (route, trace) =
        match resolver.resolve_capable(args.prompt_tokens, args.output_tokens, &allowed_refs, at) {
            Ok(result) => result,
            Err(error) => return tool_error(runtime, &error.to_string()),
        };
    let quote = match route.price_at(at) {
        Some(quote) => quote,
        None => return tool_error(runtime, "chosen route has no price quote"),
    };
    let Some(actual) = nh_routes::cost_of(&quote, args.prompt_tokens, 0, args.output_tokens) else {
        return tool_error(runtime, "usage is not priceable");
    };
    let usd_approx = resolver
        .fx()
        .and_then(|fx| nh_routes::to_usd_approx(actual, quote.currency, fx, at));
    let naive = resolver.naive_cost(&route, args.prompt_tokens, 0, args.output_tokens, at);
    let saved_pct = naive
        .as_ref()
        .and_then(|naive| nh_routes::saved_pct(actual, naive.no_cache));

    let mut cost = json!({
        "value": actual,
        "currency": quote.currency.as_str()
    });
    if let Some(usd_approx) = usd_approx {
        cost["usd_approx"] = json!(usd_approx);
    }
    let local = *Local::now().offset();
    let mut structured = json!({
        "route": {
            "id": route.id(),
            "provider": route.provider(),
            "thinking": route.thinking_dialect().as_str(),
            "peak_status": route.peak_status(at, local)
        },
        "cost": cost,
        "rejected": trace.rejections.iter().map(|rejection| json!({
            "route_id": rejection.route_id,
            "reason": rejection.reason
        })).collect::<Vec<_>>()
    });
    if let (Some(naive), Some(saved_pct)) = (naive, saved_pct) {
        structured["savings"] = json!({
            "saved_pct": saved_pct,
            "no_cache": naive.no_cache,
            "peak": naive.peak,
            "top_tier": naive.top_tier,
            "currency": naive.currency.as_str()
        });
    }
    let text = why_text(
        route.id(),
        route.provider(),
        actual,
        quote.currency,
        usd_approx,
        saved_pct,
        trace.rejections.len(),
    );
    tool_result(runtime, &text, structured, false)
}

pub(super) fn why_text(
    route_id: &str,
    provider: &str,
    actual: f64,
    currency: nh_routes::Currency,
    usd_approx: Option<f64>,
    saved_pct: Option<u8>,
    skipped: usize,
) -> String {
    let mut text = format!(
        "cheapest capable: {route_id} | {provider} | {actual:.6} {}",
        currency.as_str()
    );
    if let Some(usd) = usd_approx {
        text.push_str(&format!(" (~${usd:.6})"));
    }
    if let Some(saved_pct) = saved_pct {
        text.push_str(&format!(" | saved {saved_pct}% vs no-cache"));
    }
    text.push_str(&format!(" | {skipped} routes skipped"));
    text
}

#[derive(Deserialize)]
struct RouteCostArgs {
    #[serde(default)]
    model: Option<String>,
    prompt_tokens: u64,
    #[serde(default)]
    cached_tokens: u64,
    output_tokens: u64,
}

pub(super) fn route_cost(arguments: &Value, runtime: &Runtime) -> Value {
    route_cost_at(arguments, runtime, Utc::now())
}

pub(super) fn route_cost_at(
    arguments: &Value,
    runtime: &Runtime,
    at: chrono::DateTime<Utc>,
) -> Value {
    let args: RouteCostArgs = match serde_json::from_value(arguments.clone()) {
        Ok(args) => args,
        Err(error) => return tool_error(runtime, &error.to_string()),
    };
    let resolver = match nh_routes::RouteResolver::from_toml(&runtime.config.catalog) {
        Ok(resolver) => resolver,
        Err(error) => return tool_error(runtime, &error.to_string()),
    };
    let route = match resolver.resolve(
        args.model
            .as_deref()
            .unwrap_or(&runtime.config.default_route),
    ) {
        Ok(route) => route,
        Err(error) => return tool_error(runtime, &error.to_string()),
    };
    let quote = match route.price_at(at) {
        Some(quote) => quote,
        None => return tool_error(runtime, "route has no token price quote"),
    };
    let Some(value) = nh_routes::cost_of(
        &quote,
        args.prompt_tokens,
        args.cached_tokens,
        args.output_tokens,
    ) else {
        return tool_error(runtime, "usage is not priceable");
    };
    let usd_approx = resolver
        .fx()
        .and_then(|fx| nh_routes::to_usd_approx(value, quote.currency, fx, at));
    let mut cost = json!({
        "value": value,
        "currency": quote.currency.as_str()
    });
    if let Some(usd_approx) = usd_approx {
        cost["usd_approx"] = json!(usd_approx);
    }
    let structured = json!({
        "route": {
            "id": route.id(),
            "provider": route.provider(),
            "thinking": route.thinking_dialect().as_str()
        },
        "quote": {
            "cache_hit": quote.cache_hit,
            "cache_miss": quote.cache_miss,
            "output": quote.output,
            "currency": quote.currency.as_str(),
            "peak": quote.peak,
            "confidence": quote.confidence.as_str(),
            "stale": quote.stale
        },
        "cost": cost
    });
    let mut text = format!("{} | {value:.6} {}", route.id(), quote.currency.as_str());
    if let Some(usd) = usd_approx {
        text.push_str(&format!(" (~${usd:.6})"));
    }
    text.push_str(&format!(
        " | {} prompt ({} cached) | {} output",
        args.prompt_tokens, args.cached_tokens, args.output_tokens
    ));
    tool_result(runtime, &text, structured, false)
}
