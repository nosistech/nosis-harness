//! Catalog parsing, validation, route resolution, and rejection traces.

mod catalog;

use crate::pricing::{
    cost_of, money, usd_compare_key, Currency, Fx, NaiveCost, PriceQuote, RoutePrice,
};
use crate::route::{RouteClass, ThinkingDialect, Wire};
use crate::{is_banned, replacement_for};
use anyhow::anyhow;
use chrono::{DateTime, FixedOffset, Utc};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// A route minted from validated catalog data by [`RouteResolver`]. Its fields
/// are intentionally read-only outside the resolver.
///
/// ```compile_fail
/// fn forge(route: &nh_routes::ResolvedRoute) -> nh_routes::ResolvedRoute {
///     nh_routes::ResolvedRoute { id: "forged".to_owned(), ..route.clone() }
/// }
/// ```
#[derive(Debug, Clone)]
pub struct ResolvedRoute {
    /// Catalog key ("deepseek-v4-pro-anthropic") — may differ from `model_id`.
    id: String,
    provider: String,
    model_id: String,
    base_url: String,
    wire: Wire,
    /// nh-vault entry name; env fallback is `NH_<ENTRY>_KEY`.
    vault_entry: String,
    class: RouteClass,
    /// Subset of "text" | "image" | "video" | "audio", validated at parse.
    modality: Vec<String>,
    context: Option<u64>,
    max_out: Option<u64>,
    thinking_dialect: ThinkingDialect,
    /// True = reasoning_content must persist across turns (plan A.10.5).
    preserve_reasoning: bool,
    /// True = reasoning_content persists only while thinking is active.
    preserve_when_thinking: bool,
    /// Wire quirks matched by exact string (e.g. "empty-reasoning-content-on-tool-replay").
    quirks: Vec<String>,
    /// None = no token price (delegate routes are quota-metered, plan A.0).
    price: Option<RoutePrice>,
}
impl ResolvedRoute {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn provider(&self) -> &str {
        &self.provider
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn wire(&self) -> Wire {
        self.wire.clone()
    }

    pub fn vault_entry(&self) -> &str {
        &self.vault_entry
    }

    pub fn class(&self) -> RouteClass {
        self.class
    }

    pub fn modality(&self) -> &[String] {
        &self.modality
    }

    pub fn context(&self) -> Option<u64> {
        self.context
    }

    pub fn max_out(&self) -> Option<u64> {
        self.max_out
    }

    pub fn thinking_dialect(&self) -> ThinkingDialect {
        self.thinking_dialect
    }

    pub fn preserve_reasoning(&self) -> bool {
        self.preserve_reasoning
    }

    pub fn preserve_when_thinking(&self) -> bool {
        self.preserve_when_thinking
    }

    pub fn quirks(&self) -> &[String] {
        &self.quirks
    }

    pub fn price(&self) -> Option<&RoutePrice> {
        self.price.as_ref()
    }

    /// Price per million tokens at `at` (UTC). None when the route carries no
    /// price table. Peak windows are evaluated in the route's fixed-offset
    /// timezone; when `at` is inside a window, all three rates scale by the
    /// multiplier. `stale` = `valid_until` is absent or `at` falls after it
    /// (dated prices are valid through that whole UTC day).
    pub fn price_at(&self, at: DateTime<Utc>) -> Option<PriceQuote> {
        let price = self.price.as_ref()?;
        let stale = price.valid_until.is_none_or(|d| at.date_naive() > d);
        let (peak, factor) = match &price.peak {
            Some(p) if p.is_peak(at) => (true, p.multiplier),
            _ => (false, 1.0),
        };
        Some(PriceQuote {
            cache_hit: price.cache_hit * factor,
            cache_miss: price.cache_miss * factor,
            output: price.output * factor,
            currency: price.currency,
            peak,
            confidence: price.confidence,
            stale,
        })
    }

