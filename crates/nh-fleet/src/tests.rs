use super::*;
use crate::engine::{
    escalation_outcome, run_one_task, DurableWriter, IndexRecord, PreparedTask, QueuedTask,
    RunLock, Runtime, WorkerPool,
};
use crate::ledger::{
    append_index, ensure_single_terminal, fleet_root, latest_incomplete_run, parse_jsonl,
    queued_tasks, read_ledger, receipt_tokens, repair_uncommitted_tail, terminal_counts, tokens_in,
};
use crate::prepare::{preflight_keys, prepare_new_tasks};
use chrono::{DateTime, Utc};
use nh_core::receipt::{Outcome, Receipt};
use nh_core::wire::{ThinkingEffort, Usage};
use nh_law::LoadOptions;
use nh_routes::RouteResolver;
use nh_vault::Scrubber;
use std::fs;
use std::sync::atomic::Ordering;
use std::sync::{mpsc, MutexGuard};
use std::thread;

static ENV_LOCK: Mutex<()> = Mutex::new(());

const CATALOG: &str = r#"
    [routes.echo]
    provider = "echo"
    model_id = "echo-model"
    base_url = "https://example.invalid/v1"
    wire = "openai"
    vault_entry = "echo"
"#;

const PRICE_WINDOW_CATALOG: &str = r#"
    [routes.peak-echo]
    provider = "echo"
    model_id = "echo-model"
    base_url = "https://example.invalid/v1"
    wire = "openai"
    vault_entry = "echo"
    [routes.peak-echo.price]
    currency = "USD"
    unit = "per_million_tokens"
    cache_hit = 0.1
    cache_miss = 0.2
    output = 0.3
    price_confidence = "confirmed"
    valid_until = "2099-01-01"
    [routes.peak-echo.price.peak]
    multiplier = 2.0
    timezone = "Asia/Shanghai"
    windows = ["09:00-12:00", "14:00-18:00"]

    [routes.flat-echo]
    provider = "echo"
    model_id = "echo-model"
    base_url = "https://example.invalid/v1"
    wire = "openai"
    vault_entry = "echo"
    [routes.flat-echo.price]
    currency = "USD"
    unit = "per_million_tokens"
    cache_hit = 0.1
    cache_miss = 0.2
    output = 0.3
    price_confidence = "confirmed"
    valid_until = "2099-01-01"
"#;

