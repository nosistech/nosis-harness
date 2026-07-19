//! nh-vault — OS-native secret storage + output redaction.
//! THE LAW: no plaintext keys at rest, memory-only injection, zeroized after use.
//! Spec: NOSIS_HARNESS_Master_Plan.md §A.8, 02-architecture/SECURITY_MODEL.md.

use regex::Regex;
use zeroize::Zeroizing;

/// Keyring service name for all entries.
pub const SERVICE: &str = "nosis-harness";

pub trait Vault: Send + Sync {
    /// Fetch a secret by entry name (e.g. "deepseek"). Value is zeroized on drop.
    fn get(&self, entry: &str) -> anyhow::Result<Zeroizing<String>>;
    /// Store a secret. Never echo or log the value.
    fn set(&self, entry: &str, value: &str) -> anyhow::Result<()>;
}

/// Windows Credential Manager (DPAPI) / macOS Keychain / Linux secret-service via `keyring`.
pub struct KeyringVault;

impl Vault for KeyringVault {
    fn get(&self, entry: &str) -> anyhow::Result<Zeroizing<String>> {
        let cred = open_entry(entry)?;
        match cred.get_password() {
            Ok(secret) => Ok(Zeroizing::new(secret)),
            Err(keyring::Error::NoEntry) => Err(anyhow::anyhow!(
                "no key stored for \"{entry}\" — run `nh key add {entry}`"
            )),
            Err(e) => Err(anyhow::anyhow!(
                "could not read key \"{entry}\" from the OS key store ({e}) — run `nh key add {entry}` to re-store it"
            )),
        }
    }

    fn set(&self, entry: &str, value: &str) -> anyhow::Result<()> {
        open_entry(entry)?.set_password(value).map_err(|e| {
            anyhow::anyhow!("could not store key \"{entry}\" in the OS key store ({e})")
        })
    }
}

fn open_entry(entry: &str) -> anyhow::Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, entry)
        .map_err(|e| anyhow::anyhow!("could not open the OS key store for \"{entry}\" ({e})"))
}

/// Falls back to env var `NH_<ENTRY>_KEY` (entry uppercased) when the keyring has no value.
/// Fallback only — never the primary path. The error message when BOTH are missing must be
/// friendly and actionable: tell the user exactly to run `nh key add <entry>`.
pub struct EnvFallbackVault<V: Vault> {
    pub inner: V,
}

impl<V: Vault> Vault for EnvFallbackVault<V> {
    fn get(&self, entry: &str) -> anyhow::Result<Zeroizing<String>> {
        let inner_err = match self.inner.get(entry) {
            Ok(secret) => return Ok(secret),
            Err(error) => error,
        };
        let var = env_var_name(entry);
        match std::env::var(&var) {
            Ok(value) if !value.is_empty() => Ok(Zeroizing::new(value)),
            _ => Err(anyhow::anyhow!(
                "no key found for \"{entry}\" — run `nh key add {entry}` (or set {var}); key store said: {inner_err}"
            )),
        }
    }

    fn set(&self, entry: &str, value: &str) -> anyhow::Result<()> {
        self.inner.set(entry, value)
    }
}

/// Env var name for an entry: `NH_<ENTRY>_KEY`, uppercased, hyphens to underscores.
fn env_var_name(entry: &str) -> String {
    format!("NH_{}_KEY", entry.to_uppercase().replace('-', "_"))
}

const REDACTED: &str = "[REDACTED]";

/// Max chars of untrusted text shown on one terminal line before a visible marker.
const MAX_DISPLAY_CHARS: usize = 500;

/// Known key shapes, one alternation compiled once. `csk-` must precede `sk-`
/// so a `csk-` token is never matched from its second character onward.
const KEY_SHAPES: &str = concat!(
    r"\b(?:github_pat_[A-Za-z0-9_]{22,}",
    r"|gh[opushr]_[A-Za-z0-9]{36}",
    r"|AKIA[0-9A-Z]{16}",
    r"|AIza[0-9A-Za-z_\-]{35}",
    r"|xox[baprs]-[A-Za-z0-9-]{10,}",
    r"|glpat-[A-Za-z0-9_\-]{20}",
    r"|npm_[A-Za-z0-9]{36}",
    r"|csk-[A-Za-z0-9_\-]{8,}",
    r"|sk-[A-Za-z0-9_\-]{8,}",
    r"|eyJ[A-Za-z0-9_\-]+\.[A-Za-z0-9_\-]+\.[A-Za-z0-9_\-]+",
    r")",
);

