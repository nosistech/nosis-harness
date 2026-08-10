//! Shared cost decisions and display fragments for terminal surfaces.

use crate::wire::{Usage, UsageEvidence};
use chrono::{DateTime, Utc};
use nh_routes::{
    cache_split_cost_upper_bound, cost_of, money, money_with_gloss, saved_pct, Currency,
    PriceConfidence, ResolvedRoute, RouteClass, RouteResolver, LOCAL_METER_COPY,
};

const COST_USAGE_UNREPORTED: &str = "cost unknown - usage unreported";
const COST_USAGE_LOWER_BOUND: &str = "cost unknown - usage is a lower bound";
const COST_USAGE_UNKNOWN: &str = "cost unknown - usage unknown";
const COST_TIMESTAMP_INVALID: &str = "cost unknown - receipt timestamp is invalid";
const COST_NO_PRICE_DATA: &str = "cost unpriced - no price data";
const COST_INVALID_USAGE: &str = "cost unpriced - invalid usage; meter incomplete";

const COMPACTION_AGGREGATE: &str =
    " · aggregate money not stated - compactions affect separate next calls";
const COMPACTION_NO_PRICE_DATA: &str = " · next-call money not stated - no price data";
const COMPACTION_CACHE_UNAVAILABLE: &str =
    " · next-call money not stated - exact preceding-call cached tokens unavailable";
const COMPACTION_CACHE_TOO_SMALL: &str =
    " · next-call money not stated - measured cache does not cover the elided token estimate";
const COMPACTION_TIME_UNAVAILABLE: &str =
    " · next-call money not stated - exact compaction time unavailable";
const COMPACTION_INVALID_FACTS: &str = " · next-call money not stated - invalid compaction facts";
const COMPACTION_INVERTED_CACHE_PRICE: &str =
    " · next-call money not stated - cache-miss price is below cache-hit price";

/// Marker appended by surfaces when catalog pricing must be verified live.
pub const PRICE_VERIFY_LIVE: &str = "*price verify_live";

/// Shared decision for the cost of one provider turn.
#[derive(Debug, Clone, PartialEq)]
pub enum TurnCostVerdict {
    /// Local execution has hardware cost but no billed-token price.
    Local,
    /// The cost cannot be stated honestly; the string is the canonical reason.
    NotStated(&'static str),
    /// The turn has a usable catalog price and measured usage.
    Priced(PricedTurnCost),
}

/// Priced facts and common display fragments for one provider turn.
#[derive(Debug, Clone, PartialEq)]
pub struct PricedTurnCost {
    /// Native-currency amount accumulated into the session ledger.
    pub amount: f64,
    /// Native currency accumulated into the session ledger.
    pub currency: Currency,
    /// Whether the catalog marks the price `verify_live`.
    pub uncertain: bool,
    /// Whether the provider reported the cache split exactly.
    pub cache_split_reported: bool,
    headline: String,
    counterfactuals: Vec<String>,
}

impl PricedTurnCost {
    /// Common cost headline used by both terminal renderings.
    pub fn headline(&self) -> &str {
        &self.headline
    }

