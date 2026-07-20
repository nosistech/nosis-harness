use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use chrono::{DateTime, TimeZone as _, Utc};
use nh_core::receipt::{FailureClass, Outcome, Receipt};
use nh_core::wire::Usage;
use nh_fleet::{
    Backend, Clock, FleetConfig, Ladder, LedgerEvent, PendingSwarmClient, SwarmClient, TaskSpec,
};
use nh_law::LoadOptions;
use nh_routes::RouteResolver;

const CATALOG: &str = include_str!("../../../catalog.toml");
const TEST_PROVIDER_ENV: &str = "NH_FLEET_TEST_PROVIDER";
const TEST_SLEEP_MS_ENV: &str = "NH_FLEET_TEST_SLEEP_MS";
const TEST_OUTCOME_ENV: &str = "NH_FLEET_TEST_OUTCOME";

static ENV_LOCK: Mutex<()> = Mutex::new(());

struct TestEnv {
    _guard: MutexGuard<'static, ()>,
    old_provider: Option<std::ffi::OsString>,
    old_sleep: Option<std::ffi::OsString>,
    old_outcome: Option<std::ffi::OsString>,
}

impl TestEnv {
    fn echo(outcome: &str) -> Self {
        let guard = ENV_LOCK.lock().unwrap();
        let old_provider = std::env::var_os(TEST_PROVIDER_ENV);
        let old_sleep = std::env::var_os(TEST_SLEEP_MS_ENV);
        let old_outcome = std::env::var_os(TEST_OUTCOME_ENV);
        std::env::set_var(TEST_PROVIDER_ENV, "echo");
        std::env::set_var(TEST_SLEEP_MS_ENV, "0");
        std::env::set_var(TEST_OUTCOME_ENV, outcome);
        Self {
            _guard: guard,
            old_provider,
            old_sleep,
            old_outcome,
        }
    }

    fn clear() -> Self {
        let guard = ENV_LOCK.lock().unwrap();
        let old_provider = std::env::var_os(TEST_PROVIDER_ENV);
        let old_sleep = std::env::var_os(TEST_SLEEP_MS_ENV);
        let old_outcome = std::env::var_os(TEST_OUTCOME_ENV);
        std::env::remove_var(TEST_PROVIDER_ENV);
        std::env::remove_var(TEST_SLEEP_MS_ENV);
        std::env::remove_var(TEST_OUTCOME_ENV);
        Self {
            _guard: guard,
            old_provider,
            old_sleep,
            old_outcome,
        }
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        restore_env(TEST_PROVIDER_ENV, self.old_provider.take());
        restore_env(TEST_SLEEP_MS_ENV, self.old_sleep.take());
        restore_env(TEST_OUTCOME_ENV, self.old_outcome.take());
    }
}

fn restore_env(name: &str, value: Option<std::ffi::OsString>) {
    match value {
        Some(value) => std::env::set_var(name, value),
        None => std::env::remove_var(name),
    }
}

struct MockClock {
    now: Mutex<DateTime<Utc>>,
}

impl MockClock {
    fn new(now: DateTime<Utc>) -> Self {
        Self {
            now: Mutex::new(now),
        }
    }

    fn set(&self, now: DateTime<Utc>) {
        *self.now.lock().unwrap() = now;
    }
}

impl Clock for MockClock {
    fn now(&self) -> DateTime<Utc> {
        *self.now.lock().unwrap()
    }
}

fn config(root: &Path, tasks: Vec<TaskSpec>) -> FleetConfig {
    FleetConfig {
        resolver: RouteResolver::from_toml(CATALOG).unwrap(),
        law: nh_law::load(root, &LoadOptions { cli_autonomy: None }),
        default_route: "deepseek-v4-flash".into(),
        tasks,
        max_workers: 1,
        budget_tokens: None,
        clock: None,
        defer_offpeak: false,
        ladder: None,
        escalate_on_partial: false,
        swarm: None,
        run_root: root.to_path_buf(),
        on_event: None,
    }
}

fn task(id: &str) -> TaskSpec {
    TaskSpec {
        id: Some(id.into()),
        task: format!("execute {id}"),
        model: None,
        defer_offpeak: None,
        backend: None,
    }
}

