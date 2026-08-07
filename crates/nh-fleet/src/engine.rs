//! Durable execution engine: coordinator lock, worker pool, scheduling, and task runs.

use crate::ledger::now_utc;
use crate::model::{Backend, Clock, LedgerEvent, RunReport, SwarmClient};
#[cfg(feature = "test-provider")]
use crate::prepare::EchoClient;
use crate::prepare::{effort_for, effort_name};
use crate::{FLEET_OUTPUT_CAP, HEARTBEAT_INTERVAL, LOCK_RETRY_INTERVAL, MAX_TURNS};
use anyhow::{bail, Context as _};
use nh_core::agent::{identity_constitution, AgentLoop};
use nh_core::credential;
use nh_core::receipt::{FailureClass, Outcome, Receipt, ReceiptWriter};
use nh_core::wire::{ChatClient, ChatMessage, ThinkingEffort};
use nh_law::Law;
use nh_routes::RouteResolver;
use nh_tools::{builtin_tools, Access, Guard, ToolCtx};
use nh_vault::{EnvFallbackVault, KeyringVault, Scrubber, SecretRegistry};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub(super) struct PreparedTask {
    pub(super) task_id: String,
    pub(super) task: String,
    pub(super) route_id: String,
    pub(super) attempt: u32,
    pub(super) tier_idx: usize,
    pub(super) effort: Option<ThinkingEffort>,
    pub(super) defer_offpeak: bool,
    pub(super) backend: Backend,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct QueuedTask {
    pub(super) task: String,
    pub(super) route_id: String,
    pub(super) defer_offpeak: bool,
    pub(super) backend: Backend,
}

pub(super) struct Runtime {
    pub(super) resolver: Arc<RouteResolver>,
    pub(super) law: Arc<Law>,
    pub(super) run_root: PathBuf,
    pub(super) workdir: PathBuf,
    pub(super) key_literals: SecretRegistry,
    #[cfg(feature = "test-provider")]
    pub(super) test_provider: Option<TestProvider>,
    pub(super) clock: Arc<dyn Clock>,
    pub(super) swarm: Arc<dyn SwarmClient>,
}

#[derive(Clone)]
#[cfg(feature = "test-provider")]
pub(super) struct TestProvider {
    pub(super) execution_log: Option<PathBuf>,
    pub(super) receipt_root: PathBuf,
    pub(super) sleep: Duration,
    pub(super) outcome: Outcome,
}

#[derive(Default, Clone, Copy)]
pub(super) struct Counts {
    pub(super) done: usize,
    pub(super) failed: usize,
    pub(super) gated: usize,
}

impl Counts {
    pub(super) fn report(self, run_id: String) -> RunReport {
        RunReport {
            run_id,
            done: self.done,
            failed: self.failed,
            gated: self.gated,
        }
    }
}

pub(super) struct RunMeta {
    pub(super) created_utc: String,
    pub(super) task_count: usize,
    pub(super) max_workers: usize,
    pub(super) budget_tokens: Option<u64>,
    pub(super) escalate: bool,
}

#[derive(Serialize, Deserialize)]
pub(super) struct IndexRecord {
    pub(super) run_id: String,
    pub(super) created_utc: String,
    pub(super) task_count: usize,
    pub(super) status: String,
}

pub(super) struct RunLock {
    _file: File,
}

impl RunLock {
    pub(super) fn acquire(run_dir: &Path, retry_window: Duration) -> anyhow::Result<Self> {
        fs::create_dir_all(run_dir)
            .with_context(|| format!("could not create {}", run_dir.display()))?;
        let path = run_dir.join("coordinator.lock");
        nh_core::runtime_path::reject_symlink_or_special_file(&path, "fleet coordinator lock")?;
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .with_context(|| format!("could not open {}", path.display()))?;
        let started = Instant::now();
        loop {
            match file.try_lock() {
                Ok(()) => break,
                Err(TryLockError::WouldBlock) if started.elapsed() < retry_window => {
                    thread::sleep(
                        LOCK_RETRY_INTERVAL.min(retry_window.saturating_sub(started.elapsed())),
                    );
                }
                Err(TryLockError::WouldBlock) => return Err(live_run_error(&path)),
                Err(TryLockError::Error(error)) => {
                    return Err(error).with_context(|| format!("could not lock {}", path.display()))
                }
            }
        }

        file.set_len(0)
            .with_context(|| format!("could not update {}", path.display()))?;
        writeln!(file, "pid={}", std::process::id())
            .with_context(|| format!("could not update {}", path.display()))?;
        writeln!(file, "started={}", now_utc())
            .with_context(|| format!("could not update {}", path.display()))?;
        file.flush()
            .with_context(|| format!("could not flush {}", path.display()))?;
        Ok(Self { _file: file })
    }
}

pub(super) fn live_run_error(path: &Path) -> anyhow::Error {
    let diagnostics = fs::read_to_string(path).ok();
    let pid = diagnostics.as_deref().and_then(|text| {
        text.lines()
            .find_map(|line| line.strip_prefix("pid=").map(str::to_string))
    });
    let started = diagnostics.as_deref().and_then(|text| {
        text.lines()
            .find_map(|line| line.strip_prefix("started=").map(str::to_string))
    });
    match (pid, started) {
        (Some(pid), Some(started)) => {
            anyhow::anyhow!("run appears live (pid {pid}, started {started}) - refusing to resume")
        }
        _ => anyhow::anyhow!("run appears live - refusing to resume"),
    }
}

pub(super) struct DurableWriter {
    pub(super) path: PathBuf,
    pub(super) file: Mutex<File>,
    pub(super) scrubber: Scrubber,
}

impl DurableWriter {
    pub(super) fn open(path: &Path, scrubber: Scrubber) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("could not create {}", parent.display()))?;
        }
        nh_core::runtime_path::reject_symlink_or_special_file(path, "fleet ledger")?;
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

    pub(super) fn append<T: Serialize>(&self, value: &T) -> anyhow::Result<()> {
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

pub(super) fn append_run_failed(
    path: &Path,
    run_id: &str,
    error: &anyhow::Error,
    literals: &SecretRegistry,
) -> anyhow::Result<()> {
    DurableWriter::open(path, literals.scrubber()).and_then(|ledger| {
        ledger.append(&LedgerEvent::RunFailed {
            run_id: run_id.to_string(),
            reason: error.to_string(),
        })
    })
}

pub(super) enum WorkerEvent {
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
        job: PreparedTask,
        result: Box<Result<Receipt, String>>,
    },
}

