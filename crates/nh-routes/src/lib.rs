//! nh-routes — RouteResolver: the ONLY component that may mint a resolved route (plan §2).
//! M1: full Class-1 catalog (plan Appendix B), clock-aware pricing with fixed-offset
//! peak windows, thinking dialects, modality flags, provider defaults, banned-string
//! rejection. Catalog and pricing stay DATA in catalog.toml — never hard-coded here.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use anyhow::anyhow;
use chrono::{DateTime, FixedOffset, NaiveDate, NaiveTime, Utc};
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wire {
    OpenAi,
    AnthropicMessages,
}

/// Backend class (plan A.0): "api" = direct, token-metered; "delegate" =
/// subscription child CLI (claude/codex — adapter lands in M4, schema parses today).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteClass {
    Api,
    Delegate,
}

/// How a route expresses thinking effort on the wire (plan §3, A.1–A.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThinkingDialect {
    /// DeepSeek Non/High/Max via a body param (mapping pinned in CONTRACTS_M1.md).
    DeepseekNhm,
    /// Kimi K2.6: explicit thinking enable/disable toggle.
    KimiToggle,
    /// Kimi K2.7: no non-thinking mode exists — never send a toggle.
    AlwaysThinking,
    /// GLM thinking High/Max only.
    GlmHm,
    /// No effort toggle for this route.
    None,
}

impl ThinkingDialect {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::DeepseekNhm => "deepseek-nhm",
            Self::KimiToggle => "kimi-toggle",
            Self::AlwaysThinking => "always-thinking",
            Self::GlmHm => "glm-hm",
            Self::None => "none",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Currency {
    Cny,
    Usd,
}

impl Currency {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Cny => "CNY",
            Self::Usd => "USD",
        }
    }
}

impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Honest-cost rule (plan §7, B.8): price data carries its own confidence,
/// and stale/uncertain data is flagged — never guessed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriceConfidence {
    Confirmed,
    Reported,
    VerifyLive,
}

impl PriceConfidence {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Confirmed => "confirmed",
            Self::Reported => "reported",
            Self::VerifyLive => "verify_live",
        }
    }
}

impl fmt::Display for PriceConfidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Peak-pricing windows in a fixed-offset timezone (Asia/Shanghai = UTC+8, no DST).
/// Window start is inclusive, end exclusive, local to that timezone.
#[derive(Debug, Clone)]
pub struct PeakWindows {
    pub multiplier: f64,
    /// Timezone name as written in the catalog, for display ("Asia/Shanghai").
    pub timezone: String,
    /// Fixed UTC offset in seconds, resolved at parse time.
    pub utc_offset_secs: i32,
    pub windows: Vec<(NaiveTime, NaiveTime)>,
}

impl PeakWindows {
    fn is_peak(&self, at: DateTime<Utc>) -> bool {
        let offset =
            FixedOffset::east_opt(self.utc_offset_secs).expect("offset validated at parse");
        let local = at.with_timezone(&offset).time();
        self.windows
            .iter()
            .any(|(start, end)| local >= *start && local < *end)
    }
}

/// Off-peak base prices per million tokens, straight from catalog.toml.
#[derive(Debug, Clone)]
pub struct RoutePrice {
    pub currency: Currency,
    pub cache_hit: f64,
    pub cache_miss: f64,
    pub output: f64,
    pub confidence: PriceConfidence,
    /// Prices are valid through this whole UTC day; quotes after it are `stale`.
    pub valid_until: Option<NaiveDate>,
    pub peak: Option<PeakWindows>,
}

/// A price quote at one instant — what `/price` and the Cost HUD display.
#[derive(Debug, Clone, PartialEq)]
pub struct PriceQuote {
    pub cache_hit: f64,
    pub cache_miss: f64,
    pub output: f64,
    pub currency: Currency,
    pub peak: bool,
    pub confidence: PriceConfidence,
    /// True when the quote instant is past `valid_until` — flag it, never guess.
    pub stale: bool,
}

#[derive(Debug, Clone)]
pub struct ResolvedRoute {
    /// Catalog key ("deepseek-v4-pro-anthropic") — may differ from `model_id`.
    pub id: String,
    pub provider: String,
    pub model_id: String,
    pub base_url: String,
    pub wire: Wire,
    /// nh-vault entry name; env fallback is NH_<ENTRY>_KEY.
    pub vault_entry: String,
    pub class: RouteClass,
    /// Subset of "text" | "image" | "video" | "audio", validated at parse.
    pub modality: Vec<String>,
    pub context: Option<u64>,
    pub max_out: Option<u64>,
    pub thinking_dialect: ThinkingDialect,
    /// True = reasoning_content must persist across turns (plan A.10.5).
    pub preserve_reasoning: bool,
    /// True = reasoning_content persists only while thinking is active.
    pub preserve_when_thinking: bool,
    /// Wire quirks matched by exact string (e.g. "empty-reasoning-content-on-tool-replay").
    pub quirks: Vec<String>,
    /// None = no token price (delegate routes are quota-metered, plan A.0).
    pub price: Option<RoutePrice>,
}

