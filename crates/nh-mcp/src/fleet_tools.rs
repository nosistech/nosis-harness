//! Fleet run and status MCP tool handlers.

use super::*;

#[derive(Deserialize)]
struct FleetRunArgs {
    #[serde(default)]
    tasks: Vec<nh_fleet::TaskSpec>,
    #[serde(default)]
    max_workers: Option<usize>,
    #[serde(default)]
    budget: Option<u64>,
    #[serde(default)]
    defer_offpeak: Option<bool>,
}

pub(super) fn fleet_run(arguments: &Value, runtime: &Runtime) -> Value {
    let args: FleetRunArgs = match serde_json::from_value(arguments.clone()) {
        Ok(args) => args,
        Err(error) => return tool_error(runtime, &error.to_string()),
    };
    if args.tasks.is_empty() {
        return tool_error(runtime, "fleet_run needs a non-empty tasks array");
    }
    let budget = match args.budget {
        Some(0) => return tool_error(runtime, "fleet_run budget must be at least 1"),
        Some(budget) if budget > MAX_MCP_FLEET_BUDGET_TOKENS => {
            let message = format!(
                "fleet_run budget exceeds the MCP ceiling of {MAX_MCP_FLEET_BUDGET_TOKENS} tokens"
            );
            return tool_error(runtime, &message);
        }
        Some(budget) => budget,
        None => return tool_error(runtime, "fleet_run requires a token budget"),
    };
    let ceiling = runtime.config.max_workers.max(1);
    let requested = args.max_workers.unwrap_or(ceiling);
    if requested == 0 {
        return tool_error(runtime, "fleet_run max_workers must be at least 1");
    }
    let max_workers = requested.min(ceiling);
    let prior = runtime.active_runs.fetch_add(1, Ordering::SeqCst);
    if prior >= MAX_ACTIVE_RUNS {
        runtime.active_runs.fetch_sub(1, Ordering::SeqCst);
        return tool_error(
            runtime,
            "fleet is busy — too many active runs; retry shortly",
        );
    }
    let guard = ActiveRunGuard(Arc::clone(&runtime.active_runs));
    let resolver = match nh_routes::RouteResolver::from_toml(&runtime.config.catalog) {
        Ok(resolver) => resolver,
        Err(error) => {
            return tool_error(runtime, &format!("fleet run rejected: {error}"));
        }
    };
    if let Err(error) = preflight_fleet_run(
        &resolver,
        &runtime.config.default_route,
        &args.tasks,
        &runtime.config.law,
    ) {
        return tool_error(runtime, &format!("fleet run rejected: {error}"));
    }
    let task_count = args.tasks.len();
    let run_id = nh_fleet::new_run_id();
    if let Err(error) = nh_core::runtime_path::ensure_contained_dir(
        &runtime.config.run_root,
        &fleet_run_relative(&run_id),
    ) {
        return tool_error(
            runtime,
            &format!("fleet run rejected: could not create run directory: {error}"),
        );
    }
    let config = nh_fleet::FleetConfig {
        resolver,
        law: runtime.config.law.clone(),
        default_route: runtime.config.default_route.clone(),
        tasks: args.tasks,
        max_workers,
        budget_tokens: Some(budget),
        clock: None,
        defer_offpeak: args.defer_offpeak.unwrap_or(false),
        ladder: None,
        escalate_on_partial: false,
        swarm: None,
        run_root: runtime.config.run_root.clone(),
        on_event: None,
    };
    let id = run_id.clone();
    let warning_scrubber = runtime.scrubber.clone();
    let spawn = thread::Builder::new()
        .name(format!("nh-mcp-fleet-{id}"))
        .spawn(move || {
            let _guard = guard; // decrements active_runs on thread exit / unwind
            if let Err(error) = nh_fleet::run_with_id(id, config) {
                eprintln!(
                    "warning: {}",
                    nh_vault::safe_line(
                        &warning_scrubber,
                        &format!("nh-mcp fleet run failed after startup: {error}"),
                    )
                );
            }
        });
    if let Err(error) = spawn {
        return tool_error(
            runtime,
            &format!("fleet run rejected: could not start worker thread: {error}"),
        );
    }
    tool_result(
        runtime,
        &format!("fleet run started · run_id={run_id} · {task_count} tasks"),
        json!({ "run_id": run_id, "task_count": task_count }),
        false,
    )
}