#[test]
fn e2_deferred_task_parks_at_peak_then_dispatches_off_peak() {
    let _env = TestEnv::echo("pass");
    let tmp = tempfile::tempdir().unwrap();
    let peak = Utc.with_ymd_and_hms(2026, 7, 15, 2, 0, 0).unwrap();
    let off_peak = Utc.with_ymd_and_hms(2026, 7, 15, 10, 30, 0).unwrap();
    let clock = Arc::new(MockClock::new(peak));
    let (line_tx, line_rx) = mpsc::channel();
    let mut fleet = config(tmp.path(), vec![task("deferred")]);
    fleet.tasks[0].defer_offpeak = Some(true);
    fleet.clock = Some(clock.clone());
    fleet.on_event = Some(Arc::new(move |line| {
        let _ = line_tx.send(line.to_string());
    }));

    let run = thread::spawn(move || nh_fleet::run(fleet));
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let line = line_rx.recv_timeout(Duration::from_millis(200)).unwrap();
        if line.contains("deferred deferred") && line.contains("parked") {
            break;
        }
        assert!(Instant::now() < deadline, "deferred line did not arrive");
    }

    let ledger_path = wait_for_ledger(tmp.path());
    let parked_events = read_events(&ledger_path, false);
    assert!(!parked_events.iter().any(|event| matches!(
        event,
        LedgerEvent::TaskStarted { task_id, .. } if task_id == "deferred"
    )));

    clock.set(off_peak);
    let report = run.join().unwrap().unwrap();
    assert_eq!(report.done, 1);
    let events = read_events(&ledger_path, true);
    let started = events.iter().position(|event| {
        matches!(
            event,
            LedgerEvent::TaskStarted { task_id, .. } if task_id == "deferred"
        )
    });
    let done = events.iter().position(|event| {
        matches!(
            event,
            LedgerEvent::TaskDone { task_id, .. } if task_id == "deferred"
        )
    });
    assert!(started.is_some_and(|started| done.is_some_and(|done| started < done)));
}

#[test]
fn non_deferred_task_dispatches_during_peak() {
    let _env = TestEnv::echo("pass");
    let tmp = tempfile::tempdir().unwrap();
    let peak = Utc.with_ymd_and_hms(2026, 7, 15, 2, 0, 0).unwrap();
    let mut fleet = config(tmp.path(), vec![task("control")]);
    fleet.clock = Some(Arc::new(MockClock::new(peak)));
    let report = nh_fleet::run(fleet).unwrap();
    assert_eq!(report.done, 1);
    let events = read_events(&ledger_path(tmp.path(), &report.run_id), true);
    assert!(events.iter().any(|event| matches!(
        event,
        LedgerEvent::TaskStarted { task_id, .. } if task_id == "control"
    )));
}

#[test]
fn live_ladder_attaches_receipts_and_gates_once() {
    let _env = TestEnv::echo("fail");
    let tmp = tempfile::tempdir().unwrap();
    let lines = Arc::new(Mutex::new(Vec::new()));
    let mut fleet = config(tmp.path(), vec![task("climb")]);
    fleet.ladder = Some(Ladder::default_ladder());
    let captured_lines = Arc::clone(&lines);
    fleet.on_event = Some(Arc::new(move |line| {
        captured_lines.lock().unwrap().push(line.to_string());
    }));
    let report = nh_fleet::run(fleet).unwrap();
    assert_eq!((report.done, report.failed, report.gated), (0, 0, 1));
    assert!(lines.lock().unwrap().iter().any(|line| {
        line == "escalated climb - deepseek-v4-pro/high → deepseek-v4-pro/max (fail (verification) after 2 tries)"
    }));

    let events = read_events(&ledger_path(tmp.path(), &report.run_id), true);
    let starts: Vec<(&str, &str, u32)> = events
        .iter()
        .filter_map(|event| match event {
            LedgerEvent::TaskStarted {
                route_id,
                effort,
                attempt,
                ..
            } => Some((route_id.as_str(), effort.as_str(), *attempt)),
            _ => None,
        })
        .collect();
    assert_eq!(starts.len(), 8);
    let mut starts_per_tier = HashMap::new();
    for (route, effort, attempt) in &starts {
        *starts_per_tier.entry((*route, *effort)).or_insert(0usize) += 1;
        assert!(*attempt <= 2);
    }
    assert_eq!(
        starts_per_tier.get(&("deepseek-v4-flash", "none")),
        Some(&2)
    );
    assert_eq!(starts_per_tier.get(&("kimi-k2.7-code", "high")), Some(&2));
    assert_eq!(starts_per_tier.get(&("deepseek-v4-pro", "high")), Some(&2));
    assert_eq!(starts_per_tier.get(&("deepseek-v4-pro", "max")), Some(&2));

    let escalations: Vec<(usize, &str, &str)> = events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| match event {
            LedgerEvent::TaskEscalated {
                from_route,
                to_route,
                ..
            } => Some((index, from_route.as_str(), to_route.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(
        escalations
            .iter()
            .map(|(_, from, to)| (*from, *to))
            .collect::<Vec<_>>(),
        vec![
            ("deepseek-v4-flash", "kimi-k2.7-code"),
            ("kimi-k2.7-code", "deepseek-v4-pro"),
            ("deepseek-v4-pro", "deepseek-v4-pro"),
        ]
    );
    for (index, _, _) in escalations {
        assert!(matches!(
            events.get(index.saturating_sub(1)),
            Some(LedgerEvent::TaskReceipt { task_id, .. }) if task_id == "climb"
        ));
    }
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, LedgerEvent::TaskReceipt { .. }))
            .count(),
        8
    );
    assert_eq!(terminal_count(&events, "climb"), 1);
    assert!(events.iter().any(|event| matches!(
        event,
        LedgerEvent::TaskGate { task_id, reason }
            if task_id == "climb" && reason.contains("8 failed attempts")
    )));
}

