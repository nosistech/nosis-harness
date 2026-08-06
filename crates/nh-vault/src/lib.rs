//! nh-vault - OS-native secret storage + output redaction.
//! SECURITY INVARIANT: no plaintext keys at rest, memory-only injection, zeroized after use.
//! Spec: 02-architecture/SECURITY_MODEL.md.

use regex::Regex;
use std::net::IpAddr;
use zeroize::Zeroizing;

/// Keyring service name for all entries.
pub const SERVICE: &str = "nosis-harness";

/// An in-memory secret whose allocation is zeroized when its owner is dropped.
pub type SecretValue = Zeroizing<String>;

pub fn secret(value: impl Into<String>) -> SecretValue {
    Zeroizing::new(value.into())
}

pub trait Vault: Send + Sync {
    /// Fetch a secret by entry name (e.g. "deepseek"). Value is zeroized on drop.
    fn get(&self, entry: &str) -> anyhow::Result<Zeroizing<String>>;
    /// Store a secret. Never echo or log the value.
    fn set(&self, entry: &str, value: &str) -> anyhow::Result<()>;
    /// Remove a secret from the backing store. Returns false when it was absent.
    fn remove(&self, entry: &str) -> anyhow::Result<bool> {
        anyhow::bail!("secret store does not support removing \"{entry}\"")
    }
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
        open_entry(entry)?.set_password(value).map_err(|e| {
            anyhow::anyhow!("could not store key \"{entry}\" in the OS key store ({e})")
        })
    }

    fn remove(&self, entry: &str) -> anyhow::Result<bool> {
        match open_entry(entry)?.delete_credential() {
            Ok(()) => Ok(true),
            Err(keyring::Error::NoEntry) => Ok(false),
            Err(e) => Err(anyhow::anyhow!(
                "could not remove key \"{entry}\" from the OS key store ({e})"
            )),
        }
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
        let inner_err = match self.inner.get(entry) {
            Ok(secret) => return Ok(secret),
            Err(error) => error,
        };
        let var = env_var_name(entry);
        match std::env::var(&var) {
            Ok(value) if !value.is_empty() => Ok(Zeroizing::new(value)),
            _ => Err(anyhow::anyhow!(
                "no key found for \"{entry}\" - run `nh key add {entry}` (or set {var}); key store said: {inner_err}"
            )),
        }
    }

    fn set(&self, entry: &str, value: &str) -> anyhow::Result<()> {
        self.inner.set(entry, value)
    }

    fn remove(&self, entry: &str) -> anyhow::Result<bool> {
        self.inner.remove(entry)
    }
}

/// Env var name for an entry: `NH_<ENTRY>_KEY`, uppercased, hyphens to underscores.
fn env_var_name(entry: &str) -> String {
    format!("NH_{}_KEY", entry.to_uppercase().replace('-', "_"))
}

const REDACTED: &str = "[REDACTED]";

/// Max chars of untrusted text shown on one terminal line before a visible marker.
const MAX_DISPLAY_CHARS: usize = 500;

/// Canonical key-shape alternatives shared with the generated Git guard.
/// `csk-` must precede `sk-` so a `csk-` token is never matched from its
/// second character onward.
pub const KEY_SHAPE_ALTERNATIVES: &str = concat!(
    r"github_pat_[A-Za-z0-9_]{22,}",
    r"|gh[opushr]_[A-Za-z0-9]{36}",
    r"|AKIA[0-9A-Z]{16}",
    r"|AIza[0-9A-Za-z_\-]{35}",
    r"|xox[baprs]-[A-Za-z0-9-]{10,}",
    r"|glpat-[A-Za-z0-9_\-]{20}",
    r"|npm_[A-Za-z0-9]{36}",
    r"|csk-[A-Za-z0-9_\-]{8,}",
    r"|sk-[A-Za-z0-9_\-]{8,}",
    r"|eyJ[A-Za-z0-9_\-]+\.[A-Za-z0-9_\-]+\.[A-Za-z0-9_\-]+"
);

fn key_shape_regex() -> Regex {
    Regex::new(&format!(r"\b(?:{KEY_SHAPE_ALTERNATIVES})"))
        .expect("static key-shape regex is valid")
}

