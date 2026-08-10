//! Trusted catalog discovery and restrict-only MCP configuration assembly.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use nh_law::{read_guarded, GuardedRead};
use nh_tools::{McpAuth, McpServerConfig, McpTrust};

pub(super) const BUNDLED_CATALOG: &str = include_str!("../../../../catalog.toml");
const MAX_CATALOG_BYTES: usize = 1024 * 1024;
const MAX_MCP_CONFIG_BYTES: usize = 64 * 1024;

/// Walk up from `start` looking for the project marker `catalog.toml`.
/// Repository route data is accepted only when it is byte-identical to the
/// bundled catalog or to the operator-trusted `~/.nosis/catalog.toml`.
pub(crate) fn find_catalog(start: &Path) -> anyhow::Result<(PathBuf, String)> {
    let home = nh_law::user_home_dir();
    find_catalog_with_home(start, home.as_deref())
}

pub(super) fn find_catalog_with_home(
    start: &Path,
    home: Option<&Path>,
) -> anyhow::Result<(PathBuf, String)> {
    for dir in start.ancestors() {
        let candidate = dir.join("catalog.toml");
        if candidate.is_file() {
            let text = read_catalog_file(&candidate, Some(dir), "repository catalog.toml")?;
            if text == BUNDLED_CATALOG {
                return Ok((dir.to_path_buf(), text));
            }

            if let Some(home) = home {
                let trusted_path = home.join(".nosis").join("catalog.toml");
                match trusted_path.try_exists() {
                    Ok(true) => {
                        let trusted = read_catalog_file(
                            &trusted_path,
                            None,
                            "user-global ~/.nosis/catalog.toml",
                        )?;
                        if text == trusted {
                            return Ok((dir.to_path_buf(), trusted));
                        }
                    }
                    Ok(false) => {}
                    Err(error) => {
                        anyhow::bail!(
                            "could not inspect trusted catalog {}: {error}",
                            trusted_path.display()
                        )
                    }
                }
            }

            anyhow::bail!(
                "repository catalog.toml is not trusted - it can change credential destinations and spend; review it, then copy the exact file to ~/.nosis/catalog.toml to trust it"
            );
        }
    }
    anyhow::bail!("no catalog.toml found - run `nh init` to create one")
}

fn read_catalog_file(
    path: &Path,
    contain_under: Option<&Path>,
    label: &str,
) -> anyhow::Result<String> {
    match read_guarded(path, contain_under, MAX_CATALOG_BYTES) {
        GuardedRead::Text(text) => Ok(text),
        GuardedRead::Absent => anyhow::bail!("could not read {label}: file disappeared"),
        GuardedRead::Refused(reason) => anyhow::bail!("refused {label}: {reason}"),
    }
}

/// Assemble the effective MCP server set. User-global `~/.nosis/mcp.toml` is the trust source;
/// the repository `.nosis/mcp.toml` is RESTRICT-ONLY: it may only tighten trust and may not
/// redirect a user-global server's url/auth or introduce a new destination. Finally, drop any
/// server whose credential audience is unapproved. Each drop contributes one secret-free warning.
pub(crate) fn load_and_vet_mcp_configs(
    repo_root: &Path,
    home: Option<&Path>,
    policy: &nh_law::Policy,
    warnings: &mut Vec<String>,
) -> Vec<McpServerConfig> {
    let user_global = home.map_or_else(Vec::new, |home| {
        read_optional_mcp_config(
            &home.join(".nosis").join("mcp.toml"),
            None,
            "user-global ~/.nosis/mcp.toml",
            warnings,
        )
    });
    let repo = read_optional_mcp_config(
        &repo_root.join(".nosis").join("mcp.toml"),
        Some(repo_root),
        "repository .nosis/mcp.toml",
        warnings,
    );
    merge_and_vet(
        user_global,
        repo,
        |entry| policy.approved_audiences(entry),
        warnings,
    )
}

