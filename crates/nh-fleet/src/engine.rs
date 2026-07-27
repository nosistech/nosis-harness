//! Durable execution engine: coordinator lock, worker pool, scheduling, and task runs.

use super::*;

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
    pub(super) test_provider: Option<TestProvider>,
    pub(super) clock: Arc<dyn Clock>,
    pub(super) swarm: Arc<dyn SwarmClient>,
}

#[derive(Clone)]
pub(super) struct TestProvider {
    pub(super) execution_log: Option<PathBuf>,
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

#[allow(dead_code)]
pub(super) struct RunLock {
    pub(super) file: File,
    pub(super) path: PathBuf,
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
        Ok(Self { file, path })
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
            anyhow::anyhow!("run appears live (pid {pid}, started {started}) — refusing to resume")
        }
        _ => anyhow::anyhow!("run appears live — refusing to resume"),
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
        result: Result<Receipt, String>,
    },
}

pub(super) struct WorkerPool {
    pub(super) job_tx: Option<mpsc::Sender<PreparedTask>>,
    pub(super) workers: Vec<JoinHandle<()>>,
}

impl WorkerPool {
    pub(super) fn sender(&self) -> &mpsc::Sender<PreparedTask> {
        self.job_tx
            .as_ref()
            .expect("job sender present until finish/drop")
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

#[allow(clippy::too_many_arguments)]
#[allow(clippy::type_complexity)]
pub(super) fn execute_tasks(
    run_id: &str,
    tasks: Vec<PreparedTask>,
    max_workers: usize,
    budget_tokens: Option<u64>,
    mut used_tokens: u64,
    mut counts: Counts,
    runtime: Arc<Runtime>,
    ledger: &DurableWriter,
    on_event: &Option<Arc<dyn Fn(&str) + Send + Sync>>,
    ladder: Option<&Ladder>,
    escalate_on_partial: bool,
    mut failed_attempts: HashMap<String, u32>,
) -> anyhow::Result<RunReport> {
    let display_scrubber = runtime.key_literals.scrubber();
    let mut remaining: VecDeque<PreparedTask> = tasks.into();
    let mut deferred_announced = HashSet::new();
    let worker_count = max_workers.min(remaining.len().max(1));
    let (job_tx, job_rx) = mpsc::channel::<PreparedTask>();
    let job_rx = Arc::new(Mutex::new(job_rx));
    let event_capacity = worker_count.saturating_mul(4).max(16);
    let (event_tx, pending_event_rx) = mpsc::sync_channel::<WorkerEvent>(event_capacity);
    let workers = spawn_workers(
        worker_count,
        Arc::clone(&job_rx),
        event_tx,
        Arc::clone(&runtime),
    )?;
    let pool = WorkerPool {
        job_tx: Some(job_tx),
        workers,
    };
    // The receiver must drop before the pool on an early return. That releases
    // workers waiting for a start acknowledgement or blocked on an event send
    // before WorkerPool::drop joins them.
    let event_rx = pending_event_rx;

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
        dispatch_ready_tasks(
            &mut remaining,
            pool.sender(),
            &mut active,
            worker_count,
            &runtime,
            on_event,
            &display_scrubber,
            &mut deferred_announced,
        )?;
    }

    while active > 0 || !remaining.is_empty() {
        match event_rx.recv_timeout(SCHEDULER_WAKE_INTERVAL) {
            Ok(WorkerEvent::Started {
                task_id,
                route_id,
                effort,
                attempt,
                ack,
            }) => {
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
                        &format!("running {task_id} — attempt {attempt}"),
                    );
                }
                let failed = result.as_ref().err().cloned();
                let _ = ack.send(result);
                if let Some(error) = failed {
                    bail!("{error}");
                }
            }
            Ok(WorkerEvent::Heartbeat { task_id, ts }) => {
                ledger.append(&LedgerEvent::TaskHeartbeat { task_id, ts })?;
            }
            Ok(WorkerEvent::Progress(line)) => emit(on_event, &display_scrubber, &line),
            Ok(WorkerEvent::Finished { mut job, result }) => {
                active = active.saturating_sub(1);
                match result {
                    Ok(receipt) => {
                        if budget_tokens.is_some() && receipt.usage.is_none() {
                            emit(
                                on_event,
                                &display_scrubber,
                                &format!("usage unreported — budget cannot count {}", job.task_id),
                            );
                        }
                        used_tokens = used_tokens.saturating_add(tokens_in(&receipt));
                        let outcome = receipt.outcome;
                        let reason = typed_receipt_reason(&receipt, job.attempt);
                        ledger.append(&LedgerEvent::TaskReceipt {
                            task_id: job.task_id.clone(),
                            attempt: job.attempt,
                            receipt,
                        })?;
                        let policy_outcome = escalation_outcome(outcome, escalate_on_partial);
                        if matches!(policy_outcome, Outcome::Fail | Outcome::Timeout) {
                            failed_attempts
                                .entry(job.task_id.clone())
                                .and_modify(|count| *count = count.saturating_add(1))
                                .or_insert(1);
                        }
                        let step = ladder
                            .map(|ladder| {
                                next_step(ladder, job.tier_idx, job.attempt, policy_outcome)
                            })
                            .or(match policy_outcome {
                                Outcome::Pass | Outcome::Partial | Outcome::Skip => {
                                    Some(Step::Done)
                                }
                                Outcome::Fail | Outcome::Timeout => None,
                            });
                        match step {
                            Some(Step::Done) => {
                                ledger.append(&LedgerEvent::TaskDone {
                                    task_id: job.task_id.clone(),
                                    outcome,
                                })?;
                                counts.done += 1;
                                emit(
                                    on_event,
                                    &display_scrubber,
                                    &format!("done {} — {outcome:?}", job.task_id),
                                );
                            }
                            Some(Step::Retry) => {
                                job.attempt = job.attempt.saturating_add(1);
                                remaining.push_back(job);
                            }
                            Some(Step::Escalate(next_tier_idx)) => {
                                let ladder = ladder.expect("escalation steps require a ladder");
                                let tier = ladder.tiers().get(next_tier_idx).ok_or_else(|| {
                                    anyhow::anyhow!("escalation ladder selected a missing tier")
                                })?;
                                ledger.append(&LedgerEvent::TaskEscalated {
                                    task_id: job.task_id.clone(),
                                    from_route: job.route_id.clone(),
                                    to_route: tier.route_id.clone(),
                                    reason: reason.clone(),
                                })?;
                                let from_route = runtime.resolver.resolve(&job.route_id)?;
                                let from_effort =
                                    effort_name(job.effort.unwrap_or_else(|| {
                                        effort_for(from_route.thinking_dialect())
                                    }));
                                emit(
                                    on_event,
                                    &display_scrubber,
                                    &format!(
                                        "escalated {} — {}/{} → {}/{} ({reason})",
                                        job.task_id,
                                        job.route_id,
                                        from_effort,
                                        tier.route_id,
                                        effort_name(tier.effort)
                                    ),
                                );
                                job.route_id = tier.route_id.clone();
                                job.tier_idx = next_tier_idx;
                                job.effort = Some(tier.effort);
                                job.attempt = 1;
                                remaining.push_back(job);
                            }
                            Some(Step::Gate) => {
                                let failed_attempts =
                                    failed_attempts.get(&job.task_id).copied().unwrap_or(0);
                                ledger.append(&LedgerEvent::TaskGate {
                                    task_id: job.task_id.clone(),
                                    reason: format!(
                                        "ladder exhausted — Opus review-pause after {failed_attempts} failed attempts"
                                    ),
                                })?;
                                counts.gated += 1;
                                emit(
                                    on_event,
                                    &display_scrubber,
                                    &format!(
                                        "gated {} — ladder exhausted, needs Opus review ({failed_attempts} failed attempts)",
                                        job.task_id
                                    ),
                                );
                            }
                            None => {
                                ledger.append(&LedgerEvent::TaskFailed {
                                    task_id: job.task_id.clone(),
                                    reason: reason.clone(),
                                })?;
                                counts.failed += 1;
                                emit(
                                    on_event,
                                    &display_scrubber,
                                    &format!("failed {} — {reason}", job.task_id),
                                );
                            }
                        }
                    }
                    Err(reason) => {
                        // Infrastructure faults terminate immediately; the ladder only climbs on receipts.
                        ledger.append(&LedgerEvent::TaskFailed {
                            task_id: job.task_id.clone(),
                            reason: reason.clone(),
                        })?;
                        counts.failed += 1;
                        emit(
                            on_event,
                            &display_scrubber,
                            &format!("failed {} — {reason}", job.task_id),
                        );
                    }
                }

                if budget_tokens.is_some_and(|limit| used_tokens >= limit) {
                    if !budget_halted {
                        budget_halted = true;
                        emit(
                            on_event,
                            &display_scrubber,
                            &format!(
                                "budget halted at {used_tokens} tokens — no new tasks will start"
                            ),
                        );
                    }
                    halt_remaining_for_budget(
                        &mut remaining,
                        ledger,
                        on_event,
                        &display_scrubber,
                        &mut counts,
                    )?;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                bail!("fleet worker pool stopped unexpectedly")
            }
        }
        if !budget_halted {
            dispatch_ready_tasks(
                &mut remaining,
                pool.sender(),
                &mut active,
                worker_count,
                &runtime,
                on_event,
                &display_scrubber,
                &mut deferred_announced,
            )?;
        }
    }