#[cfg(unix)]
fn symlink_dir(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn symlink_dir(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}

struct TestEnv {
    _guard: MutexGuard<'static, ()>,
    old_provider: Option<std::ffi::OsString>,
    old_sleep: Option<std::ffi::OsString>,
    old_outcome: Option<std::ffi::OsString>,
}

impl TestEnv {
    fn echo() -> Self {
        let guard = ENV_LOCK.lock().unwrap();
        let old_provider = std::env::var_os(TEST_PROVIDER_ENV);
        let old_sleep = std::env::var_os(TEST_SLEEP_MS_ENV);
        let old_outcome = std::env::var_os(TEST_OUTCOME_ENV);
        std::env::set_var(TEST_PROVIDER_ENV, "echo");
        std::env::set_var(TEST_SLEEP_MS_ENV, "0");
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
        match self.old_provider.take() {
            Some(value) => std::env::set_var(TEST_PROVIDER_ENV, value),
            None => std::env::remove_var(TEST_PROVIDER_ENV),
        }
        match self.old_sleep.take() {
            Some(value) => std::env::set_var(TEST_SLEEP_MS_ENV, value),
            None => std::env::remove_var(TEST_SLEEP_MS_ENV),
        }
        match self.old_outcome.take() {
            Some(value) => std::env::set_var(TEST_OUTCOME_ENV, value),
            None => std::env::remove_var(TEST_OUTCOME_ENV),
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
        clock: None,
        defer_offpeak: false,
        ladder: None,
        escalate_on_partial: false,
        swarm: None,
        run_root: root.to_path_buf(),
        on_event: None,
    }
}

fn task(id: &str, text: &str) -> TaskSpec {
    TaskSpec {
        id: Some(id.into()),
        task: text.into(),
        model: None,
        defer_offpeak: None,
        backend: None,
    }
}

#[test]
fn early_error_drains_worker_before_run_lock_releases() {
    let tmp = tempfile::tempdir().unwrap();
    let run_dir = fleet_root(tmp.path()).join("guarded-run");
    let (job_tx, job_rx) = mpsc::channel::<PreparedTask>();
    let (started_tx, started_rx) = mpsc::channel();
    let (finished_tx, finished_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        let _job = job_rx.recv().unwrap();
        started_tx.send(()).unwrap();
        thread::sleep(Duration::from_millis(50));
        assert!(job_rx.recv().is_err(), "job channel must close before join");
        finished_tx.send(()).unwrap();
    });

    let result: anyhow::Result<()> = (|| {
        let _run_lock = RunLock::acquire(&run_dir, Duration::ZERO)?;
        let pool = WorkerPool {
            job_tx: Some(job_tx),
            workers: vec![worker],
        };
        pool.sender().unwrap().send(PreparedTask {
            task_id: "in-flight".into(),
            task: "finish before return".into(),
            route_id: "echo".into(),
            attempt: 1,
            tier_idx: 0,
            effort: None,
            defer_offpeak: false,
            backend: Backend::Native,
        })?;
        started_rx
            .recv_timeout(Duration::from_secs(1))
            .context("worker did not start")?;
        bail!("forced coordinator failure after dispatch")
    })();

    assert!(result
        .unwrap_err()
        .to_string()
        .contains("forced coordinator"));
    finished_rx
        .try_recv()
        .expect("worker must finish before the early error returns");
    RunLock::acquire(&run_dir, Duration::ZERO)
        .expect("run lock must be released only after worker drain");
}

#[test]
fn worker_pool_finish_joins_every_worker_and_surfaces_a_panic() {
    let (job_tx, _job_rx) = mpsc::channel();
    let (finished_tx, finished_rx) = mpsc::channel();
    let panicked = thread::spawn(|| panic!("intentional worker panic"));
    let finishing = thread::spawn(move || {
        thread::sleep(Duration::from_millis(50));
        finished_tx.send(()).unwrap();
    });
    let pool = WorkerPool {
        job_tx: Some(job_tx),
        workers: vec![panicked, finishing],
    };

    let error = pool.finish().unwrap_err();

    assert_eq!(error.to_string(), "a fleet worker panicked");
    finished_rx
        .try_recv()
        .expect("all workers must be joined even after one panics");
}

#[test]
fn runtime_and_preflight_refuse_an_unapproved_route_origin_before_key_access() {
    let root = tempfile::tempdir().unwrap();
    let resolver = RouteResolver::from_toml(
        r#"
        [routes.scoped]
        provider = "fixture"
        model_id = "fixture-model"
        base_url = "https://api.deepseek.com:8443/v1"
        wire = "openai"
        vault_entry = "deepseek"
        "#,
    )
    .unwrap();
    let law = nh_law::load(root.path(), &LoadOptions { cli_autonomy: None });
    let job = PreparedTask {
        task_id: "origin-check".into(),
        task: "fixture task".into(),
        route_id: "scoped".into(),
        attempt: 1,
        tier_idx: 0,
        effort: None,
        defer_offpeak: false,
        backend: Backend::Native,
    };

    let preflight_error = preflight_keys(&resolver, std::slice::from_ref(&job), None, &law, false)
        .unwrap_err()
        .to_string();
    assert!(
        preflight_error.contains("not approved for https://api.deepseek.com:8443"),
        "{preflight_error}"
    );

    let route = resolver.resolve("scoped").unwrap();
    let runtime = Runtime {
        resolver: Arc::new(resolver),
        law: Arc::new(law),
        run_root: root.path().to_path_buf(),
        workdir: root.path().to_path_buf(),
        key_literals: SecretRegistry::new(),
        test_provider: None,
        clock: Arc::new(SystemClock),
        swarm: Arc::new(PendingSwarmClient),
    };
    let (events, _receiver) = mpsc::sync_channel(1);
    let runtime_error = run_one_task(&runtime, &job, route, &events).unwrap_err();
    assert!(
        runtime_error.contains("not approved for https://api.deepseek.com:8443"),
        "{runtime_error}"
    );
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
            defer_offpeak: None,
            backend: None,
        },
        TaskSpec {
            id: None,
            task: "same task".into(),
            model: None,
            defer_offpeak: None,
            backend: None,
        },
    ];
    let first = prepare_new_tasks(&resolver, "echo", &specs, false, None).unwrap();
    let second = prepare_new_tasks(&resolver, "echo", &specs, false, None).unwrap();
    assert_eq!(first[0].task_id, second[0].task_id);
    assert!(first[0].task_id.starts_with("t000-"));
    assert!(first[1].task_id.starts_with("t001-"));
    assert_eq!(&first[0].task_id[5..], &first[1].task_id[5..]);
}

