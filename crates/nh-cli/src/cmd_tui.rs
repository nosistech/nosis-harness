//! nh tui resolves session data, then hands terminal ownership to nh-tui.

use std::path::Path;

use nh_core::session_ledger::RestoredSession;
use nh_law::LoadOptions;
use nh_routes::{Profiles, RouteResolver};
use nh_tools::McpToolset;
use nh_tui::{mcp_palette_entries, PaletteEntry, TuiConfig};
use nh_vault::{EnvFallbackVault, KeyringVault, Scrubber, Vault};

use crate::cmd_run;

pub fn run(model: &str, budget: Option<u64>, profile: &str) -> anyhow::Result<()> {
    run_with_resume(model, budget, profile, None)
}

pub(crate) fn resume(restored: RestoredSession) -> anyhow::Result<()> {
    let model = restored.route_id.clone();
    let profile = restored.profile.clone();
    run_with_resume(&model, None, &profile, Some(restored))
}

fn run_with_resume(
    model: &str,
    budget: Option<u64>,
    profile: &str,
    resume: Option<RestoredSession>,
) -> anyhow::Result<()> {
    let workdir = std::env::current_dir()?;
    let (repo_root, catalog) = cmd_run::find_catalog(&workdir)?;
    let law = nh_law::load_checked(&repo_root, &LoadOptions { cli_autonomy: None })?;
    let warning_scrubber = Scrubber::new(Vec::new());
    for warning in &law.warnings {
        eprintln!("warning: {}", pre_screen_line(&warning_scrubber, warning));
    }
    let mut mcp_warnings = Vec::new();
    let home = nh_law::user_home_dir();
    let palette_entries =
        load_mcp_palette(&repo_root, home.as_deref(), &law.policy, &mut mcp_warnings);
    for warning in &mcp_warnings {
        eprintln!("warning: {}", pre_screen_line(&warning_scrubber, warning));
    }
    let resolver = RouteResolver::from_toml(&catalog)?;
    let vault = EnvFallbackVault {
        inner: KeyringVault,
    };
    let credentialed_providers = credentialed_providers(&resolver, &law.policy, &vault);
    let route = resolver.resolve(model)?;
    let (profiles, profile_warnings) = Profiles::load(&repo_root);
    for warning in &profile_warnings {
        eprintln!("warning: {}", pre_screen_line(&warning_scrubber, warning));
    }
    let execution_policy = profiles.effective(profile, &route);
    if let Some(error) = cmd_run::profile_fallback_warning(profile, &execution_policy.profile) {
        anyhow::bail!("{error}");
    }
    nh_tui::run(TuiConfig {
        resolver,
        model_id: model.to_owned(),
        profiles,
        profile: execution_policy.profile,
        law,
        budget,
        repo_root,
        workdir,
        palette_entries,
        credentialed_providers,
        resume,
    })
}

fn pre_screen_line(scrubber: &Scrubber, line: &str) -> String {
    cmd_run::safe_line(scrubber, line)
}

fn credentialed_providers<V: Vault>(
    resolver: &RouteResolver,
    policy: &nh_law::Policy,
    vault: &V,
) -> Vec<String> {
    resolver
        .available_by_provider()
        .into_keys()
        .filter(|provider| {
            let Ok(route) = resolver.provider_default(provider) else {
                return false;
            };
            let approved = policy.approved_audiences(route.vault_entry());
            nh_vault::get_scoped(vault, route.vault_entry(), route.base_url(), &approved)
                .is_ok_and(|secret| !secret.trim().is_empty())
        })
        .collect()
}

