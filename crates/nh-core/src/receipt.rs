//! Typed receipts (plan §2): why runs fail, not just that they failed.

use anyhow::Context as _;

use crate::wire::RetryStats;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Pass,
    Fail,
    Partial,
    Skip,
    Timeout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FailureClass {
    Context,
    Constraint,
    Filtered,
    Verification,
    Planning,
    Unreceipted,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RepairStats {
    #[serde(default)]
    pub tool_call_repair_attempts: u32,
    #[serde(default)]
    pub edit_whitespace_matches: u32,
    #[serde(default)]
    pub edit_indentation_matches: u32,
}

impl RepairStats {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Route-agnostic facts about context compaction during one accepted task.
/// Cache evidence is retained only for a single event; aggregating distinct
/// next-call effects would violate the per-next-call honesty boundary.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CompactionStats {
    #[serde(default)]
    pub events: u32,
    #[serde(default)]
    pub messages_elided: u64,
    #[serde(default)]
    pub estimated_tokens_elided: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preceding_cached_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurred_at_unix_seconds: Option<i64>,
}

impl CompactionStats {
    /// Record one real compaction event with saturating task totals.
    pub fn record(
        &mut self,
        messages_elided: u64,
        estimated_tokens_elided: u64,
        preceding_cached_tokens: Option<u64>,
    ) {
        self.record_inner(
            messages_elided,
            estimated_tokens_elided,
            preceding_cached_tokens,
            None,
        );
    }

    /// Record one real compaction event and its measured wall-clock time.
    pub fn record_at(
        &mut self,
        messages_elided: u64,
        estimated_tokens_elided: u64,
        preceding_cached_tokens: Option<u64>,
        unix_seconds: i64,
    ) {
        self.record_inner(
            messages_elided,
            estimated_tokens_elided,
            preceding_cached_tokens,
            Some(unix_seconds),
        );
    }

    fn record_inner(
        &mut self,
        messages_elided: u64,
        estimated_tokens_elided: u64,
        preceding_cached_tokens: Option<u64>,
        occurred_at_unix_seconds: Option<i64>,
    ) {
        let first = self.events == 0;
        self.events = self.events.saturating_add(1);
        self.messages_elided = self.messages_elided.saturating_add(messages_elided);
        self.estimated_tokens_elided = self
            .estimated_tokens_elided
            .saturating_add(estimated_tokens_elided);
        self.preceding_cached_tokens = if first { preceding_cached_tokens } else { None };
        self.occurred_at_unix_seconds = if first {
            occurred_at_unix_seconds
        } else {
            None
        };
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Receipt {
    pub ts_utc: String,
    pub model_id: String,
    pub task: String,
    pub turns: u32,
    pub tool_calls: u32,
    pub outcome: Outcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_class: Option<FailureClass>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<super::wire::Usage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_hit_pct: Option<f64>,
    #[serde(default, skip_serializing_if = "RepairStats::is_empty")]
    pub repairs: RepairStats,
    #[serde(default, skip_serializing_if = "RetryStats::is_empty")]
    pub retries: RetryStats,
    #[serde(default, skip_serializing_if = "CompactionStats::is_empty")]
    pub compaction: Box<CompactionStats>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_profile: Option<String>,
}

/// Appends scrubbed JSONL lines to .nosis/receipts.jsonl (creates dir if missing).
pub struct ReceiptWriter {
    root: std::path::PathBuf,
    path: std::path::PathBuf,
    scrubber: nh_vault::Scrubber,
}

impl ReceiptWriter {
    pub fn project(root: impl Into<std::path::PathBuf>, scrubber: nh_vault::Scrubber) -> Self {
        let root = root.into();
        let path = root.join(".nosis").join("receipts.jsonl");
        Self {
            root,
            path,
            scrubber,
        }
    }

    pub fn for_path(
        root: impl Into<std::path::PathBuf>,
        path: impl Into<std::path::PathBuf>,
        scrubber: nh_vault::Scrubber,
    ) -> Self {
        Self {
            root: root.into(),
            path: path.into(),
            scrubber,
        }
    }

    pub fn replace_scrubber(&mut self, scrubber: nh_vault::Scrubber) {
        self.scrubber = scrubber;
    }

    pub fn scrubber(&self) -> &nh_vault::Scrubber {
        &self.scrubber
    }

    pub fn append(&self, receipt: &Receipt) -> anyhow::Result<()> {
        use std::io::Write as _;
        let path = crate::runtime_path::ensure_contained_file(&self.root, &self.path, "receipts")?;
        let line = serde_json::to_string(receipt).context("could not serialize receipt")?;
        let line = self.scrubber.scrub(&line);
        // read(true): Windows LockFileEx requires read/write DATA access on the
        // handle; a pure append-only handle (FILE_APPEND_DATA) fails file.lock()
        // with ACCESS_DENIED. Append semantics are preserved.
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("could not open {}", path.display()))?;
        file.lock()
            .with_context(|| format!("could not lock {}", path.display()))?;
        writeln!(file, "{line}").with_context(|| format!("could not write {}", path.display()))?;
        file.flush()
            .with_context(|| format!("could not flush {}", path.display()))?;
        file.sync_all()
            .with_context(|| format!("could not fsync {}", path.display()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::{Arc, Barrier};

    fn receipt(task: impl Into<String>) -> Receipt {
        Receipt {
            ts_utc: "2026-07-22T00:00:00Z".to_string(),
            model_id: "test-model".to_string(),
            task: task.into(),
            turns: 1,
            tool_calls: 0,
            outcome: Outcome::Pass,
            failure_class: None,
            usage: None,
            cache_hit_pct: None,
            repairs: RepairStats::default(),
            retries: RetryStats::default(),
            compaction: Default::default(),
            effective_profile: None,
        }
    }

    fn writer(root: &Path, path: std::path::PathBuf) -> ReceiptWriter {
        ReceiptWriter::for_path(root, path, nh_vault::Scrubber::new(Vec::new()))
    }

    #[cfg(unix)]
    fn symlink_file(target: &Path, link: &Path) -> std::io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    #[cfg(windows)]
    fn symlink_file(target: &Path, link: &Path) -> std::io::Result<()> {
        std::os::windows::fs::symlink_file(target, link)
    }

    #[test]
    fn symlinked_receipts_path_is_refused_without_writing_through() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("outside.jsonl");
        std::fs::write(&target, "sentinel\n").unwrap();
        let receipts_dir = temp.path().join("repo").join(".nosis");
        std::fs::create_dir_all(&receipts_dir).unwrap();
        let path = receipts_dir.join("receipts.jsonl");
        if symlink_file(&target, &path).is_err() {
            return;
        }

        let error = writer(&temp.path().join("repo"), path)
            .append(&receipt("blocked"))
            .unwrap_err();

        assert!(error
            .to_string()
            .contains("receipts path is not a regular file"));
        assert_eq!(std::fs::read_to_string(target).unwrap(), "sentinel\n");
    }

    #[test]
    fn concurrent_receipt_appends_are_complete_json_lines() {
        const WRITERS: usize = 2;
        const RECEIPTS_PER_WRITER: usize = 50;

        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(".nosis").join("receipts.jsonl");
        let root = temp.path().to_path_buf();
        let barrier = Arc::new(Barrier::new(WRITERS));
        let mut threads = Vec::new();
        for worker in 0..WRITERS {
            let path = path.clone();
            let root = root.clone();
            let barrier = Arc::clone(&barrier);
            threads.push(std::thread::spawn(move || {
                let writer = writer(&root, path);
                barrier.wait();
                for index in 0..RECEIPTS_PER_WRITER {
                    writer
                        .append(&receipt(format!("worker-{worker}-receipt-{index}")))
                        .unwrap();
                }
            }));
        }
        for thread in threads {
            thread.join().unwrap();
        }

        let contents = std::fs::read_to_string(path).unwrap();
        let receipts = contents
            .lines()
            .map(|line| serde_json::from_str::<Receipt>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(receipts.len(), WRITERS * RECEIPTS_PER_WRITER);
    }

    #[test]
    fn cache_percentage_serialization_distinguishes_absent_from_measured_zero() {
        let absent = serde_json::to_value(receipt("absent")).unwrap();
        assert!(absent.get("cache_hit_pct").is_none());

        let mut measured = receipt("measured zero");
        measured.usage = Some(crate::wire::Usage {
            prompt_tokens: 20,
            completion_tokens: 2,
            cached_tokens: Some(0),
            evidence: crate::wire::UsageEvidence::Measured,
        });
        measured.cache_hit_pct = Some(0.0);
        let measured = serde_json::to_value(measured).unwrap();
        assert_eq!(measured["cache_hit_pct"], 0.0);
        assert_eq!(measured["usage"]["cached_tokens"], 0);
    }

    #[test]
    fn legacy_usage_bytes_upgrade_to_unknown_evidence() {
        let old = br#"{"ts_utc":"2026-07-22T00:00:00Z","model_id":"test-model","task":"legacy","turns":1,"tool_calls":0,"outcome":"pass","usage":{"prompt_tokens":12,"completion_tokens":4,"cached_tokens":3}}"#;

        let parsed: Receipt = serde_json::from_slice(old).unwrap();
        assert_eq!(
            parsed.usage.as_ref().unwrap().evidence,
            crate::wire::UsageEvidence::Unknown
        );

        let upgraded = serde_json::to_vec(&parsed).unwrap();
        assert_eq!(
            upgraded,
            br#"{"ts_utc":"2026-07-22T00:00:00Z","model_id":"test-model","task":"legacy","turns":1,"tool_calls":0,"outcome":"pass","usage":{"prompt_tokens":12,"completion_tokens":4,"cached_tokens":3,"evidence":"unknown"}}"#
        );
    }

    #[test]
    fn repair_counters_are_omitted_when_empty_and_persisted_when_used() {
        let empty = serde_json::to_value(receipt("empty")).unwrap();
        assert!(empty.get("repairs").is_none());

        let mut repaired = receipt("repaired");
        repaired.repairs.tool_call_repair_attempts = 1;
        repaired.repairs.edit_indentation_matches = 2;
        let repaired = serde_json::to_value(repaired).unwrap();
        assert_eq!(repaired["repairs"]["tool_call_repair_attempts"], 1);
        assert_eq!(repaired["repairs"]["edit_indentation_matches"], 2);
    }

    #[test]
    fn retry_counters_preserve_empty_json_and_persist_when_used() {
        let empty = serde_json::to_string(&receipt("empty")).unwrap();
        assert_eq!(
            empty,
            r#"{"ts_utc":"2026-07-22T00:00:00Z","model_id":"test-model","task":"empty","turns":1,"tool_calls":0,"outcome":"pass"}"#
        );

        let mut retried = receipt("retried");
        retried.retries = crate::wire::RetryStats {
            retries: 2,
            rate_limited: 2,
        };
        let retried = serde_json::to_value(retried).unwrap();
        assert_eq!(retried["retries"]["retries"], 2);
        assert_eq!(retried["retries"]["rate_limited"], 2);
    }

    #[test]
    fn compaction_preserves_old_json_and_persists_nonempty_facts() {
        let old = r#"{"ts_utc":"2026-07-22T00:00:00Z","model_id":"test-model","task":"old","turns":1,"tool_calls":0,"outcome":"pass"}"#;
        let parsed: Receipt = serde_json::from_str(old).unwrap();
        assert_eq!(*parsed.compaction, CompactionStats::default());
        assert_eq!(serde_json::to_string(&parsed).unwrap(), old);

        let mut compacted = receipt("compacted");
        compacted
            .compaction
            .record_at(8, 512, Some(640), 1_785_432_100);
        let compacted = serde_json::to_value(compacted).unwrap();
        assert_eq!(compacted["compaction"]["events"], 1);
        assert_eq!(compacted["compaction"]["messages_elided"], 8);
        assert_eq!(compacted["compaction"]["estimated_tokens_elided"], 512);
        assert_eq!(compacted["compaction"]["preceding_cached_tokens"], 640);
        assert_eq!(
            compacted["compaction"]["occurred_at_unix_seconds"],
            1_785_432_100_i64
        );
    }
}
