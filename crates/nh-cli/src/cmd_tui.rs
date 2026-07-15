//! nh tui resolves session data, then hands terminal ownership to nh-tui.

use std::path::Path;

use nh_law::LoadOptions;
use nh_routes::RouteResolver;
use nh_tools::McpToolset;
use nh_tui::{mcp_palette_entries, parse_notify_config, NotifyConfig, PaletteEntry, TuiConfig};
use nh_vault::Scrubber;

use crate::cmd_run;

pub fn run(model: &str, budget: Option<u64>) -> anyhow::Result<()> {
    let workdir = std::env::current_dir()?;
    let (repo_root, catalog) = cmd_run::find_catalog(&workdir)?;
    let law = nh_law::load(&repo_root, &LoadOptions { cli_autonomy: None });
    let warning_scrubber = Scrubber::new(Vec::new());
    for warning in &law.warnings {
        eprintln!(
            "warning: {}",
            pre_screen_line(&warning_scrubber, warning)
        );
    }
    let mut mcp_warnings = Vec::new();
    let palette_entries = load_mcp_palette(&repo_root, &mut mcp_warnings);
    for warning in &mcp_warnings {
        eprintln!(
            "warning: {}",
            pre_screen_line(&warning_scrubber, warning)
        );
    }
    let mut notify_warnings = Vec::new();
    let notify = load_notify_config(&repo_root, &mut notify_warnings);
    for warning in &notify_warnings {
        eprintln!(
            "warning: {}",
            pre_screen_line(&warning_scrubber, warning)
        );
    }
    let resolver = RouteResolver::from_toml(&catalog)?;
    nh_tui::run(TuiConfig {
        resolver,
        model_id: model.to_owned(),
        law,
        budget,
        repo_root,
        workdir,
        palette_entries,
        notify,
    })
}

fn pre_screen_line(scrubber: &Scrubber, line: &str) -> String {
    cmd_run::safe_line(scrubber, line).replace('—', "-")
}

/// Load and discover MCP once, before nh-tui takes terminal ownership.
fn load_mcp_palette(root: &Path, warnings: &mut Vec<String>) -> Vec<PaletteEntry> {
    let path = root.join(".nosis").join("mcp.toml");
    if !path.is_file() {
        return Vec::new();
    }
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => {
            let warning =
                format!("could not read .nosis/mcp.toml ({error}) - palette marks MCP stale");
            warnings.push(warning.clone());
            return mcp_palette_entries(
                &[],
                &McpToolset {
                    tools: Vec::new(),
                    warnings: vec![warning],
                },
            );
        }
    };
    match nh_tools::load_mcp_config(&text) {
        Ok(configs) => {
            let toolset = nh_tools::mcp_tools(&configs);
            let entries = mcp_palette_entries(&configs, &toolset);
            warnings.extend(toolset.warnings.iter().cloned());
            entries
        }
        Err(error) => {
            let warning = format!(".nosis/mcp.toml: {error} - palette marks MCP stale");
            warnings.push(warning.clone());
            mcp_palette_entries(
                &[],
                &McpToolset {
                    tools: Vec::new(),
                    warnings: vec![warning],
                },
            )
        }
    }
}

/// Load notification settings once, before nh-tui takes terminal ownership.
fn load_notify_config(root: &Path, warnings: &mut Vec<String>) -> NotifyConfig {
    let path = root.join(".nosis").join("notify.toml");
    if !path.is_file() {
        warnings.push(".nosis/notify.toml not found - using bell only".into());
        return NotifyConfig::default();
    }
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => {
            warnings.push(format!(
                "could not read .nosis/notify.toml ({error}) - using bell only"
            ));
            return NotifyConfig::default();
        }
    };
    match parse_notify_config(&text) {
        Ok(config) => config,
        Err(error) => {
            warnings.push(format!(".nosis/notify.toml: {error} - using bell only"));
            NotifyConfig::default()
        }
    }
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

        let entries = load_mcp_palette(root.path(), &mut warnings);

        assert!(entries.is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn broken_mcp_config_becomes_one_stale_palette_row() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join(".nosis")).unwrap();
        std::fs::write(root.path().join(".nosis").join("mcp.toml"), "not [ valid").unwrap();
        let mut warnings = Vec::new();

        let entries = load_mcp_palette(root.path(), &mut warnings);

        assert_eq!(entries.len(), 1);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("palette marks MCP stale"));
    }

    #[test]
    fn absent_and_broken_notify_config_are_bell_only_with_one_warning() {
        let root = tempfile::tempdir().unwrap();
        let mut warnings = Vec::new();
        let absent = load_notify_config(root.path(), &mut warnings);
        assert_eq!(absent, NotifyConfig::default());
        assert_eq!(warnings.len(), 1);

        std::fs::create_dir_all(root.path().join(".nosis")).unwrap();
        std::fs::write(
            root.path().join(".nosis").join("notify.toml"),
            "not [ valid",
        )
        .unwrap();
        warnings.clear();
        let broken = load_notify_config(root.path(), &mut warnings);
        assert_eq!(broken, NotifyConfig::default());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("using bell only"));
    }

    #[test]
    fn valid_notify_config_enables_telegram_without_a_token_in_toml() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join(".nosis")).unwrap();
        std::fs::write(
            root.path().join(".nosis").join("notify.toml"),
            "[telegram]\nenabled = true\nchat_id = \"123456789\"\n",
        )
        .unwrap();
        let mut warnings = Vec::new();

        let config = load_notify_config(root.path(), &mut warnings);

        assert!(warnings.is_empty());
        assert_eq!(
            config.telegram,
            Some(nh_tui::TelegramNotifyConfig {
                enabled: true,
                chat_id: "123456789".into(),
            })
        );
    }

    #[test]
    fn pre_screen_lines_use_an_ascii_dash() {
        let line = pre_screen_line(&Scrubber::new(Vec::new()), "warning — before alt screen");
        assert_eq!(line, "warning - before alt screen");
    }
}