#[test]
fn explicit_id_collisions_fail_before_run() {
    let resolver = RouteResolver::from_toml(CATALOG).unwrap();
    let specs = vec![task("same", "one"), task("same", "two")];
    let error = prepare_new_tasks(&resolver, "echo", &specs, false, None).unwrap_err();
    assert!(error.to_string().contains("collision"));
}

#[test]
fn fleet_shape_limits_apply_before_route_resolution() {
    let too_many = vec![
        TaskSpec {
            id: None,
            task: "bounded".into(),
            model: Some("route-is-never-resolved".into()),
            defer_offpeak: None,
            backend: None,
        };
        MAX_FLEET_TASKS + 1
    ];
    let error = validate_task_specs(&too_many).unwrap_err();
    assert!(error.to_string().contains("too many tasks"));

    let oversized = vec![TaskSpec {
        id: Some("id".into()),
        task: "x".repeat(nh_core::agent::MAX_TASK_BYTES + 1),
        model: None,
        defer_offpeak: None,
        backend: None,
    }];
    let error = validate_task_specs(&oversized).unwrap_err();
    assert!(error.to_string().contains("maximum"));

    let long_id = vec![TaskSpec {
        id: Some("i".repeat(MAX_TASK_ID_BYTES + 1)),
        task: "bounded".into(),
        model: None,
        defer_offpeak: None,
        backend: None,
    }];
    let error = validate_task_specs(&long_id).unwrap_err();
    assert!(error.to_string().contains("task id is too large"));
}

#[test]
fn run_with_id_honours_the_provided_ledger_handle() {
    let _env = TestEnv::echo();
    let tmp = tempfile::tempdir().unwrap();
    let run_id = "provided-run-id".to_string();
    let report = run_with_id(
        run_id.clone(),
        config(tmp.path(), vec![task("one", "first")]),
    )
    .unwrap();
    assert_eq!(report.run_id, run_id);

    let ledger_path = fleet_root(tmp.path()).join(&run_id).join("ledger.jsonl");
    assert!(ledger_path.is_file());
    let events = read_ledger(&ledger_path).unwrap();
    assert!(matches!(
        events.last(),
        Some(LedgerEvent::RunFinished { run_id: finished, .. }) if finished == &run_id
    ));
}

#[test]
fn run_refuses_a_nosis_directory_that_resolves_outside_the_root() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    if symlink_dir(outside.path(), &root.path().join(".nosis")).is_err() {
        return;
    }

    let error = run_with_id(
        "contained-run".into(),
        config(root.path(), vec![task("one", "must not run")]),
    )
    .unwrap_err();

    assert!(error.to_string().contains("resolves outside root"));
    assert!(!outside.path().join("fleet").exists());
}