fn read_optional_mcp_config(
    path: &Path,
    contain_under: Option<&Path>,
    label: &str,
    warnings: &mut Vec<String>,
) -> Vec<McpServerConfig> {
    let text = match read_guarded(path, contain_under, MAX_MCP_CONFIG_BYTES) {
        GuardedRead::Text(text) => text,
        GuardedRead::Absent => return Vec::new(),
        GuardedRead::Refused(reason) => {
            warnings.push(format!(
                "refused {label}: {reason} - continuing without MCP from that file"
            ));
            return Vec::new();
        }
    };
    match nh_tools::load_mcp_config(&text) {
        Ok(configs) => configs,
        Err(error) => {
            warnings.push(format!(
                "{label}: {error} - continuing without MCP from that file"
            ));
            Vec::new()
        }
    }
}

pub(super) fn merge_and_vet(
    user_global: Vec<McpServerConfig>,
    repo: Vec<McpServerConfig>,
    approved_for: impl Fn(&str) -> Vec<String>,
    warnings: &mut Vec<String>,
) -> Vec<McpServerConfig> {
    let mut user_by_name: BTreeMap<_, _> = user_global
        .into_iter()
        .map(|config| (config.name.clone(), config))
        .collect();
    let mut repo_by_name: BTreeMap<_, _> = repo
        .into_iter()
        .map(|config| (config.name.clone(), config))
        .collect();
    let names = user_by_name
        .keys()
        .chain(repo_by_name.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut merged = Vec::with_capacity(names.len());

    for name in names {
        match (user_by_name.remove(&name), repo_by_name.remove(&name)) {
            (Some(mut user_config), Some(repo_config)) => {
                user_config.trust =
                    more_restrictive_mcp_trust(user_config.trust, repo_config.trust);
                merged.push(user_config);
            }
            (Some(user_config), None) => merged.push(user_config),
            (None, Some(_repo_config)) => {
                warnings.push(format!(
                    "mcp server \"{name}\": repository config cannot introduce a destination - declare it in ~/.nosis/mcp.toml first; dropped"
                ));
            }
            (None, None) => continue,
        }
    }

    filter_mcp_audiences_with(merged, warnings, approved_for)
}

fn more_restrictive_mcp_trust(left: McpTrust, right: McpTrust) -> McpTrust {
    fn rank(trust: McpTrust) -> u8 {
        match trust {
            McpTrust::Block => 0,
            McpTrust::Ask => 1,
            McpTrust::Auto => 2,
        }
    }

    if rank(left) <= rank(right) {
        left
    } else {
        right
    }
}

pub(super) fn unapproved_mcp_target<'a>(
    config: &'a McpServerConfig,
    approved: &[String],
) -> Option<(&'a str, &'a str)> {
    match &config.auth {
        McpAuth::None => None,
        McpAuth::ApiKey { vault_entry } => (!nh_vault::audience_allows(&config.url, approved))
            .then_some((vault_entry.as_str(), config.url.as_str())),
        McpAuth::OAuth2 {
            token_url,
            vault_entry,
            ..
        } => {
            if !nh_vault::audience_allows(&config.url, approved) {
                Some((vault_entry.as_str(), config.url.as_str()))
            } else if !nh_vault::audience_allows(token_url, approved) {
                Some((vault_entry.as_str(), token_url.as_str()))
            } else {
                None
            }
        }
    }
}

pub(super) fn filter_mcp_audiences_with(
    configs: Vec<McpServerConfig>,
    warnings: &mut Vec<String>,
    approved_for: impl Fn(&str) -> Vec<String>,
) -> Vec<McpServerConfig> {
    configs
        .into_iter()
        .filter_map(|config| {
            let entry = match &config.auth {
                McpAuth::None => return Some(config),
                McpAuth::ApiKey { vault_entry } | McpAuth::OAuth2 { vault_entry, .. } => {
                    vault_entry
                }
            };
            let approved = approved_for(entry);
            if let Some((entry, target)) = unapproved_mcp_target(&config, &approved) {
                warnings.push(format!(
                    "mcp server \"{}\" dropped - credential \"{entry}\" is not approved for {}",
                    config.name,
                    nh_vault::normalized_origin(target)
                        .as_deref()
                        .unwrap_or("<unparseable destination>")
                ));
                None
            } else {
                Some(config)
            }
        })
        .collect()
}
