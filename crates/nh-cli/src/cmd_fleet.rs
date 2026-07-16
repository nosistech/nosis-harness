//! `nh fleet` - durable parallel task runs and idempotent resume.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use anyhow::Context as _;
use nh_fleet::{FleetConfig, Ladder, TaskSpec};
use nh_law::LoadOptions;
use nh_routes::RouteResolver;
use nh_vault::Scrubber;
use serde::Deserialize;

use crate::cmd_run;

const DEFAULT_ROUTE: &str = "deepseek-v4-flash";
const DEFAULT_MAX_WORKERS: usize = 4;

#[derive(Deserialize)]
struct TaskFile {
    tasks: Vec<TaskSpec>,
    #[serde(default)]
    max_workers: Option<usize>,
    #[serde(default)]
    budget_tokens: Option<u64>,
    #[serde(default)]
    defer_offpeak: Option<bool>,
    #[serde(default)]
    escalate: Option<bool>,
}

pub fn run_tasks(
    tasks_path: &Path,
    max_workers: Option<usize>,
    budget: Option<u64>,
    escalate: bool,
    defer_offpeak: bool,
) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let (root, catalog) = cmd_run::find_catalog(&cwd)?;
    let law = nh_law::load(&root, &LoadOptions { cli_autonomy: None });
    print_law_warnings(&law.warnings);
    let input = fs::read_to_string(tasks_path)
        .with_context(|| format!("could not read {}", tasks_path.display()))?;
    let task_file: TaskFile = serde_json::from_str(&input)
        .with_context(|| format!("{} is not a valid fleet task file", tasks_path.display()))?;
    let defer_offpeak = defer_offpeak || task_file.defer_offpeak.unwrap_or(false);
    let escalate = escalate || task_file.escalate.unwrap_or(false);
    let resolver = RouteResolver::from_toml(&catalog)?;
    let report = nh_fleet::run(FleetConfig {
        resolver,
        law,
        default_route: DEFAULT_ROUTE.into(),
        tasks: task_file.tasks,
        max_workers: max_workers
            .or(task_file.max_workers)
            .unwrap_or(DEFAULT_MAX_WORKERS),
        budget_tokens: budget.or(task_file.budget_tokens),
        clock: None,
        defer_offpeak,
        ladder: escalate.then(Ladder::default_ladder),
        escalate_on_partial: false,
        swarm: None,
        run_root: root,
        on_event: Some(progress_printer()),
    })?;
    println!(
        "fleet {}: {} done | {} failed | {} gated",
        report.run_id, report.done, report.failed, report.gated
    );
    Ok(())
}

pub fn resume_run(run_id: Option<&str>, max_workers: Option<usize>) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let (root, catalog) = cmd_run::find_catalog(&cwd)?;
    let law = nh_law::load(&root, &LoadOptions { cli_autonomy: None });
    print_law_warnings(&law.warnings);
    let resolver = RouteResolver::from_toml(&catalog)?;
    let report = nh_fleet::resume(
        &root,
        run_id,
        FleetConfig {
            resolver,
            law,
            default_route: DEFAULT_ROUTE.into(),
            tasks: Vec::new(),
            // The library treats zero on resume as "reuse RunStarted".
            max_workers: max_workers.unwrap_or(0),
            budget_tokens: None,
            clock: None,
            defer_offpeak: false,
            ladder: None,
            escalate_on_partial: false,
            swarm: None,
            run_root: root.clone(),
            on_event: Some(progress_printer()),
        },
    )?;
    println!(
        "fleet {}: {} done | {} failed | {} gated",
        report.run_id, report.done, report.failed, report.gated
    );
    Ok(())
}

fn progress_printer() -> Arc<dyn Fn(&str) + Send + Sync> {
    Arc::new(|line| eprintln!("  {line}"))
}

pub(crate) fn print_law_warnings(warnings: &[String]) {
    let scrubber = Scrubber::new(Vec::new());
    for warning in warnings {
        eprintln!("warning: {}", cmd_run::safe_line(&scrubber, warning));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_file_defaults_are_optional() {
        let file: TaskFile = serde_json::from_str(r#"{"tasks":[{"task":"one"}]}"#).unwrap();
        assert_eq!(file.tasks.len(), 1);
        assert_eq!(file.max_workers, None);
        assert_eq!(file.budget_tokens, None);
        assert_eq!(file.defer_offpeak, None);
        assert_eq!(file.escalate, None);
    }

    #[test]
    fn task_file_accepts_locked_slice_a_shape() {
        let file: TaskFile = serde_json::from_str(
            r#"{
                "tasks":[{"id":"one","task":"do one","model":"route"}],
                "max_workers":3,
                "budget_tokens":99,
                "defer_offpeak":false,
                "escalate":true
            }"#,
        )
        .unwrap();
        assert_eq!(file.tasks[0].id.as_deref(), Some("one"));
        assert_eq!(file.tasks[0].model.as_deref(), Some("route"));
        assert_eq!(file.max_workers, Some(3));
        assert_eq!(file.budget_tokens, Some(99));
        assert_eq!(file.defer_offpeak, Some(false));
        assert_eq!(file.escalate, Some(true));
    }
}