#[test]
fn run_with_id_refuses_a_non_empty_ledger_without_appending() {
    let tmp = tempfile::tempdir().unwrap();
    let run_id = "existing-run";
    let ledger_path = fleet_root(tmp.path()).join(run_id).join("ledger.jsonl");
    let ledger = DurableWriter::open(&ledger_path, Scrubber::new(Vec::new())).unwrap();
    ledger
        .append(&LedgerEvent::RunStarted {
            run_id: run_id.into(),
            created_utc: "2026-07-22T00:00:00Z".into(),
            task_count: 1,
            max_workers: 1,
            budget_tokens: None,
            escalate: false,
        })
        .unwrap();
    drop(ledger);
    let before = read_ledger(&ledger_path).unwrap();

    let error = run_with_id(
        run_id.into(),
        config(tmp.path(), vec![task("new", "must not start")]),
    )
    .unwrap_err();

    assert!(error.to_string().contains("use `resume`"));
    let after = read_ledger(&ledger_path).unwrap();
    assert_eq!(after.len(), before.len());
    assert_eq!(
        after
            .iter()
            .filter(|event| matches!(event, LedgerEvent::RunStarted { .. }))
            .count(),
        1
    );
}

#[test]
fn run_failure_includes_run_failed_bookkeeping_failure() {
    let tmp = tempfile::tempdir().unwrap();
    let run_id = "unwritable-run-failure";
    let ledger_path = fleet_root(tmp.path()).join(run_id).join("ledger.jsonl");
    fs::create_dir_all(&ledger_path).unwrap();

    let error = run_with_id(
        run_id.into(),
        config(tmp.path(), vec![task("one", "never starts")]),
    )
    .unwrap_err();
    let error = format!("{error:#}");

    assert!(
        error
            .matches("refused: fleet ledger path is not a regular file")
            .count()
            >= 2,
        "{error}"
    );
    assert!(
        error.contains("additionally, recording the run failure to the ledger failed"),
        "{error}"
    );
}

#[test]
fn status_fold_counts_mixed_tasks_and_finished_flag() {
    let mut events = vec![
        queued("done"),
        LedgerEvent::TaskDone {
            task_id: "done".into(),
            outcome: Outcome::Pass,
        },
        queued("failed"),
        LedgerEvent::TaskFailed {
            task_id: "failed".into(),
            reason: "failed".into(),
        },
        queued("gated"),
        LedgerEvent::TaskGate {
            task_id: "gated".into(),
            reason: "review".into(),
        },
        queued("pending"),
    ];
    assert_eq!(
        status_from_ledger(&events),
        FleetStatus {
            done: 1,
            failed: 1,
            gated: 1,
            pending: 1,
            finished: false,
            failed_reason: None,
            unmetered: 0,
        }
    );

    events.push(LedgerEvent::RunFinished {
        run_id: "fold-run".into(),
        done: 1,
        failed: 1,
        gated: 1,
    });
    assert_eq!(
        status_from_ledger(&events),
        FleetStatus {
            done: 1,
            failed: 1,
            gated: 1,
            pending: 1,
            finished: true,
            failed_reason: None,
            unmetered: 0,
        }
    );
}

#[test]
fn read_run_ledger_missing_file_is_empty() {
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(
        read_run_ledger(tmp.path(), "not-started-yet")
            .unwrap()
            .len(),
        0
    );
}