    pool.finish()?;
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
            "fleet {} finished — {} done, {} failed, {} gated",
            report.run_id, report.done, report.failed, report.gated
        ),
    );
    Ok(report)
}

#[allow(clippy::too_many_arguments)]
#[allow(clippy::type_complexity)]
pub(super) fn dispatch_ready_tasks(
    remaining: &mut VecDeque<PreparedTask>,
    jobs: &mpsc::Sender<PreparedTask>,
    active: &mut usize,
    worker_count: usize,
    runtime: &Runtime,
    on_event: &Option<Arc<dyn Fn(&str) + Send + Sync>>,
    scrubber: &Scrubber,
    deferred_announced: &mut HashSet<String>,
) -> anyhow::Result<()> {
    let candidates = remaining.len();
    for _ in 0..candidates {
        if *active >= worker_count {
            break;
        }
        let Some(task) = remaining.pop_front() else {
            break;
        };
        if task.defer_offpeak {
            let now = runtime.clock.now();
            if let Ok(route) = runtime.resolver.resolve(&task.route_id) {
                if !ready_to_dispatch(&route, now) {
                    if deferred_announced.insert(task.task_id.clone()) {
                        let local: FixedOffset = *Local::now().offset();
                        emit(
                            on_event,
                            scrubber,
                            &format!(
                                "deferred {} — {}, parked",
                                task.task_id,
                                route.peak_status(now, local)
                            ),
                        );
                    }
                    remaining.push_back(task);
                    continue;
                }
            }
        }
        jobs.send(task)
            .context("fleet worker pool stopped before dispatch")?;
        *active = active.saturating_add(1);
    }
    Ok(())
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

#[allow(clippy::type_complexity)]
pub(super) fn halt_remaining_for_budget(
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
            &format!("failed {} — budget halted before dispatch", task.task_id),
        );
    }
    Ok(())
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
                    result: Err(error.to_string()),
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
        if events.send(WorkerEvent::Finished { job, result }).is_err() {
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
    let client: Box<dyn ChatClient> = match &runtime.test_provider {
        Some(provider) => Box::new(EchoClient {
            task_id: job.task_id.clone(),
            config: provider.clone(),
        }),
        None => {
            let vault = EnvFallbackVault {
                inner: KeyringVault,
            };
            credential::connect(
                &vault,
                &route,
                &runtime.law.policy.approved_audiences(route.vault_entry()),
                Some(FLEET_OUTPUT_CAP),
            )
            .map(|(client, _)| client)
            .map_err(|error| error.to_string())?
        }
    };
    let policy = runtime.law.policy.clone();
    let approval_events = events.clone();
    let approval_task_id = job.task_id.clone();
    let ctx = ToolCtx::new(
        runtime.workdir.clone(),
        Box::new(move |_| {
            let _ = approval_events.try_send(WorkerEvent::Progress(format!(
                "{approval_task_id}: approval required — denied in headless fleet"
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
    let mut agent = AgentLoop {
        client,
        tools: builtin_tools(),
        ctx,
        receipts: ReceiptWriter::project(runtime.run_root.clone(), runtime.key_literals.scrubber()),
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
    let mut receipt = agent
        .run_with_history(&mut history, &job.task)
        .map(|(_, receipt)| receipt)
        .map_err(|error| error.to_string())?;
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

pub(super) fn verdict_to_guard(verdict: nh_law::Verdict) -> Guard {
    match verdict {
        nh_law::Verdict::Allow => Guard::Allow,
        nh_law::Verdict::Ask => Guard::Ask,
        nh_law::Verdict::Block(reason) => Guard::Block(reason),
    }
}