impl ResolvedRoute {
    /// Price per million tokens at `at` (UTC). None when the route carries no
    /// price table. Peak windows are evaluated in the route's fixed-offset
    /// timezone; when `at` is inside a window, all three rates scale by the
    /// multiplier. `stale` = `at` falls after `valid_until` (prices are valid
    /// through that whole UTC day).
    pub fn price_at(&self, at: DateTime<Utc>) -> Option<PriceQuote> {
        let price = self.price.as_ref()?;
        let stale = price.valid_until.is_some_and(|d| at.date_naive() > d);
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

/// Dead/deprecated model ids (plan §A.9). Exact ids and prefixes; `mimo-v2-` does NOT
/// match `mimo-v2.5-*` (those are current). Rejection errors must name the replacement
/// (e.g. "deepseek-chat is dead as of 2026-07-24 — use deepseek-v4-flash").
pub const BANNED_EXACT: &[&str] = &["deepseek-chat", "deepseek-reasoner"];
pub const BANNED_PREFIXES: &[&str] = &["mimo-v2-", "gpt-5.2", "gpt-5.3-codex", "moonshot-v1-"];

/// Part of the ban list: replacement to name when rejecting a banned exact id.
const BANNED_REPLACEMENTS: &[(&str, &str)] = &[
    ("deepseek-chat", "deepseek-v4-flash"),
    ("deepseek-reasoner", "deepseek-v4-pro"),
];

pub fn is_banned(model_id: &str) -> bool {
    BANNED_EXACT.contains(&model_id) || BANNED_PREFIXES.iter().any(|p| model_id.starts_with(p))
}

fn replacement_for(model_id: &str) -> Option<&'static str> {
    BANNED_REPLACEMENTS
        .iter()
        .find(|(banned, _)| *banned == model_id)
        .map(|(_, replacement)| *replacement)
}

/// Raw catalog.toml shape. Unknown keys (e.g. M2's cache settings) are ignored
/// so newer catalog data never breaks this parser.
#[derive(Deserialize)]
struct RawCatalog {
    routes: BTreeMap<String, RawRoute>,
}

#[derive(Deserialize)]
struct RawRoute {
    provider: String,
    model_id: String,
    base_url: String,
    wire: String,
    vault_entry: String,
    #[serde(default = "default_class")]
    class: String,
    #[serde(default = "default_modality")]
    modality: Vec<String>,
    #[serde(default)]
    context: Option<u64>,
    #[serde(default)]
    max_out: Option<u64>,
    #[serde(default = "default_dialect")]
    thinking_dialect: String,
    #[serde(default)]
    preserve_reasoning: bool,
    #[serde(default)]
    preserve_when_thinking: bool,
    #[serde(default)]
    quirks: Vec<String>,
    #[serde(default)]
    price: Option<RawPrice>,
}

fn default_class() -> String {
    "api".into()
}
fn default_modality() -> Vec<String> {
    vec!["text".into()]
}
fn default_dialect() -> String {
    "none".into()
}

#[derive(Deserialize)]
struct RawPrice {
    currency: String,
    unit: String,
    cache_hit: f64,
    cache_miss: f64,
    output: f64,
    price_confidence: String,
    #[serde(default)]
    valid_until: Option<String>,
    #[serde(default)]
    peak: Option<RawPeak>,
}

#[derive(Deserialize)]
struct RawPeak {
    multiplier: f64,
    timezone: String,
    windows: Vec<String>,
}

const MODALITIES: &[&str] = &["text", "image", "video", "audio"];

/// Fixed offsets only, no DST (Asia/Shanghai has none). New timezones are added
/// here only when catalog data demands them — data drives code support.
fn peak_tz_offset_secs(tz: &str) -> Option<i32> {
    match tz {
        "Asia/Shanghai" => Some(8 * 3600),
        _ => None,
    }
}

fn parse_wire(id: &str, s: &str) -> anyhow::Result<Wire> {
    match s {
        "openai" => Ok(Wire::OpenAi),
        "anthropic" => Ok(Wire::AnthropicMessages),
        other => Err(anyhow!(
            "route '{id}': unknown wire '{other}' — set wire = \"openai\" or \"anthropic\" in catalog.toml"
        )),
    }
}

fn parse_class(id: &str, s: &str) -> anyhow::Result<RouteClass> {
    match s {
        "api" => Ok(RouteClass::Api),
        "delegate" => Ok(RouteClass::Delegate),
        other => Err(anyhow!(
            "route '{id}': unknown class '{other}' — set class = \"api\" or \"delegate\""
        )),
    }
}

fn parse_dialect(id: &str, s: &str) -> anyhow::Result<ThinkingDialect> {
    match s {
        "deepseek-nhm" => Ok(ThinkingDialect::DeepseekNhm),
        "kimi-toggle" => Ok(ThinkingDialect::KimiToggle),
        "always-thinking" => Ok(ThinkingDialect::AlwaysThinking),
        "glm-hm" => Ok(ThinkingDialect::GlmHm),
        "none" => Ok(ThinkingDialect::None),
        other => Err(anyhow!(
            "route '{id}': unknown thinking_dialect '{other}' — use deepseek-nhm, kimi-toggle, always-thinking, glm-hm, or none"
        )),
    }
}

fn check_modality(id: &str, modality: &[String]) -> anyhow::Result<()> {
    if modality.is_empty() {
        return Err(anyhow!(
            "route '{id}': modality must not be empty — start with [\"text\"]"
        ));
    }
    for m in modality {
        if !MODALITIES.contains(&m.as_str()) {
            return Err(anyhow!(
                "route '{id}': unknown modality '{m}' — use text, image, video, audio"
            ));
        }
    }
    Ok(())
}

fn check_rate(id: &str, field: &str, value: f64) -> anyhow::Result<()> {
    if !value.is_finite() || value < 0.0 {
        return Err(anyhow!("route '{id}': price {field} must be a number >= 0"));
    }
    Ok(())
}

fn parse_price(id: &str, raw: RawPrice) -> anyhow::Result<RoutePrice> {
    if raw.unit != "per_million_tokens" {
        return Err(anyhow!(
            "route '{id}': price unit '{}' — only \"per_million_tokens\" is supported",
            raw.unit
        ));
    }
    let currency = match raw.currency.as_str() {
        "CNY" => Currency::Cny,
        "USD" => Currency::Usd,
        other => {
            return Err(anyhow!(
                "route '{id}': unknown currency '{other}' — use CNY or USD"
            ))
        }
    };
    check_rate(id, "cache_hit", raw.cache_hit)?;
    check_rate(id, "cache_miss", raw.cache_miss)?;
    check_rate(id, "output", raw.output)?;
    let confidence = match raw.price_confidence.as_str() {
        "confirmed" => PriceConfidence::Confirmed,
        "reported" => PriceConfidence::Reported,
        "verify_live" => PriceConfidence::VerifyLive,
        other => {
            return Err(anyhow!(
                "route '{id}': unknown price_confidence '{other}' — use confirmed, reported, or verify_live"
            ))
        }
    };
    let valid_until = raw
        .valid_until
        .map(|s| {
            NaiveDate::parse_from_str(&s, "%Y-%m-%d")
                .map_err(|_| anyhow!("route '{id}': bad valid_until '{s}' — use YYYY-MM-DD"))
        })
        .transpose()?;
    let peak = raw.peak.map(|p| parse_peak(id, p)).transpose()?;
    Ok(RoutePrice {
        currency,
        cache_hit: raw.cache_hit,
        cache_miss: raw.cache_miss,
        output: raw.output,
        confidence,
        valid_until,
        peak,
    })
}

fn parse_peak(id: &str, raw: RawPeak) -> anyhow::Result<PeakWindows> {
    if !raw.multiplier.is_finite() || raw.multiplier <= 0.0 {
        return Err(anyhow!(
            "route '{id}': peak multiplier must be a number > 0"
        ));
    }
    let utc_offset_secs = peak_tz_offset_secs(&raw.timezone).ok_or_else(|| {
        anyhow!(
            "route '{id}': unsupported peak timezone '{}' — supported: Asia/Shanghai",
            raw.timezone
        )
    })?;
    if raw.windows.is_empty() {
        return Err(anyhow!("route '{id}': peak windows must not be empty"));
    }
    let mut windows = Vec::with_capacity(raw.windows.len());
    for w in &raw.windows {
        windows.push(parse_window(id, w)?);
    }
    Ok(PeakWindows {
        multiplier: raw.multiplier,
        timezone: raw.timezone,
        utc_offset_secs,
        windows,
    })
}

fn parse_window(id: &str, s: &str) -> anyhow::Result<(NaiveTime, NaiveTime)> {
    let bad = || {
        anyhow!("route '{id}': bad peak window '{s}' — use \"HH:MM-HH:MM\" with start before end")
    };
    let (a, b) = s.split_once('-').ok_or_else(bad)?;
    let start = NaiveTime::parse_from_str(a, "%H:%M").map_err(|_| bad())?;
    let end = NaiveTime::parse_from_str(b, "%H:%M").map_err(|_| bad())?;
    if start >= end {
        return Err(bad());
    }
    Ok((start, end))
}

pub struct RouteResolver {
    // catalog parsed from catalog.toml
    routes: BTreeMap<String, ResolvedRoute>,
}

impl RouteResolver {
    /// Parse a catalog.toml string (see repo-root catalog.toml for the schema).
    /// M0-era minimal routes still parse: class defaults to "api", modality to
    /// ["text"], thinking_dialect to "none"; price is optional.
    pub fn from_toml(toml_str: &str) -> anyhow::Result<Self> {
        let raw: RawCatalog = toml::from_str(toml_str)
            .map_err(|e| anyhow!("catalog.toml is invalid: {e} — fix the file and retry"))?;
        let mut routes = BTreeMap::new();
        for (id, r) in raw.routes {
            // The ban list applies to catalog data too: a clean alias must not
            // smuggle a banned model_id onto the wire.
            for banned in [id.as_str(), r.model_id.as_str()] {
                if is_banned(banned) {
                    return Err(match replacement_for(banned) {
                        Some(replacement) => {
                            anyhow!("catalog route '{id}': {banned} is dead — use {replacement}")
                        }
                        None => anyhow!(
                            "catalog route '{id}': {banned} is a dead model id — remove it from catalog.toml"
                        ),
                    });
                }
            }
            let wire = parse_wire(&id, &r.wire)?;
            let class = parse_class(&id, &r.class)?;
            let thinking_dialect = parse_dialect(&id, &r.thinking_dialect)?;
            check_modality(&id, &r.modality)?;
            let price = r.price.map(|p| parse_price(&id, p)).transpose()?;
            routes.insert(
                id.clone(),
                ResolvedRoute {
                    id,
                    provider: r.provider,
                    model_id: r.model_id,
                    base_url: r.base_url,
                    wire,
                    vault_entry: r.vault_entry,
                    class,
                    modality: r.modality,
                    context: r.context,
                    max_out: r.max_out,
                    thinking_dialect,
                    preserve_reasoning: r.preserve_reasoning,
                    preserve_when_thinking: r.preserve_when_thinking,
                    quirks: r.quirks,
                    price,
                },
            );
        }
        Ok(Self { routes })
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
    pub fn resolve_capable(
        &self,
        estimated_prompt_tokens: u64,
        estimated_output_tokens: u64,
        allowed: &[&str],
        at: DateTime<Utc>,
    ) -> anyhow::Result<(ResolvedRoute, RejectionTrace)> {
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
                trace.push(id, "delegate");
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
            capable.push((route, expected_cost));
        }

        capable.sort_by(|(left_route, left_cost), (right_route, right_cost)| {
            left_cost
                .total_cmp(right_cost)
                .then_with(|| left_route.id.cmp(&right_route.id))
        });
        let Some((chosen, chosen_cost)) = capable.first().copied() else {
            return Err(anyhow!(
                "no capable priced api route fits {} estimated tokens",
                required
            ));
        };
        for (route, cost) in capable.into_iter().skip(1) {
            let reason = if cost == chosen_cost {
                "same price; route id tie-break".to_string()
            } else if chosen_cost == 0.0 {
                "higher price".to_string()
            } else {
                format!("{:.1}x price", cost / chosen_cost)
            };
            trace.push(route.id.clone(), reason);
        }
        Ok((chosen.clone(), trace))
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    const CATALOG: &str = include_str!("../../../catalog.toml");

    fn resolver() -> RouteResolver {
        RouteResolver::from_toml(CATALOG).expect("repo-root catalog.toml must parse")
    }

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, s).unwrap()
    }

