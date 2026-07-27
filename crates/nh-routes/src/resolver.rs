//! Catalog parsing, validation, route resolution, and rejection traces.

use super::*;

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
    /// nh-vault entry name; env fallback is NH_<ENTRY>_KEY.
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

/// Raw catalog.toml shape. Unknown keys (e.g. M2's cache settings) are ignored
/// so newer catalog data never breaks this parser.
#[derive(Deserialize)]
struct RawCatalog {
    routes: BTreeMap<String, RawRoute>,
    #[serde(default)]
    fx: Option<RawFx>,
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
struct RawFx {
    usd_per_cny: f64,
    #[serde(default)]
    valid_until: Option<String>,
    price_confidence: String,
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

fn literal_loopback_host(host: &str) -> bool {
    let host = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host);
    if host == "localhost" {
        return true;
    }
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(address)) => address.is_loopback(),
        Ok(IpAddr::V6(address)) => address.is_loopback(),
        Err(_) => false,
    }
}

fn validate_route_url(id: &str, value: &str) -> anyhow::Result<()> {
    let parsed = url::Url::parse(value).map_err(|_| {
        anyhow!(
            "route '{id}': base_url is not an absolute URL — use https, or plain http for literal loopback only"
        )
    })?;
    let host = parsed.host_str().ok_or_else(|| {
        anyhow!(
            "route '{id}': base_url has no host — use https, or plain http for literal loopback only"
        )
    })?;
    let allowed = parsed.scheme() == "https"
        || (parsed.scheme() == "http" && literal_loopback_host(&host.to_ascii_lowercase()));
    if !allowed {
        return Err(anyhow!(
            "route '{id}': base_url must use https; plain http is allowed only for literal loopback (127.0.0.0/8, [::1], or localhost)"
        ));
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
    let confidence = parse_confidence(&format!("route '{id}'"), &raw.price_confidence)?;
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

fn parse_confidence(scope: &str, value: &str) -> anyhow::Result<PriceConfidence> {
    match value {
        "confirmed" => Ok(PriceConfidence::Confirmed),
        "reported" => Ok(PriceConfidence::Reported),
        "verify_live" => Ok(PriceConfidence::VerifyLive),
        other => Err(anyhow!(
            "{scope}: unknown price_confidence '{other}' — use confirmed, reported, or verify_live"
        )),
    }
}

fn parse_fx(raw: RawFx) -> anyhow::Result<Fx> {
    if !raw.usd_per_cny.is_finite() || raw.usd_per_cny <= 0.0 {
        return Err(anyhow!("fx.usd_per_cny must be a number > 0"));
    }
    let valid_until = raw
        .valid_until
        .map(|s| {
            NaiveDate::parse_from_str(&s, "%Y-%m-%d")
                .map_err(|_| anyhow!("fx: bad valid_until '{s}' — use YYYY-MM-DD"))
        })
        .transpose()?;
    Ok(Fx {
        usd_per_cny: raw.usd_per_cny,
        valid_until,
        confidence: parse_confidence("fx", &raw.price_confidence)?,
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
    fx: Option<Fx>,
}

fn validate_provider_currencies(routes: &BTreeMap<String, ResolvedRoute>) -> anyhow::Result<()> {
    let mut currencies: BTreeMap<&str, Currency> = BTreeMap::new();
    for route in routes.values() {
        let Some(price) = &route.price else { continue };
        match currencies.get(route.provider.as_str()) {
            Some(currency) if *currency != price.currency => {
                return Err(anyhow!(
                    "catalog provider '{}' mixes currencies (CNY and USD) — one provider must price in one currency",
                    route.provider
                ));
            }
            Some(_) => {}
            None => {
                currencies.insert(route.provider.as_str(), price.currency);
            }
        }
    }
    Ok(())
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
            validate_route_url(&id, &r.base_url)?;
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
        validate_provider_currencies(&routes)?;
        Ok(Self {
            routes,
            fx: raw.fx.map(parse_fx).transpose()?,
        })
    }

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
                left.usd_cost
                    .expect("retained candidates have a USD comparison key")
                    .total_cmp(
                        &right
                            .usd_cost
                            .expect("retained candidates have a USD comparison key"),
                    )
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
