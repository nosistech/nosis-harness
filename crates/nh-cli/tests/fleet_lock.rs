//! A live coordinator must exclude resume, while process death must release the lock.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use nh_fleet::LedgerEvent;

const CATALOG: &str = r#"
    [routes.deepseek-v4-flash]
    provider = "echo"
    model_id = "echo-model"
    base_url = "https://example.invalid/v1"
    wire = "openai"
    vault_entry = "echo"
"#;

#[test]
fn live_coordinator_refuses_resume_then_kill_releases_lock() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("catalog.toml"), CATALOG).unwrap();
    let home = tmp.path().join("home");
    fs::create_dir_all(home.join(".nosis")).unwrap();
    fs::write(home.join(".nosis").join("catalog.toml"), CATALOG).unwrap();
    let task_ids: Vec<String> = (0..8).map(|index| format!("task-{index:02}")).collect();
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
    let binary = env!("CARGO_BIN_EXE_nh");
    let mut child = Command::new(binary)
        .args(["fleet", "run", "tasks.json", "--max-workers", "1"])
        .current_dir(tmp.path())
        .env("USERPROFILE", &home)
        .env("HOME", &home)
        .env("NH_FLEET_TEST_PROVIDER", "echo")
        .env("NH_FLEET_TEST_SLEEP_MS", "750")
        .env_remove("NH_FLEET_TEST_EXECUTION_LOG")
        .env_remove("NH_FLEET_TEST_OUTCOME")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let ledger_path = wait_for_running_ledger(tmp.path(), &mut child);
    let blocked = Command::new(binary)
        .args(["fleet", "resume", "--max-workers", "2"])
        .current_dir(tmp.path())
        .env("USERPROFILE", &home)
        .env("HOME", &home)
        .env("NH_FLEET_TEST_PROVIDER", "echo")
        .env_remove("NH_FLEET_TEST_EXECUTION_LOG")
        .env_remove("NH_FLEET_TEST_SLEEP_MS")
        .env_remove("NH_FLEET_TEST_OUTCOME")
        .output()
        .unwrap();
    assert!(
        !blocked.status.success(),
        "a live coordinator unexpectedly allowed resume"
    );
    assert!(
        String::from_utf8_lossy(&blocked.stderr).contains("appears live"),
        "missing live-run refusal\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&blocked.stdout),
        String::from_utf8_lossy(&blocked.stderr)
    );

    child.kill().unwrap();
    let _ = child.wait_with_output().unwrap();

    let resumed = Command::new(binary)
        .args(["fleet", "resume", "--max-workers", "2"])
        .current_dir(tmp.path())
        .env("USERPROFILE", &home)
        .env("HOME", &home)
        .env("NH_FLEET_TEST_PROVIDER", "echo")
        .env("NH_FLEET_TEST_SLEEP_MS", "20")
        .env_remove("NH_FLEET_TEST_EXECUTION_LOG")
        .env_remove("NH_FLEET_TEST_OUTCOME")
        .output()
        .unwrap();
    assert!(
        resumed.status.success(),
        "resume after kill failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&resumed.stdout),
        String::from_utf8_lossy(&resumed.stderr)
    );

    let events = read_events(&ledger_path);
    assert!(matches!(
        events.last(),
        Some(LedgerEvent::RunFinished { .. })
    ));
    let mut terminals = HashMap::new();
    for event in &events {
        let task_id = match event {
            LedgerEvent::TaskDone { task_id, .. }
            | LedgerEvent::TaskFailed { task_id, .. }
            | LedgerEvent::TaskGate { task_id, .. } => Some(task_id),
            _ => None,
        };
        if let Some(task_id) = task_id {
            *terminals.entry(task_id.clone()).or_insert(0usize) += 1;
        }
    }
    assert_eq!(terminals.len(), task_ids.len());
    for task_id in task_ids {
        assert_eq!(
            terminals.get(&task_id),
            Some(&1),
            "{task_id} must have exactly one terminal"
        );
    }
}

fn wait_for_running_ledger(root: &Path, child: &mut std::process::Child) -> PathBuf {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            panic!("fleet child exited before contention test: {status}");
        }
        if let Some(path) = find_ledger(root) {
            let index = root.join(".nosis").join("fleet").join("index.jsonl");
            if index.metadata().is_ok_and(|metadata| metadata.len() > 0) {
                return path;
            }
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for a running fleet ledger"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

fn find_ledger(root: &Path) -> Option<PathBuf> {
    fs::read_dir(root.join(".nosis").join("fleet"))
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("ledger.jsonl"))
        .find(|path| path.is_file())
}

fn read_events(path: &Path) -> Vec<LedgerEvent> {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}