#[test]
fn resume_continues_escalation_from_the_ledger_ladder_position() {
    let _env = TestEnv::echo("fail");
    let tmp = tempfile::tempdir().unwrap();
    let run_id = "resume-climb";
    let ledger_path = ledger_path(tmp.path(), run_id);
    fs::create_dir_all(ledger_path.parent().unwrap()).unwrap();
    let partial_events = [
        LedgerEvent::RunStarted {
            run_id: run_id.into(),
            created_utc: "2026-07-15T10:30:00Z".into(),
            task_count: 1,
            max_workers: 1,
            budget_tokens: None,
            escalate: true,
        },
        LedgerEvent::TaskQueued {
            task_id: "climb".into(),
            task: "execute climb".into(),
            route_id: "deepseek-v4-flash".into(),
            defer_offpeak: false,
            backend: Backend::Native,
        },
        LedgerEvent::TaskStarted {
            task_id: "climb".into(),
            route_id: "deepseek-v4-flash".into(),
            effort: "none".into(),
            attempt: 1,
        },
        LedgerEvent::TaskReceipt {
            task_id: "climb".into(),
            attempt: 1,
            receipt: failed_receipt("deepseek-v4-flash"),
        },
        LedgerEvent::TaskStarted {
            task_id: "climb".into(),
            route_id: "deepseek-v4-flash".into(),
            effort: "none".into(),
            attempt: 2,
        },
        LedgerEvent::TaskReceipt {
            task_id: "climb".into(),
            attempt: 2,
            receipt: failed_receipt("deepseek-v4-flash"),
        },
        LedgerEvent::TaskEscalated {
            task_id: "climb".into(),
            from_route: "deepseek-v4-flash".into(),
            to_route: "kimi-k2.7-code".into(),
            reason: "fail (verification) after 2 tries".into(),
        },
    ];
    let contents = partial_events
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .join("\n")
        + "\n";
    fs::write(&ledger_path, contents).unwrap();

    let report =
        nh_fleet::resume(tmp.path(), Some(run_id), config(tmp.path(), Vec::new())).unwrap();
    assert_eq!((report.done, report.failed, report.gated), (0, 0, 1));

    let events = read_events(&ledger_path, true);
    let resumed_starts: Vec<(&str, &str, u32)> = events[partial_events.len()..]
        .iter()
        .filter_map(|event| match event {
            LedgerEvent::TaskStarted {
                route_id,
                effort,
                attempt,
                ..
            } => Some((route_id.as_str(), effort.as_str(), *attempt)),
            _ => None,
        })
        .collect();
    assert_eq!(
        resumed_starts,
        vec![
            ("kimi-k2.7-code", "high", 1),
            ("kimi-k2.7-code", "high", 2),
            ("deepseek-v4-pro", "high", 1),
            ("deepseek-v4-pro", "high", 2),
            ("deepseek-v4-pro", "max", 1),
            ("deepseek-v4-pro", "max", 2),
        ]
    );
    assert_eq!(terminal_count(&events, "climb"), 1);
    assert!(events.iter().any(|event| matches!(
        event,
        LedgerEvent::TaskGate { task_id, reason }
            if task_id == "climb" && reason.contains("8 failed attempts")
    )));
}

#[test]
fn no_ladder_failure_is_one_attempt_and_task_failed() {
    let _env = TestEnv::echo("fail");
    let tmp = tempfile::tempdir().unwrap();
    let report = nh_fleet::run(config(tmp.path(), vec![task("single")])).unwrap();
    assert_eq!((report.done, report.failed, report.gated), (0, 1, 0));
    let events = read_events(&ledger_path(tmp.path(), &report.run_id), true);
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, LedgerEvent::TaskStarted { .. }))
            .count(),
        1
    );
    assert_eq!(terminal_count(&events, "single"), 1);
    assert!(events.iter().any(|event| matches!(
        event,
        LedgerEvent::TaskFailed { task_id, .. } if task_id == "single"
    )));
}