pub(super) struct WorkerPool {
    pub(super) job_tx: Option<mpsc::Sender<PreparedTask>>,
    pub(super) workers: Vec<JoinHandle<()>>,
}

impl WorkerPool {
    pub(super) fn sender(&self) -> anyhow::Result<&mpsc::Sender<PreparedTask>> {
        self.job_tx
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("fleet worker pool is already closed"))
    }

    pub(super) fn finish(mut self) -> anyhow::Result<()> {
        self.job_tx = None;
        let workers = std::mem::take(&mut self.workers);
        join_workers(workers)
    }
}

impl Drop for WorkerPool {
    fn drop(&mut self) {
        self.job_tx = None;
        for worker in std::mem::take(&mut self.workers) {
            let _ = worker.join();
        }
    }
}

pub(super) fn escalation_outcome(outcome: Outcome, escalate_on_partial: bool) -> Outcome {
    if escalate_on_partial && outcome == Outcome::Partial {
        Outcome::Fail
    } else {
        outcome
    }
}

pub(super) fn typed_receipt_reason(receipt: &Receipt, attempt: u32) -> String {
    let outcome = match receipt.outcome {
        Outcome::Pass => "pass",
        Outcome::Fail => "fail",
        Outcome::Partial => "partial",
        Outcome::Skip => "skip",
        Outcome::Timeout => "timeout",
    };
    let class = receipt.failure_class.map(|class| match class {
        FailureClass::Context => "context",
        FailureClass::Constraint => "constraint",
        FailureClass::Filtered => "filtered",
        FailureClass::Verification => "verification",
        FailureClass::Planning => "planning",
        FailureClass::Unreceipted => "unreceipted",
    });
    let tries = if attempt == 1 { "try" } else { "tries" };
    match class {
        Some(class) => format!("{outcome} ({class}) after {attempt} {tries}"),
        None => format!("{outcome} after {attempt} {tries}"),
    }
}

pub(super) fn spawn_workers(
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

pub(super) fn join_workers(workers: Vec<JoinHandle<()>>) -> anyhow::Result<()> {
    let mut panicked = false;
    for worker in workers {
        if worker.join().is_err() {
            panicked = true;
        }
    }
    if panicked {
        bail!("a fleet worker panicked");
    }
    Ok(())
}

pub(super) fn worker_loop(
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
                    job,
                    result: Box::new(Err(error.to_string())),
                });
                continue;
            }
        };
        let actual_effort = job
            .effort
            .unwrap_or_else(|| effort_for(route.thinking_dialect()));
        let (ack_tx, ack_rx) = mpsc::sync_channel(0);
        if events
            .send(WorkerEvent::Started {
                task_id: job.task_id.clone(),
                route_id: job.route_id.clone(),
                effort: effort_name(actual_effort).into(),
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
                job,
                result: Box::new(result),
            })
            .is_err()
        {
            return;
        }
    }
}