#[test]
fn read_run_ledger_rejects_bad_run_id_shape() {
    let tmp = tempfile::tempdir().unwrap();
    let error = read_run_ledger(tmp.path(), "../escape").unwrap_err();
    assert_eq!(error.to_string(), "invalid fleet run id");
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

#[test]
fn live_run_lock_refuses_resume() {
    let tmp = tempfile::tempdir().unwrap();
    let run_id = "locked-run";
    let run_dir = fleet_root(tmp.path()).join(run_id);
    let _lock = RunLock::acquire(&run_dir, Duration::ZERO).unwrap();

    let error = resume(tmp.path(), Some(run_id), config(tmp.path(), Vec::new())).unwrap_err();

    assert!(error.to_string().contains("run appears live"), "{error}");
}

#[test]
fn ledger_reader_ignores_torn_tail_without_mutating_file() {
    let tmp = tempfile::tempdir().unwrap();
    let run_id = "torn-reader";
    let path = fleet_root(tmp.path()).join(run_id).join("ledger.jsonl");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let committed = serde_json::to_string(&queued("safe")).unwrap();
    fs::write(&path, format!("{committed}\n{{\"event\":\"task_sta")).unwrap();
    let before = fs::read(&path).unwrap();

    let events = read_run_ledger(tmp.path(), run_id).unwrap();

    assert_eq!(events.len(), 1);
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn ledger_reader_rejects_mid_file_corruption() {
    let tmp = tempfile::tempdir().unwrap();
    let run_id = "bad-middle";
    let path = fleet_root(tmp.path()).join(run_id).join("ledger.jsonl");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let committed = serde_json::to_string(&queued("safe")).unwrap();
    fs::write(&path, format!("{committed}\n{{bad\n{committed}\n")).unwrap();

    let error = read_run_ledger(tmp.path(), run_id).unwrap_err();

    assert!(
        error.to_string().contains("fleet ledger line 2 is invalid"),
        "{error}"
    );
}

#[test]
fn torn_index_tail_is_read_only_then_repaired_before_append() {
    let tmp = tempfile::tempdir().unwrap();
    let root = fleet_root(tmp.path());
    fs::create_dir_all(&root).unwrap();
    let index_path = root.join("index.jsonl");
    let first = IndexRecord {
        run_id: "first-run".into(),
        created_utc: "2026-07-19T00:00:00Z".into(),
        task_count: 1,
        status: "running".into(),
    };
    let first_line = serde_json::to_string(&first).unwrap();
    fs::write(&index_path, format!("{first_line}\n{{\"run_id\":")).unwrap();
    let before = fs::read(&index_path).unwrap();

    assert_eq!(latest_incomplete_run(&index_path).unwrap(), "first-run");
    assert_eq!(fs::read(&index_path).unwrap(), before);

    append_index(
        &index_path,
        &IndexRecord {
            run_id: "second-run".into(),
            created_utc: "2026-07-19T00:01:00Z".into(),
            task_count: 1,
            status: "running".into(),
        },
        &SecretRegistry::new(),
    )
    .unwrap();
    let bytes = fs::read(&index_path).unwrap();
    let records: Vec<IndexRecord> = parse_jsonl(&bytes, "fleet index").unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].run_id, "first-run");
    assert_eq!(records[1].run_id, "second-run");
}

#[test]
fn latest_run_rejects_traversal_without_touching_out_of_root_file() {
    let tmp = tempfile::tempdir().unwrap();
    let root = fleet_root(tmp.path());
    fs::create_dir_all(&root).unwrap();
    let index_path = root.join("index.jsonl");
    let record = IndexRecord {
        run_id: "../evil".into(),
        created_utc: "2026-07-19T00:00:00Z".into(),
        task_count: 1,
        status: "running".into(),
    };
    fs::write(
        &index_path,
        format!("{}\n", serde_json::to_string(&record).unwrap()),
    )
    .unwrap();
    let outside = root.parent().unwrap().join("evil").join("ledger.jsonl");
    fs::create_dir_all(outside.parent().unwrap()).unwrap();
    fs::write(&outside, b"must stay byte-identical").unwrap();
    let before = fs::read(&outside).unwrap();

    let error = resume(tmp.path(), None, config(tmp.path(), Vec::new())).unwrap_err();

    assert_eq!(error.to_string(), "invalid fleet run id");
    assert_eq!(fs::read(&outside).unwrap(), before);
}