    /// Short clock-pricing chip for terminal cost HUDs. Peak boundaries are
    /// evaluated in the route timezone and displayed in the user's UTC offset.
    pub fn peak_status(&self, at: DateTime<Utc>, local: FixedOffset) -> String {
        if self.class == RouteClass::Local {
            return "local".into();
        }
        let Some(quote) = self.price_at(at) else {
            return "no price data".into();
        };
        if !quote.peak {
            return "off-peak".into();
        }
        let Some(peak) = self.price.as_ref().and_then(|price| price.peak.as_ref()) else {
            return "peak".into();
        };
        let Some(route_offset) = FixedOffset::east_opt(peak.utc_offset_secs) else {
            return "peak".into();
        };
        let route_local = at.with_timezone(&route_offset);
        let time = route_local.time();
        let Some((_, end)) = peak
            .windows
            .iter()
            .find(|(start, end)| time >= *start && time < *end)
        else {
            return "peak".into();
        };
        let end = match route_local
            .date_naive()
            .and_time(*end)
            .and_local_timezone(route_offset)
        {
            chrono::LocalResult::Single(end) => end,
            _ => return "peak".into(),
        };
        format!(
            "peak {}x until {}",
            trim_multiplier(peak.multiplier),
            end.with_timezone(&local).format("%H:%M")
        )
    }

    /// True when catalog.toml lists `name` in this route's quirks array.
    pub fn has_quirk(&self, name: &str) -> bool {
        self.quirks.iter().any(|q| q == name)
    }
}

fn trim_multiplier(multiplier: f64) -> String {
    if (multiplier - multiplier.round()).abs() < 1e-9 {
        format!("{multiplier:.0}")
    } else {
        format!("{multiplier}")
    }
}

pub struct RouteResolver {
    // catalog parsed from catalog.toml
    routes: BTreeMap<String, ResolvedRoute>,
    fx: Option<Fx>,
}

impl RouteResolver {
    /// Optional catalog FX data for approximate display glosses.
    pub fn fx(&self) -> Option<&Fx> {
        self.fx.as_ref()
    }

    /// Honest counterfactuals for one turn using catalog price data.
    pub fn naive_cost(
        &self,
        route: &ResolvedRoute,
        prompt_tokens: u64,
        cached_tokens: u64,
        output_tokens: u64,
        at: DateTime<Utc>,
    ) -> Option<NaiveCost> {
        let quote = route.price_at(at)?;
        let actual = cost_of(&quote, prompt_tokens, cached_tokens, output_tokens)?;
        let no_cache = cost_of(&quote, prompt_tokens, 0, output_tokens)?;

        let peak = route
            .price
            .as_ref()
            .and_then(|price| {
                price.peak.as_ref().map(|peak| PriceQuote {
                    cache_hit: price.cache_hit * peak.multiplier,
                    cache_miss: price.cache_miss * peak.multiplier,
                    output: price.output * peak.multiplier,
                    currency: price.currency,
                    peak: true,
                    confidence: price.confidence,
                    stale: quote.stale,
                })
            })
            .map_or(Some(actual), |peak_quote| {
                cost_of(&peak_quote, prompt_tokens, cached_tokens, output_tokens)
            })?;

        let top_tier_quote = self
            .routes
            .values()
            .filter(|candidate| candidate.class == RouteClass::Api)
            .filter_map(|candidate| candidate.price_at(at))
            .filter(|candidate| {
                candidate.currency == quote.currency && candidate.cache_miss > quote.cache_miss
            })
            .max_by(|left, right| left.cache_miss.total_cmp(&right.cache_miss));
        let top_tier = top_tier_quote.map_or(Some(actual), |top_quote| {
            cost_of(&top_quote, prompt_tokens, cached_tokens, output_tokens)
        })?;

        Some(NaiveCost {
            no_cache,
            peak,
            top_tier,
            currency: quote.currency,
        })
    }

