//! Run identifiers, append-only JSONL persistence, recovery, and ledger queries.

use crate::engine::{escalation_outcome, Counts, DurableWriter, IndexRecord, QueuedTask, RunMeta};
use crate::model::{LedgerEvent, RunReport};
use crate::RUN_SEQUENCE;
use anyhow::{bail, Context as _};
use chrono::{SecondsFormat, Utc};
use nh_core::receipt::{Outcome, Receipt};
use nh_vault::SecretRegistry;
use serde::de::DeserializeOwned;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

pub(super) fn now_utc() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub fn new_run_id() -> String {
    let sequence = RUN_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!(
        "{}-{}-{sequence}",
        Utc::now().format("%Y%m%dT%H%M%S%3fZ"),
        std::process::id()
    )
}

#[cfg(test)]
pub(super) fn fleet_root(run_root: &Path) -> PathBuf {
    run_root.join(".nosis").join("fleet")
}

pub(super) fn checked_fleet_root(run_root: &Path) -> anyhow::Result<PathBuf> {
    nh_core::runtime_path::ensure_contained_dir(run_root, Path::new(".nosis/fleet"))
}

pub(super) fn validate_run_id(run_id: &str) -> anyhow::Result<()> {
    if run_id.is_empty()
        || !run_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        bail!("invalid fleet run id");
    }
    Ok(())
}

/// Read one run's committed ledger, returning empty while its file is not created yet.
pub fn read_run_ledger(run_root: &Path, run_id: &str) -> anyhow::Result<Vec<LedgerEvent>> {
    validate_run_id(run_id)?;
    let Some(run_dir) = nh_core::runtime_path::resolve_contained_dir(
        run_root,
        &Path::new(".nosis").join("fleet").join(run_id),
    )?
    else {
        return Ok(Vec::new());
    };
    let path = run_dir.join("ledger.jsonl");
    if !path.exists() {
        return Ok(Vec::new());
    }
    read_ledger(&path)
}

pub(super) fn append_index(
    path: &Path,
    record: &IndexRecord,
    literals: &SecretRegistry,
) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("fleet index path has no parent directory"))?;
    fs::create_dir_all(parent).with_context(|| format!("could not create {}", parent.display()))?;
    let lock_path = parent.join("index.lock");
    nh_core::runtime_path::reject_symlink_or_special_file(&lock_path, "fleet index lock")?;
    let index_lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .with_context(|| format!("could not open {}", lock_path.display()))?;
    index_lock
        .lock()
        .with_context(|| format!("could not lock {}", lock_path.display()))?;
    repair_uncommitted_tail(path)?;
    DurableWriter::open(path, literals.scrubber())?.append(record)
}

pub(super) fn finish_index(
    fleet_root: &Path,
    created_utc: &str,
    task_count: usize,
    report: &RunReport,
    literals: &SecretRegistry,
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

pub(super) fn latest_incomplete_run(index_path: &Path) -> anyhow::Result<String> {
    nh_core::runtime_path::reject_symlink_or_special_file(index_path, "fleet index")?;
    let bytes = fs::read(index_path)
        .with_context(|| "no fleet index found — run `nh fleet run <tasks.json>` first")?;
    let records: Vec<IndexRecord> = parse_jsonl(&bytes, "fleet index")?;
    let mut latest_status = HashMap::new();
    for record in &records {
        latest_status.insert(record.run_id.clone(), record.status.clone());
    }
    let run_id = records
        .iter()
        .rev()
        .find(|record| {
            latest_status
                .get(&record.run_id)
                .is_some_and(|status| status != "finished")
        })
        .map(|record| record.run_id.clone())
        .ok_or_else(|| anyhow::anyhow!("no incomplete fleet run found"))?;
    validate_run_id(&run_id)?;
    Ok(run_id)
}

pub(super) fn read_ledger(path: &Path) -> anyhow::Result<Vec<LedgerEvent>> {
    nh_core::runtime_path::reject_symlink_or_special_file(path, "fleet ledger")?;
    let bytes = fs::read(path)
        .with_context(|| format!("could not read fleet ledger {}", path.display()))?;
    parse_jsonl(&bytes, "fleet ledger")
}

pub(super) fn ledger_has_committed_events(path: &Path) -> anyhow::Result<bool> {
    nh_core::runtime_path::reject_symlink_or_special_file(path, "fleet ledger")?;
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("could not read fleet ledger {}", path.display()))
        }
    };
    Ok(!parse_jsonl::<LedgerEvent>(&bytes, "fleet ledger")?.is_empty())
}