#[test]
fn task_queued_defaults_old_ledgers_and_round_trips_resume_fields() {
    let old: LedgerEvent = serde_json::from_str(
        r#"{"event":"task_queued","task_id":"old","task":"work","route_id":"echo"}"#,
    )
    .unwrap();
    assert!(matches!(
        old,
        LedgerEvent::TaskQueued {
            defer_offpeak: false,
            backend: Backend::Native,
            ..
        }
    ));

    let current = LedgerEvent::TaskQueued {
        task_id: "current".into(),
        task: "work".into(),
        route_id: "echo".into(),
        defer_offpeak: true,
        backend: Backend::KimiSwarm,
    };
    let encoded = serde_json::to_string(&current).unwrap();
    let decoded: LedgerEvent = serde_json::from_str(&encoded).unwrap();
    let queued = queued_tasks(&[decoded]).unwrap();
    assert_eq!(
        queued.get("current"),
        Some(&QueuedTask {
            task: "work".into(),
            route_id: "echo".into(),
            defer_offpeak: true,
            backend: Backend::KimiSwarm,
        })
    );
}

struct PeakThenOffPeakClock {
    calls: AtomicU64,
}

impl Clock for PeakThenOffPeakClock {
    fn now(&self) -> DateTime<Utc> {
        use chrono::TimeZone as _;

        if self.calls.fetch_add(1, Ordering::Relaxed) == 0 {
            Utc.with_ymd_and_hms(2026, 7, 15, 2, 0, 0).unwrap()
        } else {
            Utc.with_ymd_and_hms(2026, 7, 15, 10, 30, 0).unwrap()
        }
    }
}

#[test]
fn resume_restores_defer_offpeak_and_parks_at_peak() {
    let _env = TestEnv::echo();
    let tmp = tempfile::tempdir().unwrap();
    let run_id = "resume-deferred";
    let path = fleet_root(tmp.path()).join(run_id).join("ledger.jsonl");
    let ledger = DurableWriter::open(&path, Scrubber::new(Vec::new())).unwrap();
    ledger
        .append(&LedgerEvent::RunStarted {
            run_id: run_id.into(),
            created_utc: "2026-07-19T00:00:00Z".into(),
            task_count: 1,
            max_workers: 1,
            budget_tokens: None,
            escalate: false,
        })
        .unwrap();
    ledger
        .append(&LedgerEvent::TaskQueued {
            task_id: "deferred".into(),
            task: "wait for the cheap window".into(),
            route_id: "peak-echo".into(),
            defer_offpeak: true,
            backend: Backend::Native,
        })
        .unwrap();
    drop(ledger);

    let lines = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&lines);
    let mut fleet = config(tmp.path(), Vec::new());
    fleet.resolver = RouteResolver::from_toml(PRICE_WINDOW_CATALOG).unwrap();
    fleet.defer_offpeak = false;
    fleet.clock = Some(Arc::new(PeakThenOffPeakClock {
        calls: AtomicU64::new(0),
    }));
    fleet.on_event = Some(Arc::new(move |line| {
        captured.lock().unwrap().push(line.to_string());
    }));

    let report = resume(tmp.path(), Some(run_id), fleet).unwrap();

    assert_eq!(report.done, 1);
    let parked = lines
        .lock()
        .unwrap()
        .iter()
        .any(|line| line.contains("deferred deferred") && line.contains("parked"));
    assert!(parked);
}

#[test]
fn preledger_run_error_is_recorded_as_run_failed() {
    let _env = TestEnv::echo();
    let tmp = tempfile::tempdir().unwrap();
    let run_id = "preledger-failure";
    let mut missing = task("missing", "cannot resolve");
    missing.model = Some("not-a-route".into());

    let original = run_with_id(run_id.into(), config(tmp.path(), vec![missing])).unwrap_err();
    let events = read_run_ledger(tmp.path(), run_id).unwrap();

    assert_eq!(events.len(), 1);
    let reason = match &events[0] {
        LedgerEvent::RunFailed {
            run_id: failed_id,
            reason,
        } => {
            assert_eq!(failed_id, run_id);
            reason
        }
        _ => panic!("expected one run_failed event"),
    };
    assert_eq!(reason, &original.to_string());
    assert_eq!(
        status_from_ledger(&events).failed_reason.as_deref(),
        Some(reason.as_str())
    );
}

