//! nh tui resolves session data, then hands terminal ownership to nh-tui.

use nh_law::LoadOptions;
use nh_routes::RouteResolver;
use nh_tui::TuiConfig;
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
            cmd_run::safe_line(&warning_scrubber, warning)
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
    })
}
