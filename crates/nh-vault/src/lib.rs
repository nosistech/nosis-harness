//! nh-vault — OS-native secret storage + output redaction.
//! THE LAW: no plaintext keys at rest, memory-only injection, zeroized after use.
//! Spec: NOSIS_HARNESS_Master_Plan.md §A.8, 02-architecture/SECURITY_MODEL.md.

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
    fn get(&self, _entry: &str) -> anyhow::Result<Zeroizing<String>> {
        todo!("build agent: keyring::Entry::new(SERVICE, entry)")
    }
    fn set(&self, _entry: &str, _value: &str) -> anyhow::Result<()> {
        todo!("build agent")
    }
}

/// Falls back to env var `NH_<ENTRY>_KEY` (entry uppercased) when the keyring has no value.
/// Fallback only — never the primary path. The error message when BOTH are missing must be
/// friendly and actionable: tell the user exactly to run `nh key add <entry>`.
pub struct EnvFallbackVault<V: Vault> {
    pub inner: V,
}

impl<V: Vault> Vault for EnvFallbackVault<V> {
    fn get(&self, _entry: &str) -> anyhow::Result<Zeroizing<String>> {
        todo!("build agent")
    }
    fn set(&self, entry: &str, value: &str) -> anyhow::Result<()> {
        self.inner.set(entry, value)
    }
}

/// Redacts known key shapes (`sk-…`, `csk-…`, JWT-like) plus the literal secret values
/// currently loaded. Sits on EVERY output path: stdout, logs, receipts.
/// A leaked key shape in any output is a failing test.
pub struct Scrubber {
    literals: Vec<String>,
}

impl Scrubber {
    pub fn new(literal_secrets: Vec<String>) -> Self {
        Self { literals: literal_secrets }
    }
    pub fn scrub(&self, _text: &str) -> String {
        todo!("build agent: replace matches with [REDACTED]")
    }
}
