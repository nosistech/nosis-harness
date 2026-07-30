//! Opt-in, display-only observation of provider usage JSON.

use std::ffi::OsStr;
use std::io::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::value::RawValue;

pub(super) const DEBUG_USAGE_ENV: &str = "NH_DEBUG_USAGE";

pub(super) struct UsageDebug {
    route_id: String,
    wire: &'static str,
    scrubber: nh_vault::Scrubber,
    next_request: AtomicU64,
}

#[derive(serde::Deserialize)]
struct UsageEnvelope<'a> {
    #[serde(borrow, default)]
    usage: Option<&'a RawValue>,
}

impl UsageDebug {
    pub(super) fn from_env(route_id: &str, wire: &'static str, api_key: &str) -> Option<Self> {
        let setting = std::env::var_os(DEBUG_USAGE_ENV);
        Self::from_setting(setting.as_deref(), route_id, wire, api_key)
    }

    fn from_setting(
        setting: Option<&OsStr>,
        route_id: &str,
        wire: &'static str,
        api_key: &str,
    ) -> Option<Self> {
        if setting != Some(OsStr::new("1")) {
            return None;
        }
        Some(Self {
            route_id: nh_vault::escape_untrusted(route_id),
            wire,
            scrubber: nh_vault::Scrubber::new(vec![api_key.to_owned()]),
            next_request: AtomicU64::new(1),
        })
    }

    pub(super) fn emit(&self, body: &str) {
        let request = self.next_request.fetch_add(1, Ordering::Relaxed);
        let line = self.render(request, body);
        let _ = writeln!(std::io::stderr().lock(), "{line}");
    }

    fn render(&self, request: u64, body: &str) -> String {
        let usage = match serde_json::from_str::<UsageEnvelope<'_>>(body) {
            Ok(envelope) => envelope.usage.map_or("usage absent", RawValue::get),
            Err(_) => "usage unavailable (response JSON could not be inspected)",
        };
        self.scrubber.scrub(&format!(
            "[{DEBUG_USAGE_ENV} route={} wire={} request={request}] {usage}",
            self.route_id, self.wire
        ))
    }
}

#[cfg(test)]
impl UsageDebug {
    pub(super) fn from_test_setting(
        setting: Option<&OsStr>,
        route_id: &str,
        wire: &'static str,
        api_key: &str,
    ) -> Option<Self> {
        Self::from_setting(setting, route_id, wire, api_key)
    }

    pub(super) fn render_for_test(&self, request: u64, body: &str) -> String {
        self.render(request, body)
    }
}
