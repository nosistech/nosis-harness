//! Typed receipts (plan §2): why runs fail, not just that they failed.

use anyhow::Context as _;

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
}
