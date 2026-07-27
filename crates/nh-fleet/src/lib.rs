//! Durable, resumable fleet execution over the existing Nosis agent loop.
//!
//! The ledger is the source of truth. A single coordinator serializes every
//! event through one mutex-guarded append handle and fsyncs before continuing.

mod engine;
mod ledger;
mod model;
mod prepare;

use engine::*;
use ledger::*;
use prepare::*;

pub use ledger::{new_run_id, read_run_ledger};
pub use model::{
    ladder_position, next_step, plan_from_ledger, ready_to_dispatch, status_from_ledger,
    validate_task_specs, Backend, Clock, FleetConfig, FleetStatus, Ladder, LedgerEvent,
    PendingSwarmClient, ResumePlan, RunReport, Step, SwarmClient, SystemClock, TaskSpec, Tier,
    MAX_FLEET_TASKS, MAX_TASK_ID_BYTES,
};

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{bail, Context as _};
#[cfg(test)]
use chrono::DateTime;
use chrono::{FixedOffset, Local, SecondsFormat, Utc};
use nh_core::agent::{identity_constitution, AgentLoop};
use nh_core::credential;
use nh_core::receipt::{FailureClass, Outcome, Receipt, ReceiptWriter};
use nh_core::wire::{ChatClient, ChatMessage, ChatRequest, ChatResponse, ThinkingEffort, Usage};
use nh_law::Law;
use nh_routes::{RouteClass, RouteResolver, ThinkingDialect};
use nh_tools::{builtin_tools, Access, Guard, ToolCtx};
use nh_vault::{EnvFallbackVault, KeyringVault, Scrubber, SecretRegistry};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

const DEFAULT_MAX_WORKERS: usize = 4;
const MAX_TURNS: u32 = 20;
const FLEET_OUTPUT_CAP: u64 = 16_384;
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
const TEST_PROVIDER_ENV: &str = "NH_FLEET_TEST_PROVIDER";
#[cfg(any(test, debug_assertions))]
const TEST_EXECUTION_LOG_ENV: &str = "NH_FLEET_TEST_EXECUTION_LOG";
#[cfg(any(test, debug_assertions))]
const TEST_SLEEP_MS_ENV: &str = "NH_FLEET_TEST_SLEEP_MS";
#[cfg(any(test, debug_assertions))]
const TEST_OUTCOME_ENV: &str = "NH_FLEET_TEST_OUTCOME";
const SCHEDULER_WAKE_INTERVAL: Duration = Duration::from_millis(100);
const LOCK_RETRY_INTERVAL: Duration = Duration::from_millis(50);
#[cfg(not(test))]
const RESUME_LOCK_RETRY_WINDOW: Duration = Duration::from_secs(2);
#[cfg(test)]
const RESUME_LOCK_RETRY_WINDOW: Duration = Duration::from_millis(150);

static RUN_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static TEST_LOG_LOCK: Mutex<()> = Mutex::new(());

pub fn run(config: FleetConfig) -> anyhow::Result<RunReport> {
    run_with_id(new_run_id(), config)
}

/// Start a new fleet run with a caller-provided durable ledger handle.
pub fn run_with_id(run_id: String, config: FleetConfig) -> anyhow::Result<RunReport> {
    validate_run_id(&run_id)?;
    let fleet_root = checked_fleet_root(&config.run_root)?;
    let run_dir = nh_core::runtime_path::ensure_contained_dir(
        &config.run_root,
        &Path::new(".nosis").join("fleet").join(&run_id),
    )?;
    let _run_lock = RunLock::acquire(&run_dir, Duration::ZERO)?;
    let ledger_path = run_dir.join("ledger.jsonl");
    let mut failure_literals = SecretRegistry::new();
    let mut refused_existing_ledger = false;
    let result = (|| {
        repair_uncommitted_tail(&ledger_path)?;
        if ledger_has_committed_events(&ledger_path)? {
            refused_existing_ledger = true;
            bail!(
                "run '{run_id}' already has a ledger - use `resume` to continue it, not a new run"
            );
        }
        run_with_id_inner(
            run_id.clone(),
            config,
            &fleet_root,
            &ledger_path,
            &mut failure_literals,
        )
    })();
    if refused_existing_ledger {
        return result;
    }
    match result {
        Ok(report) => Ok(report),
        Err(error) => {
            if let Err(bookkeeping) =
                append_run_failed(&ledger_path, &run_id, &error, &failure_literals)
            {
                Err(error.context(format!(
                    "additionally, recording the run failure to the ledger failed: {bookkeeping:#}"
                )))
            } else {
                Err(error)
            }
        }
    }
}