/// Load and discover MCP once, before nh-tui takes terminal ownership.
fn load_mcp_palette(
    root: &Path,
    home: Option<&Path>,
    policy: &nh_law::Policy,
    warnings: &mut Vec<String>,
) -> Vec<PaletteEntry> {
    let warning_start = warnings.len();
    let configs = cmd_run::load_and_vet_mcp_configs(root, home, policy, warnings);
    if configs.is_empty() && warnings.len() > warning_start {
        for warning in &mut warnings[warning_start..] {
            warning.push_str(" - palette marks MCP stale");
        }
    }
    let send_allowed = |host: &str| !matches!(policy.send_verdict(host), nh_law::Verdict::Block(_));
    let McpToolset {
        tools,
        warnings: discovery_warnings,
    } = nh_tools::mcp_tools(&configs, &send_allowed);
    let mut palette_warnings = warnings[warning_start..].to_vec();
    palette_warnings.extend(discovery_warnings.iter().cloned());
    let toolset = McpToolset {
        tools,
        warnings: palette_warnings,
    };
    let entries = mcp_palette_entries(&configs, &toolset);
    warnings.extend(discovery_warnings);
    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    const PROVIDER_CATALOG: &str = r#"
        [routes.deepseek-route]
        provider = "deepseek"
        model_id = "deepseek-route"
        base_url = "https://api.deepseek.com"
        wire = "openai"
        vault_entry = "deepseek"
        context = 10000
        [routes.deepseek-route.price]
        currency = "USD"
        unit = "per_million_tokens"
        cache_hit = 0.1
        cache_miss = 0.1
        output = 0.1
        price_confidence = "confirmed"

        [routes.glm-route]
        provider = "glm"
        model_id = "glm-route"
        base_url = "https://api.z.ai"
        wire = "openai"
        vault_entry = "glm"
        context = 10000
        [routes.glm-route.price]
        currency = "USD"
        unit = "per_million_tokens"
        cache_hit = 0.1
        cache_miss = 0.1
        output = 0.1
        price_confidence = "confirmed"

        [routes.kimi-route]
        provider = "kimi"
        model_id = "kimi-route"
        base_url = "https://api.moonshot.ai"
        wire = "openai"
        vault_entry = "kimi"
        context = 10000
        [routes.kimi-route.price]
        currency = "USD"
        unit = "per_million_tokens"
        cache_hit = 0.1
        cache_miss = 0.1
        output = 0.1
        price_confidence = "confirmed"

        [routes.rogue-route]
        provider = "rogue"
        model_id = "rogue-route"
        base_url = "https://unapproved.invalid"
        wire = "openai"
        vault_entry = "deepseek"
        context = 10000
        [routes.rogue-route.price]
        currency = "USD"
        unit = "per_million_tokens"
        cache_hit = 0.1
        cache_miss = 0.1
        output = 0.1
        price_confidence = "confirmed"
    "#;

    struct StubVault {
        entries: BTreeSet<String>,
    }

    impl Vault for StubVault {
        fn get(&self, entry: &str) -> anyhow::Result<nh_vault::SecretValue> {
            if self.entries.contains(entry) {
                Ok(nh_vault::secret(format!("fake-credential-{entry}")))
            } else {
                anyhow::bail!("missing test credential")
            }
        }

        fn set(&self, _entry: &str, _value: &str) -> anyhow::Result<()> {
            anyhow::bail!("test vault is read-only")
        }
    }

    #[test]
    fn empty_mcp_config_has_no_palette_rows_or_warnings() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join(".nosis")).unwrap();
        std::fs::write(root.path().join(".nosis").join("mcp.toml"), "").unwrap();
        let mut warnings = Vec::new();
        let law = nh_law::load(root.path(), &LoadOptions { cli_autonomy: None });

        let entries = load_mcp_palette(root.path(), None, &law.policy, &mut warnings);

        assert!(entries.is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn broken_mcp_config_becomes_one_stale_palette_row() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join(".nosis")).unwrap();
        std::fs::write(root.path().join(".nosis").join("mcp.toml"), "not [ valid").unwrap();
        let mut warnings = Vec::new();
        let law = nh_law::load(root.path(), &LoadOptions { cli_autonomy: None });

        let entries = load_mcp_palette(root.path(), None, &law.policy, &mut warnings);

        assert_eq!(entries.len(), 1);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("palette marks MCP stale"));
    }

    #[test]
    fn pre_screen_lines_use_an_ascii_dash() {
        let line = pre_screen_line(&Scrubber::new(Vec::new()), "warning - before alt screen");
        assert_eq!(line, "warning - before alt screen");
    }

    #[test]
    fn provider_picker_source_keeps_only_usable_scoped_credentials() {
        let root = tempfile::tempdir().unwrap();
        let law = nh_law::load(root.path(), &LoadOptions { cli_autonomy: None });
        let resolver = RouteResolver::from_toml(PROVIDER_CATALOG).unwrap();
        let vault = StubVault {
            entries: ["deepseek".to_owned(), "kimi".to_owned()]
                .into_iter()
                .collect(),
        };

        assert_eq!(
            credentialed_providers(&resolver, &law.policy, &vault),
            ["deepseek", "kimi"]
        );
    }
}
