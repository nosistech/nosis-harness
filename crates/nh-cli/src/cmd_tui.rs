//! nh tui resolves session data, then hands terminal ownership to nh-tui.

use std::path::Path;

use nh_law::LoadOptions;
use nh_routes::{Profiles, RouteResolver};
use nh_tools::McpToolset;
use nh_tui::{mcp_palette_entries, PaletteEntry, TuiConfig};
use nh_vault::Scrubber;

use crate::cmd_run;

pub fn run(model: &str, budget: Option<u64>, profile: &str) -> anyhow::Result<()> {
    let workdir = std::env::current_dir()?;
    let (repo_root, catalog) = cmd_run::find_catalog(&workdir)?;
    let law = nh_law::load(&repo_root, &LoadOptions { cli_autonomy: None });
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
    let route = resolver.resolve(model)?;
    let (profiles, profile_warnings) = Profiles::load(&repo_root);
    for warning in &profile_warnings {
        eprintln!("warning: {}", pre_screen_line(&warning_scrubber, warning));
    }
    let execution_policy = profiles.effective(profile, &route);
    if let Some(warning) = cmd_run::profile_fallback_warning(profile, &execution_policy.profile) {
        eprintln!("warning: {}", pre_screen_line(&warning_scrubber, &warning));
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
    })
}

fn pre_screen_line(scrubber: &Scrubber, line: &str) -> String {
    cmd_run::safe_line(scrubber, line).replace('—', "-")
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
        let line = pre_screen_line(&Scrubber::new(Vec::new()), "warning — before alt screen");
        assert_eq!(line, "warning - before alt screen");
    }
}
