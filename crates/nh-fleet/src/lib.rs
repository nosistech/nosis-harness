//! Durable, resumable fleet execution over the existing Nosis agent loop.
//!
//! The ledger is the source of truth. A single coordinator serializes every
//! event through one mutex-guarded append handle and fsyncs before continuing.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{bail, Context as _};
use chrono::{SecondsFormat, Utc};
use nh_core::agent::AgentLoop;
use nh_core::receipt::{Outcome, Receipt, ReceiptWriter};
use nh_core::wire::{
    make_client, ChatClient, ChatMessage, ChatRequest, ChatResponse, ThinkingEffort, Usage,
};
use nh_law::Law;
use nh_routes::{RouteClass, RouteResolver, ThinkingDialect};
use nh_tools::{builtin_tools, Access, Guard, ToolCtx};
use nh_vault::{EnvFallbackVault, KeyringVault, Scrubber, Vault};
use serde::{Deserialize, Serialize};

const DEFAULT_MAX_WORKERS: usize = 4;
const MAX_TURNS: u32 = 20;
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
const TEST_PROVIDER_ENV: &str = "NH_FLEET_TEST_PROVIDER";
const TEST_EXECUTION_LOG_ENV: &str = "NH_FLEET_TEST_EXECUTION_LOG";
const TEST_SLEEP_MS_ENV: &str = "NH_FLEET_TEST_SLEEP_MS";

static RUN_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static TEST_LOG_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSpec {
    #[serde(default)]
    pub id: Option<String>,
    pub task: String,
    #[serde(default)]
    pub model: Option<String>,
}

