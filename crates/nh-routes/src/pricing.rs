//! Clock-aware route pricing, currency conversion, and stable money display.

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveTime, Utc};
use std::fmt;

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

/// Price data carries its catalog confidence; rates are never guessed.
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

/// Approximate CNY-to-USD display rate carried by catalog data.
#[derive(Debug, Clone, PartialEq)]
pub struct Fx {
    pub usd_per_cny: f64,
    pub valid_until: Option<NaiveDate>,
    pub confidence: PriceConfidence,
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
    pub(super) fn is_peak(&self, at: DateTime<Utc>) -> bool {
        let Some(offset) = FixedOffset::east_opt(self.utc_offset_secs) else {
            return false;
        };
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
    pub peak: Option<PeakWindows>,
}

/// A price quote at one instant - what `/price` and the Cost HUD display.
#[derive(Debug, Clone, PartialEq)]
pub struct PriceQuote {
    pub cache_hit: f64,
    pub cache_miss: f64,
    pub output: f64,
    pub currency: Currency,
    pub peak: bool,
    pub confidence: PriceConfidence,
}

/// Counterfactual costs for the same turn and token counts.
#[derive(Debug, Clone, PartialEq)]
pub struct NaiveCost {
    pub no_cache: f64,
    pub peak: f64,
    pub top_tier: f64,
    pub currency: Currency,
}

/// Currency cost of one turn in the quote's native currency. Returns `None`
/// when usage is inconsistent or the computed amount is non-finite.
pub fn cost_of(
    quote: &PriceQuote,
    prompt_tokens: u64,
    cached_tokens: u64,
    output_tokens: u64,
) -> Option<f64> {
    if cached_tokens > prompt_tokens {
        return None;
    }
    let miss_tokens = prompt_tokens - cached_tokens;
    let amount = (cached_tokens as f64 * quote.cache_hit
        + miss_tokens as f64 * quote.cache_miss
        + output_tokens as f64 * quote.output)
        / 1_000_000.0;
    amount.is_finite().then_some(amount)
}

/// Maximum possible cost when the provider reported prompt and output tokens
/// but omitted the cached-token split. Both feasible endpoints are evaluated
/// so the bound remains sound regardless of cache-hit and cache-miss ordering.
pub fn cache_split_cost_upper_bound(
    quote: &PriceQuote,
    prompt_tokens: u64,
    output_tokens: u64,
) -> Option<f64> {
    let no_cache = cost_of(quote, prompt_tokens, 0, output_tokens)?;
    let all_cached = cost_of(quote, prompt_tokens, prompt_tokens, output_tokens)?;
    Some(no_cache.max(all_cached))
}

/// Percent saved by cache reuse versus the same route with no cache.
pub fn saved_pct(actual: f64, no_cache: f64) -> Option<u8> {
    if !actual.is_finite() || !no_cache.is_finite() || no_cache <= actual {
        return None;
    }
    Some(
        (((no_cache - actual) / no_cache) * 100.0)
            .round()
            .clamp(0.0, 99.0) as u8,
    )
}

/// Approximate USD for a CNY amount with present, unexpired FX metadata.
/// USD-native amounts need no gloss.
pub fn to_usd_approx(amount: f64, currency: Currency, fx: &Fx, at: DateTime<Utc>) -> Option<f64> {
    if currency != Currency::Cny
        || fx
            .valid_until
            .is_none_or(|valid_until| at.date_naive() > valid_until)
    {
        return None;
    }
    let amount = amount * fx.usd_per_cny;
    amount.is_finite().then_some(amount)
}

pub(super) fn usd_compare_key(
    cost: f64,
    currency: Currency,
    fx: Option<&Fx>,
    at: DateTime<Utc>,
) -> Option<f64> {
    if !cost.is_finite() {
        return None;
    }
    match currency {
        Currency::Usd => Some(cost),
        Currency::Cny => fx.and_then(|fx| to_usd_approx(cost, currency, fx, at)),
    }
}

/// Format native currency money for terminal surfaces.
pub fn money(amount: f64, currency: Currency) -> String {
    let Some((is_tiny, digits)) = format_money_digits(amount) else {
        return "unpriced".to_owned();
    };
    let marker = if is_tiny { "<" } else { "" };
    let symbol = match currency {
        Currency::Cny => "¥",
        Currency::Usd => "$",
    };
    format!("{marker}{symbol}{digits}")
}

/// Format native money with a fresh approximate USD gloss when useful.
pub fn money_with_gloss(
    amount: f64,
    currency: Currency,
    fx: Option<&Fx>,
    at: DateTime<Utc>,
) -> String {
    let native = money(amount, currency);
    match fx
        .and_then(|fx| to_usd_approx(amount, currency, fx, at))
        .and_then(format_money_digits)
    {
        Some((is_tiny, digits)) => {
            let marker = if is_tiny { "<" } else { "" };
            format!("{native} (≈{marker}${digits})")
        }
        None => native,
    }
}

pub(super) fn format_money_digits(amount: f64) -> Option<(bool, String)> {
    if !amount.is_finite() {
        return None;
    }
    if amount == 0.0 || amount >= 0.01 {
        return Some((false, format!("{amount:.2}")));
    }
    if amount >= 0.0001 {
        return Some((false, format!("{amount:.4}")));
    }
    if amount > 0.0 {
        return Some((true, "0.0001".to_owned()));
    }
    Some((false, format!("{amount:.2}")))
}