/// Redacts known key shapes (`sk-…`, `csk-…`, JWT-like) plus the literal secret values
/// currently loaded. Sits on EVERY output path: stdout, logs, receipts.
/// A leaked key shape in any output is a failing test.
#[derive(Clone)]
pub struct Scrubber {
    literals: Vec<Zeroizing<String>>,
    shapes: Regex,
}

impl Scrubber {
    pub fn new(literal_secrets: Vec<String>) -> Self {
        Self {
            literals: literal_secrets.into_iter().map(Zeroizing::new).collect(),
            shapes: Regex::new(KEY_SHAPES).expect("static key-shape regex is valid"),
        }
    }

    pub fn scrub(&self, text: &str) -> String {
        let mut out = text.to_string();
        for literal in &self.literals {
            if !literal.is_empty() {
                out = out.replace(literal.as_str(), REDACTED);
            }
        }
        self.shapes.replace_all(&out, REDACTED).into_owned()
    }
}

/// Build a scrubber from every vault entry that can be read. Missing or
/// unavailable entries are skipped so redaction setup never blocks a session.
pub fn from_vault<V: Vault>(vault: &V, entries: &[String]) -> Scrubber {
    let mut literals = Vec::new();
    for entry in entries {
        if let Ok(secret) = vault.get(entry) {
            let literal = secret.as_str().to_owned();
            if !literal.is_empty() && !literals.contains(&literal) {
                literals.push(literal);
            }
        }
    }
    Scrubber::new(literals)
}

/// Parse and normalize a destination host with the same URL parser used by
/// reqwest, preventing credential-broker and HTTP-client authority drift.
pub fn normalized_host(value: &str) -> Option<String> {
    let value = value.trim();
    let absolute;
    let value = if value.contains("://") {
        value
    } else {
        absolute = format!("https://{value}");
        &absolute
    };
    url::Url::parse(value)
        .ok()?
        .host_str()
        .map(|host| host.to_ascii_lowercase())
}

/// Whether a requested destination host is permitted for one vault entry.
/// Undeclared entries fail closed.
pub fn audience_allows(requested_host: &str, approved: &[String]) -> bool {
    if approved.is_empty() {
        return false;
    }
    let Some(requested) = normalized_host(requested_host) else {
        return false;
    };
    approved
        .iter()
        .filter_map(|candidate| normalized_host(candidate))
        .any(|candidate| candidate == requested)
}

/// A credential request refused before the secret was materialized.
#[derive(Debug)]
pub struct AudienceRefused {
    pub entry: String,
    pub host: String,
}

impl std::fmt::Display for AudienceRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "refused: \"{}\" is not approved for {} — add [credential.{}] audience = [\"{}\"] to your user law",
            self.entry, self.host, self.entry, self.host
        )
    }
}

impl std::error::Error for AudienceRefused {}

/// Fetch a secret only after its destination host passes the trusted audience
/// policy supplied by the caller.
pub fn get_scoped<V: Vault>(
    vault: &V,
    entry: &str,
    requested_host: &str,
    approved: &[String],
) -> anyhow::Result<Zeroizing<String>> {
    let host = normalized_host(requested_host).ok_or_else(|| {
        anyhow::Error::new(AudienceRefused {
            entry: entry.to_string(),
            host: "<unparseable destination>".to_string(),
        })
    })?;
    if !audience_allows(requested_host, approved) {
        return Err(anyhow::Error::new(AudienceRefused {
            entry: entry.to_string(),
            host,
        }));
    }
    vault.get(entry)
}

fn is_bidi_control(c: char) -> bool {
    c == '\u{061c}'
        || ('\u{202a}'..='\u{202e}').contains(&c)
        || ('\u{2066}'..='\u{2069}').contains(&c)
}

