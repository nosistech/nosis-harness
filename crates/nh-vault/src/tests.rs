use super::*;

/// Inner vault that never has the key (keyring miss stand-in).
struct AlwaysMiss;
impl Vault for AlwaysMiss {
    fn get(&self, entry: &str) -> anyhow::Result<Zeroizing<String>> {
        anyhow::bail!("no key stored for \"{entry}\"")
    }
    fn set(&self, _entry: &str, _value: &str) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Inner vault that always answers (to prove env is fallback-only).
struct AlwaysHit;
impl Vault for AlwaysHit {
    fn get(&self, _entry: &str) -> anyhow::Result<Zeroizing<String>> {
        Ok(Zeroizing::new("sk-test-0000-inner".to_string()))
    }
    fn set(&self, _entry: &str, _value: &str) -> anyhow::Result<()> {
        Ok(())
    }
}

#[test]
fn scrub_redacts_sk_shape() {
    let s = Scrubber::new(vec![]);
    assert_eq!(
        s.scrub("auth: sk-test-0000abcd done"),
        "auth: [REDACTED] done"
    );
}

#[test]
fn scrub_redacts_csk_shape_fully() {
    let s = Scrubber::new(vec![]);
    // Whole token redacted — no stray leading "c" from the sk- alternative.
    assert_eq!(
        s.scrub("auth: csk-test-0000abcd done"),
        "auth: [REDACTED] done"
    );
}

#[test]
fn scrub_does_not_redact_sk_shape_inside_words() {
    let s = Scrubber::new(vec![]);
    assert_eq!(s.scrub("risk-assessment"), "risk-assessment");
    assert_eq!(s.scrub("desk-organizer-v2"), "desk-organizer-v2");
    assert_eq!(s.scrub(" sk-test-0000abcd "), " [REDACTED] ");
}

#[test]
fn scrub_redacts_jwt_shape() {
    let s = Scrubber::new(vec![]);
    assert_eq!(
        s.scrub("token eyJfake-header.fake-payload.fake-sig end"),
        "token [REDACTED] end"
    );
}

#[test]
fn scrub_redacts_literal_secret() {
    let s = Scrubber::new(vec!["hunter2-fake-secret".to_string()]);
    assert_eq!(s.scrub("value=hunter2-fake-secret;"), "value=[REDACTED];");
}

#[test]
fn zeroizing_registry_deduplicates_redacts_and_never_debugs_values() {
    let literal = "fixture-secret-without-a-known-shape";
    let mut registry = SecretRegistry::new();
    registry.insert(secret(literal));
    registry.insert(secret(literal));

    assert_eq!(registry.len(), 1);
    assert!(registry.contains(literal));
    assert_eq!(registry.scrubber().scrub(literal), "[REDACTED]");
    let debug = format!("{registry:?}");
    assert!(debug.contains("count"));
    assert!(!debug.contains(literal));
}

#[test]
fn scrub_redacts_extended_high_precision_key_shapes() {
    let shapes = [
        format!("github_pat_{}", "A".repeat(22)),
        format!("ghp_{}", "B".repeat(36)),
        format!("gho_{}", "C".repeat(36)),
        format!("ghu_{}", "D".repeat(36)),
        format!("ghs_{}", "E".repeat(36)),
        format!("ghr_{}", "F".repeat(36)),
        format!("AKIA{}", "G".repeat(16)),
        format!("AIza{}", "H".repeat(35)),
        "xoxb-1234567890-abcdef".to_string(),
        format!("glpat-{}", "i".repeat(20)),
        format!("npm_{}", "j".repeat(36)),
    ];
    let scrubber = Scrubber::new(Vec::new());
    for shape in shapes {
        assert_eq!(
            scrubber.scrub(&format!("before {shape} after")),
            "before [REDACTED] after",
            "shape was not fully redacted: {shape}"
        );
    }
}

#[test]
fn scrub_leaves_normal_text_alone() {
    let s = Scrubber::new(vec!["hunter2-fake-secret".to_string()]);
    let text = "ran cargo test, 3 passed; ask me anything. risk-free task-list eyJustKidding";
    assert_eq!(s.scrub(text), text);
}

#[test]
fn scrub_ignores_empty_literal() {
    let s = Scrubber::new(vec![String::new()]);
    assert_eq!(s.scrub("plain text"), "plain text");
}

#[test]
fn scrub_redacts_overlapping_literals_longest_first() {
    for order in [
        vec!["abc123".to_string(), "abc123def456".to_string()],
        vec!["abc123def456".to_string(), "abc123".to_string()],
    ] {
        let scrubber = Scrubber::new(order);
        let out = scrubber.scrub("token=abc123def456 end");
        assert!(!out.contains("def456"), "{out}");
    }
}

#[test]
fn sanitize_line_escapes_control_chars_visibly() {
    // A spoof attempt: CR + ANSI erase-line to hide the real command.
    let spoofed = "echo safe\r\x1b[2K && rm -rf /";
    let display = sanitize_line(spoofed);
    assert!(!display.chars().any(|c| c.is_control()), "got: {display}");
    assert!(display.contains("\\r"), "CR must be visible: {display}");
    assert!(
        display.contains("\\u{1b}"),
        "ESC must be visible: {display}"
    );
    assert!(
        display.contains("rm -rf /"),
        "payload must stay visible: {display}"
    );
}

#[test]
fn sanitize_line_truncates_with_visible_marker() {
    let display = sanitize_line(&"x".repeat(600));
    assert!(
        display.chars().count() < 600,
        "got len {}",
        display.chars().count()
    );
    assert!(display.contains("(+100 more chars)"), "got: {display}");
    // Short text passes through untouched.
    assert_eq!(sanitize_line("cargo test"), "cargo test");
}

#[test]
fn sanitizers_escape_bidi_controls_visibly() {
    let spoofed = "allow\u{202e}deny";
    for sanitized in [sanitize_line(spoofed), sanitize_untrusted_text(spoofed)] {
        assert_eq!(sanitized, "allow\\u{202e}deny");
        assert!(!sanitized.contains('\u{202e}'));
    }
}

#[test]
fn escape_untrusted_escapes_bidi_removes_carriers_without_truncating() {
    let escaped = escape_untrusted("allow\u{202e}den\u{200b}y");
    assert_eq!(escaped, "allow\\u{202e}deny");
    assert!(!escaped.contains('\u{202e}'));
    assert!(!escaped.contains('\u{200b}'));

    let long = "x".repeat(1_100);
    assert_eq!(escape_untrusted(&long), long);
    assert!(sanitize_line(&long).chars().count() < long.chars().count());
}

#[test]
fn sanitize_untrusted_text_escapes_controls_removes_carriers_and_caps() {
    let sanitized = sanitize_untrusted_text("safe\r\x1b[2K\u{200b}\u{e0001}payload");
    assert_eq!(sanitized, "safe\\r\\u{1b}[2Kpayload");
    assert!(!sanitized.chars().any(char::is_control));
    assert_eq!(
        sanitize_untrusted_text("normal tool description"),
        "normal tool description"
    );

    let capped = sanitize_untrusted_text(&"x".repeat(1_100));
    assert!(capped.contains("(+100 more chars)"), "got: {capped}");
    assert!(capped.chars().count() < 1_100);
}

#[test]
fn audience_matching_requires_exact_scheme_and_effective_port() {
    let approved = vec!["api.deepseek.com".to_string()];
    assert!(!audience_allows("https://evil.example", &approved));
    assert!(audience_allows(
        "https://API.DEEPSEEK.COM:443/anthropic",
        &approved
    ));
    assert!(!audience_allows(
        "https://api.deepseek.com:8443/v1",
        &approved
    ));
    assert!(!audience_allows("http://api.deepseek.com", &approved));
    assert!(!audience_allows("https://anything.example/path", &[]));
    assert!(!audience_allows(
        "https://evil.example\\@api.deepseek.com/v1",
        &approved
    ));
    assert_eq!(
        normalized_host("https://API.DEEPSEEK.COM/anthropic").as_deref(),
        Some("https://api.deepseek.com:443")
    );
}

#[test]
fn host_of_returns_a_bare_lowercased_host() {
    assert_eq!(
        host_of("http://127.0.0.1:8080/mcp"),
        Some("127.0.0.1".into())
    );
    assert_eq!(
        host_of("https://Api.Example.COM/mcp"),
        Some("api.example.com".into())
    );
    assert_eq!(host_of("http://[::1]:9/mcp"), Some("::1".into()));
    assert_eq!(host_of("example.com:3000/x"), Some("example.com".into()));
    assert_eq!(host_of(""), None);
    assert_eq!(host_of("::://bad"), None);
}

#[test]
fn link_local_metadata_classification_is_literal_only() {
    assert!(is_link_local_or_metadata("169.254.169.254"));
    assert!(is_link_local_or_metadata("169.254.0.1"));
    assert!(is_link_local_or_metadata("fe80::1"));
    assert!(is_link_local_or_metadata("::ffff:169.254.169.254"));
    assert!(!is_link_local_or_metadata("127.0.0.1"));
    assert!(!is_link_local_or_metadata("10.0.0.5"));
    assert!(!is_link_local_or_metadata("api.deepseek.com"));
}

#[test]
fn literal_loopback_http_is_allowed_without_name_resolution() {
    for origin in [
        "http://127.42.0.9:8080",
        "http://[::1]:8080",
        "http://localhost:8080",
    ] {
        let approved = vec![origin.to_owned()];
        assert!(audience_allows(origin, &approved), "{origin}");
        let got = get_scoped(&AlwaysHit, "local", origin, &approved).unwrap();
        assert_eq!(got.as_str(), "sk-test-0000-inner");
    }
    for non_literal in [
        "http://localhost.example:8080",
        "http://loopback.invalid:8080",
        "http://[::ffff:127.0.0.1]:8080",
    ] {
        assert!(!audience_allows(non_literal, &[non_literal.to_owned()]));
    }
}

#[test]
fn scoped_get_refuses_before_materializing_and_allows_a_match() {
    struct PanicVault;
    impl Vault for PanicVault {
        fn get(&self, _entry: &str) -> anyhow::Result<Zeroizing<String>> {
            panic!("secret must not materialize for a refused audience")
        }
        fn set(&self, _entry: &str, _value: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }

    let approved = vec!["api.deepseek.com".to_string()];
    let error = get_scoped(&PanicVault, "deepseek", "https://evil.example", &approved).unwrap_err();
    assert!(error.downcast_ref::<AudienceRefused>().is_some());
    let err = error.to_string();
    assert_eq!(
        err,
        "refused: \"deepseek\" is not approved for https://evil.example:443 — add [credential.deepseek] audience = [\"https://evil.example:443\"] to your user law"
    );

    let downgrade = get_scoped(
        &PanicVault,
        "deepseek",
        "http://api.deepseek.com",
        &approved,
    )
    .unwrap_err();
    assert!(downgrade.downcast_ref::<AudienceRefused>().is_some());
    assert!(downgrade.to_string().contains("cannot be sent"));
    assert!(downgrade.to_string().contains("literal loopback"));

    let empty_error = get_scoped(&PanicVault, "custom", "https://custom.example", &[]).unwrap_err();
    assert!(empty_error.downcast_ref::<AudienceRefused>().is_some());
    assert!(empty_error
        .to_string()
        .contains("[credential.custom] audience = [\"https://custom.example:443\"]"));

    let got = get_scoped(
        &AlwaysHit,
        "deepseek",
        "https://api.deepseek.com/v1",
        &approved,
    )
    .unwrap();
    assert_eq!(got.as_str(), "sk-test-0000-inner");
}

#[test]
fn env_var_name_uppercases_and_replaces_hyphens() {
    assert_eq!(env_var_name("test-entry"), "NH_TEST_ENTRY_KEY");
}

#[test]
fn env_fallback_reads_env_on_inner_miss() {
    std::env::set_var("NH_TEST_FALLBACK_KEY", "sk-test-0000-env");
    let vault = EnvFallbackVault { inner: AlwaysMiss };
    let got = vault.get("test-fallback").expect("env fallback should hit");
    assert_eq!(got.as_str(), "sk-test-0000-env");
    std::env::remove_var("NH_TEST_FALLBACK_KEY");
}

#[test]
fn env_fallback_prefers_inner() {
    std::env::set_var("NH_TEST_INNERWINS_KEY", "sk-test-0000-env");
    let vault = EnvFallbackVault { inner: AlwaysHit };
    let got = vault.get("test-innerwins").expect("inner should hit");
    assert_eq!(got.as_str(), "sk-test-0000-inner");
    std::env::remove_var("NH_TEST_INNERWINS_KEY");
}

#[test]
fn env_fallback_both_missing_gives_actionable_error() {
    std::env::remove_var("NH_TEST_MISSING_KEY");
    let vault = EnvFallbackVault { inner: AlwaysMiss };
    let err = vault.get("test-missing").unwrap_err().to_string();
    assert!(err.contains("nh key add test-missing"), "error was: {err}");
    assert!(err.contains("NH_TEST_MISSING_KEY"), "error was: {err}");
    assert!(
        err.contains("key store said: no key stored for \"test-missing\""),
        "error was: {err}"
    );
}

/// Touches the real OS credential store — run manually with `cargo test -- --ignored`.
#[test]
#[ignore]
fn keyring_round_trip() {
    let entry = "nh-vault-test-entry";
    let vault = KeyringVault;
    vault
        .set(entry, "sk-test-0000")
        .expect("set should succeed");
    let got = vault.get(entry);
    // Clean up before asserting so a failure never leaves a test credential behind.
    keyring::Entry::new(SERVICE, entry)
        .expect("open test entry")
        .delete_credential()
        .expect("delete test credential");
    assert_eq!(got.expect("get should succeed").as_str(), "sk-test-0000");
}
