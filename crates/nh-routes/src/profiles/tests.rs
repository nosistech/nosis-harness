use super::*;
use crate::RouteResolver;

fn layer(text: &str) -> ProfilesLayer {
    ProfilesLayer::parse(text).unwrap()
}

fn route(max_out: u64) -> ResolvedRoute {
    RouteResolver::from_toml(&format!(
        r#"
        [routes.test]
        provider = "test"
        model_id = "test-model"
        base_url = "https://example.invalid"
        wire = "openai"
        vault_entry = "test"
        context = 100000
        max_out = {max_out}
        thinking_dialect = "deepseek-nhm"
        preserve_reasoning = true
        preserve_when_thinking = true
        quirks = ["test-quirk"]
        "#
    ))
    .unwrap()
    .resolve("test")
    .unwrap()
}

#[test]
fn compile_clamps_repo_loosen_and_keeps_repo_tightening() {
    let bundled = layer(BUNDLED_PROFILES);
    let user = layer(
        r#"
        [profiles.frugal]
        thinking = "floor"
        max_output_tokens = 12000
        prefer_offpeak = true

        [profiles.balanced]
        thinking = "default"
        max_output_tokens = 24000
        "#,
    );
    let repo = layer(
        r#"
        [profiles.frugal]
        thinking = "ceiling"
        max_output_tokens = 50000

        [profiles.balanced]
        thinking = "floor"
        max_output_tokens = 8000
        prefer_offpeak = true
        "#,
    );

    let (profiles, warnings) = Profiles::compile(bundled, Some(user), Some(repo));
    let r = route(100_000);
    let frugal = profiles.effective("frugal", &r);
    assert_eq!(frugal.posture, ThinkingPosture::Floor);
    assert_eq!(frugal.output_cap, Some(12_000));
    assert!(frugal.prefer_offpeak);
    let balanced = profiles.effective("balanced", &r);
    assert_eq!(balanced.posture, ThinkingPosture::Floor);
    assert_eq!(balanced.output_cap, Some(8_000));
    assert!(balanced.prefer_offpeak);
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("cannot loosen profile 'frugal'"));
}

#[test]
fn bundled_profiles_bound_default_output_and_require_opt_in_for_more() {
    let profiles = Profiles::bundled();
    let r = route(64_000);

    assert_eq!(profiles.effective("frugal", &r).output_cap, Some(16_384));
    assert_eq!(profiles.effective("balanced", &r).output_cap, Some(16_384));
    assert_eq!(
        profiles.effective("max-quality", &r).output_cap,
        r.max_out()
    );
}

#[test]
fn effective_policy_clamps_without_minting_a_route() {
    let profiles = Profiles::bundled();
    let r = route(64_000);
    let policy = profiles.effective("frugal", &r);

    assert_eq!(policy.output_cap, Some(16_384));
    assert_eq!(r.max_out(), Some(64_000));
}

#[test]
fn min_cap_covers_the_full_option_table() {
    assert_eq!(min_cap(Some(20), Some(10)), Some(10));
    assert_eq!(min_cap(Some(10), None), Some(10));
    assert_eq!(min_cap(None, Some(10)), Some(10));
    assert_eq!(min_cap(None, None), None);
}

#[test]
fn unknown_posture_is_a_clear_error() {
    let error = ProfilesLayer::parse(
        r#"
        [profiles.frugal]
        thinking = "expensive"
        "#,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("unknown thinking posture 'expensive'"));
    assert!(error.contains("floor, default, or ceiling"));
}

#[test]
fn optional_profile_warning_preserves_the_actionable_parse_error() {
    let path = std::env::temp_dir().join(format!(
        "nh-routes-profiles-warning-{}.toml",
        std::process::id()
    ));
    fs::write(
        &path,
        r#"
        [profiles.frugal]
        thinking = "expensive"
        "#,
    )
    .unwrap();
    let mut warnings = Vec::new();

    assert!(read_optional_profiles(&path, "test profiles", &mut warnings).is_none());
    fs::remove_file(&path).unwrap();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("unknown thinking posture 'expensive'"));
    assert!(warnings[0].contains("defaults kept"));
}

#[test]
fn unknown_profile_falls_back_to_balanced() {
    let policy = Profiles::bundled().effective("missing", &route(64_000));
    assert_eq!(policy.profile, "balanced");
    assert_eq!(policy.posture, ThinkingPosture::Default);
    assert_eq!(policy.output_cap, Some(16_384));
}

#[test]
fn bundled_profiles_parse_independently_of_catalog() {
    let resolver = RouteResolver::from_toml(
        r#"
        [routes.test]
        provider = "test"
        model_id = "test"
        base_url = "https://example.invalid"
        wire = "openai"
        vault_entry = "test"
        max_out = 64000
        "#,
    )
    .unwrap();
    let route = resolver.resolve("test").unwrap();
    assert_eq!(
        Profiles::bundled().effective("max-quality", &route).posture,
        ThinkingPosture::Ceiling
    );
}
