use nh_vault::{
    audience_allows, escape_untrusted, host_of, is_link_local_or_metadata, normalized_origin,
    sanitize_line, sanitize_untrusted_text, secret, Scrubber, SecretRegistry,
};

#[test]
fn scrubber_redacts_registered_secret_and_key_shape_in_context() {
    let registered = "registered credential fixture";
    // Split so the pre-commit guard never sees a complete key-shaped fixture in source.
    let key_shaped = concat!("sk", "-other-0000abcd");
    let mut registry = SecretRegistry::new();
    registry.insert(secret(registered));
    let scrubber: Scrubber = registry.scrubber();

    assert_eq!(
        scrubber.scrub(&format!("before {registered}; {key_shaped}; after")),
        "before [REDACTED]; [REDACTED]; after"
    );
}

#[test]
fn escape_untrusted_escapes_controls_and_bidi_but_drops_carriers() {
    let escaped = escape_untrusted("safe\r\x1b\u{202e}hidden\u{200b}\u{e0001}tail");

    assert_eq!(escaped, "safe\\r\\u{1b}\\u{202e}hiddentail");
    assert!(!escaped.contains('\u{200b}'));
    assert!(!escaped.contains('\u{e0001}'));
}

#[test]
fn line_and_untrusted_text_sanitizers_have_different_bounds() {
    let input = "x".repeat(600);

    assert_eq!(sanitize_untrusted_text(&input), input);
    assert_eq!(
        sanitize_line(&input),
        format!("{}\u{2026} (+100 more chars)", "x".repeat(500))
    );
    assert_ne!(sanitize_line(&input), sanitize_untrusted_text(&input));
}

#[test]
fn normalized_origin_and_host_of_return_security_distinct_values() {
    let destination = "https://Api.Example.COM:8443/v1/messages";

    assert_eq!(
        normalized_origin(destination).as_deref(),
        Some("https://api.example.com:8443")
    );
    assert_eq!(host_of(destination).as_deref(), Some("api.example.com"));
    assert_ne!(normalized_origin(destination), host_of(destination));
}

#[test]
fn audience_allows_only_an_exact_origin() {
    let approved = vec!["https://api.example.com:443".to_owned()];

    assert!(audience_allows(
        "https://API.EXAMPLE.COM/v1/messages",
        &approved
    ));
    assert!(!audience_allows("http://api.example.com", &approved));
    assert!(!audience_allows("https://api.example.com:8443", &approved));
    assert!(!audience_allows("https://other.example.com", &approved));
    assert!(!audience_allows("https://evilapi.example.com", &approved));
}

#[test]
fn link_local_and_metadata_classification_excludes_public_hosts() {
    assert!(is_link_local_or_metadata("169.254.169.254"));
    assert!(is_link_local_or_metadata("169.254.42.7"));
    assert!(is_link_local_or_metadata("fe80::1"));
    assert!(!is_link_local_or_metadata("example.com"));
}