    /// Resolve by route id (catalog key). Banned strings error with the replacement
    /// suggestion; unknown ids error listing available routes (friendly UX).
    pub fn resolve(&self, model_id: &str) -> anyhow::Result<ResolvedRoute> {
        if is_banned(model_id) {
            return Err(match replacement_for(model_id) {
                Some(replacement) => anyhow!("{model_id} is dead — use {replacement}"),
                None => anyhow!(
                    "{model_id} is a dead model id — use one of: {}",
                    self.available_list()
                ),
            });
        }
        match self.routes.get(model_id) {
            Some(route) => Ok(route.clone()),
            None => Err(anyhow!(
                "unknown model id '{model_id}' — available: {}",
                self.available_list()
            )),
        }
    }

    /// Default route for a provider. Rule (data-driven, documented in
    /// CONTRACTS_M1.md): the provider's cheapest class="api" route by off-peak
    /// output price, ties broken alphabetically by route id; routes without a
    /// price table are skipped (honest-cost rule: no price, no comparison).
    /// All of one provider's routes share a currency, so raw numbers compare.
    pub fn provider_default(&self, provider: &str) -> anyhow::Result<ResolvedRoute> {
        let mut best: Option<(&ResolvedRoute, f64)> = None;
        // BTreeMap iterates in key order, and we replace only on strictly-lower
        // price, so ties resolve alphabetically by route id for free.
        for route in self.routes.values() {
            if route.provider != provider || route.class != RouteClass::Api {
                continue;
            }
            let Some(price) = &route.price else { continue };
            if best.is_none_or(|(_, cheapest)| price.output < cheapest) {
                best = Some((route, price.output));
            }
        }
        if let Some((route, _)) = best {
            return Ok(route.clone());
        }
        let providers = self.available_by_provider();
        if providers.contains_key(provider) {
            Err(anyhow!(
                "provider '{provider}' has no priced api routes — pick a model directly with /model"
            ))
        } else {
            Err(anyhow!(
                "unknown provider '{provider}' — available: {}",
                providers.keys().cloned().collect::<Vec<_>>().join(", ")
            ))
        }
    }

    /// Pick the cheapest priced API route that fits the estimated request.
    /// Unknown context is rejected when the request has a non-zero estimate:
    /// the honest meter never promises capacity the catalog does not establish.
    /// Same-currency candidates compare native costs; mixed currencies compare
    /// only through fresh catalog FX; otherwise non-normalizable routes are refused.
    pub fn resolve_capable(
        &self,
        estimated_prompt_tokens: u64,
        estimated_output_tokens: u64,
        allowed: &[&str],
        at: DateTime<Utc>,
    ) -> anyhow::Result<(ResolvedRoute, RejectionTrace)> {
        struct Candidate<'a> {
            route: &'a ResolvedRoute,
            native_cost: f64,
            currency: Currency,
            usd_cost: Option<f64>,
        }

        let required = estimated_prompt_tokens.saturating_add(estimated_output_tokens);
        let allowed: BTreeSet<&str> = allowed.iter().copied().collect();
        let mut trace = RejectionTrace::default();
        let mut capable = Vec::new();

        for id in allowed {
            let Some(route) = self.routes.get(id) else {
                trace.push(id, "unknown route");
                continue;
            };
            if route.class != RouteClass::Api {
                trace.push(id, route.class.as_str());
                continue;
            }
            match route.context {
                Some(context) if context < required => {
                    trace.push(
                        id,
                        format!(
                            "ctx {} < {}",
                            compact_token_count(context),
                            compact_token_count(required)
                        ),
                    );
                    continue;
                }
                None if required > 0 => {
                    trace.push(id, "unknown context");
                    continue;
                }
                _ => {}
            }
            let Some(price) = route.price_at(at) else {
                trace.push(id, "no price");
                continue;
            };
            let expected_cost = (price.cache_miss * estimated_prompt_tokens as f64
                + price.output * estimated_output_tokens as f64)
                / 1_000_000.0;
            capable.push(Candidate {
                route,
                native_cost: expected_cost,
                currency: price.currency,
                usd_cost: usd_compare_key(
                    expected_cost,
                    price.currency,
                    self.fx.as_ref(),
                    at,
                    price.stale,
                ),
            });
        }