pub(super) fn parse_jsonl<T: DeserializeOwned>(
    bytes: &[u8],
    label: &str,
) -> anyhow::Result<Vec<T>> {
    let ends_in_newline = bytes.last() == Some(&b'\n');
    let lines: Vec<&[u8]> = bytes.split(|byte| *byte == b'\n').collect();
    let last_non_empty = lines.iter().rposition(|line| !jsonl_line_is_empty(line));
    let mut values = Vec::new();
    for (index, line) in lines.into_iter().enumerate() {
        if jsonl_line_is_empty(line) {
            continue;
        }
        match serde_json::from_slice(line) {
            Ok(value) => values.push(value),
            Err(_) if !ends_in_newline && Some(index) == last_non_empty => {}
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("{label} line {} is invalid", index.saturating_add(1))
                })
            }
        }
    }
    Ok(values)
}

pub(super) fn jsonl_line_is_empty(line: &[u8]) -> bool {
    std::str::from_utf8(line).is_ok_and(|text| text.trim().is_empty())
}

/// A process can die after writing part of an event but before its flush/fsync
/// acknowledgement. Such bytes were never committed. Recovery removes only
/// that non-newline-terminated tail; every committed JSONL event remains
/// append-only and byte-for-byte unchanged.
pub(super) fn repair_uncommitted_tail(path: &Path) -> anyhow::Result<()> {
    nh_core::runtime_path::reject_symlink_or_special_file(path, "fleet ledger")?;
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

pub(super) fn run_meta(events: &[LedgerEvent], expected_run_id: &str) -> anyhow::Result<RunMeta> {
    events
        .iter()
        .find_map(|event| match event {
            LedgerEvent::RunStarted {
                run_id,
                created_utc,
                task_count,
                max_workers,
                budget_tokens,
                escalate,
            } if run_id == expected_run_id => Some(RunMeta {
                created_utc: created_utc.clone(),
                task_count: *task_count,
                max_workers: *max_workers,
                budget_tokens: *budget_tokens,
                escalate: *escalate,
            }),
            _ => None,
        })
        .ok_or_else(|| anyhow::anyhow!("fleet ledger has no matching run_started event"))
}

pub(super) fn queued_tasks(events: &[LedgerEvent]) -> anyhow::Result<BTreeMap<String, QueuedTask>> {
    let mut queued = BTreeMap::new();
    for event in events {
        if let LedgerEvent::TaskQueued {
            task_id,
            task,
            route_id,
            defer_offpeak,
            backend,
        } = event
        {
            if queued
                .insert(
                    task_id.clone(),
                    QueuedTask {
                        task: task.clone(),
                        route_id: route_id.clone(),
                        defer_offpeak: *defer_offpeak,
                        backend: *backend,
                    },
                )
                .is_some()
            {
                bail!("fleet ledger has duplicate queued task ids");
            }
        }
    }
    Ok(queued)
}

pub(super) fn attempts_by_task(events: &[LedgerEvent]) -> HashMap<String, u32> {
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

pub(super) fn terminal_counts(events: &[LedgerEvent]) -> Counts {
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

pub(super) fn ensure_single_terminal(events: &[LedgerEvent]) -> anyhow::Result<()> {
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

pub(super) fn has_finished(events: &[LedgerEvent]) -> bool {
    events
        .iter()
        .any(|event| matches!(event, LedgerEvent::RunFinished { .. }))
}

pub(super) fn receipt_tokens(events: &[LedgerEvent]) -> u64 {
    events
        .iter()
        .filter_map(|event| match event {
            LedgerEvent::TaskReceipt { receipt, .. } => Some(tokens_in(receipt)),
            _ => None,
        })
        .fold(0u64, u64::saturating_add)
}

pub(super) fn failure_receipts_by_task(
    events: &[LedgerEvent],
    escalate_on_partial: bool,
) -> HashMap<String, u32> {
    let mut failures = HashMap::new();
    for event in events {
        if let LedgerEvent::TaskReceipt {
            task_id, receipt, ..
        } = event
        {
            let outcome = escalation_outcome(receipt.outcome, escalate_on_partial);
            if matches!(outcome, Outcome::Fail | Outcome::Timeout) {
                failures
                    .entry(task_id.clone())
                    .and_modify(|count: &mut u32| *count = count.saturating_add(1))
                    .or_insert(1);
            }
        }
    }
    failures
}

pub(super) fn tokens_in(receipt: &Receipt) -> u64 {
    receipt.usage.as_ref().map_or(0, |usage| {
        usage.prompt_tokens.saturating_add(usage.completion_tokens)
    })
}
