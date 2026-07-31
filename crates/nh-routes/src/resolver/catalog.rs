//! Trusted catalog schema, validation, and route minting.

use super::{ResolvedRoute, RouteResolver};
use crate::pricing::{Currency, Fx, PeakWindows, PriceConfidence, RoutePrice};
use crate::route::{RouteClass, ThinkingDialect, Wire};
use crate::{is_banned, replacement_for};
use anyhow::anyhow;
use chrono::{NaiveDate, NaiveTime};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::net::IpAddr;

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
        "local" => Ok(RouteClass::Local),
        "delegate" => Ok(RouteClass::Delegate),
        other => Err(anyhow!(
            "route '{id}': unknown class '{other}' — set class = \"api\", \"local\", or \"delegate\""
        )),
    }
}

fn parse_dialect(id: &str, s: &str) -> anyhow::Result<ThinkingDialect> {
    match s {
        "deepseek-nhm" => Ok(ThinkingDialect::DeepseekNhm),
        "kimi-toggle" => Ok(ThinkingDialect::KimiToggle),
        "always-thinking" => Ok(ThinkingDialect::AlwaysThinking),
        "always-thinking-effort" => Ok(ThinkingDialect::AlwaysThinkingEffort),
        "glm-hm" => Ok(ThinkingDialect::GlmHm),
        "none" => Ok(ThinkingDialect::None),
        other => Err(anyhow!(
            "route '{id}': unknown thinking_dialect '{other}' — use deepseek-nhm, kimi-toggle, always-thinking, always-thinking-effort, glm-hm, or none"
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

fn validate_route_url(id: &str, value: &str, class: RouteClass) -> anyhow::Result<()> {
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
    if class == RouteClass::Local && !literal_loopback_host(&host.to_ascii_lowercase()) {
        return Err(anyhow!(
            "route '{id}': class = \"local\" requires a literal-loopback base_url"
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
    let peak = raw.peak.map(|p| parse_peak(id, p)).transpose()?;
    Ok(RoutePrice {
        currency,
        cache_hit: raw.cache_hit,
        cache_miss: raw.cache_miss,
        output: raw.output,
        confidence,
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
            let class = parse_class(&id, &r.class)?;
            validate_route_url(&id, &r.base_url, class)?;
            let wire = parse_wire(&id, &r.wire)?;
            if class == RouteClass::Local && wire != Wire::OpenAi {
                return Err(anyhow!(
                    "route '{id}': class = \"local\" must reuse wire = \"openai\""
                ));
            }
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
}