/// Render untrusted text as one safe terminal line: control characters (\n, \r,
/// ESC/ANSI, …) become visible escapes so model output cannot spoof the display,
/// and very long text truncates with an explicit marker.
pub fn sanitize_line(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_control() || is_bidi_control(c) {
            escaped.extend(c.escape_debug());
        } else {
            escaped.push(c);
        }
    }
    let len = escaped.chars().count();
    if len > MAX_DISPLAY_CHARS {
        let head: String = escaped.chars().take(MAX_DISPLAY_CHARS).collect();
        format!("{head}… (+{} more chars)", len - MAX_DISPLAY_CHARS)
    } else {
        escaped
    }
}

/// Make MCP-provided descriptions and schema strings visibly inert and bounded.
/// Invisible carrier characters are removed; controls are escaped, never run.
pub fn sanitize_untrusted_text(text: &str) -> String {
    const MAX_UNTRUSTED_CHARS: usize = 1_000;

    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(
            c,
            '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{2060}' | '\u{feff}'
        ) || ('\u{e0000}'..='\u{e007f}').contains(&c)
        {
            continue;
        }
        if c.is_control() || is_bidi_control(c) {
            escaped.extend(c.escape_debug());
        } else {
            escaped.push(c);
        }
    }
    let len = escaped.chars().count();
    if len > MAX_UNTRUSTED_CHARS {
        let head: String = escaped.chars().take(MAX_UNTRUSTED_CHARS).collect();
        format!("{head}… (+{} more chars)", len - MAX_UNTRUSTED_CHARS)
    } else {
        escaped
    }
}

/// Scrub secrets, then escape and truncate untrusted text for terminal display.
pub fn safe_line(scrubber: &Scrubber, text: &str) -> String {
    sanitize_line(&scrubber.scrub(text))
}

#[cfg(test)]
mod tests {
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
    fn audience_matching_is_host_only_and_empty_is_refused() {
        let approved = vec!["api.deepseek.com".to_string()];
        assert!(!audience_allows("https://evil.example", &approved));
        assert!(audience_allows(
            "https://API.DEEPSEEK.COM:443/anthropic",
            &approved
        ));
        assert!(!audience_allows("https://anything.example/path", &[]));
        assert!(!audience_allows(
            "https://evil.example\\@api.deepseek.com/v1",
            &approved
        ));
    }

    #[test]
    fn ipv6_audience_is_normalized_once_and_allowed() {
        let approved = vec!["[::1]".to_string()];
        assert!(audience_allows("http://[::1]:8080", &approved));
        let got = get_scoped(&AlwaysHit, "local", "http://[::1]:11434", &approved).unwrap();
        assert_eq!(got.as_str(), "sk-test-0000-inner");
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
        let error =
            get_scoped(&PanicVault, "deepseek", "https://evil.example", &approved).unwrap_err();
        assert!(error.downcast_ref::<AudienceRefused>().is_some());
        let err = error.to_string();
        assert_eq!(
            err,
            "refused: \"deepseek\" is not approved for evil.example — add [credential.deepseek] audience = [\"evil.example\"] to your user law"
        );

        let empty_error =
            get_scoped(&PanicVault, "custom", "https://custom.example", &[]).unwrap_err();
        assert!(empty_error.downcast_ref::<AudienceRefused>().is_some());
        assert!(empty_error
            .to_string()
            .contains("[credential.custom] audience = [\"custom.example\"]"));

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
    fn scrubber_from_vault_registers_every_resolvable_literal() {
        struct RegistryVault;
        impl Vault for RegistryVault {
            fn get(&self, entry: &str) -> anyhow::Result<Zeroizing<String>> {
                match entry {
                    "one" => Ok(Zeroizing::new("odd-secret-one".to_string())),
                    "two" => Ok(Zeroizing::new("odd-secret-two".to_string())),
                    _ => anyhow::bail!("missing"),
                }
            }
            fn set(&self, _entry: &str, _value: &str) -> anyhow::Result<()> {
                Ok(())
            }
        }

        let entries = vec!["one".into(), "missing".into(), "two".into(), "one".into()];
        let scrubber = from_vault(&RegistryVault, &entries);
        assert_eq!(
            scrubber.scrub("odd-secret-one / odd-secret-two"),
            "[REDACTED] / [REDACTED]"
        );
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
}
