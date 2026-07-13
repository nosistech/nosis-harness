//! nh-vault - OS-native secret storage + output redaction.
//! SECURITY INVARIANT: no plaintext keys at rest, memory-only injection, zeroized after use.
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
                "no key stored for \"{entry}\" - run `nh key add {entry}`"
            )),
            Err(e) => Err(anyhow::anyhow!(
                "could not read key \"{entry}\" from the OS key store ({e}) - run `nh key add {entry}` to re-store it"
            )),
        }
    }

    fn set(&self, entry: &str, value: &str) -> anyhow::Result<()> {
        open_entry(entry)?
            .set_password(value)
            .map_err(|e| anyhow::anyhow!("could not store key \"{entry}\" in the OS key store ({e})"))
    }
}

fn open_entry(entry: &str) -> anyhow::Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, entry)
        .map_err(|e| anyhow::anyhow!("could not open the OS key store for \"{entry}\" ({e})"))
}

/// Falls back to env var `NH_<ENTRY>_KEY` (entry uppercased) when the keyring has no value.
/// Fallback only - never the primary path. The error message when BOTH are missing must be
/// friendly and actionable: tell the user exactly to run `nh key add <entry>`.
pub struct EnvFallbackVault<V: Vault> {
    pub inner: V,
}

impl<V: Vault> Vault for EnvFallbackVault<V> {
    fn get(&self, entry: &str) -> anyhow::Result<Zeroizing<String>> {
        if let Ok(secret) = self.inner.get(entry) {
            return Ok(secret);
        }
        let var = env_var_name(entry);
        match std::env::var(&var) {
            Ok(value) if !value.is_empty() => Ok(Zeroizing::new(value)),
            _ => Err(anyhow::anyhow!(
                "no key found for \"{entry}\" - run `nh key add {entry}` (or set {var})"
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

/// Known key shapes, one alternation compiled once. `csk-` must precede `sk-`
/// so a `csk-` token is never matched from its second character onward.
const KEY_SHAPES: &str = concat!(
    r"csk-[A-Za-z0-9_\-]{8,}",
    r"|sk-[A-Za-z0-9_\-]{8,}",
    r"|eyJ[A-Za-z0-9_\-]+\.[A-Za-z0-9_\-]+\.[A-Za-z0-9_\-]+",
);

/// Redacts known key shapes (`sk-…`, `csk-…`, JWT-like) plus the literal secret values
/// currently loaded. Sits on EVERY output path: stdout, logs, receipts.
/// A leaked key shape in any output is a failing test.
pub struct Scrubber {
    literals: Vec<String>,
    shapes: Regex,
}

impl Scrubber {
    pub fn new(literal_secrets: Vec<String>) -> Self {
        Self {
            literals: literal_secrets,
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
        assert_eq!(s.scrub("auth: sk-test-0000abcd done"), "auth: [REDACTED] done");
    }

    #[test]
    fn scrub_redacts_csk_shape_fully() {
        let s = Scrubber::new(vec![]);
        // Whole token redacted - no stray leading "c" from the sk- alternative.
        assert_eq!(s.scrub("auth: csk-test-0000abcd done"), "auth: [REDACTED] done");
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
    }

    /// Touches the real OS credential store - run manually with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn keyring_round_trip() {
        let entry = "nh-vault-test-entry";
        let vault = KeyringVault;
        vault.set(entry, "sk-test-0000").expect("set should succeed");
        let got = vault.get(entry);
        // Clean up before asserting so a failure never leaves a test credential behind.
        keyring::Entry::new(SERVICE, entry)
            .expect("open test entry")
            .delete_credential()
            .expect("delete test credential");
        assert_eq!(got.expect("get should succeed").as_str(), "sk-test-0000");
    }
}
