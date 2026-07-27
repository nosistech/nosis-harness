//! `nh fleet` — durable parallel task runs and idempotent resume.

use std::fs::File;
use std::io::Read as _;
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
const MAX_TASK_FILE_BYTES: usize = 1024 * 1024;

fn flag_or_file(cli: Option<bool>, file: Option<bool>) -> bool {
    cli.or(file).unwrap_or(false)
}

fn required_budget(cli: Option<u64>, file: Option<u64>) -> anyhow::Result<u64> {
    let budget = cli.or(file).ok_or_else(|| {
        anyhow::anyhow!(
            "fleet runs require a token budget — pass --budget or set budget_tokens in the task file"
        )
    })?;
    if budget == 0 {
        anyhow::bail!("fleet token budget must be at least 1");
    }
    Ok(budget)
}

fn read_task_file(path: &Path) -> anyhow::Result<String> {
    let file = File::open(path).with_context(|| format!("could not read {}", path.display()))?;
    let mut bytes = Vec::new();
    file.take((MAX_TASK_FILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .with_context(|| format!("could not read {}", path.display()))?;
    if bytes.len() > MAX_TASK_FILE_BYTES {
        anyhow::bail!(
            "{} is too large — fleet task files are limited to {} bytes",
            path.display(),
            MAX_TASK_FILE_BYTES
        );
    }
    String::from_utf8(bytes).with_context(|| format!("{} is not valid UTF-8", path.display()))
}

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
    escalate: Option<bool>,
    defer_offpeak: Option<bool>,
) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let (root, catalog) = cmd_run::find_catalog(&cwd)?;
    let law = nh_law::load(&root, &LoadOptions { cli_autonomy: None });
    print_law_warnings(&law.warnings);
    let input = read_task_file(tasks_path)?;
    let task_file: TaskFile = serde_json::from_str(&input)
        .with_context(|| format!("{} is not a valid fleet task file", tasks_path.display()))?;
    let defer_offpeak = flag_or_file(defer_offpeak, task_file.defer_offpeak);
    let escalate = flag_or_file(escalate, task_file.escalate);
    let budget_tokens = required_budget(budget, task_file.budget_tokens)?;
    let resolver = RouteResolver::from_toml(&catalog)?;
    let report = nh_fleet::run(FleetConfig {
        resolver,
        law,
        default_route: DEFAULT_ROUTE.into(),
        tasks: task_file.tasks,
        max_workers: max_workers
            .or(task_file.max_workers)
            .unwrap_or(DEFAULT_MAX_WORKERS),
        budget_tokens: Some(budget_tokens),
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

    #[test]
    fn cli_false_overrides_true_task_file_flag() {
        assert!(!flag_or_file(Some(false), Some(true)));
        assert!(flag_or_file(None, Some(true)));
    }

    #[test]
    fn fleet_budget_is_required_positive_and_cli_wins() {
        assert!(required_budget(None, None)
            .unwrap_err()
            .to_string()
            .contains("--budget"));
        assert!(required_budget(Some(0), Some(10)).is_err());
        assert_eq!(required_budget(None, Some(10)).unwrap(), 10);
        assert_eq!(required_budget(Some(5), Some(10)).unwrap(), 5);
    }

    #[test]
    fn oversized_task_file_is_rejected_before_json_parsing() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("tasks.json");
        std::fs::write(&path, vec![b'x'; MAX_TASK_FILE_BYTES + 1]).unwrap();

        let error = read_task_file(&path).unwrap_err();

        assert!(error.to_string().contains("too large"));
    }
}