pub struct FleetConfig {
    pub resolver: RouteResolver,
    pub law: Law,
    pub default_route: String,
    pub tasks: Vec<TaskSpec>,
    /// Must be at least one for `run`; `0` on `resume` reuses the original value.
    pub max_workers: usize,
    pub budget_tokens: Option<u64>,
    /// Repository root; fleet data is stored below `.nosis/fleet`.
    pub run_root: PathBuf,
    #[allow(clippy::type_complexity)]
    pub on_event: Option<Arc<dyn Fn(&str) + Send + Sync>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunReport {
    pub run_id: String,
    pub done: usize,
    pub failed: usize,
    pub gated: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum LedgerEvent {
    RunStarted {
        run_id: String,
        created_utc: String,
        task_count: usize,
        max_workers: usize,
        budget_tokens: Option<u64>,
    },
    TaskQueued {
        task_id: String,
        task: String,
        route_id: String,
    },
    TaskStarted {
        task_id: String,
        route_id: String,
        effort: String,
        attempt: u32,
    },
    TaskReceipt {
        task_id: String,
        attempt: u32,
        receipt: Receipt,
    },
    TaskEscalated {
        task_id: String,
        from_route: String,
        to_route: String,
        reason: String,
    },
    TaskDone {
        task_id: String,
        outcome: Outcome,
    },
    TaskGate {
        task_id: String,
        reason: String,
    },
    TaskFailed {
        task_id: String,
        reason: String,
    },
    /// Best-effort liveness only; resume correctness never depends on it.
    TaskHeartbeat {
        task_id: String,
        ts: String,
    },
    RunFinished {
        run_id: String,
        done: usize,
        failed: usize,
        gated: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumePlan {
    pub done: Vec<String>,
    pub todo: Vec<String>,
}

/// Fold committed ledger events into terminal and runnable task IDs. Pure: no I/O.
pub fn plan_from_ledger(events: &[LedgerEvent]) -> ResumePlan {
    let mut order = Vec::new();
    let mut seen = HashSet::new();
    let mut terminal = HashSet::new();

    for event in events {
        let task_id = match event {
            LedgerEvent::TaskQueued { task_id, .. }
            | LedgerEvent::TaskStarted { task_id, .. }
            | LedgerEvent::TaskReceipt { task_id, .. }
            | LedgerEvent::TaskEscalated { task_id, .. }
            | LedgerEvent::TaskDone { task_id, .. }
            | LedgerEvent::TaskGate { task_id, .. }
            | LedgerEvent::TaskFailed { task_id, .. }
            | LedgerEvent::TaskHeartbeat { task_id, .. } => Some(task_id),
            LedgerEvent::RunStarted { .. } | LedgerEvent::RunFinished { .. } => None,
        };
        if let Some(task_id) = task_id {
            if seen.insert(task_id.clone()) {
                order.push(task_id.clone());
            }
            if matches!(
                event,
                LedgerEvent::TaskDone { .. }
                    | LedgerEvent::TaskGate { .. }
                    | LedgerEvent::TaskFailed { .. }
            ) {
                terminal.insert(task_id.clone());
            }
        }
    }

    let (done, todo) = order
        .into_iter()
        .partition(|task_id| terminal.contains(task_id));
    ResumePlan { done, todo }
}

pub fn run(config: FleetConfig) -> anyhow::Result<RunReport> {
    if config.tasks.is_empty() {
        bail!("tasks.json has no tasks - add at least one task");
    }
    if config.max_workers == 0 {
        bail!("max_workers must be at least 1");
    }
    let max_workers = config.max_workers;
    let workdir = std::env::current_dir().context("could not read the current directory")?;
    let test_provider = test_provider_from_env()?;
    let mut tasks = prepare_new_tasks(&config.resolver, &config.default_route, &config.tasks)?;
    let key_literals = preflight_keys(&config.resolver, &tasks, test_provider.is_some())?;
    scrub_prepared_tasks(&mut tasks, &Scrubber::new(key_literals.clone()))?;

    let run_id = new_run_id();
    let created_utc = now_utc();
    let fleet_root = fleet_root(&config.run_root);
    let ledger_path = fleet_root.join(&run_id).join("ledger.jsonl");
    let ledger = DurableWriter::open(&ledger_path, Scrubber::new(key_literals.clone()))?;
    ledger.append(&LedgerEvent::RunStarted {
        run_id: run_id.clone(),
        created_utc: created_utc.clone(),
        task_count: tasks.len(),
        max_workers,
        budget_tokens: config.budget_tokens,
    })?;
    for task in &tasks {
        ledger.append(&LedgerEvent::TaskQueued {
            task_id: task.task_id.clone(),
            task: task.task.clone(),
            route_id: task.route_id.clone(),
        })?;
        emit(
            &config.on_event,
            &Scrubber::new(key_literals.clone()),
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
        &Scrubber::new(key_literals.clone()),
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
        key_literals: Arc::new(key_literals.clone()),
        test_provider,
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
    )?;
    finish_index(
        &fleet_root,
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
    let fleet_root = fleet_root(run_root);
    let run_id = match run_id {
        Some(id) => {
            validate_run_id(id)?;
            id.to_string()
        }
        None => latest_incomplete_run(&fleet_root.join("index.jsonl"))?,
    };
    let ledger_path = fleet_root.join(&run_id).join("ledger.jsonl");
    repair_uncommitted_tail(&ledger_path)?;
    let events = read_ledger(&ledger_path)?;
    let meta = run_meta(&events, &run_id)?;
    ensure_single_terminal(&events)?;
    let plan = plan_from_ledger(&events);
    let queued = queued_tasks(&events)?;
    let attempts = attempts_by_task(&events);
    let mut tasks = Vec::with_capacity(plan.todo.len());
    for task_id in &plan.todo {
        let (task, route_id) = queued.get(task_id).ok_or_else(|| {
            anyhow::anyhow!("ledger cannot resume task '{task_id}' - its queued event is missing")
        })?;
        tasks.push(PreparedTask {
            task_id: task_id.clone(),
            task: task.clone(),
            route_id: route_id.clone(),
            attempt: attempts
                .get(task_id)
                .copied()
                .unwrap_or(0)
                .saturating_add(1),
        });
    }

    let max_workers = effective_workers(config.max_workers, Some(meta.max_workers))?;
    let budget_tokens = config.budget_tokens.or(meta.budget_tokens);
    let test_provider = test_provider_from_env()?;
    let key_literals = preflight_keys(&config.resolver, &tasks, test_provider.is_some())?;
    let ledger = DurableWriter::open(&ledger_path, Scrubber::new(key_literals.clone()))?;
    let counts = terminal_counts(&events);

    if tasks.is_empty() && has_finished(&events) {
        let report = counts.report(run_id);
        finish_index(
            &fleet_root,
            &meta.created_utc,
            meta.task_count,
            &report,
            &key_literals,
        )?;
        return Ok(report);
    }

    emit(
        &config.on_event,
        &Scrubber::new(key_literals.clone()),
        &format!("resuming {run_id} - {} tasks remaining", tasks.len()),
    );
    let workdir = std::env::current_dir().context("could not read the current directory")?;
    let runtime = Arc::new(Runtime {
        resolver: Arc::new(config.resolver),
        law: Arc::new(config.law),
        run_root: run_root.to_path_buf(),
        workdir,
        key_literals: Arc::new(key_literals.clone()),
        test_provider,
    });
    let used_tokens = receipt_tokens(&events);
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
    )?;
    finish_index(
        &fleet_root,
        &meta.created_utc,
        meta.task_count,
        &report,
        &key_literals,
    )?;
    Ok(report)
}

#[derive(Debug, Clone)]
struct PreparedTask {
    task_id: String,
    task: String,
    route_id: String,
    attempt: u32,
}

struct Runtime {
    resolver: Arc<RouteResolver>,
    law: Arc<Law>,
    run_root: PathBuf,
    workdir: PathBuf,
    key_literals: Arc<Vec<String>>,
    test_provider: Option<TestProvider>,
}

#[derive(Clone)]
struct TestProvider {
    execution_log: Option<PathBuf>,
    sleep: Duration,
}

#[derive(Default, Clone, Copy)]
struct Counts {
    done: usize,
    failed: usize,
    gated: usize,
}

impl Counts {
    fn report(self, run_id: String) -> RunReport {
        RunReport {
            run_id,
            done: self.done,
            failed: self.failed,
            gated: self.gated,
        }
    }
}

struct RunMeta {
    created_utc: String,
    task_count: usize,
    max_workers: usize,
    budget_tokens: Option<u64>,
}

#[derive(Serialize, Deserialize)]
struct IndexRecord {
    run_id: String,
    created_utc: String,
    task_count: usize,
    status: String,
}

struct DurableWriter {
    path: PathBuf,
    file: Mutex<File>,
    scrubber: Scrubber,
}

impl DurableWriter {
    fn open(path: &Path, scrubber: Scrubber) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("could not create {}", parent.display()))?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .with_context(|| format!("could not open {}", path.display()))?;
        Ok(Self {
            path: path.to_path_buf(),
            file: Mutex::new(file),
            scrubber,
        })
    }

    fn append<T: Serialize>(&self, value: &T) -> anyhow::Result<()> {
        let line = serde_json::to_string(value).context("could not serialize fleet event")?;
        let line = self.scrubber.scrub(&line);
        let record = format!("{line}\n");
        let mut file = self
            .file
            .lock()
            .map_err(|_| anyhow::anyhow!("fleet ledger writer lock was poisoned"))?;
        file.write_all(record.as_bytes())
            .with_context(|| format!("could not write {}", self.path.display()))?;
        file.flush()
            .with_context(|| format!("could not flush {}", self.path.display()))?;
        file.sync_all()
            .with_context(|| format!("could not fsync {}", self.path.display()))?;
        Ok(())
    }
}

enum WorkerEvent {
    Started {
        task_id: String,
        route_id: String,
        effort: String,
        attempt: u32,
        ack: mpsc::SyncSender<Result<(), String>>,
    },
    Heartbeat {
        task_id: String,
        ts: String,
    },
    Progress(String),
    Finished {
        task_id: String,
        attempt: u32,
        result: Result<Receipt, String>,
    },
}

#[allow(clippy::too_many_arguments)]
#[allow(clippy::type_complexity)]
fn execute_tasks(
    run_id: &str,
    tasks: Vec<PreparedTask>,
    max_workers: usize,
    budget_tokens: Option<u64>,
    mut used_tokens: u64,
    mut counts: Counts,
    runtime: Arc<Runtime>,
    ledger: &DurableWriter,
    on_event: &Option<Arc<dyn Fn(&str) + Send + Sync>>,
) -> anyhow::Result<RunReport> {
    let display_scrubber = Scrubber::new(runtime.key_literals.as_ref().clone());
    let mut remaining: VecDeque<PreparedTask> = tasks.into();
    let worker_count = max_workers.min(remaining.len().max(1));
    let (job_tx, job_rx) = mpsc::channel::<PreparedTask>();
    let job_rx = Arc::new(Mutex::new(job_rx));
    let event_capacity = worker_count.saturating_mul(4).max(16);
    let (event_tx, event_rx) = mpsc::sync_channel::<WorkerEvent>(event_capacity);
    let workers = spawn_workers(worker_count, Arc::clone(&job_rx), event_tx, runtime)?;

    let mut active = 0usize;
    let mut budget_halted = budget_tokens.is_some_and(|limit| used_tokens >= limit);
    if budget_halted {
        halt_remaining_for_budget(
            &mut remaining,
            ledger,
            on_event,
            &display_scrubber,
            &mut counts,
        )?;
    } else {
        while active < worker_count {
            let Some(task) = remaining.pop_front() else {
                break;
            };
            job_tx
                .send(task)
                .context("fleet worker pool stopped before dispatch")?;
            active += 1;
        }
    }

    while active > 0 {
        match event_rx
            .recv()
            .context("fleet worker pool stopped unexpectedly")?
        {
            WorkerEvent::Started {
                task_id,
                route_id,
                effort,
                attempt,
                ack,
            } => {
                let result = ledger
                    .append(&LedgerEvent::TaskStarted {
                        task_id: task_id.clone(),
                        route_id,
                        effort,
                        attempt,
                    })
                    .map_err(|error| error.to_string());
                if result.is_ok() {
                    emit(
                        on_event,
                        &display_scrubber,
                        &format!("running {task_id} - attempt {attempt}"),
                    );
                }
                let failed = result.as_ref().err().cloned();
                let _ = ack.send(result);
                if let Some(error) = failed {
                    bail!("{error}");
                }
            }
            WorkerEvent::Heartbeat { task_id, ts } => {
                ledger.append(&LedgerEvent::TaskHeartbeat { task_id, ts })?;
            }
            WorkerEvent::Progress(line) => emit(on_event, &display_scrubber, &line),
            WorkerEvent::Finished {
                task_id,
                attempt,
                result,
            } => {
                active = active.saturating_sub(1);
                match result {
                    Ok(receipt) => {
                        used_tokens = used_tokens.saturating_add(tokens_in(&receipt));
                        let outcome = receipt.outcome;
                        ledger.append(&LedgerEvent::TaskReceipt {
                            task_id: task_id.clone(),
                            attempt,
                            receipt,
                        })?;
                        ledger.append(&LedgerEvent::TaskDone {
                            task_id: task_id.clone(),
                            outcome,
                        })?;
                        counts.done += 1;
                        emit(
                            on_event,
                            &display_scrubber,
                            &format!("done {task_id} - {outcome:?}"),
                        );
                    }
                    Err(reason) => {
                        ledger.append(&LedgerEvent::TaskFailed {
                            task_id: task_id.clone(),
                            reason: reason.clone(),
                        })?;
                        counts.failed += 1;
                        emit(
                            on_event,
                            &display_scrubber,
                            &format!("failed {task_id} - {reason}"),
                        );
                    }
                }

                if !budget_halted && budget_tokens.is_some_and(|limit| used_tokens >= limit) {
                    budget_halted = true;
                    emit(
                        on_event,
                        &display_scrubber,
                        &format!("budget halted at {used_tokens} tokens - no new tasks will start"),
                    );
                    halt_remaining_for_budget(
                        &mut remaining,
                        ledger,
                        on_event,
                        &display_scrubber,
                        &mut counts,
                    )?;
                }
                if !budget_halted {
                    if let Some(task) = remaining.pop_front() {
                        job_tx
                            .send(task)
                            .context("fleet worker pool stopped before dispatch")?;
                        active += 1;
                    }
                }
            }
        }
    }

    drop(job_tx);
    join_workers(workers)?;
    let report = counts.report(run_id.to_string());
    ledger.append(&LedgerEvent::RunFinished {
        run_id: report.run_id.clone(),
        done: report.done,
        failed: report.failed,
        gated: report.gated,
    })?;
    emit(
        on_event,
        &display_scrubber,
        &format!(
            "fleet {} finished - {} done, {} failed, {} gated",
            report.run_id, report.done, report.failed, report.gated
        ),
    );
    Ok(report)
}

#[allow(clippy::type_complexity)]
fn halt_remaining_for_budget(
    remaining: &mut VecDeque<PreparedTask>,
    ledger: &DurableWriter,
    on_event: &Option<Arc<dyn Fn(&str) + Send + Sync>>,
    scrubber: &Scrubber,
    counts: &mut Counts,
) -> anyhow::Result<()> {
    while let Some(task) = remaining.pop_front() {
        ledger.append(&LedgerEvent::TaskFailed {
            task_id: task.task_id.clone(),
            reason: "budget halted before dispatch".into(),
        })?;
        counts.failed += 1;
        emit(
            on_event,
            scrubber,
            &format!("failed {} - budget halted before dispatch", task.task_id),
        );
    }
    Ok(())
}

fn spawn_workers(
    count: usize,
    jobs: Arc<Mutex<mpsc::Receiver<PreparedTask>>>,
    events: mpsc::SyncSender<WorkerEvent>,
    runtime: Arc<Runtime>,
) -> anyhow::Result<Vec<JoinHandle<()>>> {
    let mut workers = Vec::with_capacity(count);
    for index in 0..count {
        let jobs = Arc::clone(&jobs);
        let events = events.clone();
        let runtime = Arc::clone(&runtime);
        let join = thread::Builder::new()
            .name(format!("nh-fleet-{index}"))
            .spawn(move || worker_loop(jobs, events, runtime))
            .context("could not start a fleet worker")?;
        workers.push(join);
    }
    Ok(workers)
}

fn join_workers(workers: Vec<JoinHandle<()>>) -> anyhow::Result<()> {
    for worker in workers {
        worker
            .join()
            .map_err(|_| anyhow::anyhow!("a fleet worker panicked"))?;
    }
    Ok(())
}

fn worker_loop(
    jobs: Arc<Mutex<mpsc::Receiver<PreparedTask>>>,
    events: mpsc::SyncSender<WorkerEvent>,
    runtime: Arc<Runtime>,
) {
    loop {
        let job = match jobs.lock() {
            Ok(receiver) => receiver.recv(),
            Err(_) => return,
        };
        let Ok(job) = job else { return };
        let route = match runtime.resolver.resolve(&job.route_id) {
            Ok(route) => route,
            Err(error) => {
                let _ = events.send(WorkerEvent::Finished {
                    task_id: job.task_id,
                    attempt: job.attempt,
                    result: Err(error.to_string()),
                });
                continue;
            }
        };
        let (ack_tx, ack_rx) = mpsc::sync_channel(0);
        if events
            .send(WorkerEvent::Started {
                task_id: job.task_id.clone(),
                route_id: job.route_id.clone(),
                effort: effort_name(effort_for(route.thinking_dialect)).into(),
                attempt: job.attempt,
                ack: ack_tx,
            })
            .is_err()
        {
            return;
        }
        if !matches!(ack_rx.recv(), Ok(Ok(()))) {
            return;
        }

        let (stop_tx, stop_rx) = mpsc::channel();
        let heartbeat_events = events.clone();
        let heartbeat_task_id = job.task_id.clone();
        let heartbeat = thread::spawn(move || loop {
            match stop_rx.recv_timeout(HEARTBEAT_INTERVAL) {
                Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    let _ = heartbeat_events.try_send(WorkerEvent::Heartbeat {
                        task_id: heartbeat_task_id.clone(),
                        ts: now_utc(),
                    });
                }
            }
        });
        let result = run_one_task(&runtime, &job, route, &events);
        let _ = stop_tx.send(());
        let _ = heartbeat.join();
        if events
            .send(WorkerEvent::Finished {
                task_id: job.task_id,
                attempt: job.attempt,
                result,
            })
            .is_err()
        {
            return;
        }
    }
}

fn run_one_task(
    runtime: &Runtime,
    job: &PreparedTask,
    route: nh_routes::ResolvedRoute,
    events: &mpsc::SyncSender<WorkerEvent>,
) -> Result<Receipt, String> {
    let client: Box<dyn ChatClient> = match &runtime.test_provider {
        Some(provider) => Box::new(EchoClient {
            task_id: job.task_id.clone(),
            config: provider.clone(),
        }),
        None => {
            let vault = EnvFallbackVault {
                inner: KeyringVault,
            };
            let key = vault
                .get(&route.vault_entry)
                .map_err(|error| error.to_string())?;
            make_client(&route, key)
        }
    };
    let policy = runtime.law.policy.clone();
    let approval_events = events.clone();
    let approval_task_id = job.task_id.clone();
    let ctx = ToolCtx::new(
        runtime.workdir.clone(),
        Box::new(move |_| {
            let _ = approval_events.try_send(WorkerEvent::Progress(format!(
                "{approval_task_id}: approval required - denied in headless fleet"
            )));
            false
        }),
    )
    .with_guard(Box::new(move |access| match access {
        Access::Write(path) => verdict_to_guard(policy.write_verdict(path)),
        Access::Exec(command) => verdict_to_guard(policy.exec_verdict(command)),
    }));
    let progress_events = events.clone();
    let progress_task_id = job.task_id.clone();
    let progress_scrubber = Scrubber::new(runtime.key_literals.as_ref().clone());
    let mut agent = AgentLoop {
        client,
        tools: builtin_tools(),
        ctx,
        receipts: ReceiptWriter {
            path: runtime.run_root.join(".nosis").join("receipts.jsonl"),
            scrubber: Scrubber::new(runtime.key_literals.as_ref().clone()),
        },
        model_id: route.model_id,
        max_turns: MAX_TURNS,
        thinking: effort_for(route.thinking_dialect),
        constitution: Some(runtime.law.constitution.clone()),
        context_limit: route.context,
        on_event: Some(Box::new(move |line| {
            let line = nh_vault::safe_line(&progress_scrubber, line);
            let _ = progress_events
                .try_send(WorkerEvent::Progress(format!("{progress_task_id}: {line}")));
        })),
    };
    let mut history: Vec<ChatMessage> = Vec::new();
    agent
        .run_with_history(&mut history, &job.task)
        .map(|(_, receipt)| receipt)
        .map_err(|error| error.to_string())
}

fn verdict_to_guard(verdict: nh_law::Verdict) -> Guard {
    match verdict {
        nh_law::Verdict::Allow => Guard::Allow,
        nh_law::Verdict::Ask => Guard::Ask,
        nh_law::Verdict::Block(reason) => Guard::Block(reason),
    }
}

fn prepare_new_tasks(
    resolver: &RouteResolver,
    default_route: &str,
    specs: &[TaskSpec],
) -> anyhow::Result<Vec<PreparedTask>> {
    let mut ids = HashSet::new();
    let mut tasks = Vec::with_capacity(specs.len());
    for (index, spec) in specs.iter().enumerate() {
        let task = spec.task.trim();
        if task.is_empty() {
            bail!("task {} is empty - add a task description", index + 1);
        }
        let task_id = match spec.id.as_deref() {
            Some(id) if id.trim().is_empty() => bail!("task ids cannot be empty"),
            Some(id) => id.to_string(),
            None => format!("t{index:03}-{:08x}", stable_hash(task) as u32),
        };
        if !ids.insert(task_id.clone()) {
            bail!("task id collision - choose unique ids");
        }
        let route_id = spec.model.as_deref().unwrap_or(default_route);
        let route = resolver.resolve(route_id)?;
        if route.class == RouteClass::Delegate {
            bail!("delegate routes are not available to Slice A fleet workers - pick an api route");
        }
        tasks.push(PreparedTask {
            task_id,
            task: task.to_string(),
            route_id: route.id,
            attempt: 1,
        });
    }
    Ok(tasks)
}

fn scrub_prepared_tasks(tasks: &mut [PreparedTask], scrubber: &Scrubber) -> anyhow::Result<()> {
    let mut ids = HashSet::new();
    for task in tasks {
        task.task_id = scrubber.scrub(&task.task_id);
        task.task = scrubber.scrub(&task.task);
        task.route_id = scrubber.scrub(&task.route_id);
        if !ids.insert(task.task_id.clone()) {
            bail!("task ids collide after secret redaction - choose different ids");
        }
    }
    Ok(())
}

fn preflight_keys(
    resolver: &RouteResolver,
    tasks: &[PreparedTask],
    using_test_provider: bool,
) -> anyhow::Result<Vec<String>> {
    if using_test_provider {
        return Ok(Vec::new());
    }
    let vault = EnvFallbackVault {
        inner: KeyringVault,
    };
    let mut entries = BTreeSet::new();
    let mut literals = Vec::new();
    for task in tasks {
        let route = resolver.resolve(&task.route_id)?;
        if entries.insert(route.vault_entry.clone()) {
            let key = vault.get(&route.vault_entry)?;
            literals.push(key.as_str().to_owned());
        }
    }
    Ok(literals)
}

/// TEST-ONLY provider seam used by the kill/resume process test. It is inert
/// unless the exact `NH_FLEET_TEST_PROVIDER=echo` opt-in is present; ordinary
/// runs always take the vault-backed `make_client` path.
fn test_provider_from_env() -> anyhow::Result<Option<TestProvider>> {
    match std::env::var(TEST_PROVIDER_ENV) {
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(error) => Err(anyhow::anyhow!(
            "could not read {TEST_PROVIDER_ENV}: {error}"
        )),
        Ok(value) if value == "echo" => {
            let sleep_ms = std::env::var(TEST_SLEEP_MS_ENV)
                .ok()
                .map(|value| value.parse::<u64>())
                .transpose()
                .context("NH_FLEET_TEST_SLEEP_MS must be a whole number")?
                .unwrap_or(150);
            Ok(Some(TestProvider {
                execution_log: std::env::var_os(TEST_EXECUTION_LOG_ENV).map(PathBuf::from),
                sleep: Duration::from_millis(sleep_ms),
            }))
        }
        Ok(_) => bail!("NH_FLEET_TEST_PROVIDER only accepts the test value 'echo'"),
    }
}

struct EchoClient {
    task_id: String,
    config: TestProvider,
}

impl ChatClient for EchoClient {
    fn complete(&self, _req: &ChatRequest) -> anyhow::Result<ChatResponse> {
        thread::sleep(self.config.sleep);
        if let Some(path) = &self.config.execution_log {
            append_execution_log(path, &self.task_id)?;
        }
        Ok(ChatResponse {
            message: ChatMessage {
                role: "assistant".into(),
                content: Some(format!("echo completed {}", self.task_id)),
                tool_calls: None,
                tool_call_id: None,
                reasoning_content: None,
            },
            finish_reason: "stop".into(),
            usage: Some(Usage {
                prompt_tokens: 1,
                completion_tokens: 1,
                cached_tokens: None,
            }),
        })
    }
}

fn append_execution_log(path: &Path, task_id: &str) -> anyhow::Result<()> {
    let _guard = TEST_LOG_LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("test execution log lock was poisoned"))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{task_id}")?;
    file.flush()?;
    file.sync_all()?;
    Ok(())
}

fn stable_hash(text: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn effort_for(dialect: ThinkingDialect) -> ThinkingEffort {
    match dialect {
        ThinkingDialect::AlwaysThinking | ThinkingDialect::GlmHm => ThinkingEffort::High,
        ThinkingDialect::DeepseekNhm | ThinkingDialect::None => ThinkingEffort::None,
    }
}

fn effort_name(effort: ThinkingEffort) -> &'static str {
    match effort {
        ThinkingEffort::None => "none",
        ThinkingEffort::Low => "low",
        ThinkingEffort::High => "high",
        ThinkingEffort::Max => "max",
    }
}

fn effective_workers(requested: usize, original: Option<usize>) -> anyhow::Result<usize> {
    let workers = if requested == 0 {
        original.unwrap_or(DEFAULT_MAX_WORKERS)
    } else {
        requested
    };
    if workers == 0 {
        bail!("max_workers must be at least 1");
    }
    Ok(workers)
}

#[allow(clippy::type_complexity)]
fn emit(callback: &Option<Arc<dyn Fn(&str) + Send + Sync>>, scrubber: &Scrubber, line: &str) {
    if let Some(callback) = callback {
        callback(&nh_vault::safe_line(scrubber, line));
    }
}

fn now_utc() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn new_run_id() -> String {
    let sequence = RUN_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!(
        "{}-{}-{sequence}",
        Utc::now().format("%Y%m%dT%H%M%S%3fZ"),
        std::process::id()
    )
}

fn fleet_root(run_root: &Path) -> PathBuf {
    run_root.join(".nosis").join("fleet")
}

fn validate_run_id(run_id: &str) -> anyhow::Result<()> {
    if run_id.is_empty()
        || !run_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        bail!("invalid fleet run id");
    }
    Ok(())
}