    /// Fixed date 2026-07-15, Beijing wall-clock time (UTC+8) expressed as UTC.
    fn beijing(h: u32, mi: u32) -> DateTime<Utc> {
        utc(2026, 7, 15, h - 8, mi, 0)
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    fn route_toml(extra: &str) -> String {
        format!(
            r#"
            [routes.test-model]
            provider = "p"
            model_id = "test-model"
            base_url = "https://example.invalid"
            wire = "openai"
            vault_entry = "p"
            {extra}
        "#
        )
    }

    fn priced_route(
        id: &str,
        class: &str,
        context: Option<u64>,
        cache_miss: f64,
        output: f64,
    ) -> String {
        let context = context.map_or_else(String::new, |value| format!("context = {value}"));
        format!(
            r#"
            [routes."{id}"]
            provider = "p"
            model_id = "{id}"
            base_url = "https://example.invalid"
            wire = "openai"
            vault_entry = "p"
            class = "{class}"
            {context}
            [routes."{id}".price]
            currency = "USD"
            unit = "per_million_tokens"
            cache_hit = 0.1
            cache_miss = {cache_miss}
            output = {output}
            price_confidence = "reported"
            "#
        )
    }

    fn capable_resolver() -> RouteResolver {
        let catalog = [
            priced_route("cheap-small", "api", Some(32_000), 0.1, 0.1),
            priced_route("fit-a", "api", Some(64_000), 1.0, 1.0),
            priced_route("fit-b", "api", Some(64_000), 1.0, 1.0),
            priced_route("fit-expensive", "api", Some(64_000), 4.0, 4.0),
            priced_route("unknown-context", "api", None, 0.1, 0.1),
            priced_route("delegated", "delegate", Some(64_000), 0.0, 0.0),
            r#"
            [routes.unpriced]
            provider = "p"
            model_id = "unpriced"
            base_url = "https://example.invalid"
            wire = "openai"
            vault_entry = "p"
            context = 64000
            "#
            .to_string(),
        ]
        .join("\n");
        RouteResolver::from_toml(&catalog).unwrap()
    }

    // ---------------------------------------------------------------- catalog shape

    #[test]
    fn parses_repo_catalog_with_all_class1_routes() {
        let r = resolver();
        assert_eq!(
            r.available(),
            vec![
                "deepseek-v4-flash",
                "deepseek-v4-flash-anthropic",
                "deepseek-v4-pro",
                "deepseek-v4-pro-anthropic",
                "glm-4.5-flash",
                "glm-4.6v-flash",
                "glm-4.7-flash",
                "glm-5.2",
                "kimi-k2.6",
                "kimi-k2.7-code",
                "kimi-k2.7-code-highspeed",
                "mimo-v2.5",
                "mimo-v2.5-pro",
            ]
        );
    }

    #[test]
    fn resolves_route_with_openai_wire() {
        let route = resolver().resolve("deepseek-v4-flash").expect("known id");
        assert_eq!(route.id, "deepseek-v4-flash");
        assert_eq!(route.wire, Wire::OpenAi);
        assert_eq!(route.provider, "deepseek");
        assert_eq!(route.model_id, "deepseek-v4-flash");
        assert_eq!(route.base_url, "https://api.deepseek.com");
        assert_eq!(route.vault_entry, "deepseek");
        assert_eq!(route.class, RouteClass::Api);
    }

    #[test]
    fn anthropic_wire_variant_keeps_model_id_and_changes_base_url() {
        // The deepclaude-proven path: same model, Anthropic Messages wire.
        let route = resolver()
            .resolve("deepseek-v4-pro-anthropic")
            .expect("known id");
        assert_eq!(route.id, "deepseek-v4-pro-anthropic");
        assert_eq!(route.model_id, "deepseek-v4-pro");
        assert_eq!(route.wire, Wire::AnthropicMessages);
        assert_eq!(route.base_url, "https://api.deepseek.com/anthropic");
        assert_eq!(route.vault_entry, "deepseek");
    }

    #[test]
    fn deepseek_routes_carry_dialect_quirk_and_limits() {
        let route = resolver().resolve("deepseek-v4-pro").unwrap();
        assert_eq!(route.thinking_dialect, ThinkingDialect::DeepseekNhm);
        assert!(route.has_quirk("empty-reasoning-content-on-tool-replay"));
        assert!(!route.has_quirk("no-such-quirk"));
        assert!(!route.preserve_reasoning);
        assert_eq!(route.context, Some(1_000_000));
        assert_eq!(route.max_out, Some(384_000));
        assert_eq!(route.modality, vec!["text"]);
    }

    #[test]
    fn kimi_k27_routes_are_always_thinking_and_preserve_reasoning() {
        let r = resolver();
        for id in ["kimi-k2.7-code", "kimi-k2.7-code-highspeed"] {
            let route = r.resolve(id).unwrap();
            assert_eq!(
                route.thinking_dialect,
                ThinkingDialect::AlwaysThinking,
                "{id}"
            );
            assert!(
                route.preserve_reasoning,
                "{id} must preserve reasoning (plan A.10.5)"
            );
            assert_eq!(route.modality, vec!["text", "image", "video"], "{id}");
        }
    }

    #[test]
    fn kimi_k26_uses_toggle_and_state_aware_reasoning_replay() {
        let route = resolver().resolve("kimi-k2.6").unwrap();
        assert_eq!(route.thinking_dialect, ThinkingDialect::KimiToggle);
        assert!(!route.preserve_reasoning);
        assert!(route.preserve_when_thinking);
        assert_eq!(route.thinking_dialect.as_str(), "kimi-toggle");
    }

    #[test]
    fn mimo_routes_preserve_reasoning_and_are_omni_modal() {
        let r = resolver();
        for id in ["mimo-v2.5", "mimo-v2.5-pro"] {
            let route = r.resolve(id).unwrap();
            assert!(route.preserve_reasoning, "{id}");
            assert_eq!(
                route.modality,
                vec!["text", "image", "video", "audio"],
                "{id}"
            );
            assert_eq!(route.context, Some(1_000_000), "{id}");
        }
    }

    #[test]
    fn glm_52_uses_glm_hm_dialect() {
        let route = resolver().resolve("glm-5.2").unwrap();
        assert_eq!(route.thinking_dialect, ThinkingDialect::GlmHm);
        assert_eq!(route.max_out, Some(128_000));
    }

    #[test]
    fn minimal_m0_route_parses_with_defaults() {
        // Old user catalogs (M0 shape) keep working: everything M1 defaults.
        let r = RouteResolver::from_toml(&route_toml("")).unwrap();
        let route = r.resolve("test-model").unwrap();
        assert_eq!(route.class, RouteClass::Api);
        assert_eq!(route.modality, vec!["text"]);
        assert_eq!(route.thinking_dialect, ThinkingDialect::None);
        assert!(!route.preserve_reasoning);
        assert!(!route.preserve_when_thinking);
        assert!(route.quirks.is_empty());
        assert!(route.context.is_none());
        assert!(route.max_out.is_none());
        assert!(route.price.is_none());
    }

    #[test]
    fn delegate_class_parses() {
        let r = RouteResolver::from_toml(&route_toml(r#"class = "delegate""#)).unwrap();
        assert_eq!(r.resolve("test-model").unwrap().class, RouteClass::Delegate);
    }

    // ---------------------------------------------------------------- pricing

    #[test]
    fn peak_boundary_math_in_beijing_time() {
        // Peak = Beijing 09:00-12:00 & 14:00-18:00, start inclusive, end exclusive.
        let route = resolver().resolve("deepseek-v4-pro").unwrap();
        let cases = [
            (8, 59, false),
            (9, 0, true),
            (11, 59, true),
            (12, 0, false),
            (13, 59, false),
            (14, 0, true),
            (17, 59, true),
            (18, 0, false),
        ];
        for (h, m, want_peak) in cases {
            let quote = route.price_at(beijing(h, m)).expect("priced route");
            assert_eq!(quote.peak, want_peak, "Beijing {h:02}:{m:02}");
        }
    }

    #[test]
    fn peak_quote_doubles_all_rates() {
        // Plan 0.1 confirmed: V4-Pro peak = ¥0.05 / ¥6.00 / ¥12.00.
        let route = resolver().resolve("deepseek-v4-pro").unwrap();
        let quote = route.price_at(beijing(10, 0)).unwrap();
        assert!(quote.peak);
        assert!(close(quote.cache_hit, 0.05), "got {}", quote.cache_hit);
        assert!(close(quote.cache_miss, 6.00), "got {}", quote.cache_miss);
        assert!(close(quote.output, 12.00), "got {}", quote.output);
        assert_eq!(quote.currency, Currency::Cny);
        assert_eq!(quote.confidence, PriceConfidence::Confirmed);
        assert!(!quote.stale);
    }

    #[test]
    fn off_peak_quote_is_the_base_rate() {
        let route = resolver().resolve("deepseek-v4-pro").unwrap();
        let quote = route.price_at(beijing(8, 0)).unwrap();
        assert!(!quote.peak);
        assert!(close(quote.cache_hit, 0.025));
        assert!(close(quote.cache_miss, 3.00));
        assert!(close(quote.output, 6.00));
    }

    #[test]
    fn peak_status_is_short_local_and_boundary_exact() {
        let route = resolver().resolve("deepseek-v4-pro").unwrap();
        let beijing_offset = FixedOffset::east_opt(8 * 3600).unwrap();
        assert_eq!(
            route.peak_status(beijing(15, 0), beijing_offset),
            "peak 2x until 18:00"
        );
        assert_eq!(
            route.peak_status(beijing(18, 0), beijing_offset),
            "off-peak"
        );
        let user_offset = FixedOffset::west_opt(6 * 3600).unwrap();
        assert_eq!(
            route.peak_status(beijing(10, 30), user_offset),
            "peak 2x until 22:00"
        );
    }

    #[test]
    fn routes_without_peak_windows_are_never_peak() {
        let route = resolver().resolve("kimi-k2.6").unwrap();
        let quote = route.price_at(beijing(10, 0)).unwrap();
        assert!(!quote.peak);
        // First-party prices confirmed 2026-07-13 (platform.kimi.ai/docs/pricing/chat-k26.md).
        assert!(close(quote.output, 4.00));
        assert_eq!(quote.currency, Currency::Usd);
        assert_eq!(quote.confidence, PriceConfidence::Confirmed);
    }

    #[test]
    fn stale_flag_flips_after_valid_until() {
        // deepseek prices carry valid_until = 2026-07-24 (valid through that UTC day).
        let route = resolver().resolve("deepseek-v4-flash").unwrap();
        let fresh = route.price_at(utc(2026, 7, 24, 23, 59, 59)).unwrap();
        assert!(!fresh.stale, "still valid on the valid_until day");
        let stale = route.price_at(utc(2026, 7, 25, 0, 0, 0)).unwrap();
        assert!(
            stale.stale,
            "past valid_until must flag stale — honest-cost rule"
        );
    }

    #[test]
    fn free_glm_route_quotes_zero_without_stale_or_peak() {
        let route = resolver().resolve("glm-4.7-flash").unwrap();
        let quote = route.price_at(utc(2026, 12, 1, 12, 0, 0)).unwrap();
        assert!(close(quote.cache_hit, 0.0));
        assert!(close(quote.cache_miss, 0.0));
        assert!(close(quote.output, 0.0));
        assert_eq!(quote.currency, Currency::Usd);
        // Free tier confirmed 2026-07-13 (docs.z.ai/guides/overview/pricing).
        assert_eq!(quote.confidence, PriceConfidence::Confirmed);
        assert!(!quote.peak);
        assert!(!quote.stale, "no valid_until means never stale");
    }

    #[test]
    fn mimo_prices_are_confirmed_first_party() {
        // Plan B.3 pricing conflict resolved 2026-07-13 against mimo.mi.com/docs/pricing.
        let r = resolver();
        for id in ["mimo-v2.5", "mimo-v2.5-pro"] {
            let route = r.resolve(id).unwrap();
            let quote = route.price_at(utc(2026, 7, 15, 12, 0, 0)).unwrap();
            assert_eq!(quote.confidence, PriceConfidence::Confirmed, "{id}");
        }
    }

    #[test]
    fn route_without_price_table_quotes_none() {
        let r = RouteResolver::from_toml(&route_toml("")).unwrap();
        let route = r.resolve("test-model").unwrap();
        assert!(route.price_at(utc(2026, 7, 15, 12, 0, 0)).is_none());
    }

    // ---------------------------------------------------------------- provider defaults

    #[test]
    fn resolve_capable_picks_cheapest_fitting_route_and_explains_every_skip() {
        let resolver = capable_resolver();
        let allowed = [
            "fit-expensive",
            "missing",
            "cheap-small",
            "fit-b",
            "delegated",
            "fit-a",
            "unknown-context",
            "unpriced",
        ];
        let (route, trace) = resolver
            .resolve_capable(40_000, 5_000, &allowed, utc(2026, 7, 15, 12, 0, 0))
            .unwrap();
        assert_eq!(route.id, "fit-a", "equal-cost ties break by route id");

        let reasons: BTreeMap<&str, &str> = trace
            .rejections
            .iter()
            .map(|entry| (entry.route_id.as_str(), entry.reason.as_str()))
            .collect();
        assert_eq!(reasons.len(), allowed.len() - 1);
        assert_eq!(reasons["cheap-small"], "ctx 32K < 45K");
        assert_eq!(reasons["delegated"], "delegate");
        assert_eq!(reasons["unknown-context"], "unknown context");
        assert_eq!(reasons["unpriced"], "no price");
        assert_eq!(reasons["missing"], "unknown route");
        assert_eq!(reasons["fit-b"], "same price; route id tie-break");
        assert_eq!(reasons["fit-expensive"], "4.0x price");
        assert!(trace
            .lines()
            .iter()
            .all(|line| line.starts_with("skipped ")));
        assert_eq!(trace.to_string(), trace.lines().join("\n"));
    }

    #[test]
    fn provider_default_picks_lowest_output_price() {
        let r = resolver();
        // Rule: cheapest api route by off-peak output price, ties alphabetical.
        assert_eq!(
            r.provider_default("deepseek").unwrap().id,
            "deepseek-v4-flash"
        );
        assert_eq!(r.provider_default("kimi").unwrap().id, "kimi-k2.6");
        assert_eq!(r.provider_default("mimo").unwrap().id, "mimo-v2.5");
        // Three free GLM routes tie at 0 — alphabetical order breaks the tie.
        assert_eq!(r.provider_default("glm").unwrap().id, "glm-4.5-flash");
    }

    #[test]
    fn provider_default_unknown_provider_lists_providers() {
        let msg = resolver().provider_default("acme").unwrap_err().to_string();
        assert!(msg.contains("unknown provider 'acme'"), "got: {msg}");
        for p in ["deepseek", "glm", "kimi", "mimo"] {
            assert!(msg.contains(p), "must list {p}: {msg}");
        }
    }

    #[test]
    fn provider_default_skips_unpriced_and_delegate_routes() {
        let toml = r#"
            [routes.pricey]
            provider = "p"
            model_id = "pricey"
            base_url = "https://example.invalid"
            wire = "openai"
            vault_entry = "p"
            [routes.pricey.price]
            currency = "USD"
            unit = "per_million_tokens"
            cache_hit = 1.0
            cache_miss = 2.0
            output = 9.0
            price_confidence = "reported"

            [routes.unpriced-cheap]
            provider = "p"
            model_id = "unpriced-cheap"
            base_url = "https://example.invalid"
            wire = "openai"
            vault_entry = "p"

            [routes.delegate-model]
            provider = "p"
            model_id = "delegate-model"
            base_url = ""
            wire = "openai"
            vault_entry = "p"
            class = "delegate"
            [routes.delegate-model.price]
            currency = "USD"
            unit = "per_million_tokens"
            cache_hit = 0.0
            cache_miss = 0.0
            output = 0.0
            price_confidence = "reported"
        "#;
        let r = RouteResolver::from_toml(toml).unwrap();
        assert_eq!(r.provider_default("p").unwrap().id, "pricey");
    }

    #[test]
    fn provider_with_no_priced_api_routes_gets_actionable_error() {
        let r = RouteResolver::from_toml(&route_toml("")).unwrap();
        let msg = r.provider_default("p").unwrap_err().to_string();
        assert!(msg.contains("no priced api routes"), "got: {msg}");
        assert!(msg.contains("/model"), "must say what to do next: {msg}");
    }

    #[test]
    fn available_by_provider_groups_and_sorts() {
        let map = resolver().available_by_provider();
        assert_eq!(
            map.keys().cloned().collect::<Vec<_>>(),
            vec!["deepseek", "glm", "kimi", "mimo"]
        );
        assert_eq!(
            map["deepseek"],
            vec![
                "deepseek-v4-flash",
                "deepseek-v4-flash-anthropic",
                "deepseek-v4-pro",
                "deepseek-v4-pro-anthropic",
            ]
        );
        assert_eq!(map["mimo"], vec!["mimo-v2.5", "mimo-v2.5-pro"]);
    }

    // ---------------------------------------------------------------- banned strings

    #[test]
    fn banned_exact_ids_are_banned() {
        for id in BANNED_EXACT {
            assert!(is_banned(id), "{id} must be banned");
        }
    }

    #[test]
    fn banned_exact_errors_name_replacement() {
        let r = resolver();
        for (banned, replacement) in super::BANNED_REPLACEMENTS {
            let msg = r.resolve(banned).unwrap_err().to_string();
            assert!(
                msg.contains(replacement),
                "error for {banned} must name {replacement}: {msg}"
            );
        }
    }

    #[test]
    fn each_banned_prefix_matches() {
        for prefix in BANNED_PREFIXES {
            let id = format!("{prefix}0test");
            assert!(is_banned(&id), "{id} must be banned");
            assert!(is_banned(prefix), "bare prefix {prefix} must be banned");
        }
    }

    #[test]
    fn banned_prefix_resolve_gives_generic_error_listing_routes() {
        let r = resolver();
        let id = format!("{}0test", BANNED_PREFIXES[0]);
        let msg = r.resolve(&id).unwrap_err().to_string();
        assert!(msg.contains("dead"), "must say the id is dead: {msg}");
        assert!(msg.contains("deepseek-v4-flash"), "must list routes: {msg}");
    }

    #[test]
    fn mimo_v2_5_is_allowed() {
        assert!(!is_banned("mimo-v2.5-pro"));
        assert!(!is_banned("mimo-v2.5"));
    }

    #[test]
    fn unknown_id_error_lists_available_routes() {
        let msg = resolver().resolve("no-such-model").unwrap_err().to_string();
        assert!(msg.contains("no-such-model"), "must echo the bad id: {msg}");
        assert!(msg.contains("deepseek-v4-flash"), "must list routes: {msg}");
        assert!(msg.contains("kimi-k2.7-code"), "must list routes: {msg}");
    }

    #[test]
    fn catalog_alias_hiding_banned_model_id_is_rejected() {
        // A clean route key must not smuggle a banned model_id onto the wire.
        let toml = format!(
            r#"
            [routes.flash]
            provider = "p"
            model_id = "{}"
            base_url = "https://example.invalid"
            wire = "openai"
            vault_entry = "p"
        "#,
            BANNED_EXACT[0]
        );
        let msg = RouteResolver::from_toml(&toml)
            .err()
            .expect("must fail")
            .to_string();
        assert!(msg.contains("dead"), "must say the id is dead: {msg}");
        assert!(
            msg.contains("deepseek-v4-flash"),
            "must name the replacement: {msg}"
        );
    }

    #[test]
    fn catalog_banned_route_key_is_rejected() {
        let banned_key = format!("{}0test", BANNED_PREFIXES[0]);
        let toml = format!(
            r#"
            [routes."{banned_key}"]
            provider = "p"
            model_id = "clean-model"
            base_url = "https://example.invalid"
            wire = "openai"
            vault_entry = "p"
        "#
        );
        let msg = RouteResolver::from_toml(&toml)
            .err()
            .expect("must fail")
            .to_string();
        assert!(msg.contains("dead"), "must say the id is dead: {msg}");
        assert!(
            msg.contains("remove it from catalog.toml"),
            "must be actionable: {msg}"
        );
    }

    // ---------------------------------------------------------------- validation errors

    #[test]
    fn unknown_wire_is_rejected() {
        let toml = route_toml("").replace(r#"wire = "openai""#, r#"wire = "carrier-pigeon""#);
        let msg = RouteResolver::from_toml(&toml)
            .err()
            .expect("must fail")
            .to_string();
        assert!(
            msg.contains("carrier-pigeon"),
            "must name the bad wire: {msg}"
        );
        assert!(msg.contains("openai"), "must say valid values: {msg}");
    }

    #[test]
    fn unknown_class_is_rejected() {
        let msg = RouteResolver::from_toml(&route_toml(r#"class = "bogus""#))
            .err()
            .expect("must fail")
            .to_string();
        assert!(msg.contains("bogus"), "got: {msg}");
        assert!(msg.contains("delegate"), "must say valid values: {msg}");
    }

    #[test]
    fn unknown_dialect_is_rejected() {
        let msg = RouteResolver::from_toml(&route_toml(r#"thinking_dialect = "vibes""#))
            .err()
            .expect("must fail")
            .to_string();
        assert!(msg.contains("vibes"), "got: {msg}");
        assert!(
            msg.contains("always-thinking"),
            "must say valid values: {msg}"
        );
    }

    #[test]
    fn unknown_modality_is_rejected() {
        let msg = RouteResolver::from_toml(&route_toml(r#"modality = ["smell"]"#))
            .err()
            .expect("must fail")
            .to_string();
        assert!(msg.contains("smell"), "got: {msg}");
        assert!(msg.contains("text"), "must say valid values: {msg}");
    }

    #[test]
    fn bad_price_fields_are_rejected() {
        let price = |line: &str| {
            route_toml(&format!(
                r#"
                [routes.test-model.price]
                currency = "USD"
                unit = "per_million_tokens"
                cache_hit = 0.1
                cache_miss = 0.2
                output = 0.3
                price_confidence = "reported"
                {line}
            "#
            ))
        };
        // Wrong unit.
        let toml = price("").replace("per_million_tokens", "per_token");
        let msg = RouteResolver::from_toml(&toml)
            .err()
            .expect("must fail")
            .to_string();
        assert!(msg.contains("per_million_tokens"), "got: {msg}");
        // Bad currency.
        let toml = price("").replace(r#"currency = "USD""#, r#"currency = "EUR""#);
        let msg = RouteResolver::from_toml(&toml)
            .err()
            .expect("must fail")
            .to_string();
        assert!(msg.contains("EUR") && msg.contains("CNY"), "got: {msg}");
        // Bad confidence.
        let toml = price("").replace("reported", "hopeful");
        let msg = RouteResolver::from_toml(&toml)
            .err()
            .expect("must fail")
            .to_string();
        assert!(
            msg.contains("hopeful") && msg.contains("verify_live"),
            "got: {msg}"
        );
        // Negative rate.
        let toml = price("").replace("output = 0.3", "output = -1.0");
        let msg = RouteResolver::from_toml(&toml)
            .err()
            .expect("must fail")
            .to_string();
        assert!(msg.contains("output"), "got: {msg}");
        // Bad valid_until.
        let msg = RouteResolver::from_toml(&price(r#"valid_until = "July 24""#))
            .err()
            .expect("must fail")
            .to_string();
        assert!(msg.contains("YYYY-MM-DD"), "got: {msg}");
    }

    #[test]
    fn bad_peak_tables_are_rejected() {
        let peak = |mult: &str, tz: &str, window: &str| {
            route_toml(&format!(
                r#"
                [routes.test-model.price]
                currency = "CNY"
                unit = "per_million_tokens"
                cache_hit = 0.1
                cache_miss = 0.2
                output = 0.3
                price_confidence = "reported"
                [routes.test-model.price.peak]
                multiplier = {mult}
                timezone = "{tz}"
                windows = ["{window}"]
            "#
            ))
        };
        // Unknown timezone.
        let msg = RouteResolver::from_toml(&peak("2.0", "Mars/Olympus", "09:00-12:00"))
            .err()
            .expect("must fail")
            .to_string();
        assert!(msg.contains("Mars/Olympus"), "got: {msg}");
        assert!(
            msg.contains("Asia/Shanghai"),
            "must name a supported tz: {msg}"
        );
        // Malformed window.
        let msg = RouteResolver::from_toml(&peak("2.0", "Asia/Shanghai", "9am-noon"))
            .err()
            .expect("must fail")
            .to_string();
        assert!(msg.contains("HH:MM-HH:MM"), "got: {msg}");
        // Start not before end.
        let msg = RouteResolver::from_toml(&peak("2.0", "Asia/Shanghai", "12:00-09:00"))
            .err()
            .expect("must fail")
            .to_string();
        assert!(msg.contains("start before end"), "got: {msg}");
        // Zero multiplier.
        let msg = RouteResolver::from_toml(&peak("0.0", "Asia/Shanghai", "09:00-12:00"))
            .err()
            .expect("must fail")
            .to_string();
        assert!(msg.contains("multiplier"), "got: {msg}");
    }

    #[test]
    fn invalid_toml_is_a_friendly_error() {
        let msg = RouteResolver::from_toml("not [ valid")
            .err()
            .expect("must fail")
            .to_string();
        assert!(
            msg.contains("catalog.toml is invalid"),
            "friendly message: {msg}"
        );
    }

    // ---------------------------------------------------------------- display helpers

    #[test]
    fn display_strings_match_catalog_vocabulary() {
        assert_eq!(Currency::Cny.to_string(), "CNY");
        assert_eq!(Currency::Usd.to_string(), "USD");
        assert_eq!(PriceConfidence::Confirmed.to_string(), "confirmed");
        assert_eq!(PriceConfidence::VerifyLive.to_string(), "verify_live");
        assert_eq!(ThinkingDialect::DeepseekNhm.as_str(), "deepseek-nhm");
    }
}
