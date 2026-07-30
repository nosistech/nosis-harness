//! Fleet coordinator state machine.
//!
//! This module owns scheduling and ledger transitions. Worker construction and
//! task execution stay in `engine`; durable parsing and recovery stay in
//! `ledger`.

use crate::engine::{
    escalation_outcome, spawn_workers, typed_receipt_reason, Counts, DurableWriter, PreparedTask,
    Runtime, WorkerEvent, WorkerPool,
};
use crate::ledger::tokens_in;
use crate::model::{
    next_step, ready_to_dispatch, EventCallback, Ladder, LedgerEvent, RunReport, Step,
};
use crate::prepare::{effort_for, effort_name, emit};
use crate::SCHEDULER_WAKE_INTERVAL;
use anyhow::{bail, Context as _};
use chrono::{FixedOffset, Local};
use nh_core::receipt::{Outcome, Receipt};
use nh_vault::Scrubber;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{mpsc, Arc, Mutex};

pub(super) struct ExecutionRequest<'a> {
    pub(super) run_id: &'a str,
    pub(super) tasks: Vec<PreparedTask>,
    pub(super) max_workers: usize,
    pub(super) budget_tokens: Option<u64>,
    pub(super) used_tokens: u64,
    pub(super) counts: Counts,
    pub(super) runtime: Arc<Runtime>,
    pub(super) ledger: &'a DurableWriter,
    pub(super) on_event: &'a Option<EventCallback>,
    pub(super) ladder: Option<&'a Ladder>,
    pub(super) escalate_on_partial: bool,
    pub(super) failed_attempts: HashMap<String, u32>,
}

pub(super) fn execute_tasks(request: ExecutionRequest<'_>) -> anyhow::Result<RunReport> {
    let worker_count = request.max_workers.min(request.tasks.len().max(1));
    let (job_tx, job_rx) = mpsc::channel::<PreparedTask>();
    let job_rx = Arc::new(Mutex::new(job_rx));
    let event_capacity = worker_count.saturating_mul(4).max(16);
    let (event_tx, pending_event_rx) = mpsc::sync_channel::<WorkerEvent>(event_capacity);
    let workers = spawn_workers(
        worker_count,
        Arc::clone(&job_rx),
        event_tx,
        Arc::clone(&request.runtime),
    )?;
    let pool = WorkerPool {
        job_tx: Some(job_tx),
        workers,
    };

    // The receiver must drop before the pool on an early return. That releases
    // workers waiting for a start acknowledgement or blocked on an event send
    // before WorkerPool::drop joins them.
    let event_rx = pending_event_rx;
    let mut scheduler = Scheduler::new(request, worker_count);
    let jobs = pool.sender()?;
    scheduler.start(jobs)?;
    scheduler.run_until_complete(&event_rx, jobs)?;
    pool.finish()?;
    scheduler.finish()
}

struct Scheduler<'a> {
    run_id: &'a str,
    remaining: VecDeque<PreparedTask>,
    worker_count: usize,
    budget_tokens: Option<u64>,
    used_tokens: u64,
    counts: Counts,
    runtime: Arc<Runtime>,
    ledger: &'a DurableWriter,
    on_event: &'a Option<EventCallback>,
    ladder: Option<&'a Ladder>,
    escalate_on_partial: bool,
    failed_attempts: HashMap<String, u32>,
    display_scrubber: Scrubber,
    deferred_announced: HashSet<String>,
    active: usize,
    budget_halted: bool,
}

impl<'a> Scheduler<'a> {
    fn new(request: ExecutionRequest<'a>, worker_count: usize) -> Self {
        let display_scrubber = request.runtime.key_literals.scrubber();
        let budget_halted = request
            .budget_tokens
            .is_some_and(|limit| request.used_tokens >= limit);
        Self {
            run_id: request.run_id,
            remaining: request.tasks.into(),
            worker_count,
            budget_tokens: request.budget_tokens,
            used_tokens: request.used_tokens,
            counts: request.counts,
            runtime: request.runtime,
            ledger: request.ledger,
            on_event: request.on_event,
            ladder: request.ladder,
            escalate_on_partial: request.escalate_on_partial,
            failed_attempts: request.failed_attempts,
            display_scrubber,
            deferred_announced: HashSet::new(),
            active: 0,
            budget_halted,
        }
    }

    fn start(&mut self, jobs: &mpsc::Sender<PreparedTask>) -> anyhow::Result<()> {
        if self.budget_halted {
            self.halt_remaining()
        } else {
            self.dispatch(jobs)
        }
    }