        let single_currency = capable.first().is_none_or(|first| {
            capable
                .iter()
                .all(|candidate| candidate.currency == first.currency)
        });
        if single_currency {
            capable.sort_by(|left, right| {
                left.native_cost
                    .total_cmp(&right.native_cost)
                    .then_with(|| left.route.id.cmp(&right.route.id))
            });
        } else {
            capable.retain(|candidate| {
                if candidate.usd_cost.is_some() {
                    true
                } else {
                    trace.push(candidate.route.id.clone(), "fx stale — ¥/$ not comparable");
                    false
                }
            });
            capable.sort_by(|left, right| {
                match (left.usd_cost, right.usd_cost) {
                    (Some(left_cost), Some(right_cost)) => left_cost.total_cmp(&right_cost),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => std::cmp::Ordering::Equal,
                }
                .then_with(|| left.route.id.cmp(&right.route.id))
            });
        }
        if capable.is_empty() {
            return Err(anyhow!(
                "no capable priced api route fits {} estimated tokens",
                required
            ));
        }
        let chosen = capable.remove(0);
        for candidate in capable {
            let reason = if candidate.currency != chosen.currency {
                format!(
                    "{} vs chosen {} — different currency, not directly comparable",
                    money(candidate.native_cost, candidate.currency),
                    money(chosen.native_cost, chosen.currency)
                )
            } else if candidate.native_cost == chosen.native_cost {
                "same price; route id tie-break".to_string()
            } else if chosen.native_cost == 0.0 {
                "higher price".to_string()
            } else {
                format!("{:.1}x price", candidate.native_cost / chosen.native_cost)
            };
            trace.push(candidate.route.id.clone(), reason);
        }
        Ok((chosen.route.clone(), trace))
    }

    /// Route ids grouped by provider, both levels sorted; used for provider
    /// suggestions and catalog listings.
    pub fn available_by_provider(&self) -> BTreeMap<String, Vec<String>> {
        let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (id, route) in &self.routes {
            map.entry(route.provider.clone())
                .or_default()
                .push(id.clone());
        }
        map
    }

    /// All routable model ids, for `--model` help text and error messages.
    pub fn available(&self) -> Vec<String> {
        // BTreeMap keys iterate in sorted order.
        self.routes.keys().cloned().collect()
    }

    fn available_list(&self) -> String {
        if self.routes.is_empty() {
            "none (catalog.toml has no routes)".to_string()
        } else {
            self.available().join(", ")
        }
    }
}

/// One plain-data explanation for a route that was not selected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteRejection {
    pub route_id: String,
    pub reason: String,
}

/// Auditable reasons why allowed routes were skipped by `resolve_capable`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RejectionTrace {
    pub rejections: Vec<RouteRejection>,
}

impl RejectionTrace {
    fn push(&mut self, route_id: impl Into<String>, reason: impl Into<String>) {
        self.rejections.push(RouteRejection {
            route_id: route_id.into(),
            reason: reason.into(),
        });
    }

    /// Short, side-effect-free lines suitable for a later scrubbed `/why` view.
    pub fn lines(&self) -> Vec<String> {
        self.rejections
            .iter()
            .map(|rejection| format!("skipped {}: {}", rejection.route_id, rejection.reason))
            .collect()
    }
}

impl fmt::Display for RejectionTrace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.lines().join("\n"))
    }
}

fn compact_token_count(tokens: u64) -> String {
    if tokens >= 1_000_000 && tokens.is_multiple_of(1_000_000) {
        format!("{}M", tokens / 1_000_000)
    } else if tokens >= 1_000 && tokens.is_multiple_of(1_000) {
        format!("{}K", tokens / 1_000)
    } else {
        tokens.to_string()
    }
}