struct MockSwarm;

impl SwarmClient for MockSwarm {
    fn submit_and_collect(&self, task_id: &str, brief: &str) -> anyhow::Result<Receipt> {
        Ok(Receipt {
            ts_utc: "2026-07-15T10:30:00Z".into(),
            model_id: "swarm-mock".into(),
            task: format!("{task_id}:{brief}"),
            turns: 1,
            tool_calls: 0,
            outcome: Outcome::Pass,
            failure_class: None,
            usage: Some(Usage {
                prompt_tokens: 3,
                completion_tokens: 2,
                cached_tokens: None,
            }),
            effective_profile: None,
        })
    }
}

#[test]
fn kimi_swarm_mock_lands_one_receipt_and_pending_stub_is_honest() {
    let _env = TestEnv::clear();
    let tmp = tempfile::tempdir().unwrap();
    let mut swarm_task = task("swarm");
    swarm_task.backend = Some(Backend::KimiSwarm);
    let mut fleet = config(tmp.path(), vec![swarm_task]);
    fleet.swarm = Some(Arc::new(MockSwarm));
    let report = nh_fleet::run(fleet).unwrap();
    assert_eq!(report.done, 1);
    let events = read_events(&ledger_path(tmp.path(), &report.run_id), true);
    let receipts: Vec<&Receipt> = events
        .iter()
        .filter_map(|event| match event {
            LedgerEvent::TaskReceipt { receipt, .. } => Some(receipt),
            _ => None,
        })
        .collect();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].model_id, "swarm-mock");
    assert!(events.iter().any(|event| matches!(
        event,
        LedgerEvent::TaskDone { task_id, outcome: Outcome::Pass } if task_id == "swarm"
    )));

    let error = PendingSwarmClient
        .submit_and_collect("swarm", "brief")
        .unwrap_err();
    assert!(error.to_string().contains("arrives live in M6"));

    let pending_tmp = tempfile::tempdir().unwrap();
    let mut pending_task = task("pending");
    pending_task.backend = Some(Backend::KimiSwarm);
    let pending_report = nh_fleet::run(config(pending_tmp.path(), vec![pending_task])).unwrap();
    assert_eq!(pending_report.failed, 1);
    let pending_events = read_events(
        &ledger_path(pending_tmp.path(), &pending_report.run_id),
        true,
    );
    assert!(pending_events.iter().any(|event| matches!(
        event,
        LedgerEvent::TaskFailed { task_id, reason }
            if task_id == "pending" && reason.contains("arrives live in M6")
    )));
}

fn terminal_count(events: &[LedgerEvent], task_id: &str) -> usize {
    events
        .iter()
        .filter(|event| match event {
            LedgerEvent::TaskDone {
                task_id: event_task,
                ..
            }
            | LedgerEvent::TaskGate {
                task_id: event_task,
                ..
            }
            | LedgerEvent::TaskFailed {
                task_id: event_task,
                ..
            } => event_task == task_id,
            _ => false,
        })
        .count()
}

fn failed_receipt(route_id: &str) -> Receipt {
    Receipt {
        ts_utc: "2026-07-15T10:30:00Z".into(),
        model_id: route_id.into(),
        task: "execute climb".into(),
        turns: 1,
        tool_calls: 0,
        outcome: Outcome::Fail,
        failure_class: Some(FailureClass::Verification),
        usage: Some(Usage {
            prompt_tokens: 3,
            completion_tokens: 2,
            cached_tokens: None,
        }),
        effective_profile: None,
    }
}

fn wait_for_ledger(root: &Path) -> PathBuf {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let fleet_root = root.join(".nosis").join("fleet");
        if let Ok(entries) = fs::read_dir(fleet_root) {
            if let Some(path) = entries
                .filter_map(Result::ok)
                .map(|entry| entry.path().join("ledger.jsonl"))
                .find(|path| path.is_file())
            {
                return path;
            }
        }
        assert!(Instant::now() < deadline, "fleet ledger did not appear");
        thread::sleep(Duration::from_millis(10));
    }
}

fn ledger_path(root: &Path, run_id: &str) -> PathBuf {
    root.join(".nosis")
        .join("fleet")
        .join(run_id)
        .join("ledger.jsonl")
}

fn read_events(path: &Path, strict: bool) -> Vec<LedgerEvent> {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let event = serde_json::from_str(line);
            if strict {
                Some(event.unwrap())
            } else {
                event.ok()
            }
        })
        .collect()
}
