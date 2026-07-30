use super::*;
use chrono::{DateTime, FixedOffset, NaiveDate, TimeZone, Utc};
use std::collections::BTreeMap;

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

fn peak_route() -> ResolvedRoute {
    let catalog = r#"
        [routes.peak-route]
        provider = "test"
        model_id = "peak-route"
        base_url = "https://example.invalid"
        wire = "openai"
        vault_entry = "test"
        [routes.peak-route.price]
        currency = "CNY"
        unit = "per_million_tokens"
        cache_hit = 0.025
        cache_miss = 3.00
        output = 6.00
        price_confidence = "confirmed"
        valid_until = "2099-12-31"
        [routes.peak-route.price.peak]
        multiplier = 2.0
        timezone = "Asia/Shanghai"
        windows = ["09:00-12:00", "14:00-18:00"]
    "#;
    RouteResolver::from_toml(catalog)
        .unwrap()
        .resolve("peak-route")
        .unwrap()
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

fn route_toml_with_url(base_url: &str) -> String {
    format!(
        r#"
        [routes.test-model]
        provider = "p"
        model_id = "test-model"
        base_url = "{base_url}"
        wire = "openai"
        vault_entry = "p"
        "#
    )
}

#[test]
fn route_urls_require_https_or_literal_loopback_http() {
    for allowed in [
        "https://api.example.invalid/v1",
        "http://127.77.0.1:8080/v1",
        "http://[::1]:8080/v1",
        "http://localhost:8080/v1",
    ] {
        RouteResolver::from_toml(&route_toml_with_url(allowed)).unwrap();
    }

    for refused in [
        "http://api.example.invalid/v1",
        "http://localhost.example:8080/v1",
        "http://[::ffff:127.0.0.1]:8080/v1",
        "ftp://127.0.0.1/resource",
    ] {
        let error = RouteResolver::from_toml(&route_toml_with_url(refused))
            .err()
            .expect("insecure route URL must be refused")
            .to_string();
        assert!(error.contains("must use https"), "{refused}: {error}");
        assert!(error.contains("literal loopback"), "{refused}: {error}");
    }
}

fn priced_route(
    id: &str,
    class: &str,
    context: Option<u64>,
    cache_miss: f64,
    output: f64,
) -> String {
    let context = context.map_or_else(String::new, |value| format!("context = {value}"));
    let base_url = if class == "local" {
        "http://127.0.0.1:11434/v1"
    } else {
        "https://example.invalid"
    };
    format!(
        r#"
        [routes."{id}"]
        provider = "p"
        model_id = "{id}"
        base_url = "{base_url}"
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
        priced_route("local-zero", "local", Some(64_000), 0.0, 0.0),
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

fn mixed_currency_catalog(fx_valid_until: Option<&str>) -> String {
    let fx_valid_until =
        fx_valid_until.map_or_else(String::new, |value| format!(r#"valid_until = "{value}""#));
    format!(
        r#"
        [fx]
        usd_per_cny = 0.10
        {fx_valid_until}
        price_confidence = "reported"

        [routes.cny-route]
        provider = "cny-provider"
        model_id = "cny-route"
        base_url = "https://example.invalid"
        wire = "openai"
        vault_entry = "cny-provider"
        context = 2000000
        [routes.cny-route.price]
        currency = "CNY"
        unit = "per_million_tokens"
        cache_hit = 3.0
        cache_miss = 3.0
        output = 3.0
        valid_until = "2099-12-31"
        price_confidence = "reported"

        [routes.usd-route]
        provider = "usd-provider"
        model_id = "usd-route"
        base_url = "https://example.invalid"
        wire = "openai"
        vault_entry = "usd-provider"
        context = 2000000
        [routes.usd-route.price]
        currency = "USD"
        unit = "per_million_tokens"
        cache_hit = 2.0
        cache_miss = 2.0
        output = 2.0
        valid_until = "2099-12-31"
        price_confidence = "reported"
        "#
    )
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
            "kimi-k3",
            "mimo-v2.5",
            "mimo-v2.5-pro",
        ]
    );
}

#[test]
fn resolves_route_with_openai_wire() {
    let route = resolver().resolve("deepseek-v4-flash").expect("known id");
    assert_eq!(route.id(), "deepseek-v4-flash");
    assert_eq!(route.wire(), Wire::OpenAi);
    assert_eq!(route.provider(), "deepseek");
    assert_eq!(route.model_id(), "deepseek-v4-flash");
    assert_eq!(route.base_url(), "https://api.deepseek.com");
    assert_eq!(route.vault_entry(), "deepseek");
    assert_eq!(route.class(), RouteClass::Api);
}

#[test]
fn anthropic_wire_variant_keeps_model_id_and_changes_base_url() {
    // The deepclaude-proven path: same model, Anthropic Messages wire.
    let route = resolver()
        .resolve("deepseek-v4-pro-anthropic")
        .expect("known id");
    assert_eq!(route.id(), "deepseek-v4-pro-anthropic");
    assert_eq!(route.model_id(), "deepseek-v4-pro");
    assert_eq!(route.wire(), Wire::AnthropicMessages);
    assert_eq!(route.base_url(), "https://api.deepseek.com/anthropic");
    assert_eq!(route.vault_entry(), "deepseek");
}

#[test]
fn deepseek_routes_carry_dialect_quirk_and_limits() {
    let resolver = resolver();
    for id in [
        "deepseek-v4-pro",
        "deepseek-v4-pro-anthropic",
        "deepseek-v4-flash",
        "deepseek-v4-flash-anthropic",
    ] {
        let route = resolver.resolve(id).unwrap();
        assert_eq!(
            route.thinking_dialect(),
            ThinkingDialect::DeepseekNhm,
            "{id}"
        );
        assert!(
            route.has_quirk("empty-reasoning-content-on-tool-replay"),
            "{id}"
        );
        assert!(!route.has_quirk("no-such-quirk"), "{id}");
        assert!(!route.preserve_reasoning(), "{id}");
        assert!(route.preserve_when_thinking(), "{id}");
    }

    let route = resolver.resolve("deepseek-v4-pro").unwrap();
    assert_eq!(route.context(), Some(1_000_000));
    assert_eq!(route.max_out(), Some(384_000));
    assert_eq!(route.modality(), vec!["text"]);
}

#[test]
fn repo_catalog_prices_are_fresh_and_comparable_on_verification_date() {
    let resolver = resolver();
    let verified_at = utc(2026, 7, 26, 12, 0, 0);
    for id in resolver.available() {
        let route = resolver.resolve(&id).unwrap();
        let quote = route
            .price_at(verified_at)
            .expect("every v1 route is priced");
        assert_eq!(quote.currency, Currency::Usd, "{id}");
        assert!(!quote.stale, "{id}");
    }

    let pro = resolver
        .resolve("deepseek-v4-pro")
        .unwrap()
        .price_at(verified_at)
        .unwrap();
    assert!(close(pro.cache_hit, 0.003625));
    assert!(close(pro.cache_miss, 0.435));
    assert!(close(pro.output, 0.87));
    assert!(!pro.peak);

    let flash = resolver
        .resolve("deepseek-v4-flash")
        .unwrap()
        .price_at(verified_at)
        .unwrap();
    assert!(close(flash.cache_hit, 0.0028));
    assert!(close(flash.cache_miss, 0.14));
    assert!(close(flash.output, 0.28));
    assert!(!flash.peak);
}

#[test]
fn kimi_k27_routes_are_always_thinking_and_preserve_reasoning() {
    let r = resolver();
    for id in ["kimi-k2.7-code", "kimi-k2.7-code-highspeed"] {
        let route = r.resolve(id).unwrap();
        assert_eq!(
            route.thinking_dialect(),
            ThinkingDialect::AlwaysThinking,
            "{id}"
        );
        assert!(
            route.preserve_reasoning(),
            "{id} must preserve reasoning (plan A.10.5)"
        );
        assert_eq!(route.modality(), vec!["text", "image", "video"], "{id}");
    }
}

#[test]
fn kimi_k26_uses_toggle_and_state_aware_reasoning_replay() {
    let route = resolver().resolve("kimi-k2.6").unwrap();
    assert_eq!(route.thinking_dialect(), ThinkingDialect::KimiToggle);
    assert!(!route.preserve_reasoning());
    assert!(route.preserve_when_thinking());
    assert_eq!(route.thinking_dialect().as_str(), "kimi-toggle");
}

#[test]
fn kimi_k3_has_verified_capacity_price_and_effort_control() {
    let resolver = resolver();
    let route = resolver.resolve("kimi-k3").unwrap();
    assert_eq!(
        route.thinking_dialect(),
        ThinkingDialect::AlwaysThinkingEffort
    );
    assert_eq!(route.thinking_dialect().as_str(), "always-thinking-effort");
    assert!(route.preserve_reasoning());
    assert_eq!(route.modality(), vec!["text", "image", "video"]);
    assert_eq!(route.context(), Some(1_048_576));
    assert_eq!(route.max_out(), Some(1_048_576));

    let quote = route.price_at(utc(2026, 7, 28, 12, 0, 0)).unwrap();
    assert_eq!(quote.currency, Currency::Usd);
    assert_eq!(quote.confidence, PriceConfidence::Confirmed);
    assert!(close(quote.cache_hit, 0.30));
    assert!(close(quote.cache_miss, 3.00));
    assert!(close(quote.output, 15.00));

    let (candidate, trace) = resolver
        .resolve_capable(
            300_000,
            10_000,
            &["kimi-k2.6", "kimi-k2.7-code", "kimi-k3"],
            utc(2026, 7, 28, 12, 0, 0),
        )
        .unwrap();
    assert_eq!(candidate.id(), "kimi-k3");
    assert!(
        trace
            .rejections
            .iter()
            .any(|entry| entry.route_id == "kimi-k2.6" && entry.reason.starts_with("ctx ")),
        "{trace}"
    );
}

#[test]
fn mimo_routes_preserve_reasoning_and_are_omni_modal() {
    let r = resolver();
    for id in ["mimo-v2.5", "mimo-v2.5-pro"] {
        let route = r.resolve(id).unwrap();
        assert!(route.preserve_reasoning(), "{id}");
        assert_eq!(
            route.thinking_dialect(),
            ThinkingDialect::KimiToggle,
            "{id}"
        );
        assert_eq!(
            route.modality(),
            vec!["text", "image", "video", "audio"],
            "{id}"
        );
        assert_eq!(route.context(), Some(1_000_000), "{id}");
    }
}

#[test]
fn glm_52_uses_glm_hm_dialect() {
    let route = resolver().resolve("glm-5.2").unwrap();
    assert_eq!(route.thinking_dialect(), ThinkingDialect::GlmHm);
    assert_eq!(route.max_out(), Some(128_000));
}

#[test]
fn free_glm_routes_have_documented_caps_and_are_capable_candidates() {
    let resolver = resolver();
    let cases = [
        ("glm-4.7-flash", 200_000, 128_000),
        ("glm-4.6v-flash", 128_000, 32_000),
        ("glm-4.5-flash", 128_000, 96_000),
    ];

    for (id, context, max_out) in cases {
        let route = resolver.resolve(id).unwrap();
        assert_eq!(route.context(), Some(context), "{id}");
        assert_eq!(route.max_out(), Some(max_out), "{id}");

        let (candidate, trace) = resolver
            .resolve_capable(1_000, 1_000, &[id], utc(2026, 7, 28, 12, 0, 0))
            .unwrap();
        assert_eq!(candidate.id(), id);
        assert!(
            trace
                .rejections
                .iter()
                .all(|entry| entry.reason != "unknown context"),
            "{id}: {trace}"
        );
    }
}

#[test]
fn minimal_m0_route_parses_with_defaults() {
    // Old user catalogs (M0 shape) keep working: everything M1 defaults.
    let r = RouteResolver::from_toml(&route_toml("")).unwrap();
    let route = r.resolve("test-model").unwrap();
    assert_eq!(route.class(), RouteClass::Api);
    assert_eq!(route.modality(), vec!["text"]);
    assert_eq!(route.thinking_dialect(), ThinkingDialect::None);
    assert!(!route.preserve_reasoning());
    assert!(!route.preserve_when_thinking());
    assert!(route.quirks().is_empty());
    assert!(route.context().is_none());
    assert!(route.max_out().is_none());
    assert!(route.price().is_none());
}

#[test]
fn delegate_class_parses() {
    let r = RouteResolver::from_toml(&route_toml(r#"class = "delegate""#)).unwrap();
    assert_eq!(
        r.resolve("test-model").unwrap().class(),
        RouteClass::Delegate
    );
}

#[test]
fn local_class_parses_for_an_explicit_loopback_route() {
    let route = RouteResolver::from_toml(
        r#"
        [routes.local-test]
        provider = "local"
        model_id = "user-filled-model"
        base_url = "http://127.0.0.1:11434/v1"
        wire = "openai"
        vault_entry = "local-test"
        class = "local"
        "#,
    )
    .unwrap()
    .resolve("local-test")
    .unwrap();

    assert_eq!(route.class(), RouteClass::Local);
    assert_eq!(
        route.peak_status(utc(2026, 7, 29, 0, 0, 0), FixedOffset::east_opt(0).unwrap()),
        "local"
    );
}

#[test]
fn local_class_is_confined_to_loopback_on_the_openai_wire() {
    let remote = route_toml(r#"class = "local""#);
    let error = RouteResolver::from_toml(&remote)
        .err()
        .expect("remote local route must fail")
        .to_string();
    assert!(
        error.contains("requires a literal-loopback base_url"),
        "{error}"
    );

    let anthropic = r#"
        [routes.local-test]
        provider = "local"
        model_id = "user-filled-model"
        base_url = "http://127.0.0.1:8080/v1"
        wire = "anthropic"
        vault_entry = "local-test"
        class = "local"
    "#;
    let error = RouteResolver::from_toml(anthropic)
        .err()
        .expect("non-OpenAI local route must fail")
        .to_string();
    assert!(error.contains("must reuse wire = \"openai\""), "{error}");
}

// ---------------------------------------------------------------- pricing

#[test]
fn peak_boundary_math_in_beijing_time() {
    // Peak = Beijing 09:00-12:00 & 14:00-18:00, start inclusive, end exclusive.
    let route = peak_route();
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
    let route = peak_route();
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
    let route = peak_route();
    let quote = route.price_at(beijing(8, 0)).unwrap();
    assert!(!quote.peak);
    assert!(close(quote.cache_hit, 0.025));
    assert!(close(quote.cache_miss, 3.00));
    assert!(close(quote.output, 6.00));
}

#[test]
fn peak_status_is_short_local_and_boundary_exact() {
    let route = peak_route();
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
    // First-party prices confirmed 2026-07-26 (platform.kimi.ai/docs/pricing/chat-k26).
    assert!(close(quote.output, 4.00));
    assert_eq!(quote.currency, Currency::Usd);
    assert_eq!(quote.confidence, PriceConfidence::Confirmed);
}

#[test]
fn stale_flag_flips_after_valid_until() {
    // Provider prices carry valid_until = 2026-08-02 (valid through that UTC day).
    let route = resolver().resolve("deepseek-v4-flash").unwrap();
    let fresh = route.price_at(utc(2026, 8, 2, 23, 59, 59)).unwrap();
    assert!(!fresh.stale, "still valid on the valid_until day");
    let stale = route.price_at(utc(2026, 8, 3, 0, 0, 0)).unwrap();
    assert!(
        stale.stale,
        "past valid_until must flag stale - honest-cost rule"
    );
}

#[test]
fn free_glm_route_quotes_zero_and_obeys_recheck_deadline() {
    let route = resolver().resolve("glm-4.7-flash").unwrap();
    let quote = route.price_at(utc(2026, 8, 2, 12, 0, 0)).unwrap();
    assert!(close(quote.cache_hit, 0.0));
    assert!(close(quote.cache_miss, 0.0));
    assert!(close(quote.output, 0.0));
    assert_eq!(quote.currency, Currency::Usd);
    // Free tier confirmed 2026-07-26 (docs.z.ai/guides/overview/pricing).
    assert_eq!(quote.confidence, PriceConfidence::Confirmed);
    assert!(!quote.peak);
    assert!(!quote.stale);
    assert!(
        route
            .price_at(utc(2026, 8, 3, 0, 0, 0))
            .is_some_and(|quote| quote.stale),
        "the free tier still needs a price recheck"
    );
}

#[test]
fn mimo_prices_are_confirmed_first_party() {
    // Plan B.3 pricing conflict resolved 2026-07-26 against mimo.mi.com/docs/pricing.
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

#[test]
fn cost_of_splits_cached_miss_and_output_tokens() {
    let quote = PriceQuote {
        cache_hit: 1.0,
        cache_miss: 10.0,
        output: 20.0,
        currency: Currency::Cny,
        peak: false,
        confidence: PriceConfidence::Confirmed,
        stale: false,
    };
    assert!(close(cost_of(&quote, 1_000, 400, 100).unwrap(), 0.0084));
    assert_eq!(cost_of(&quote, 100, 200, 0), None);

    let non_finite = PriceQuote {
        output: f64::INFINITY,
        ..quote
    };
    assert!(
        cost_of(&non_finite, 100, 0, 1).is_none(),
        "a non-finite result is not priceable"
    );
}

#[test]
fn undated_price_remains_available_for_native_cost_and_counterfactuals() {
    let catalog = r#"
        [routes.native]
        provider = "p"
        model_id = "native"
        base_url = "https://example.invalid"
        wire = "openai"
        vault_entry = "p"
        [routes.native.price]
        currency = "CNY"
        unit = "per_million_tokens"
        cache_hit = 1.0
        cache_miss = 3.0
        output = 2.0
        price_confidence = "confirmed"
    "#;
    let resolver = RouteResolver::from_toml(catalog).unwrap();
    let route = resolver.resolve("native").unwrap();
    let at = utc(2026, 7, 20, 0, 0, 0);
    let quote = route.price_at(at).unwrap();
    assert!(quote.stale);
    let actual = cost_of(&quote, 1_000_000, 500_000, 100_000).unwrap();
    let costs = resolver
        .naive_cost(&route, 1_000_000, 500_000, 100_000, at)
        .unwrap();

    assert!(close(actual, 2.2));
    assert!(close(costs.no_cache, 3.2));
    assert_eq!(costs.currency, Currency::Cny);
}

#[test]
fn naive_no_cache_is_not_less_than_actual() {
    let resolver = resolver();
    let route = resolver.resolve("deepseek-v4-flash").unwrap();
    let costs = resolver
        .naive_cost(&route, 20_000, 15_000, 2_000, utc(2026, 7, 15, 0, 0, 0))
        .unwrap();
    let actual = cost_of(
        &route.price_at(utc(2026, 7, 15, 0, 0, 0)).unwrap(),
        20_000,
        15_000,
        2_000,
    )
    .unwrap();
    assert!(costs.no_cache >= actual);
}

#[test]
fn saved_pct_rounds_and_omits_non_savings() {
    assert_eq!(saved_pct(0.07, 1.0), Some(93));
    assert_eq!(saved_pct(0.0, 1.0), Some(99));
    assert_eq!(saved_pct(1.0, 1.0), None);
    assert_eq!(saved_pct(2.0, 1.0), None);
}

#[test]
fn naive_top_tier_never_crosses_currency() {
    let catalog = r#"
        [routes.actual]
        provider = "p"
        model_id = "actual"
        base_url = "https://example.invalid"
        wire = "openai"
        vault_entry = "p"
        [routes.actual.price]
        currency = "CNY"
        unit = "per_million_tokens"
        cache_hit = 1.0
        cache_miss = 2.0
        output = 1.0
        price_confidence = "confirmed"

        [routes.cny-top]
        provider = "p"
        model_id = "cny-top"
        base_url = "https://example.invalid"
        wire = "openai"
        vault_entry = "p"
        [routes.cny-top.price]
        currency = "CNY"
        unit = "per_million_tokens"
        cache_hit = 1.0
        cache_miss = 3.0
        output = 1.0
        price_confidence = "confirmed"

        [routes.usd-top]
        provider = "usd-provider"
        model_id = "usd-top"
        base_url = "https://example.invalid"
        wire = "openai"
        vault_entry = "usd-provider"
        [routes.usd-top.price]
        currency = "USD"
        unit = "per_million_tokens"
        cache_hit = 99.0
        cache_miss = 99.0
        output = 99.0
        price_confidence = "confirmed"

        [routes.local-not-an-anchor]
        provider = "p"
        model_id = "local-not-an-anchor"
        base_url = "http://127.0.0.1:8080/v1"
        wire = "openai"
        vault_entry = "local-test"
        class = "local"
        [routes.local-not-an-anchor.price]
        currency = "CNY"
        unit = "per_million_tokens"
        cache_hit = 100.0
        cache_miss = 100.0
        output = 100.0
        price_confidence = "confirmed"
    "#;
    let resolver = RouteResolver::from_toml(catalog).unwrap();
    let route = resolver.resolve("actual").unwrap();
    let costs = resolver
        .naive_cost(&route, 1_000_000, 0, 0, utc(2026, 7, 15, 0, 0, 0))
        .unwrap();
    assert_eq!(costs.currency, Currency::Cny);
    // The USD route cannot be compared, and the higher-priced local route is
    // deliberately not a top-tier API anchor.
    assert!(close(costs.top_tier, 3.0));
}

#[test]
fn usd_approx_requires_fresh_cny_and_formats_consistently() {
    let fx = Fx {
        usd_per_cny: 0.139,
        valid_until: Some(NaiveDate::from_ymd_opt(2026, 7, 24).unwrap()),
        confidence: PriceConfidence::Reported,
    };
    let fresh = utc(2026, 7, 24, 23, 59, 59);
    let stale = utc(2026, 7, 25, 0, 0, 0);
    assert!(close(
        to_usd_approx(0.11, Currency::Cny, &fx, fresh).unwrap(),
        0.01529
    ));
    assert!(to_usd_approx(0.11, Currency::Cny, &fx, stale).is_none());
    assert!(to_usd_approx(0.11, Currency::Usd, &fx, fresh).is_none());
    let undated = Fx {
        valid_until: None,
        ..fx.clone()
    };
    assert!(to_usd_approx(0.11, Currency::Cny, &undated, fresh).is_none());
    assert_eq!(money(0.11, Currency::Cny), "¥0.11");
    assert_eq!(money(0.015, Currency::Usd), "$0.01");
    assert_eq!(
        money_with_gloss(0.11, Currency::Cny, Some(&fx), fresh),
        "¥0.11 (≈$0.02)"
    );
    assert_eq!(
        money_with_gloss(0.11, Currency::Cny, Some(&fx), stale),
        "¥0.11"
    );
    assert_eq!(
        money_with_gloss(0.02, Currency::Usd, Some(&fx), fresh),
        "$0.02"
    );
}

#[test]
fn money_uses_adaptive_precision_without_hiding_positive_spend() {
    assert_eq!(money(0.11, Currency::Cny), "¥0.11");
    assert_eq!(money(0.09, Currency::Usd), "$0.09");
    assert_eq!(money(0.01, Currency::Usd), "$0.01");
    assert_eq!(money(0.0027, Currency::Usd), "$0.0027");
    assert_eq!(money(0.0001, Currency::Usd), "$0.0001");
    assert_eq!(money(0.00009, Currency::Usd), "<$0.0001");
    assert_eq!(money(0.0, Currency::Usd), "$0.00");
    assert_eq!(money(0.0, Currency::Cny), "¥0.00");
}

#[test]
fn money_with_gloss_uses_adaptive_precision_for_usd() {
    let fx = Fx {
        usd_per_cny: 0.139,
        valid_until: Some(NaiveDate::from_ymd_opt(2026, 7, 24).unwrap()),
        confidence: PriceConfidence::Reported,
    };
    let at = utc(2026, 7, 18, 0, 0, 0);
    assert_eq!(
        money_with_gloss(0.019, Currency::Cny, Some(&fx), at),
        "¥0.02 (≈$0.0026)"
    );
    assert_eq!(
        money_with_gloss(0.00019, Currency::Cny, Some(&fx), at),
        "¥0.0002 (≈<$0.0001)"
    );
}

#[test]
fn money_falls_back_safely_for_non_finite_amounts() {
    assert_eq!(format_money_digits(f64::NAN), None);
    assert_eq!(money(f64::NAN, Currency::Cny), "unpriced");
    assert_eq!(money(f64::INFINITY, Currency::Usd), "unpriced");
    assert_eq!(money(f64::NEG_INFINITY, Currency::Usd), "unpriced");

    let fx = Fx {
        usd_per_cny: 0.139,
        valid_until: None,
        confidence: PriceConfidence::Reported,
    };
    assert_eq!(
        money_with_gloss(
            f64::NAN,
            Currency::Cny,
            Some(&fx),
            utc(2026, 7, 18, 0, 0, 0)
        ),
        "unpriced"
    );
}

#[test]
fn fx_is_optional_catalog_data() {
    let without_fx = RouteResolver::from_toml(&route_toml("")).unwrap();
    assert!(without_fx.fx().is_none());
    assert!(resolver().fx().is_none(), "the all-USD catalog needs no FX");
    let with_fx = RouteResolver::from_toml(&format!(
        r#"
        [fx]
        usd_per_cny = 0.10
        valid_until = "2026-08-02"
        price_confidence = "reported"
        {}
        "#,
        route_toml("")
    ))
    .unwrap();
    assert_eq!(
        with_fx.fx().map(|fx| fx.confidence),
        Some(PriceConfidence::Reported)
    );
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
        "local-zero",
        "fit-a",
        "unknown-context",
        "unpriced",
    ];
    let (route, trace) = resolver
        .resolve_capable(40_000, 5_000, &allowed, utc(2026, 7, 15, 12, 0, 0))
        .unwrap();
    assert_eq!(route.id(), "fit-a", "equal-cost ties break by route id");

    let reasons: BTreeMap<&str, &str> = trace
        .rejections
        .iter()
        .map(|entry| (entry.route_id.as_str(), entry.reason.as_str()))
        .collect();
    assert_eq!(reasons.len(), allowed.len() - 1);
    assert_eq!(reasons["cheap-small"], "ctx 32K < 45K");
    assert_eq!(reasons["delegated"], "delegate");
    assert_eq!(reasons["local-zero"], "local");
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
fn resolve_capable_normalizes_fresh_mixed_currencies_and_traces_native_money() {
    let resolver = RouteResolver::from_toml(&mixed_currency_catalog(Some("2026-07-24"))).unwrap();
    let (route, trace) = resolver
        .resolve_capable(
            1_000_000,
            0,
            &["cny-route", "usd-route"],
            utc(2026, 7, 20, 0, 0, 0),
        )
        .unwrap();

    assert_eq!(route.id(), "cny-route", "¥3 normalizes to $0.30");
    let usd_reason = trace
        .rejections
        .iter()
        .find(|entry| entry.route_id == "usd-route")
        .map(|entry| entry.reason.as_str())
        .unwrap();
    assert!(usd_reason.contains("$2.00"));
    assert!(usd_reason.contains("¥3.00"));
    assert!(usd_reason.contains("different currency"));
    assert!(!usd_reason.contains("x price"));
}

#[test]
fn resolve_capable_refuses_noncomparable_currency_when_fx_is_stale() {
    let resolver = RouteResolver::from_toml(&mixed_currency_catalog(Some("2026-07-19"))).unwrap();
    let (route, trace) = resolver
        .resolve_capable(
            1_000_000,
            0,
            &["cny-route", "usd-route"],
            utc(2026, 7, 20, 0, 0, 0),
        )
        .unwrap();

    assert_eq!(route.id(), "usd-route");
    let cny_reason = trace
        .rejections
        .iter()
        .find(|entry| entry.route_id == "cny-route")
        .map(|entry| entry.reason.as_str())
        .unwrap();
    assert_eq!(cny_reason, "fx stale - ¥/$ not comparable");
}

#[test]
fn resolve_capable_refuses_cross_currency_comparison_when_fx_is_undated() {
    let resolver = RouteResolver::from_toml(&mixed_currency_catalog(None)).unwrap();
    let (route, trace) = resolver
        .resolve_capable(
            1_000_000,
            0,
            &["cny-route", "usd-route"],
            utc(2026, 7, 20, 0, 0, 0),
        )
        .unwrap();

    assert_eq!(route.id(), "usd-route");
    assert!(trace.rejections.iter().any(|entry| {
        entry.route_id == "cny-route" && entry.reason == "fx stale - ¥/$ not comparable"
    }));
}

#[test]
fn resolve_capable_same_currency_still_uses_a_price_ratio() {
    let resolver = capable_resolver();
    let (_, trace) = resolver
        .resolve_capable(
            1_000,
            0,
            &["fit-a", "fit-expensive"],
            utc(2026, 7, 20, 0, 0, 0),
        )
        .unwrap();
    assert_eq!(trace.rejections[0].reason, "4.0x price");
}

#[test]
fn provider_default_picks_lowest_output_price() {
    let r = resolver();
    // Rule: cheapest api route by off-peak output price, ties alphabetical.
    assert_eq!(
        r.provider_default("deepseek").unwrap().id(),
        "deepseek-v4-flash"
    );
    assert_eq!(r.provider_default("kimi").unwrap().id(), "kimi-k2.6");
    assert_eq!(r.provider_default("mimo").unwrap().id(), "mimo-v2.5");
    // Three free GLM routes tie at 0 - alphabetical order breaks the tie.
    assert_eq!(r.provider_default("glm").unwrap().id(), "glm-4.5-flash");
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
        base_url = "https://example.invalid"
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
    assert_eq!(r.provider_default("p").unwrap().id(), "pricey");
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
    assert_eq!(
        map["kimi"],
        vec![
            "kimi-k2.6",
            "kimi-k2.7-code",
            "kimi-k2.7-code-highspeed",
            "kimi-k3",
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
    assert!(msg.contains("local"), "must say valid values: {msg}");
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
        msg.contains("always-thinking-effort"),
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
fn catalog_rejects_mixed_currencies_within_one_provider() {
    let mixed_providers = mixed_currency_catalog(Some("2026-07-24"));
    assert!(
        RouteResolver::from_toml(&mixed_providers).is_ok(),
        "different providers may use different currencies"
    );

    let one_provider = mixed_providers.replace("usd-provider", "cny-provider");
    let error = RouteResolver::from_toml(&one_provider)
        .err()
        .expect("one provider mixing currencies must fail")
        .to_string();
    assert!(error.contains("provider 'cny-provider' mixes currencies"));
    assert!(error.contains("CNY and USD"));
    assert!(error.contains("one provider must price in one currency"));
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
    assert_eq!(
        ThinkingDialect::AlwaysThinkingEffort.as_str(),
        "always-thinking-effort"
    );
}
