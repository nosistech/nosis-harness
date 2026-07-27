//! nh-routes — RouteResolver: the ONLY component that may mint a resolved route (plan §2).
//! M1: full Class-1 catalog (plan Appendix B), clock-aware pricing with fixed-offset
//! peak windows, thinking dialects, modality flags, provider defaults, banned-string
//! rejection. Catalog and pricing stay DATA in catalog.toml — never hard-coded here.

mod pricing;
mod profiles;
mod resolver;
mod route;

pub use pricing::{
    cost_of, money, money_with_gloss, saved_pct, to_usd_approx, Currency, Fx, NaiveCost,
    PeakWindows, PriceConfidence, PriceQuote, RoutePrice,
};
pub use profiles::*;
pub use resolver::{RejectionTrace, ResolvedRoute, RouteRejection, RouteResolver};
pub use route::{RouteClass, ThinkingDialect, Wire};

#[cfg(test)]
use pricing::format_money_digits;
use pricing::usd_compare_key;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::net::IpAddr;

use anyhow::anyhow;
use chrono::{DateTime, FixedOffset, NaiveDate, NaiveTime, Utc};
use serde::Deserialize;

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

#[cfg(test)]
mod tests;