pub(super) fn preflight_fleet_run(
    resolver: &nh_routes::RouteResolver,
    default_route: &str,
    tasks: &[nh_fleet::TaskSpec],
    law: &nh_law::Law,
) -> anyhow::Result<()> {
    nh_fleet::validate_task_specs(tasks)?;
    let using_test_provider = cfg!(any(test, debug_assertions))
        && std::env::var("NH_FLEET_TEST_PROVIDER").as_deref() == Ok("echo");
    let vault = nh_vault::EnvFallbackVault {
        inner: nh_vault::KeyringVault,
    };
    let mut routes = BTreeSet::new();
    for task in tasks {
        let route = resolver.resolve(task.model.as_deref().unwrap_or(default_route))?;
        if route.class() == nh_routes::RouteClass::Delegate {
            bail!("delegate routes are not available to fleet workers — pick an api route");
        }
        let native = task.backend.unwrap_or(nh_fleet::Backend::Native) == nh_fleet::Backend::Native;
        if native && !using_test_provider && routes.insert(route.id().to_owned()) {
            drop(credential::connect(
                &vault,
                &route,
                &law.policy.approved_audiences(route.vault_entry()),
                None,
            )?);
        }
    }
    Ok(())
}

#[derive(Deserialize)]
struct FleetStatusArgs {
    run_id: String,
}

pub(super) fn fleet_status(arguments: &Value, runtime: &Runtime) -> Value {
    let args: FleetStatusArgs = match serde_json::from_value(arguments.clone()) {
        Ok(args) => args,
        Err(_) => return tool_error(runtime, "fleet_status needs a run_id"),
    };
    let events = match nh_fleet::read_run_ledger(&runtime.config.run_root, &args.run_id) {
        Ok(events) => events,
        Err(error) => return tool_error(runtime, &error.to_string()),
    };
    let run_exists = match nh_core::runtime_path::resolve_contained_dir(
        &runtime.config.run_root,
        &fleet_run_relative(&args.run_id),
    ) {
        Ok(path) => path.is_some(),
        Err(error) => return tool_error(runtime, &error.to_string()),
    };
    if !run_exists {
        return tool_result(
            runtime,
            &format!("unknown run: {}", args.run_id),
            json!({
                "run_id": args.run_id,
                "state": "unknown",
                "done": 0,
                "failed": 0,
                "gated": 0,
                "pending": 0,
                "unmetered": 0
            }),
            true,
        );
    }
    let status = nh_fleet::status_from_ledger(&events);
    let state = if status.finished {
        "finished"
    } else if status.failed_reason.is_some() {
        "failed"
    } else if events.is_empty() {
        "starting"
    } else {
        "running"
    };
    let state_word = if status.finished {
        "finished".to_string()
    } else if let Some(reason) = &status.failed_reason {
        format!("failed: {reason}")
    } else if events.is_empty() {
        "starting".to_string()
    } else {
        "running".to_string()
    };
    let unmetered_suffix = if status.unmetered > 0 {
        format!(" · {} unmetered", status.unmetered)
    } else {
        String::new()
    };
    let text = format!(
        "{} · {} · {} done · {} failed · {} gated · {} pending{}",
        args.run_id,
        state_word,
        status.done,
        status.failed,
        status.gated,
        status.pending,
        unmetered_suffix
    );
    let mut structured = json!({
        "run_id": args.run_id,
        "state": state,
        "done": status.done,
        "failed": status.failed,
        "gated": status.gated,
        "pending": status.pending,
        "unmetered": status.unmetered
    });
    if let Some(reason) = status.failed_reason {
        structured["failed_reason"] = json!(reason);
    }
    tool_result(runtime, &text, structured, false)
}

pub(super) fn fleet_run_relative(run_id: &str) -> PathBuf {
    Path::new(".nosis").join("fleet").join(run_id)
}