/// Zeroizing registry of the credentials active in one session.
///
/// Keeping this type separate from [`Scrubber`] makes credential lifetime explicit:
/// callers retain only zeroizing values and derive redactors without ordinary `String`
/// copies. Switched-away credentials may remain here until session end so later output
/// is still redacted.
#[derive(Clone, Default)]
pub struct SecretRegistry {
    literals: Vec<SecretValue>,
}

impl std::fmt::Debug for SecretRegistry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SecretRegistry")
            .field("count", &self.literals.len())
            .finish_non_exhaustive()
    }
}

impl SecretRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, literal: SecretValue) {
        if literal.is_empty() || self.contains(literal.as_str()) {
            return;
        }
        self.literals.push(literal);
        self.literals
            .sort_by_key(|literal| std::cmp::Reverse(literal.as_str().len()));
    }

    pub fn contains(&self, literal: &str) -> bool {
        self.literals.iter().any(|known| known.as_str() == literal)
    }

    pub fn len(&self) -> usize {
        self.literals.len()
    }

    pub fn is_empty(&self) -> bool {
        self.literals.is_empty()
    }

    pub fn scrubber(&self) -> Scrubber {
        Scrubber::from_registry(self)
    }
}

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
        let mut scrubber = Self {
            literals: Vec::new(),
            shapes: key_shape_regex(),
        };
        scrubber.add_literals(literal_secrets);
        scrubber
    }

    pub fn from_registry(registry: &SecretRegistry) -> Self {
        Self {
            literals: registry.literals.clone(),
            shapes: key_shape_regex(),
        }
    }

    /// Add literal secrets without exposing the registry contents.
    pub fn add_literals(&mut self, literal_secrets: Vec<String>) {
        for literal in literal_secrets {
            if !literal.is_empty() && !self.literals.iter().any(|known| known.as_str() == literal) {
                self.literals.push(Zeroizing::new(literal));
            }
        }
        // Redact the longest secret first so a shorter secret that is a prefix of a
        // longer one can never turn into "[REDACTED]<suffix>".
        self.literals
            .sort_by_key(|literal| std::cmp::Reverse(literal.as_str().len()));
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExactOrigin {
    scheme: String,
    host: String,
    port: u16,
}

impl std::fmt::Display for ExactOrigin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.host.contains(':') {
            write!(f, "{}://[{}]:{}", self.scheme, self.host, self.port)
        } else {
            write!(f, "{}://{}:{}", self.scheme, self.host, self.port)
        }
    }
}

enum OriginRefusal {
    Unparseable,
    Insecure(ExactOrigin),
}

fn literal_loopback(host: &str) -> bool {
    if host == "localhost" {
        return true;
    }
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(address)) => address.is_loopback(),
        Ok(IpAddr::V6(address)) => address.is_loopback(),
        Err(_) => false,
    }
}

fn exact_origin(value: &str) -> Result<ExactOrigin, OriginRefusal> {
    let value = value.trim();
    if value.is_empty() {
        return Err(OriginRefusal::Unparseable);
    }
    let absolute;
    let value = if value.contains("://") {
        value
    } else {
        absolute = format!("https://{value}");
        &absolute
    };
    let parsed = url::Url::parse(value).map_err(|_| OriginRefusal::Unparseable)?;
    let host = parsed.host_str().ok_or(OriginRefusal::Unparseable)?;
    let host = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host)
        .to_ascii_lowercase();
    let port = parsed
        .port_or_known_default()
        .ok_or(OriginRefusal::Unparseable)?;
    let origin = ExactOrigin {
        scheme: parsed.scheme().to_ascii_lowercase(),
        host,
        port,
    };
    match origin.scheme.as_str() {
        "https" => Ok(origin),
        "http" if literal_loopback(&origin.host) => Ok(origin),
        _ => Err(OriginRefusal::Insecure(origin)),
    }
}

/// Parse and normalize a credential destination to its exact effective origin
/// (scheme + host + port) with the same URL parser used by reqwest. Host-only
/// policy entries remain shorthand for their default HTTPS origin.
pub fn normalized_host(value: &str) -> Option<String> {
    exact_origin(value).ok().map(|origin| origin.to_string())
}

/// Bare lowercased host of a URL, for policy matching. Accepts scheme-less input
/// (treated as `https://<value>`). IPv6 hosts come back without brackets. Any scheme
/// is accepted (this does NOT enforce transport - callers apply their own policy).
pub fn host_of(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let absolute;
    let candidate = if value.contains("://") {
        value
    } else {
        absolute = format!("https://{value}");
        &absolute
    };
    let parsed = url::Url::parse(candidate).ok()?;
    let host = parsed.host_str()?;
    let host = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host)
        .to_ascii_lowercase();
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}