fn run_with_id_inner(
    run_id: String,
    config: FleetConfig,
    fleet_root: &Path,
    ledger_path: &Path,
    failure_literals: &mut SecretRegistry,
) -> anyhow::Result<RunReport> {
    if config.tasks.is_empty() {
        bail!("tasks.json has no tasks - add at least one task");
    }
    if config.max_workers == 0 {
        bail!("max_workers must be at least 1");
    }
    let max_workers = config.max_workers;
    let workdir = std::env::current_dir().context("could not read the current directory")?;
    let test_provider = test_provider_from_env()?;
    let ladder = config.ladder.clone();
    let mut tasks = prepare_new_tasks(
        &config.resolver,
        &config.default_route,
        &config.tasks,
        config.defer_offpeak,
        ladder.as_ref(),
    )?;
    let key_literals = preflight_keys(
        &config.resolver,
        &tasks,
        ladder.as_ref(),
        &config.law,
        test_provider.is_some(),
    )?;
    failure_literals.clone_from(&key_literals);
    scrub_prepared_tasks(&mut tasks, &key_literals.scrubber())?;

    let created_utc = now_utc();
    let ledger = DurableWriter::open(ledger_path, key_literals.scrubber())?;
    ledger.append(&LedgerEvent::RunStarted {
        run_id: run_id.clone(),
        created_utc: created_utc.clone(),
        task_count: tasks.len(),
        max_workers,
        budget_tokens: config.budget_tokens,
        escalate: ladder.is_some(),
    })?;
    for task in &tasks {
        ledger.append(&LedgerEvent::TaskQueued {
            task_id: task.task_id.clone(),
            task: task.task.clone(),
            route_id: task.route_id.clone(),
            defer_offpeak: task.defer_offpeak,
            backend: task.backend,
        })?;
        emit(
            &config.on_event,
            &key_literals.scrubber(),
            &format!("queued {} - {}", task.task_id, task.route_id),
        );
    }
    append_index(
        &fleet_root.join("index.jsonl"),
        &IndexRecord {
            run_id: run_id.clone(),
            created_utc: created_utc.clone(),
            task_count: tasks.len(),
            status: "running".into(),
        },
        &key_literals,
    )?;

    emit(
        &config.on_event,
        &key_literals.scrubber(),
        &format!(
            "fleet {run_id} started - {} tasks, {max_workers} workers",
            tasks.len()
        ),
    );
    let runtime = Arc::new(Runtime {
        resolver: Arc::new(config.resolver),
        law: Arc::new(config.law),
        run_root: config.run_root.clone(),
        workdir,
        key_literals: key_literals.clone(),
        test_provider,
        clock: config
            .clock
            .unwrap_or_else(|| Arc::new(SystemClock) as Arc<dyn Clock>),
        swarm: config
            .swarm
            .unwrap_or_else(|| Arc::new(PendingSwarmClient) as Arc<dyn SwarmClient>),
    });
    let report = execute_tasks(
        &run_id,
        tasks,
        max_workers,
        config.budget_tokens,
        0,
        Counts::default(),
        runtime,
        &ledger,
        &config.on_event,
        ladder.as_ref(),
        config.escalate_on_partial,
        HashMap::new(),
    )?;
    finish_index(
        fleet_root,
        &created_utc,
        config.tasks.len(),
        &report,
        &key_literals,
    )?;
    Ok(report)
}

pub fn resume(
    run_root: &Path,
    run_id: Option<&str>,
    config: FleetConfig,
) -> anyhow::Result<RunReport> {
    let fleet_root = checked_fleet_root(run_root)?;
    let run_id = match run_id {
        Some(id) => {
            validate_run_id(id)?;
            id.to_string()
        }
        None => latest_incomplete_run(&fleet_root.join("index.jsonl"))?,
    };
    let run_dir = nh_core::runtime_path::ensure_contained_dir(
        run_root,
        &Path::new(".nosis").join("fleet").join(&run_id),
    )?;
    let _run_lock = RunLock::acquire(&run_dir, RESUME_LOCK_RETRY_WINDOW)?;
    let ledger_path = run_dir.join("ledger.jsonl");
    let mut failure_literals = SecretRegistry::new();
    let result = (|| {
        repair_uncommitted_tail(&ledger_path)?;
        resume_inner(
            run_root,
            run_id.clone(),
            config,
            &fleet_root,
            &ledger_path,
            &mut failure_literals,
        )
    })();
    match result {
        Ok(report) => Ok(report),
        Err(error) => {
            if let Err(bookkeeping) =
                append_run_failed(&ledger_path, &run_id, &error, &failure_literals)
            {
                Err(error.context(format!(
                    "additionally, recording the run failure to the ledger failed: {bookkeeping:#}"
                )))
            } else {
                Err(error)
            }
        }
    }
}