fn append_index(path: &Path, record: &IndexRecord, literals: &[String]) -> anyhow::Result<()> {
    DurableWriter::open(path, Scrubber::new(literals.to_vec()))?.append(record)
}

fn finish_index(
    fleet_root: &Path,
    created_utc: &str,
    task_count: usize,
    report: &RunReport,
    literals: &[String],
) -> anyhow::Result<()> {
    append_index(
        &fleet_root.join("index.jsonl"),
        &IndexRecord {
            run_id: report.run_id.clone(),
            created_utc: created_utc.to_string(),
            task_count,
            status: "finished".into(),
        },
        literals,
    )
}

fn latest_incomplete_run(index_path: &Path) -> anyhow::Result<String> {
    repair_uncommitted_tail(index_path)?;
    let text = fs::read_to_string(index_path)
        .with_context(|| "no fleet index found - run `nh fleet run <tasks.json>` first")?;
    let mut records = Vec::new();
    for (line_index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let record: IndexRecord = serde_json::from_str(line).with_context(|| {
            format!(
                "fleet index line {} is invalid",
                line_index.saturating_add(1)
            )
        })?;
        records.push(record);
    }
    let mut latest_status = HashMap::new();
    for record in &records {
        latest_status.insert(record.run_id.clone(), record.status.clone());
    }
    records
        .iter()
        .rev()
        .find(|record| {
            latest_status
                .get(&record.run_id)
                .is_some_and(|status| status != "finished")
        })
        .map(|record| record.run_id.clone())
        .ok_or_else(|| anyhow::anyhow!("no incomplete fleet run found"))
}

