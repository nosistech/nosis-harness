//! `nh why` - a keyless explanation of the cheapest capable route.

use chrono::{DateTime, Utc};
use nh_routes::{cost_of, money_with_gloss, PriceConfidence, RouteClass, RouteResolver};
use nh_vault::Scrubber;

use crate::cmd_run;

const OUTPUT_ESTIMATE: u64 = 1_024;

pub fn run(task: Option<&str>, model: Option<&str>) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let (_, catalog) = cmd_run::find_catalog(&cwd)?;
    let resolver = RouteResolver::from_toml(&catalog)?;
    let lines = render(&resolver, task, model, Utc::now())?;
    let scrubber = Scrubber::new(Vec::new());
    for line in lines {
        println!("{}", cmd_run::safe_line(&scrubber, &line));
    }
    Ok(())
}

pub(crate) fn render(
    resolver: &RouteResolver,
    task: Option<&str>,
    model: Option<&str>,
    at: DateTime<Utc>,
) -> anyhow::Result<Vec<String>> {
    let explicit = model.map(|id| resolver.resolve(id)).transpose()?;
    if explicit
        .as_ref()
        .is_some_and(|route| route.class() != RouteClass::Api)
    {
        anyhow::bail!("selected route is a delegate - choose an api route with --model");
    }

    let prompt_estimate = task
        .map(|task| u64::try_from(task.len()).unwrap_or(u64::MAX).div_ceil(4))
        .unwrap_or(0);
    let available = resolver.available();
    let allowed: Vec<&str> = available
        .iter()
        .filter(|id| {
            resolver
                .resolve(id)
                .is_ok_and(|route| route.class() == RouteClass::Api)
        })
        .map(String::as_str)
        .collect();
    let (chosen, trace) =
        resolver.resolve_capable(prompt_estimate, OUTPUT_ESTIMATE, &allowed, at)?;

    let mut lines = vec![format!(
        "route: {} (cheapest capable at ~{} tokens, est)",
        chosen.id(),
        prompt_estimate.saturating_add(OUTPUT_ESTIMATE)
    )];
    if let Some(quote) = chosen.price_at(at) {
        let mut cost = match cost_of(&quote, prompt_estimate, 0, OUTPUT_ESTIMATE) {
            Some(estimate) => format!(
                "  {} this turn (est)",
                money_with_gloss(estimate, quote.currency, resolver.fx(), at)
            ),
            None => "  unpriced this turn (est) - cost unavailable".into(),
        };
        if quote.stale {
            cost.push_str(" · *price stale");
        } else if quote.confidence == PriceConfidence::VerifyLive {
            cost.push_str(" · *price verify_live");
        }
        lines.push(cost);
    }
    lines.extend(trace.lines());
    if let Some(explicit) = explicit {
        if explicit.id() != chosen.id() {
            lines.push(format!(
                "current route {} was selected explicitly; cheapest capable is {}",
                explicit.id(),
                chosen.id()
            ));
        }
    }
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    const CATALOG: &str = r#"
        [fx]
        usd_per_cny = 0.139
        valid_until = "2099-01-01"
        price_confidence = "reported"

        [routes.cheap]
        provider = "test"
        model_id = "cheap"
        base_url = "https://example.invalid"
        wire = "openai"
        vault_entry = "test"
        context = 100000
        [routes.cheap.price]
        currency = "CNY"
        unit = "per_million_tokens"
        cache_hit = 0.1
        cache_miss = 1.0
        output = 2.0
        price_confidence = "confirmed"

        [routes.expensive]
        provider = "test"
        model_id = "expensive"
        base_url = "https://example.invalid"
        wire = "openai"
        vault_entry = "test"
        context = 100000
        [routes.expensive.price]
        currency = "CNY"
        unit = "per_million_tokens"
        cache_hit = 0.2
        cache_miss = 4.0
        output = 8.0
        price_confidence = "confirmed"
    "#;

    #[test]
    fn why_render_names_chosen_route_and_rejection() {
        let resolver = RouteResolver::from_toml(CATALOG).unwrap();
        let at = Utc.with_ymd_and_hms(2026, 7, 18, 12, 0, 0).unwrap();
        let lines = render(&resolver, Some("explain this route"), Some("expensive"), at).unwrap();
        assert!(lines[0].starts_with("route: cheap"));
        assert!(lines
            .iter()
            .any(|line| line.starts_with("skipped expensive:")));
        assert!(lines.iter().any(|line| {
            line == "current route expensive was selected explicitly; cheapest capable is cheap"
        }));
    }
}