    fn run_until_complete(
        &mut self,
        events: &mpsc::Receiver<WorkerEvent>,
        jobs: &mpsc::Sender<PreparedTask>,
    ) -> anyhow::Result<()> {
        while self.active > 0 || !self.remaining.is_empty() {
            match events.recv_timeout(SCHEDULER_WAKE_INTERVAL) {
                Ok(event) => self.handle_event(event)?,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    bail!("fleet worker pool stopped unexpectedly")
                }
            }
            if !self.budget_halted {
                self.dispatch(jobs)?;
            }
        }
        Ok(())
    }

    fn handle_event(&mut self, event: WorkerEvent) -> anyhow::Result<()> {
        match event {
            WorkerEvent::Started {
                task_id,
                route_id,
                effort,
                attempt,
                ack,
            } => self.handle_started(task_id, route_id, effort, attempt, ack),
            WorkerEvent::Heartbeat { task_id, ts } => self
                .ledger
                .append(&LedgerEvent::TaskHeartbeat { task_id, ts }),
            WorkerEvent::Progress(line) => {
                self.emit(&line);
                Ok(())
            }
            WorkerEvent::Finished { job, result } => self.handle_finished(job, result),
        }
    }

    fn handle_started(
        &self,
        task_id: String,
        route_id: String,
        effort: String,
        attempt: u32,
        ack: mpsc::SyncSender<Result<(), String>>,
    ) -> anyhow::Result<()> {
        let result = self
            .ledger
            .append(&LedgerEvent::TaskStarted {
                task_id: task_id.clone(),
                route_id,
                effort,
                attempt,
            })
            .map_err(|error| error.to_string());
        if result.is_ok() {
            self.emit(&format!("running {task_id} - attempt {attempt}"));
        }
        let failed = result.as_ref().err().cloned();
        let _ = ack.send(result);
        if let Some(error) = failed {
            bail!("{error}");
        }
        Ok(())
    }

    fn handle_finished(
        &mut self,
        job: PreparedTask,
        result: Result<Receipt, String>,
    ) -> anyhow::Result<()> {
        self.active = self.active.saturating_sub(1);
        match result {
            Ok(receipt) => self.handle_receipt(job, receipt)?,
            Err(reason) => self.fail_task(&job.task_id, &reason)?,
        }
        self.enforce_budget()
    }

    fn handle_receipt(&mut self, job: PreparedTask, receipt: Receipt) -> anyhow::Result<()> {
        if self.budget_tokens.is_some() && receipt.usage.is_none() {
            self.emit(&format!(
                "usage unreported - budget cannot count {}",
                job.task_id
            ));
        }
        self.used_tokens = self.used_tokens.saturating_add(tokens_in(&receipt));
        let outcome = receipt.outcome;
        let reason = typed_receipt_reason(&receipt, job.attempt);
        self.ledger.append(&LedgerEvent::TaskReceipt {
            task_id: job.task_id.clone(),
            attempt: job.attempt,
            receipt,
        })?;

        let policy_outcome = escalation_outcome(outcome, self.escalate_on_partial);
        if matches!(policy_outcome, Outcome::Fail | Outcome::Timeout) {
            self.failed_attempts
                .entry(job.task_id.clone())
                .and_modify(|count| *count = count.saturating_add(1))
                .or_insert(1);
        }
        let step = self
            .ladder
            .map(|ladder| next_step(ladder, job.tier_idx, job.attempt, policy_outcome))
            .or(match policy_outcome {
                Outcome::Pass | Outcome::Partial | Outcome::Skip => Some(Step::Done),
                Outcome::Fail | Outcome::Timeout => None,
            });
        self.apply_step(job, outcome, &reason, step)
    }

    fn apply_step(
        &mut self,
        mut job: PreparedTask,
        outcome: Outcome,
        reason: &str,
        step: Option<Step>,
    ) -> anyhow::Result<()> {
        match step {
            Some(Step::Done) => {
                self.ledger.append(&LedgerEvent::TaskDone {
                    task_id: job.task_id.clone(),
                    outcome,
                })?;
                self.counts.done += 1;
                self.emit(&format!("done {} - {outcome:?}", job.task_id));
            }
            Some(Step::Retry) => {
                job.attempt = job.attempt.saturating_add(1);
                self.remaining.push_back(job);
            }
            Some(Step::Escalate(next_tier_idx)) => {
                self.escalate(job, next_tier_idx, reason)?;
            }
            Some(Step::Gate) => {
                let failed_attempts = self.failed_attempts.get(&job.task_id).copied().unwrap_or(0);
                self.ledger.append(&LedgerEvent::TaskGate {
                    task_id: job.task_id.clone(),
                    reason: format!(
                        "ladder exhausted - paused for human review after {failed_attempts} failed attempts"
                    ),
                })?;
                self.counts.gated += 1;
                self.emit(&format!(
                    "gated {} - ladder exhausted, needs human review ({failed_attempts} failed attempts)",
                    job.task_id
                ));
            }
            None => self.fail_task(&job.task_id, reason)?,
        }
        Ok(())
    }

    fn escalate(
        &mut self,
        mut job: PreparedTask,
        next_tier_idx: usize,
        reason: &str,
    ) -> anyhow::Result<()> {
        let (to_route, to_effort) = {
            let ladder = self.ladder.ok_or_else(|| {
                anyhow::anyhow!("escalation was selected without a configured ladder")
            })?;
            let tier = ladder
                .tiers()
                .get(next_tier_idx)
                .ok_or_else(|| anyhow::anyhow!("escalation ladder selected a missing tier"))?;
            (tier.route_id.clone(), tier.effort)
        };
        self.ledger.append(&LedgerEvent::TaskEscalated {
            task_id: job.task_id.clone(),
            from_route: job.route_id.clone(),
            to_route: to_route.clone(),
            reason: reason.to_owned(),
        })?;
        let from_route = self.runtime.resolver.resolve(&job.route_id)?;
        let from_effort = effort_name(
            job.effort
                .unwrap_or_else(|| effort_for(from_route.thinking_dialect())),
        );
        self.emit(&format!(
            "escalated {} - {}/{} → {}/{} ({reason})",
            job.task_id,
            job.route_id,
            from_effort,
            to_route,
            effort_name(to_effort)
        ));
        job.route_id = to_route;
        job.tier_idx = next_tier_idx;
        job.effort = Some(to_effort);
        job.attempt = 1;
        self.remaining.push_back(job);
        Ok(())
    }

    fn fail_task(&mut self, task_id: &str, reason: &str) -> anyhow::Result<()> {
        self.ledger.append(&LedgerEvent::TaskFailed {
            task_id: task_id.to_owned(),
            reason: reason.to_owned(),
        })?;
        self.counts.failed += 1;
        self.emit(&format!("failed {task_id} - {reason}"));
        Ok(())
    }

    fn enforce_budget(&mut self) -> anyhow::Result<()> {
        if self
            .budget_tokens
            .is_none_or(|limit| self.used_tokens < limit)
        {
            return Ok(());
        }
        if !self.budget_halted {
            self.budget_halted = true;
            self.emit(&format!(
                "budget halted at {} tokens - no new tasks will start",
                self.used_tokens
            ));
        }
        self.halt_remaining()
    }

    fn halt_remaining(&mut self) -> anyhow::Result<()> {
        while let Some(task) = self.remaining.pop_front() {
            self.ledger.append(&LedgerEvent::TaskFailed {
                task_id: task.task_id.clone(),
                reason: "budget halted before dispatch".into(),
            })?;
            self.counts.failed += 1;
            self.emit(&format!(
                "failed {} - budget halted before dispatch",
                task.task_id
            ));
        }
        Ok(())
    }

    fn dispatch(&mut self, jobs: &mpsc::Sender<PreparedTask>) -> anyhow::Result<()> {
        let candidates = self.remaining.len();
        for _ in 0..candidates {
            if self.active >= self.worker_count {
                break;
            }
            let Some(task) = self.remaining.pop_front() else {
                break;
            };
            if task.defer_offpeak {
                let now = self.runtime.clock.now();
                if let Ok(route) = self.runtime.resolver.resolve(&task.route_id) {
                    if !ready_to_dispatch(&route, now) {
                        if self.deferred_announced.insert(task.task_id.clone()) {
                            let local: FixedOffset = *Local::now().offset();
                            self.emit(&format!(
                                "deferred {} - {}, parked",
                                task.task_id,
                                route.peak_status(now, local)
                            ));
                        }
                        self.remaining.push_back(task);
                        continue;
                    }
                }
            }
            jobs.send(task)
                .context("fleet worker pool stopped before dispatch")?;
            self.active = self.active.saturating_add(1);
        }
        Ok(())
    }

    fn emit(&self, line: &str) {
        emit(self.on_event, &self.display_scrubber, line);
    }

    fn finish(&self) -> anyhow::Result<RunReport> {
        let report = self.counts.report(self.run_id.to_owned());
        self.ledger.append(&LedgerEvent::RunFinished {
            run_id: report.run_id.clone(),
            done: report.done,
            failed: report.failed,
            gated: report.gated,
        })?;
        self.emit(&format!(
            "fleet {} finished - {} done, {} failed, {} gated",
            report.run_id, report.done, report.failed, report.gated
        ));
        Ok(report)
    }
}