fn read_ledger(path: &Path) -> anyhow::Result<Vec<LedgerEvent>> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("could not read fleet ledger {}", path.display()))?;
    let mut events = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        events.push(serde_json::from_str(line).with_context(|| {
            format!("fleet ledger line {} is invalid", index.saturating_add(1))
        })?);
    }
    Ok(events)
}

/// A process can die after writing part of an event but before its flush/fsync
/// acknowledgement. Such bytes were never committed. Recovery removes only
/// that non-newline-terminated tail; every committed JSONL event remains
/// append-only and byte-for-byte unchanged.
fn repair_uncommitted_tail(path: &Path) -> anyhow::Result<()> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error).with_context(|| format!("could not inspect {}", path.display()))
        }
    };
    if bytes.is_empty() || bytes.last() == Some(&b'\n') {
        return Ok(());
    }
    let committed_len = bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index.saturating_add(1));
    let file = OpenOptions::new()
        .write(true)
        .open(path)
        .with_context(|| format!("could not recover {}", path.display()))?;
    file.set_len(committed_len as u64)
        .with_context(|| format!("could not recover {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("could not fsync recovered {}", path.display()))?;
    Ok(())
}

fn run_meta(events: &[LedgerEvent], expected_run_id: &str) -> anyhow::Result<RunMeta> {
    events
        .iter()
        .find_map(|event| match event {
            LedgerEvent::RunStarted {
                run_id,
                created_utc,
                task_count,
                max_workers,
                budget_tokens,
            } if run_id == expected_run_id => Some(RunMeta {
                created_utc: created_utc.clone(),
                task_count: *task_count,
                max_workers: *max_workers,
                budget_tokens: *budget_tokens,
            }),
            _ => None,
        })
        .ok_or_else(|| anyhow::anyhow!("fleet ledger has no matching run_started event"))
}

fn queued_tasks(events: &[LedgerEvent]) -> anyhow::Result<BTreeMap<String, (String, String)>> {
    let mut queued = BTreeMap::new();
    for event in events {
        if let LedgerEvent::TaskQueued {
            task_id,
            task,
            route_id,
        } = event
        {
            if queued
                .insert(task_id.clone(), (task.clone(), route_id.clone()))
                .is_some()
            {
                bail!("fleet ledger has duplicate queued task ids");
            }
        }
    }
    Ok(queued)
}

fn attempts_by_task(events: &[LedgerEvent]) -> HashMap<String, u32> {
    let mut attempts: HashMap<String, u32> = HashMap::new();
    for event in events {
        if let LedgerEvent::TaskStarted {
            task_id, attempt, ..
        } = event
        {
            attempts
                .entry(task_id.clone())
                .and_modify(|seen| *seen = (*seen).max(*attempt))
                .or_insert(*attempt);
        }
    }
    attempts
}

fn terminal_counts(events: &[LedgerEvent]) -> Counts {
    let mut counts = Counts::default();
    for event in events {
        match event {
            LedgerEvent::TaskDone { .. } => counts.done += 1,
            LedgerEvent::TaskFailed { .. } => counts.failed += 1,
            LedgerEvent::TaskGate { .. } => counts.gated += 1,
            _ => {}
        }
    }
    counts
}

fn ensure_single_terminal(events: &[LedgerEvent]) -> anyhow::Result<()> {
    let mut terminal = HashSet::new();
    for event in events {
        let task_id = match event {
            LedgerEvent::TaskDone { task_id, .. }
            | LedgerEvent::TaskFailed { task_id, .. }
            | LedgerEvent::TaskGate { task_id, .. } => Some(task_id),
            _ => None,
        };
        if task_id.is_some_and(|task_id| !terminal.insert(task_id.clone())) {
            bail!("fleet ledger has more than one terminal event for a task");
        }
    }
    Ok(())
}

fn has_finished(events: &[LedgerEvent]) -> bool {
    events
        .iter()
        .any(|event| matches!(event, LedgerEvent::RunFinished { .. }))
}

fn receipt_tokens(events: &[LedgerEvent]) -> u64 {
    events
        .iter()
        .filter_map(|event| match event {
            LedgerEvent::TaskReceipt { receipt, .. } => Some(tokens_in(receipt)),
            _ => None,
        })
        .fold(0u64, u64::saturating_add)
}

fn tokens_in(receipt: &Receipt) -> u64 {
    receipt.usage.as_ref().map_or(0, |usage| {
        usage.prompt_tokens.saturating_add(usage.completion_tokens)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nh_law::LoadOptions;
    use std::sync::MutexGuard;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    const CATALOG: &str = r#"
        [routes.echo]
        provider = "echo"
        model_id = "echo-model"
        base_url = "https://example.invalid/v1"
        wire = "openai"
        vault_entry = "echo"
    "#;

    struct TestEnv {
        _guard: MutexGuard<'static, ()>,
        old_provider: Option<std::ffi::OsString>,
        old_sleep: Option<std::ffi::OsString>,
    }

    impl TestEnv {
        fn echo() -> Self {
            let guard = ENV_LOCK.lock().unwrap();
            let old_provider = std::env::var_os(TEST_PROVIDER_ENV);
            let old_sleep = std::env::var_os(TEST_SLEEP_MS_ENV);
            std::env::set_var(TEST_PROVIDER_ENV, "echo");
            std::env::set_var(TEST_SLEEP_MS_ENV, "0");
            Self {
                _guard: guard,
                old_provider,
                old_sleep,
            }
        }
    }

    impl Drop for TestEnv {
        fn drop(&mut self) {
            match self.old_provider.take() {
                Some(value) => std::env::set_var(TEST_PROVIDER_ENV, value),
                None => std::env::remove_var(TEST_PROVIDER_ENV),
            }
            match self.old_sleep.take() {
                Some(value) => std::env::set_var(TEST_SLEEP_MS_ENV, value),
                None => std::env::remove_var(TEST_SLEEP_MS_ENV),
            }
        }
    }

    fn config(root: &Path, tasks: Vec<TaskSpec>) -> FleetConfig {
        FleetConfig {
            resolver: RouteResolver::from_toml(CATALOG).unwrap(),
            law: nh_law::load(root, &LoadOptions { cli_autonomy: None }),
            default_route: "echo".into(),
            tasks,
            max_workers: 2,
            budget_tokens: None,
            run_root: root.to_path_buf(),
            on_event: None,
        }
    }

    fn task(id: &str, text: &str) -> TaskSpec {
        TaskSpec {
            id: Some(id.into()),
            task: text.into(),
            model: None,
        }
    }

    #[test]
    fn resume_plan_mixes_terminal_interrupted_and_queued_exactly_once() {
        let events = vec![
            queued("done"),
            started("done", 1),
            LedgerEvent::TaskDone {
                task_id: "done".into(),
                outcome: Outcome::Pass,
            },
            queued("interrupted"),
            started("interrupted", 1),
            queued("queued"),
            queued("failed"),
            LedgerEvent::TaskFailed {
                task_id: "failed".into(),
                reason: "no".into(),
            },
            started("interrupted", 1),
        ];
        assert_eq!(
            plan_from_ledger(&events),
            ResumePlan {
                done: vec!["done".into(), "failed".into()],
                todo: vec!["interrupted".into(), "queued".into()],
            }
        );
    }

    #[test]
    fn derived_ids_are_stable_and_index_scoped() {
        let resolver = RouteResolver::from_toml(CATALOG).unwrap();
        let specs = vec![
            TaskSpec {
                id: None,
                task: "  same task  ".into(),
                model: None,
            },
            TaskSpec {
                id: None,
                task: "same task".into(),
                model: None,
            },
        ];
        let first = prepare_new_tasks(&resolver, "echo", &specs).unwrap();
        let second = prepare_new_tasks(&resolver, "echo", &specs).unwrap();
        assert_eq!(first[0].task_id, second[0].task_id);
        assert!(first[0].task_id.starts_with("t000-"));
        assert!(first[1].task_id.starts_with("t001-"));
        assert_eq!(&first[0].task_id[5..], &first[1].task_id[5..]);
    }

    #[test]
    fn explicit_id_collisions_fail_before_run() {
        let resolver = RouteResolver::from_toml(CATALOG).unwrap();
        let specs = vec![task("same", "one"), task("same", "two")];
        let error = prepare_new_tasks(&resolver, "echo", &specs).unwrap_err();
        assert!(error.to_string().contains("collision"));
    }

    #[test]
    fn completed_run_is_durable_parseable_and_has_one_terminal_per_task() {
        let _env = TestEnv::echo();
        let tmp = tempfile::tempdir().unwrap();
        let report = run(config(
            tmp.path(),
            vec![task("one", "first"), task("two", "second")],
        ))
        .unwrap();
        assert_eq!(report.done, 2);
        let path = fleet_root(tmp.path())
            .join(&report.run_id)
            .join("ledger.jsonl");
        let events = read_ledger(&path).unwrap();
        assert!(matches!(
            events.last(),
            Some(LedgerEvent::RunFinished { .. })
        ));
        ensure_single_terminal(&events).unwrap();
        assert_eq!(terminal_counts(&events).done, 2);
        for line in fs::read_to_string(path).unwrap().lines() {
            let event: LedgerEvent = serde_json::from_str(line).unwrap();
            let encoded = serde_json::to_string(&event).unwrap();
            let _: LedgerEvent = serde_json::from_str(&encoded).unwrap();
        }
    }

    #[test]
    fn ledger_scrubber_redacts_fake_key_literals() {
        let _env = TestEnv::echo();
        let tmp = tempfile::tempdir().unwrap();
        let report = run(config(
            tmp.path(),
            vec![task("secret", "do not persist sk-test-00000000")],
        ))
        .unwrap();
        let text = fs::read_to_string(
            fleet_root(tmp.path())
                .join(report.run_id)
                .join("ledger.jsonl"),
        )
        .unwrap();
        assert!(!text.contains("sk-test-00000000"));
        assert!(text.contains("[REDACTED]"));
    }

    #[test]
    fn budget_halts_new_dispatch_and_terminals_every_task() {
        let _env = TestEnv::echo();
        let tmp = tempfile::tempdir().unwrap();
        let mut fleet = config(
            tmp.path(),
            vec![
                task("one", "first"),
                task("two", "second"),
                task("three", "third"),
            ],
        );
        fleet.max_workers = 1;
        fleet.budget_tokens = Some(2);
        let report = run(fleet).unwrap();
        assert_eq!(report.done, 1);
        assert_eq!(report.failed, 2);
        let events = read_ledger(
            &fleet_root(tmp.path())
                .join(report.run_id)
                .join("ledger.jsonl"),
        )
        .unwrap();
        ensure_single_terminal(&events).unwrap();
    }

    #[test]
    fn recovery_discards_only_a_torn_uncommitted_tail() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("ledger.jsonl");
        let committed = serde_json::to_string(&queued("safe")).unwrap();
        fs::write(&path, format!("{committed}\n{{\"event\":\"task_sta")).unwrap();
        repair_uncommitted_tail(&path).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), format!("{committed}\n"));
        let events = read_ledger(&path).unwrap();
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            LedgerEvent::TaskQueued { task_id, .. } if task_id == "safe"
        ));
    }

    fn queued(id: &str) -> LedgerEvent {
        LedgerEvent::TaskQueued {
            task_id: id.into(),
            task: id.into(),
            route_id: "echo".into(),
        }
    }

    fn started(id: &str, attempt: u32) -> LedgerEvent {
        LedgerEvent::TaskStarted {
            task_id: id.into(),
            route_id: "echo".into(),
            effort: "none".into(),
            attempt,
        }
    }
}