#[test]
fn run_finished_supersedes_the_last_run_failed_reason() {
    let mut events = vec![
        LedgerEvent::RunFailed {
            run_id: "fold-run".into(),
            reason: "first failure".into(),
        },
        LedgerEvent::RunFailed {
            run_id: "fold-run".into(),
            reason: "last failure".into(),
        },
    ];
    let failed = status_from_ledger(&events);
    assert!(!failed.finished);
    assert_eq!(failed.failed_reason.as_deref(), Some("last failure"));

    events.push(LedgerEvent::RunFinished {
        run_id: "fold-run".into(),
        done: 0,
        failed: 0,
        gated: 0,
    });
    let finished = status_from_ledger(&events);
    assert!(finished.finished);
    assert_eq!(finished.failed_reason, None);
}

#[test]
fn budget_halt_drains_requeued_inflight_failures() {
    let _env = TestEnv::echo();
    std::env::set_var(TEST_SLEEP_MS_ENV, "50");
    std::env::set_var(TEST_OUTCOME_ENV, "fail");
    let tmp = tempfile::tempdir().unwrap();
    let mut fleet = config(
        tmp.path(),
        vec![task("first", "fail first"), task("second", "fail second")],
    );
    fleet.max_workers = 2;
    fleet.budget_tokens = Some(2);
    fleet.ladder = Some(Ladder::from_tiers(vec![
        Tier {
            route_id: "echo".into(),
            effort: ThinkingEffort::None,
        },
        Tier {
            route_id: "echo".into(),
            effort: ThinkingEffort::High,
        },
    ]));
    let (tx, rx) = mpsc::channel();
    let runner = thread::spawn(move || {
        let result = run(fleet).map_err(|error| error.to_string());
        let _ = tx.send(result);
    });

    let report = rx
        .recv_timeout(Duration::from_secs(30))
        .expect("budget-halted fleet hung with re-queued work")
        .unwrap();
    runner.join().unwrap();

    assert_eq!(report.failed, 2);
    let events = read_run_ledger(tmp.path(), &report.run_id).unwrap();
    ensure_single_terminal(&events).unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(
                event,
                LedgerEvent::TaskFailed { reason, .. }
                    if reason == "budget halted before dispatch"
            ))
            .count(),
        2
    );
}

#[test]
fn status_fold_counts_unmetered_receipts_without_fabricating_tokens() {
    let unmetered = receipt(None);
    let metered = receipt(Some(Usage {
        prompt_tokens: 3,
        completion_tokens: 2,
        cached_tokens: None,
    }));
    assert_eq!(tokens_in(&unmetered), 0);
    let events = vec![
        LedgerEvent::TaskReceipt {
            task_id: "unknown".into(),
            attempt: 1,
            receipt: unmetered,
        },
        LedgerEvent::TaskReceipt {
            task_id: "known".into(),
            attempt: 1,
            receipt: metered,
        },
    ];

    assert_eq!(status_from_ledger(&events).unmetered, 1);
    assert_eq!(receipt_tokens(&events), 5);
}

fn receipt(usage: Option<Usage>) -> Receipt {
    Receipt {
        ts_utc: "2026-07-19T00:00:00Z".into(),
        model_id: "echo-model".into(),
        task: "fixture".into(),
        turns: 1,
        tool_calls: 0,
        outcome: Outcome::Pass,
        failure_class: None,
        usage,
        cache_hit_pct: None,
        repairs: Default::default(),
        effective_profile: None,
    }
}