    /// Ordered peak, no-cache, and top-tier fragments that callers place differently.
    pub fn counterfactuals(&self) -> &[String] {
        &self.counterfactuals
    }
}

/// Apply the common evidence-and-price ladder for one turn.
pub fn turn_cost(
    resolver: &RouteResolver,
    route: &ResolvedRoute,
    usage: Option<&Usage>,
    at: Option<DateTime<Utc>>,
) -> TurnCostVerdict {
    if route.class() == RouteClass::Local {
        return TurnCostVerdict::Local;
    }
    let Some(usage) = usage else {
        return TurnCostVerdict::NotStated(COST_USAGE_UNREPORTED);
    };
    match usage.evidence {
        UsageEvidence::Measured => {}
        UsageEvidence::Partial => {
            return TurnCostVerdict::NotStated(COST_USAGE_LOWER_BOUND);
        }
        UsageEvidence::Unknown => return TurnCostVerdict::NotStated(COST_USAGE_UNKNOWN),
    }
    let Some(at) = at else {
        return TurnCostVerdict::NotStated(COST_TIMESTAMP_INVALID);
    };
    let Some(quote) = route.price_at(at) else {
        return TurnCostVerdict::NotStated(COST_NO_PRICE_DATA);
    };
    let actual = usage.cached_tokens.map_or_else(
        || cache_split_cost_upper_bound(&quote, usage.prompt_tokens, usage.completion_tokens),
        |cached| cost_of(&quote, usage.prompt_tokens, cached, usage.completion_tokens),
    );
    let Some(actual) = actual else {
        return TurnCostVerdict::NotStated(COST_INVALID_USAGE);
    };

    let uncertain = quote.confidence == PriceConfidence::VerifyLive;
    let mut paid = money_with_gloss(actual, quote.currency, resolver.fx(), at);
    if uncertain {
        paid.push('*');
    }
    let Some(cached) = usage.cached_tokens else {
        return TurnCostVerdict::Priced(PricedTurnCost {
            amount: actual,
            currency: quote.currency,
            uncertain,
            cache_split_reported: false,
            headline: format!("cost at most {paid} - cache split not reported by provider"),
            counterfactuals: Vec::new(),
        });
    };

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
    let counterfactuals = naive.map_or_else(Vec::new, |costs| {
        let mut counterfactuals = Vec::with_capacity(3);
        if let Some(peak) = costs.peak {
            counterfactuals.push(format!("peak {}", money(peak, costs.currency)));
        }
        counterfactuals.push(format!(
            "no-cache {}",
            money(costs.no_cache, costs.currency)
        ));
        counterfactuals.push(format!(
            "top-tier {}",
            money(costs.top_tier, costs.currency)
        ));
        counterfactuals
    });

    TurnCostVerdict::Priced(PricedTurnCost {
        amount: actual,
        currency: quote.currency,
        uncertain,
        cache_split_reported: true,
        headline,
        counterfactuals,
    })
}

/// Shared decision for the next-call money effect of one compaction.
#[derive(Debug, Clone, PartialEq)]
pub enum CompactionCostVerdict {
    /// The effect cannot be stated honestly; the string is the canonical suffix.
    NotStated(String),
    /// The next-call effect can be estimated from the retained cache facts.
    Priced(PricedCompactionCost),
}

impl CompactionCostVerdict {
    /// Canonical suffix appended to the caller's compaction fact line.
    pub fn suffix(&self) -> String {
        match self {
            Self::NotStated(reason) => reason.clone(),
            Self::Priced(cost) => {
                let mut suffix = format!(
                    " · next-call estimate: cache-hit saving {} · cache-reset surcharge {} · {} {}",
                    cost.cache_hit_saving,
                    cost.cache_reset_surcharge,
                    cost.net_label,
                    cost.net_display
                );
                if cost.uncertain {
                    suffix.push_str(" · ");
                    suffix.push_str(PRICE_VERIFY_LIVE);
                }
                suffix
            }
        }
    }
}

/// Priced fragments for one compaction's next-call effect.
#[derive(Debug, Clone, PartialEq)]
pub struct PricedCompactionCost {
    /// Break-even, loss, or saving label selected by the common float comparison.
    pub net_label: &'static str,
    /// Approximate marker, money display, and optional `verify_live` asterisk.
    pub net_display: String,
    cache_hit_saving: String,
    cache_reset_surcharge: String,
    uncertain: bool,
}

/// Apply the separate compaction-price ladder for one recorded event or aggregate.
pub fn compaction_cost(
    resolver: &RouteResolver,
    route: Option<&ResolvedRoute>,
    events: u32,
    estimated_tokens_elided: u64,
    preceding_cached_tokens: Option<u64>,
    occurred_at: Option<DateTime<Utc>>,
) -> CompactionCostVerdict {
    let not_stated = |reason: &str| CompactionCostVerdict::NotStated(reason.to_owned());

    if events != 1 {
        return not_stated(COMPACTION_AGGREGATE);
    }
    let Some(route) = route else {
        return not_stated(COMPACTION_NO_PRICE_DATA);
    };
    if route.class() == RouteClass::Local {
        return CompactionCostVerdict::NotStated(format!(
            " · next-call money not stated - {LOCAL_METER_COPY}"
        ));
    }
    let Some(cached) = preceding_cached_tokens else {
        return not_stated(COMPACTION_CACHE_UNAVAILABLE);
    };
    let Some(retained) = cached.checked_sub(estimated_tokens_elided) else {
        return not_stated(COMPACTION_CACHE_TOO_SMALL);
    };
    let Some(at) = occurred_at else {
        return not_stated(COMPACTION_TIME_UNAVAILABLE);
    };
    let Some(quote) = route.price_at(at) else {
        return not_stated(COMPACTION_NO_PRICE_DATA);
    };
    let Some(saving) = cost_of(&quote, estimated_tokens_elided, estimated_tokens_elided, 0) else {
        return not_stated(COMPACTION_INVALID_FACTS);
    };
    let (Some(retained_miss), Some(retained_hit)) = (
        cost_of(&quote, retained, 0, 0),
        cost_of(&quote, retained, retained, 0),
    ) else {
        return not_stated(COMPACTION_INVALID_FACTS);
    };
    if retained_miss < retained_hit {
        return not_stated(COMPACTION_INVERTED_CACHE_PRICE);
    }

    let surcharge = retained_miss - retained_hit;
    let net = saving - surcharge;
    let is_break_even = net.abs() <= f64::EPSILON * saving.abs().max(surcharge.abs()).max(1.0);
    let (net_label, net_amount) = if is_break_even {
        ("net break-even", 0.0)
    } else if net < 0.0 {
        ("net loss", -net)
    } else {
        ("net saving", net)
    };
    let uncertain = quote.confidence == PriceConfidence::VerifyLive;
    let mut net_display = format!(
        "~{}",
        money_with_gloss(net_amount, quote.currency, resolver.fx(), at)
    );
    if uncertain {
        net_display.push('*');
    }

    CompactionCostVerdict::Priced(PricedCompactionCost {
        net_label,
        net_display,
        cache_hit_saving: format!("~{}", money(saving, quote.currency)),
        cache_reset_surcharge: format!("~{}", money(surcharge, quote.currency)),
        uncertain,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const CATALOG: &str = r#"
        [routes.priced]
        provider = "test"
        model_id = "priced"
        base_url = "https://example.invalid"
        wire = "openai"
        vault_entry = "test"
        [routes.priced.price]
        currency = "USD"
        unit = "per_million_tokens"
        cache_hit = 0.1
        cache_miss = 1.0
        output = 2.0
        price_confidence = "confirmed"
        [routes.priced.price.peak]
        multiplier = 2.0
        timezone = "Asia/Shanghai"
        windows = ["09:00-12:00"]

        [routes.top]
        provider = "test"
        model_id = "top"
        base_url = "https://example.invalid"
        wire = "openai"
        vault_entry = "test"
        [routes.top.price]
        currency = "USD"
        unit = "per_million_tokens"
        cache_hit = 0.2
        cache_miss = 4.0
        output = 8.0
        price_confidence = "confirmed"

        [routes.unpriced]
        provider = "test"
        model_id = "unpriced"
        base_url = "https://example.invalid"
        wire = "openai"
        vault_entry = "test"

        [routes.local]
        provider = "local"
        model_id = "local"
        base_url = "http://127.0.0.1:11434"
        wire = "openai"
        vault_entry = "local"
        class = "local"
    "#;

    fn fixed_at() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-07-18T12:00:00Z")
            .expect("fixed timestamp")
            .with_timezone(&Utc)
    }

    fn measured(cached_tokens: Option<u64>) -> Usage {
        Usage {
            prompt_tokens: 100_000,
            completion_tokens: 50_000,
            cached_tokens,
            evidence: UsageEvidence::Measured,
        }
    }

    fn not_stated(verdict: TurnCostVerdict) -> &'static str {
        match verdict {
            TurnCostVerdict::NotStated(reason) => reason,
            other => panic!("expected not-stated turn cost, got {other:?}"),
        }
    }

    fn compaction_not_stated(verdict: CompactionCostVerdict) -> String {
        match verdict {
            CompactionCostVerdict::NotStated(reason) => reason,
            other => panic!("expected not-stated compaction cost, got {other:?}"),
        }
    }

    fn rate_resolver(cache_hit: f64, cache_miss: f64) -> RouteResolver {
        RouteResolver::from_toml(&format!(
            r#"
                [routes.rate]
                provider = "rate"
                model_id = "rate"
                base_url = "https://example.invalid"
                wire = "openai"
                vault_entry = "rate"
                [routes.rate.price]
                currency = "USD"
                unit = "per_million_tokens"
                cache_hit = {cache_hit:e}
                cache_miss = {cache_miss:e}
                output = 0.0
                price_confidence = "confirmed"
            "#
        ))
        .expect("rate catalog")
    }

    #[test]
    fn turn_ladder_covers_local_and_every_not_stated_reason() {
        let resolver = RouteResolver::from_toml(CATALOG).expect("catalog");
        let priced = resolver.resolve("priced").expect("priced route");
        let unpriced = resolver.resolve("unpriced").expect("unpriced route");
        let local = resolver.resolve("local").expect("local route");
        let at = fixed_at();

        assert_eq!(
            turn_cost(&resolver, &local, None, None),
            TurnCostVerdict::Local
        );
        assert_eq!(
            not_stated(turn_cost(&resolver, &priced, None, Some(at))),
            COST_USAGE_UNREPORTED
        );
        for (evidence, reason) in [
            (UsageEvidence::Partial, COST_USAGE_LOWER_BOUND),
            (UsageEvidence::Unknown, COST_USAGE_UNKNOWN),
        ] {
            let usage = Usage {
                evidence,
                ..measured(Some(90_000))
            };
            assert_eq!(
                not_stated(turn_cost(&resolver, &priced, Some(&usage), None)),
                reason
            );
        }
        assert_eq!(
            not_stated(turn_cost(
                &resolver,
                &priced,
                Some(&measured(Some(90_000))),
                None,
            )),
            COST_TIMESTAMP_INVALID
        );
        assert_eq!(
            not_stated(turn_cost(
                &resolver,
                &unpriced,
                Some(&measured(Some(90_000))),
                Some(at),
            )),
            COST_NO_PRICE_DATA
        );
        let invalid = Usage {
            prompt_tokens: 10,
            completion_tokens: 1,
            cached_tokens: Some(11),
            evidence: UsageEvidence::Measured,
        };
        assert_eq!(
            not_stated(turn_cost(&resolver, &priced, Some(&invalid), Some(at),)),
            COST_INVALID_USAGE
        );
    }

    #[test]
    fn turn_ladder_bounds_an_unreported_cache_split() {
        let resolver = RouteResolver::from_toml(CATALOG).expect("catalog");
        let route = resolver.resolve("priced").expect("priced route");
        let TurnCostVerdict::Priced(cost) =
            turn_cost(&resolver, &route, Some(&measured(None)), Some(fixed_at()))
        else {
            panic!("expected priced turn");
        };

        assert!((cost.amount - 0.2).abs() < f64::EPSILON);
        assert_eq!(cost.currency, Currency::Usd);
        assert!(!cost.uncertain);
        assert!(!cost.cache_split_reported);
        assert_eq!(
            cost.headline(),
            "cost at most $0.20 - cache split not reported by provider"
        );
        assert!(cost.counterfactuals().is_empty());
    }

    #[test]
    fn turn_ladder_prices_the_common_headline_and_counterfactuals() {
        let resolver = RouteResolver::from_toml(CATALOG).expect("catalog");
        let route = resolver.resolve("priced").expect("priced route");
        let TurnCostVerdict::Priced(cost) = turn_cost(
            &resolver,
            &route,
            Some(&measured(Some(90_000))),
            Some(fixed_at()),
        ) else {
            panic!("expected priced turn");
        };

        assert!(cost.cache_split_reported);
        assert_eq!(cost.headline(), "cost $0.12 - saved 41% vs no-cache");
        assert_eq!(
            cost.counterfactuals(),
            ["peak $0.24", "no-cache $0.20", "top-tier $0.46"]
        );
    }

    #[test]
    fn compaction_ladder_covers_every_not_stated_reason() {
        let resolver = RouteResolver::from_toml(CATALOG).expect("catalog");
        let priced = resolver.resolve("priced").expect("priced route");
        let unpriced = resolver.resolve("unpriced").expect("unpriced route");
        let local = resolver.resolve("local").expect("local route");
        let at = fixed_at();

        assert_eq!(
            compaction_not_stated(compaction_cost(
                &resolver,
                Some(&priced),
                2,
                10,
                Some(20),
                Some(at),
            )),
            COMPACTION_AGGREGATE
        );
        assert_eq!(
            compaction_not_stated(compaction_cost(&resolver, None, 1, 10, Some(20), Some(at),)),
            COMPACTION_NO_PRICE_DATA
        );
        assert_eq!(
            compaction_not_stated(compaction_cost(
                &resolver,
                Some(&local),
                1,
                10,
                None,
                Some(at),
            )),
            format!(" · next-call money not stated - {LOCAL_METER_COPY}")
        );
        assert_eq!(
            compaction_not_stated(compaction_cost(
                &resolver,
                Some(&priced),
                1,
                10,
                None,
                Some(at),
            )),
            COMPACTION_CACHE_UNAVAILABLE
        );
        assert_eq!(
            compaction_not_stated(compaction_cost(
                &resolver,
                Some(&priced),
                1,
                10,
                Some(9),
                Some(at),
            )),
            COMPACTION_CACHE_TOO_SMALL
        );
        assert_eq!(
            compaction_not_stated(compaction_cost(
                &resolver,
                Some(&priced),
                1,
                10,
                Some(20),
                None,
            )),
            COMPACTION_TIME_UNAVAILABLE
        );
        assert_eq!(
            compaction_not_stated(compaction_cost(
                &resolver,
                Some(&unpriced),
                1,
                10,
                Some(20),
                Some(at),
            )),
            COMPACTION_NO_PRICE_DATA
        );

        let huge = rate_resolver(1e308, 1e308);
        let huge_route = huge.resolve("rate").expect("huge route");
        assert_eq!(
            compaction_not_stated(compaction_cost(
                &huge,
                Some(&huge_route),
                1,
                u64::MAX,
                Some(u64::MAX),
                Some(at),
            )),
            COMPACTION_INVALID_FACTS
        );

        let inverted = rate_resolver(2.0, 1.0);
        let inverted_route = inverted.resolve("rate").expect("inverted route");
        assert_eq!(
            compaction_not_stated(compaction_cost(
                &inverted,
                Some(&inverted_route),
                1,
                10,
                Some(20),
                Some(at),
            )),
            COMPACTION_INVERTED_CACHE_PRICE
        );
    }

    #[test]
    fn compaction_ladder_distinguishes_break_even_loss_and_saving() {
        let resolver = rate_resolver(1.0, 2.0);
        let route = resolver.resolve("rate").expect("rate route");
        let at = Some(fixed_at());

        for (elided, cached, expected) in [
            (10, 20, "net break-even"),
            (5, 15, "net loss"),
            (10, 15, "net saving"),
        ] {
            let verdict = compaction_cost(&resolver, Some(&route), 1, elided, Some(cached), at);
            let CompactionCostVerdict::Priced(cost) = &verdict else {
                panic!("expected priced compaction");
            };
            assert_eq!(cost.net_label, expected);
            assert!(verdict.suffix().contains(expected));
        }
    }
}