fn resume_inner(
    run_root: &Path,
    run_id: String,
    config: FleetConfig,
    fleet_root: &Path,
    ledger_path: &Path,
    failure_literals: &mut SecretRegistry,
) -> anyhow::Result<RunReport> {
    let events = read_ledger(ledger_path)?;
    let meta = run_meta(&events, &run_id)?;
    ensure_single_terminal(&events)?;
    let plan = plan_from_ledger(&events);
    let queued = queued_tasks(&events)?;
    let attempts = attempts_by_task(&events);
    let effective_ladder = config
        .ladder
        .clone()
        .or_else(|| meta.escalate.then(Ladder::default_ladder));
    let mut tasks = Vec::with_capacity(plan.todo.len());
    for task_id in &plan.todo {
        let queued_task = queued.get(task_id).ok_or_else(|| {
            anyhow::anyhow!("ledger cannot resume task '{task_id}' - its queued event is missing")
        })?;
        let (route_id, tier_idx, effort, attempt) = match effective_ladder.as_ref() {
            Some(ladder) => {
                let (tier_idx, attempt) = ladder_position(&events, task_id);
                let tier = ladder.tiers().get(tier_idx).ok_or_else(|| {
                    anyhow::anyhow!(
                        "ledger ladder position for '{task_id}' is past the final worker tier"
                    )
                })?;
                (tier.route_id.clone(), tier_idx, Some(tier.effort), attempt)
            }
            None => (
                queued_task.route_id.clone(),
                0,
                None,
                attempts
                    .get(task_id)
                    .copied()
                    .unwrap_or(0)
                    .saturating_add(1),
            ),
        };
        tasks.push(PreparedTask {
            task_id: task_id.clone(),
            task: queued_task.task.clone(),
            route_id,
            attempt,
            tier_idx,
            effort,
            defer_offpeak: queued_task.defer_offpeak,
            backend: queued_task.backend,
        });
    }

    let max_workers = effective_workers(config.max_workers, Some(meta.max_workers))?;
    let budget_tokens = config.budget_tokens.or(meta.budget_tokens);
    let test_provider = test_provider_from_env()?;
    let key_literals = preflight_keys(
        &config.resolver,
        &tasks,
        effective_ladder.as_ref(),
        &config.law,
        test_provider.is_some(),
    )?;
    failure_literals.clone_from(&key_literals);
    let ledger = DurableWriter::open(ledger_path, key_literals.scrubber())?;
    let counts = terminal_counts(&events);

    if tasks.is_empty() && has_finished(&events) {
        let report = counts.report(run_id);
        finish_index(
            fleet_root,
            &meta.created_utc,
            meta.task_count,
            &report,
            &key_literals,
        )?;
        return Ok(report);
    }

    emit(
        &config.on_event,
        &key_literals.scrubber(),
        &format!("resuming {run_id} - {} tasks remaining", tasks.len()),
    );
    let workdir = std::env::current_dir().context("could not read the current directory")?;
    let runtime = Arc::new(Runtime {
        resolver: Arc::new(config.resolver),
        law: Arc::new(config.law),
        run_root: run_root.to_path_buf(),
        workdir,
        key_literals: key_literals.clone(),
        test_provider,
        clock: config
            .clock
            .unwrap_or_else(|| Arc::new(SystemClock) as Arc<dyn Clock>),
        swarm: config
            .swarm
            .unwrap_or_else(|| Arc::new(PendingSwarmClient) as Arc<dyn SwarmClient>),
    });
    let used_tokens = receipt_tokens(&events);
    let failed_attempts = failure_receipts_by_task(&events, config.escalate_on_partial);
    let report = execute_tasks(
        &run_id,
        tasks,
        max_workers,
        budget_tokens,
        used_tokens,
        counts,
        runtime,
        &ledger,
        &config.on_event,
        effective_ladder.as_ref(),
        config.escalate_on_partial,
        failed_attempts,
    )?;
    finish_index(
        fleet_root,
        &meta.created_utc,
        meta.task_count,
        &report,
        &key_literals,
    )?;
    Ok(report)
}

#[cfg(test)]
mod tests;