fn queued(id: &str) -> LedgerEvent {
    LedgerEvent::TaskQueued {
        task_id: id.into(),
        task: id.into(),
        route_id: "echo".into(),
        defer_offpeak: false,
        backend: Backend::Native,
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

#[test]
fn ready_to_dispatch_uses_route_price_windows() {
    use chrono::TimeZone as _;

    let resolver = RouteResolver::from_toml(PRICE_WINDOW_CATALOG).unwrap();
    let peak = Utc.with_ymd_and_hms(2026, 7, 15, 2, 0, 0).unwrap();
    let off_peak = Utc.with_ymd_and_hms(2026, 7, 15, 10, 30, 0).unwrap();
    let peak_route = resolver.resolve("peak-echo").unwrap();
    assert!(!ready_to_dispatch(&peak_route, peak));
    assert!(ready_to_dispatch(&peak_route, off_peak));

    let flat_route = resolver.resolve("flat-echo").unwrap();
    assert!(ready_to_dispatch(&flat_route, peak));
    assert!(ready_to_dispatch(&flat_route, off_peak));

    let no_price = RouteResolver::from_toml(CATALOG)
        .unwrap()
        .resolve("echo")
        .unwrap();
    assert!(ready_to_dispatch(&no_price, peak));
}

#[test]
fn next_step_walks_every_default_ladder_tier() {
    let ladder = Ladder::default_ladder();
    for tier_idx in 0..ladder.tiers().len() {
        assert_eq!(next_step(&ladder, tier_idx, 1, Outcome::Pass), Step::Done);
        assert_eq!(next_step(&ladder, tier_idx, 1, Outcome::Skip), Step::Done);
        assert_eq!(
            next_step(&ladder, tier_idx, 1, Outcome::Partial),
            Step::Done
        );
        for outcome in [Outcome::Fail, Outcome::Timeout] {
            assert_eq!(next_step(&ladder, tier_idx, 1, outcome), Step::Retry);
            let expected = if tier_idx + 1 < ladder.tiers().len() {
                Step::Escalate(tier_idx + 1)
            } else {
                Step::Gate
            };
            assert_eq!(next_step(&ladder, tier_idx, 2, outcome), expected);
        }
        assert_eq!(
            next_step(
                &ladder,
                tier_idx,
                1,
                escalation_outcome(Outcome::Partial, true)
            ),
            Step::Retry
        );
        let partial_second = next_step(
            &ladder,
            tier_idx,
            2,
            escalation_outcome(Outcome::Partial, true),
        );
        let fail_second = next_step(&ladder, tier_idx, 2, Outcome::Fail);
        assert_eq!(partial_second, fail_second);
    }
}

#[test]
fn ladder_rejects_per_task_model_with_one_actionable_line() {
    let resolver = RouteResolver::from_toml(include_str!("../../../catalog.toml")).unwrap();
    let mut explicit = task("owned", "work");
    explicit.model = Some("glm-4.7-flash".into());
    let ladder = Ladder::default_ladder();
    let error = prepare_new_tasks(
        &resolver,
        "glm-4.7-flash",
        &[explicit],
        false,
        Some(&ladder),
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "escalation ladder owns route selection - remove per-task model, or drop --escalate"
    );
}

#[test]
fn kimi_swarm_backend_uses_the_locked_serde_name() {
    let spec: TaskSpec = serde_json::from_str(r#"{"task":"work","backend":"kimi-swarm"}"#).unwrap();
    assert_eq!(spec.backend, Some(Backend::KimiSwarm));
    assert!(serde_json::from_str::<TaskSpec>(r#"{"task":"work","backend":"kimi_swarm"}"#).is_err());
}

#[test]
fn ladder_position_resumes_current_tier_at_next_attempt() {
    let events = vec![
        queued("climb"),
        started("climb", 1),
        started("climb", 2),
        LedgerEvent::TaskEscalated {
            task_id: "climb".into(),
            from_route: "tier-zero".into(),
            to_route: "tier-one".into(),
            reason: "fail after 2 tries".into(),
        },
        started("climb", 1),
        started("climb", 2),
        LedgerEvent::TaskEscalated {
            task_id: "climb".into(),
            from_route: "tier-one".into(),
            to_route: "tier-two".into(),
            reason: "fail after 2 tries".into(),
        },
        LedgerEvent::TaskStarted {
            task_id: "climb".into(),
            route_id: "tier-two".into(),
            effort: "high".into(),
            attempt: 1,
        },
    ];
    assert_eq!(ladder_position(&events, "climb"), (2, 2));
    assert_eq!(plan_from_ledger(&events).todo, vec!["climb"]);
}
