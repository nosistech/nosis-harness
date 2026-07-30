//! Public fleet configuration, ledger schema, and pure state folds.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::bail;
use chrono::{DateTime, Utc};
use nh_core::agent::validate_task;
use nh_core::receipt::{Outcome, Receipt};
use nh_core::wire::ThinkingEffort;
use nh_law::Law;
use nh_routes::RouteResolver;
use serde::{Deserialize, Serialize};

pub const MAX_FLEET_TASKS: usize = 256;
pub const MAX_TASK_ID_BYTES: usize = 128;

/// Thread-safe progress sink used by CLI and MCP frontends.
pub type EventCallback = Arc<dyn Fn(&str) + Send + Sync>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSpec {
    #[serde(default)]
    pub id: Option<String>,
    pub task: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub defer_offpeak: Option<bool>,
    #[serde(default)]
    pub backend: Option<Backend>,
}

/// Validate the caller-controlled shape of a fleet before route lookup,
/// credential access, directory creation, or worker dispatch.
pub fn validate_task_specs(specs: &[TaskSpec]) -> anyhow::Result<()> {
    if specs.is_empty() {
        bail!("fleet needs at least one task");
    }
    if specs.len() > MAX_FLEET_TASKS {
        bail!("fleet has too many tasks — maximum is {MAX_FLEET_TASKS}");
    }

    let mut explicit_ids = HashSet::new();
    for (index, spec) in specs.iter().enumerate() {
        validate_task(&spec.task)
            .map_err(|error| anyhow::anyhow!("task {}: {error}", index + 1))?;
        if let Some(id) = spec.id.as_deref() {
            if id.trim().is_empty() {
                bail!("task ids cannot be empty");
            }
            if id.len() > MAX_TASK_ID_BYTES {
                bail!("task id is too large — maximum is {MAX_TASK_ID_BYTES} bytes");
            }
            if !explicit_ids.insert(id) {
                bail!("task id collision — choose unique ids");
            }
        }
    }
    Ok(())
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    #[default]
    Native,
    #[serde(rename = "kimi-swarm")]
    KimiSwarm,
}

pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// Off-peak and routes without price data dispatch immediately; peak routes park.
pub fn ready_to_dispatch(route: &nh_routes::ResolvedRoute, now: DateTime<Utc>) -> bool {
    route.price_at(now).is_none_or(|quote| !quote.peak)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tier {
    pub route_id: String,
    pub effort: ThinkingEffort,
}

#[derive(Debug, Clone)]
pub struct Ladder {
    tiers: Vec<Tier>,
}

impl Ladder {
    pub fn default_ladder() -> Self {
        Self {
            tiers: vec![
                Tier {
                    route_id: "deepseek-v4-flash".into(),
                    effort: ThinkingEffort::None,
                },
                Tier {
                    route_id: "kimi-k2.7-code".into(),
                    effort: ThinkingEffort::High,
                },
                Tier {
                    route_id: "deepseek-v4-pro".into(),
                    effort: ThinkingEffort::High,
                },
                Tier {
                    route_id: "deepseek-v4-pro".into(),
                    effort: ThinkingEffort::Max,
                },
            ],
        }
    }

    pub(crate) fn tiers(&self) -> &[Tier] {
        &self.tiers
    }

    #[cfg(test)]
    pub(crate) fn from_tiers(tiers: Vec<Tier>) -> Self {
        Self { tiers }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Retry,
    Escalate(usize),
    Gate,
    Done,
}

/// Decide the next ladder action from the attempt that just completed.
pub fn next_step(ladder: &Ladder, tier_idx: usize, attempt: u32, outcome: Outcome) -> Step {
    match outcome {
        Outcome::Pass | Outcome::Partial | Outcome::Skip => Step::Done,
        Outcome::Fail | Outcome::Timeout if attempt < 2 => Step::Retry,
        Outcome::Fail | Outcome::Timeout if tier_idx.saturating_add(1) < ladder.tiers.len() => {
            Step::Escalate(tier_idx + 1)
        }
        Outcome::Fail | Outcome::Timeout => Step::Gate,
    }
}

pub trait SwarmClient: Send + Sync {
    /// Submit one brief and collect one typed receipt; transport details arrive in M6.
    fn submit_and_collect(&self, task_id: &str, brief: &str) -> anyhow::Result<Receipt>;
}

pub struct PendingSwarmClient;

impl SwarmClient for PendingSwarmClient {
    fn submit_and_collect(&self, _task_id: &str, _brief: &str) -> anyhow::Result<Receipt> {
        bail!("kimi swarm arrives live in M6 — provide a SwarmClient or use backend=native")
    }
}

pub struct FleetConfig {
    pub resolver: RouteResolver,
    pub law: Law,
    pub default_route: String,
    pub tasks: Vec<TaskSpec>,
    /// Must be at least one for `run`; `0` on `resume` reuses the original value.
    pub max_workers: usize,
    /// Stops new dispatch after completed receipts report this many tokens.
    /// Already-running provider calls can finish beyond the threshold.
    pub budget_tokens: Option<u64>,
    pub clock: Option<Arc<dyn Clock>>,
    pub defer_offpeak: bool,
    pub ladder: Option<Ladder>,
    pub escalate_on_partial: bool,
    pub swarm: Option<Arc<dyn SwarmClient>>,
    /// Repository root; fleet data is stored below `.nosis/fleet`.
    pub run_root: PathBuf,
    pub on_event: Option<EventCallback>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunReport {
    pub run_id: String,
    pub done: usize,
    pub failed: usize,
    pub gated: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FleetStatus {
    pub done: usize,
    pub failed: usize,
    pub gated: usize,
    pub pending: usize,
    pub finished: bool,
    pub failed_reason: Option<String>,
    pub unmetered: usize,
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
        #[serde(default)]
        escalate: bool,
    },
    TaskQueued {
        task_id: String,
        task: String,
        route_id: String,
        #[serde(default)]
        defer_offpeak: bool,
        #[serde(default)]
        backend: Backend,
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
    RunFailed {
        run_id: String,
        reason: String,
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
            LedgerEvent::RunStarted { .. }
            | LedgerEvent::RunFinished { .. }
            | LedgerEvent::RunFailed { .. } => None,
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

/// Fold committed ledger events into run status. Pure: no I/O.
pub fn status_from_ledger(events: &[LedgerEvent]) -> FleetStatus {
    let finished = events
        .iter()
        .any(|event| matches!(event, LedgerEvent::RunFinished { .. }));
    let failed_reason = if finished {
        None
    } else {
        events.iter().rev().find_map(|event| match event {
            LedgerEvent::RunFailed { reason, .. } => Some(reason.clone()),
            _ => None,
        })
    };
    FleetStatus {
        done: events
            .iter()
            .filter(|event| matches!(event, LedgerEvent::TaskDone { .. }))
            .count(),
        failed: events
            .iter()
            .filter(|event| matches!(event, LedgerEvent::TaskFailed { .. }))
            .count(),
        gated: events
            .iter()
            .filter(|event| matches!(event, LedgerEvent::TaskGate { .. }))
            .count(),
        pending: plan_from_ledger(events).todo.len(),
        finished,
        failed_reason,
        unmetered: events
            .iter()
            .filter(|event| {
                matches!(
                    event,
                    LedgerEvent::TaskReceipt { receipt, .. } if receipt.usage.is_none()
                )
            })
            .count(),
    }
}

/// Reconstruct the next ladder tier and attempt for an interrupted task. Pure: no I/O.
pub fn ladder_position(events: &[LedgerEvent], task_id: &str) -> (usize, u32) {
    let mut tier_idx = 0usize;
    let mut max_attempt = 0u32;
    for event in events {
        match event {
            LedgerEvent::TaskEscalated {
                task_id: event_task,
                ..
            } if event_task == task_id => {
                tier_idx = tier_idx.saturating_add(1);
                max_attempt = 0;
            }
            LedgerEvent::TaskStarted {
                task_id: event_task,
                attempt,
                ..
            } if event_task == task_id => {
                max_attempt = max_attempt.max(*attempt);
            }
            _ => {}
        }
    }
    (tier_idx, max_attempt.saturating_add(1).max(1))
}