/// True if `host` is a LITERAL link-local or cloud-metadata IP address:
/// IPv4 169.254.0.0/16 (includes the 169.254.169.254 metadata endpoint),
/// IPv6 fe80::/10, and IPv4-mapped-in-IPv6 forms of the above. Non-IP hosts
/// (DNS names) return false - they are deliberately not resolved here.
pub fn is_link_local_or_metadata(host: &str) -> bool {
    let host = host.trim().trim_start_matches('[').trim_end_matches(']');
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(address)) => address.is_link_local(),
        Ok(IpAddr::V6(address)) => {
            if let Some(mapped) = address.to_ipv4_mapped() {
                mapped.is_link_local()
            } else {
                (address.segments()[0] & 0xffc0) == 0xfe80
            }
        }
        Err(_) => false,
    }
}

/// Whether a requested destination's exact effective origin is permitted for
/// one vault entry. Undeclared, malformed, and insecure entries fail closed.
pub fn audience_allows(requested_destination: &str, approved: &[String]) -> bool {
    if approved.is_empty() {
        return false;
    }
    let Ok(requested) = exact_origin(requested_destination) else {
        return false;
    };
    approved
        .iter()
        .filter_map(|candidate| exact_origin(candidate).ok())
        .any(|candidate| candidate == requested)
}

#[derive(Debug)]
enum AudienceRefusalKind {
    Unapproved,
    InsecureTransport,
}

/// A credential request refused before the secret was materialized.
#[derive(Debug)]
pub struct AudienceRefused {
    pub entry: String,
    pub host: String,
    kind: AudienceRefusalKind,
}

impl std::fmt::Display for AudienceRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.kind {
            AudienceRefusalKind::Unapproved => write!(
                f,
                "refused: \"{}\" is not approved for {} - add [credential.{}] audience = [\"{}\"] to your user law",
                self.entry, self.host, self.entry, self.host
            ),
            AudienceRefusalKind::InsecureTransport => write!(
                f,
                "refused: \"{}\" cannot be sent to {} - use https, or plain http only for literal loopback (127.0.0.0/8, [::1], or localhost)",
                self.entry, self.host
            ),
        }
    }
}

impl std::error::Error for AudienceRefused {}

/// Fetch a secret only after its destination passes the transport invariant
/// and exact-origin audience policy supplied by the caller.
pub fn get_scoped<V: Vault>(
    vault: &V,
    entry: &str,
    requested_destination: &str,
    approved: &[String],
) -> anyhow::Result<Zeroizing<String>> {
    let origin = match exact_origin(requested_destination) {
        Ok(origin) => origin,
        Err(OriginRefusal::Unparseable) => {
            return Err(anyhow::Error::new(AudienceRefused {
                entry: entry.to_string(),
                host: "<unparseable destination>".to_string(),
                kind: AudienceRefusalKind::Unapproved,
            }))
        }
        Err(OriginRefusal::Insecure(origin)) => {
            return Err(anyhow::Error::new(AudienceRefused {
                entry: entry.to_string(),
                host: origin.to_string(),
                kind: AudienceRefusalKind::InsecureTransport,
            }))
        }
    };
    if !audience_allows(requested_destination, approved) {
        return Err(anyhow::Error::new(AudienceRefused {
            entry: entry.to_string(),
            host: origin.to_string(),
            kind: AudienceRefusalKind::Unapproved,
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

/// Escape untrusted text for terminal display WITHOUT truncating: strip invisible
/// carrier characters, escape control + bidirectional-format characters so model/MCP
/// output cannot spoof or reorder the display. Use where the full text must remain
/// visible (e.g. an approval prompt); pair with a caller-side length bound if needed.
pub fn escape_untrusted(text: &str) -> String {
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
    escaped
}

/// Make MCP-provided descriptions and schema strings visibly inert and bounded.
/// Invisible carrier characters are removed; controls are escaped, never run.
pub fn sanitize_untrusted_text(text: &str) -> String {
    const MAX_UNTRUSTED_CHARS: usize = 1_000;

    let escaped = escape_untrusted(text);
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
mod tests;
