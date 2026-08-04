//! E1: abrupt child termination must preserve committed task terminals and resume safely.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use nh_fleet::LedgerEvent;

const CATALOG: &str = r#"
    [routes.echo]
    provider = "echo"
    model_id = "echo-model"
    base_url = "https://example.invalid/v1"
    wire = "openai"
    vault_entry = "echo"

    [routes.deepseek-v4-flash]
    provider = "echo"
    model_id = "echo-model"
    base_url = "https://example.invalid/v1"
    wire = "openai"
    vault_entry = "echo"
"#;

#[test]
fn kill_then_resume_does_not_rerun_committed_tasks() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("catalog.toml"), CATALOG).unwrap();
    let home = tmp.path().join("home");
    fs::create_dir_all(home.join(".nosis")).unwrap();
    fs::write(home.join(".nosis").join("catalog.toml"), CATALOG).unwrap();
    let task_ids: Vec<String> = (0..10).map(|index| format!("task-{index:02}")).collect();
    let tasks: Vec<serde_json::Value> = task_ids
        .iter()
        .map(|id| serde_json::json!({"id": id, "task": format!("execute {id}")}))
        .collect();
    fs::write(
        tmp.path().join("tasks.json"),
        serde_json::to_vec_pretty(&serde_json::json!({"tasks": tasks, "budget_tokens": 1_000_000}))
            .unwrap(),
    )
    .unwrap();
    let execution_log = tmp
        .path()
        .join(".nosis")
        .join("fleet-test-provider")
        .join("execution.log");
    let binary = env!("CARGO_BIN_EXE_nh");

    let mut child = Command::new(binary)
        .args(["fleet", "run", "tasks.json", "--max-workers", "2"])
        .current_dir(tmp.path())
        .env("USERPROFILE", &home)
        .env("HOME", &home)
        .env("NH_FLEET_TEST_PROVIDER", "echo")
        .env("NH_FLEET_TEST_EXECUTION_LOG", "execution.log")
        .env("NH_FLEET_TEST_SLEEP_MS", "250")
        .env_remove("NH_FLEET_TEST_OUTCOME")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(20);
    let (ledger_path, before_kill) = loop {
        if let Some(status) = child.try_wait().unwrap() {
            let output = child.wait_with_output().unwrap();
            panic!(
                "fleet child exited before it could be killed: {status}\nstdout: {}\nstderr: {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        if let Some(path) = find_ledger(tmp.path()) {
            let events = read_events(&path);
            let done = events
                .iter()
                .filter(|event| matches!(event, LedgerEvent::TaskDone { .. }))
                .count();
            if done >= 3 {
                break (path, events);
            }
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for three durable TaskDone events"
        );
        thread::sleep(Duration::from_millis(20));
    };
    let committed_before_kill: HashSet<String> = before_kill
        .iter()
        .filter_map(|event| match event {
            LedgerEvent::TaskDone { task_id, .. } => Some(task_id.clone()),
            _ => None,
        })
        .collect();
    let started_before_kill: HashSet<String> = before_kill
        .iter()
        .filter_map(|event| match event {
            LedgerEvent::TaskStarted { task_id, .. } => Some(task_id.clone()),
            _ => None,
        })
        .collect();
    assert!(committed_before_kill.len() >= 3);
    child.kill().unwrap();
    let _ = child.wait_with_output().unwrap();

    let resumed = Command::new(binary)
        .args(["fleet", "resume", "--max-workers", "2"])
        .current_dir(tmp.path())
        .env("USERPROFILE", &home)
        .env("HOME", &home)
        .env("NH_FLEET_TEST_PROVIDER", "echo")
        .env("NH_FLEET_TEST_EXECUTION_LOG", "execution.log")
        .env("NH_FLEET_TEST_SLEEP_MS", "20")
        .env_remove("NH_FLEET_TEST_OUTCOME")
        .output()
        .unwrap();
    assert!(
        resumed.status.success(),
        "resume failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&resumed.stdout),
        String::from_utf8_lossy(&resumed.stderr)
    );

    let final_events = read_events_strict(&ledger_path);
    assert!(matches!(
        final_events.last(),
        Some(LedgerEvent::RunFinished { .. })
    ));
    let mut done_counts: HashMap<String, usize> = HashMap::new();
    for event in &final_events {
        match event {
            LedgerEvent::TaskDone { task_id, .. } => {
                *done_counts.entry(task_id.clone()).or_default() += 1
            }
            LedgerEvent::TaskFailed { task_id, reason } => {
                panic!("task {task_id} unexpectedly failed: {reason}")
            }
            LedgerEvent::TaskGate { task_id, reason } => {
                panic!("task {task_id} unexpectedly gated: {reason}")
            }
            _ => {}
        }
    }
    assert_eq!(done_counts.len(), 10);
    for id in &task_ids {
        assert_eq!(
            done_counts.get(id),
            Some(&1),
            "{id} must have exactly one terminal TaskDone"
        );
    }

    for event in final_events.iter().skip(before_kill.len()) {
        if let LedgerEvent::TaskStarted { task_id, .. } = event {
            assert!(
                !committed_before_kill.contains(task_id),
                "committed task {task_id} was started again after resume"
            );
        }
    }

    let mut executions: HashMap<String, usize> = HashMap::new();
    for id in fs::read_to_string(&execution_log).unwrap().lines() {
        *executions.entry(id.to_string()).or_default() += 1;
    }
    for id in &task_ids {
        let count = executions.get(id).copied().unwrap_or(0);
        assert!(count >= 1, "{id} never executed");
        if !started_before_kill.contains(id) {
            assert_eq!(
                count, 1,
                "queued-only task {id} must execute once on resume"
            );
        } else {
            assert!(count <= 2, "interrupted task {id} executed more than twice");
        }
    }
    for id in &committed_before_kill {
        assert_eq!(
            executions.get(id),
            Some(&1),
            "committed task {id} executed twice"
        );
    }
}

