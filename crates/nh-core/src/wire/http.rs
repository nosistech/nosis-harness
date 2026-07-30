//! Shared, fail-closed HTTP behavior for every provider wire.

use std::io::Read as _;
use std::time::Duration;

use anyhow::Context as _;

/// Non-streaming completions from thinking routes legitimately run for
/// minutes; a dead host still fails quickly through the connect timeout.
pub(super) const REQUEST_TIMEOUT: Duration = Duration::from_secs(600);
pub(super) const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
pub(super) const MAX_PROVIDER_BODY_BYTES: usize = 8 * 1024 * 1024;

/// Both provider adapters use explicit timeouts and reject redirects. Reqwest
/// can forward custom credentials across redirects, so redirect following is
/// unsafe at this boundary.
pub(super) fn client() -> anyhow::Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .connect_timeout(CONNECT_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .context("could not initialize the provider HTTP client")
}

/// Read a response under a hard byte ceiling, even when Content-Length is
/// absent or dishonest.
pub(super) fn read_body_capped(
    response: reqwest::blocking::Response,
    max: usize,
) -> anyhow::Result<String> {
    if let Some(len) = response.content_length() {
        if len > max as u64 {
            anyhow::bail!("provider response too large: {len} bytes exceeds cap {max}");
        }
    }
    let mut bytes = Vec::new();
    response.take(max as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > max {
        anyhow::bail!("provider response exceeded cap of {max} bytes");
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

pub(super) fn send_error(url: &str, error: &reqwest::Error) -> anyhow::Error {
    anyhow::anyhow!(
        "{}",
        send_error_line(
            url,
            error.is_timeout() && !error.is_connect(),
            &error.to_string(),
        )
    )
}

pub(super) fn send_error_line(url: &str, timed_out: bool, detail: &str) -> String {
    if timed_out {
        format!(
            "provider at {url} did not answer within {}s - retry, or switch to another route",
            REQUEST_TIMEOUT.as_secs()
        )
    } else {
        format!("could not reach provider at {url}: {detail}")
    }
}

/// Shared provider-error UX: actionable status plus a scrubbed one-line body.
pub(super) fn provider_error(status: reqwest::StatusCode, body: &str, key: &str) -> anyhow::Error {
    let hint = match status.as_u16() {
        401 | 403 => " - key rejected; run `nh key add <provider>`",
        429 => " - rate limited; retry later",
        _ => "",
    };
    anyhow::anyhow!(
        "provider returned HTTP {}{}: {}",
        status.as_u16(),
        hint,
        scrub_snippet(body, key)
    )
}

/// One-line, truncated body snippet with the API key literal redacted.
pub(super) fn scrub_snippet(body: &str, key: &str) -> String {
    let scrubbed = if key.is_empty() {
        body.to_owned()
    } else {
        body.replace(key, "[REDACTED]")
    };
    let scrubbed = scrubbed.split_whitespace().collect::<Vec<_>>().join(" ");
    if scrubbed.is_empty() {
        return "(empty body)".into();
    }
    if scrubbed.chars().count() > 200 {
        let mut truncated: String = scrubbed.chars().take(200).collect();
        truncated.push('…');
        truncated
    } else {
        scrubbed
    }
}
