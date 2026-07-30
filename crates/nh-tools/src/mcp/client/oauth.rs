//! OAuth refresh, rotation, expiry, and coalescing.

use super::{read_body_capped, McpClient};
use crate::mcp::config::McpAuth;
use nh_vault::{EnvFallbackVault, KeyringVault, SecretRegistry, SecretValue, Vault};
use std::time::{Duration, Instant};

const MAX_OAUTH_BODY_BYTES: usize = 256 * 1024;
const OAUTH_EXPIRY_SKEW: Duration = Duration::from_secs(30);
const OAUTH_DEFAULT_EXPIRES_IN: u64 = 3_600;

#[derive(Default)]
pub(in crate::mcp) struct OAuthState {
    pub(super) access: Option<SecretValue>,
    pub(super) expires_at: Option<Instant>,
    pub(super) refresh: Option<SecretValue>,
}

#[derive(serde::Deserialize)]
struct OAuthTokenResponse {
    access_token: Option<String>,
    expires_in: Option<u64>,
    refresh_token: Option<String>,
}

impl McpClient {
    fn oauth_state(&self) -> anyhow::Result<std::sync::MutexGuard<'_, OAuthState>> {
        self.oauth
            .lock()
            .map_err(|_| anyhow::anyhow!("MCP OAuth state is unavailable after an internal panic"))
    }

    pub(super) fn oauth_access_token_with(
        &self,
        http: &reqwest::blocking::Client,
    ) -> anyhow::Result<SecretValue> {
        let valid = {
            let state = self.oauth_state()?;
            state.access.is_some()
                && state
                    .expires_at
                    .and_then(|expires_at| expires_at.checked_duration_since(Instant::now()))
                    .is_some_and(|remaining| remaining > OAUTH_EXPIRY_SKEW)
        };
        if !valid {
            self.refresh_oauth(http, None)?;
        }
        self.oauth_state()?
            .access
            .clone()
            .ok_or_else(|| anyhow::anyhow!("mcp oauth refresh completed without an access token"))
    }

    pub(super) fn refresh_oauth(
        &self,
        http: &reqwest::blocking::Client,
        rejected_access: Option<&str>,
    ) -> anyhow::Result<()> {
        let McpAuth::OAuth2 {
            token_url,
            client_id,
            vault_entry,
        } = &self.config.auth
        else {
            return Ok(());
        };
        let _refresh = self.refresh_lock.lock().map_err(|_| {
            anyhow::anyhow!("MCP OAuth refresh is unavailable after an internal panic")
        })?;
        let already_refreshed = {
            let state = self.oauth_state()?;
            let valid = state.access.is_some()
                && state
                    .expires_at
                    .and_then(|expires_at| expires_at.checked_duration_since(Instant::now()))
                    .is_some_and(|remaining| remaining > OAUTH_EXPIRY_SKEW);
            match rejected_access {
                Some(rejected) => {
                    valid && state.access.as_ref().map(|value| value.as_str()) != Some(rejected)
                }
                None => valid,
            }
        };
        if already_refreshed {
            return Ok(());
        }
        let failure = || {
            anyhow::anyhow!(
                "mcp server \"{}\": oauth refresh failed — re-authorize with `nh key add {}-refresh` and `nh key add {}-secret` (or check token_url in .nosis/mcp.toml)",
                self.config.name,
                vault_entry,
                vault_entry
            )
        };
        let vault = EnvFallbackVault {
            inner: KeyringVault,
        };
        let cached_refresh = self.oauth_state()?.refresh.clone();
        let refresh_entry = format!("{vault_entry}-refresh");
        let refresh_from_vault = if cached_refresh.is_none() {
            Some(vault.get(&refresh_entry).map_err(|_| failure())?)
        } else {
            None
        };
        let refresh_token = cached_refresh
            .as_ref()
            .map(|value| value.as_str())
            .or_else(|| refresh_from_vault.as_ref().map(|value| value.as_str()))
            .ok_or_else(failure)?;
        let secret_entry = format!("{vault_entry}-secret");
        let client_secret = vault.get(&secret_entry).map_err(|_| failure())?;
        let scope = self.config.scopes.join(" ");
        let mut form = vec![
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", client_id.as_str()),
            ("client_secret", client_secret.as_str()),
        ];
        if !scope.is_empty() {
            form.push(("scope", scope.as_str()));
        }
        form.push(("resource", self.config.url.trim_end_matches('/')));

        let response = http
            .post(token_url)
            .form(&form)
            .send()
            .map_err(|_| failure())?;
        if !response.status().is_success() {
            return Err(failure());
        }
        let body = nh_vault::secret(
            read_body_capped(response, MAX_OAUTH_BODY_BYTES).map_err(|_| failure())?,
        );
        let token: OAuthTokenResponse =
            serde_json::from_str(body.as_str()).map_err(|_| failure())?;
        let access = token
            .access_token
            .filter(|value| !value.is_empty())
            .map(nh_vault::secret)
            .ok_or_else(failure)?;
        let now = Instant::now();
        let expires_at = now
            .checked_add(Duration::from_secs(
                token.expires_in.unwrap_or(OAUTH_DEFAULT_EXPIRES_IN),
            ))
            .unwrap_or(now + Duration::from_secs(OAUTH_DEFAULT_EXPIRES_IN));

        let mut state = self.oauth_state()?;
        state.access = Some(access);
        state.expires_at = Some(expires_at);
        if let Some(refresh) = token
            .refresh_token
            .filter(|value| !value.is_empty())
            .map(nh_vault::secret)
        {
            if vault.set(&refresh_entry, refresh.as_str()).is_err() {
                let mut registry = SecretRegistry::new();
                registry.insert(refresh.clone());
                let scrubber = registry.scrubber();
                eprintln!(
                    "warning: {}",
                    scrubber.scrub(&format!(
                        "could not persist rotated refresh token for {vault_entry} — re-auth may be needed next session"
                    ))
                );
            }
            state.refresh = Some(refresh);
        }
        Ok(())
    }

    #[cfg(test)]
    pub(in crate::mcp) fn expire_oauth_for_test(&self) -> anyhow::Result<()> {
        self.oauth_state()?.expires_at = Some(Instant::now());
        Ok(())
    }

    #[cfg(test)]
    pub(in crate::mcp) fn oauth_access_token(&self) -> anyhow::Result<SecretValue> {
        self.oauth_access_token_with(&self.http)
    }
}