pub(super) fn run_one_task(
    runtime: &Runtime,
    job: &PreparedTask,
    route: nh_routes::ResolvedRoute,
    events: &mpsc::SyncSender<WorkerEvent>,
) -> Result<Receipt, String> {
    if job.backend == Backend::KimiSwarm {
        return runtime
            .swarm
            .submit_and_collect(&job.task_id, &job.task)
            .map_err(|error| error.to_string());
    }
    let client = client_for_task(runtime, job, &route)?;
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
    .with_scrubber(runtime.key_literals.scrubber())
    .with_guard(Box::new(move |access| match access {
        Access::Read(path) => verdict_to_guard(policy.read_verdict(path)),
        Access::Write(path) => verdict_to_guard(policy.write_verdict(path)),
        Access::Exec(command) => verdict_to_guard(policy.exec_verdict(command)),
        Access::Send(target) => verdict_to_guard(policy.send_verdict(target)),
    }));
    let progress_events = events.clone();
    let progress_task_id = job.task_id.clone();
    let progress_scrubber = runtime.key_literals.scrubber();
    let receipt_root = runtime.run_root.clone();
    #[cfg(feature = "test-provider")]
    let receipt_root = runtime
        .test_provider
        .as_ref()
        .map_or(receipt_root, |provider| provider.receipt_root.clone());
    let mut agent = AgentLoop {
        client,
        tools: builtin_tools(),
        ctx,
        receipts: ReceiptWriter::project(receipt_root, runtime.key_literals.scrubber()),
        model_id: route.model_id().to_owned(),
        max_turns: MAX_TURNS,
        thinking: job
            .effort
            .unwrap_or_else(|| effort_for(route.thinking_dialect())),
        profile: None,
        constitution: Some(identity_constitution(
            &runtime.law.constitution,
            route.id(),
            route.provider(),
        )),
        context_limit: route.context(),
        on_event: Some(Box::new(move |line| {
            let line = nh_vault::safe_line(&progress_scrubber, line);
            let _ = progress_events
                .try_send(WorkerEvent::Progress(format!("{progress_task_id}: {line}")));
        })),
    };
    let mut history: Vec<ChatMessage> = Vec::new();
    let receipt = agent
        .run_with_history(&mut history, &job.task)
        .map(|(_, receipt)| receipt)
        .map_err(|error| error.to_string())?;
    #[cfg(feature = "test-provider")]
    let mut receipt = receipt;
    #[cfg(feature = "test-provider")]
    if let Some(provider) = &runtime.test_provider {
        // The echo seam is inert outside its explicit opt-in and only controls test receipts.
        receipt.outcome = provider.outcome;
        receipt.failure_class = match provider.outcome {
            Outcome::Fail => Some(FailureClass::Verification),
            Outcome::Timeout => Some(FailureClass::Constraint),
            Outcome::Pass | Outcome::Partial | Outcome::Skip => None,
        };
    }
    Ok(receipt)
}

fn client_for_task(
    runtime: &Runtime,
    job: &PreparedTask,
    route: &nh_routes::ResolvedRoute,
) -> Result<Box<dyn ChatClient>, String> {
    #[cfg(feature = "test-provider")]
    if let Some(provider) = &runtime.test_provider {
        return Ok(Box::new(EchoClient {
            task_id: job.task_id.clone(),
            config: provider.clone(),
        }));
    }
    #[cfg(not(feature = "test-provider"))]
    let _ = job;
    let vault = EnvFallbackVault {
        inner: KeyringVault,
    };
    credential::connect(
        &vault,
        route,
        &runtime.law.policy.approved_audiences(route.vault_entry()),
        Some(FLEET_OUTPUT_CAP),
    )
    .map(|(client, _)| client)
    .map_err(|error| error.to_string())
}

pub(super) fn verdict_to_guard(verdict: nh_law::Verdict) -> Guard {
    match verdict {
        nh_law::Verdict::Allow => Guard::Allow,
        nh_law::Verdict::Ask => Guard::Ask,
        nh_law::Verdict::Block(reason) => Guard::Block(reason),
    }
}