#[test]
fn binary_without_the_test_provider_feature_refuses_the_switch() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("catalog.toml"), CATALOG).unwrap();
    let home = tmp.path().join("home");
    fs::create_dir_all(home.join(".nosis")).unwrap();
    fs::write(home.join(".nosis").join("catalog.toml"), CATALOG).unwrap();
    fs::write(
        tmp.path().join("tasks.json"),
        br#"{"tasks":[{"id":"one","task":"execute one"}],"budget_tokens":1000}"#,
    )
    .unwrap();

    let output = Command::new(build_plain_nh_binary())
        .args(["fleet", "run", "tasks.json", "--max-workers", "1"])
        .current_dir(tmp.path())
        .env("USERPROFILE", &home)
        .env("HOME", &home)
        .env("NH_FLEET_TEST_PROVIDER", "echo")
        .env_remove("NH_FLEET_TEST_EXECUTION_LOG")
        .env_remove("NH_FLEET_TEST_SLEEP_MS")
        .env_remove("NH_FLEET_TEST_OUTCOME")
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(
            "NH_FLEET_TEST_PROVIDER is unavailable in builds without the test-provider feature"
        ),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn build_plain_nh_binary() -> PathBuf {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap();
    let target = workspace.join("target").join("no-test-provider");
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command
        .current_dir(&workspace)
        .args([
            "build",
            "--locked",
            "--offline",
            "--no-default-features",
            "-p",
            "nh-cli",
            "--bin",
            "nh",
            "--target-dir",
        ])
        .arg(&target)
        .env_remove("NH_FLEET_TEST_PROVIDER")
        .env_remove("NH_FLEET_TEST_EXECUTION_LOG")
        .env_remove("NH_FLEET_TEST_SLEEP_MS")
        .env_remove("NH_FLEET_TEST_OUTCOME");
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        command.arg("--release");
        "release"
    };
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "plain nh build failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    target
        .join(profile)
        .join(format!("nh{}", std::env::consts::EXE_SUFFIX))
}

fn find_ledger(root: &Path) -> Option<PathBuf> {
    let fleet = root.join(".nosis").join("fleet");
    fs::read_dir(fleet)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("ledger.jsonl"))
        .find(|path| path.is_file())
}

fn read_events(path: &Path) -> Vec<LedgerEvent> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

fn read_events_strict(path: &Path) -> Vec<LedgerEvent> {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}
