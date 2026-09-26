//! Shared durability primitives for append-only JSONL files.

use anyhow::Context as _;
use serde::de::DeserializeOwned;
use std::io::Write as _;
use std::path::Path;

/// SECURITY INVARIANT: each complete line is locked, flushed, and synced before return.
pub(crate) fn append_locked_line(path: &Path, line: &str) -> anyhow::Result<()> {
    append_locked_line_inner(path, line, None, false)
}

/// Append one complete line while refusing growth beyond `max_bytes`.
pub(crate) fn append_locked_line_bounded(
    path: &Path,
    line: &str,
    max_bytes: u64,
) -> anyhow::Result<()> {
    append_locked_line_inner(path, line, Some(max_bytes), true)
}

fn append_locked_line_inner(
    path: &Path,
    line: &str,
    max_bytes: Option<u64>,
    nonblocking_lock: bool,
) -> anyhow::Result<()> {
    // read(true): Windows LockFileEx requires read/write DATA access on the
    // handle; a pure append-only handle (FILE_APPEND_DATA) fails file.lock()
    // with ACCESS_DENIED. Append semantics are preserved.
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("could not open {}", path.display()))?;
    if nonblocking_lock {
        file.try_lock()
            .with_context(|| format!("could not acquire {} without waiting", path.display()))?;
    } else {
        file.lock()
            .with_context(|| format!("could not lock {}", path.display()))?;
    }
    if let Some(max_bytes) = max_bytes {
        let current = file
            .metadata()
            .with_context(|| format!("could not inspect {}", path.display()))?
            .len();
        let appended = u64::try_from(line.len())
            .unwrap_or(u64::MAX)
            .saturating_add(1);
        if current.saturating_add(appended) > max_bytes {
            anyhow::bail!(
                "{} reached its {max_bytes}-byte growth limit",
                path.display()
            );
        }
    }
    writeln!(file, "{line}").with_context(|| format!("could not write {}", path.display()))?;
    file.flush()
        .with_context(|| format!("could not flush {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("could not fsync {}", path.display()))?;
    Ok(())
}

/// SECURITY INVARIANT: only one malformed final record without a newline is tolerated.
pub(crate) fn parse_jsonl_records<T, F>(
    bytes: &[u8],
    invalid_record: F,
) -> anyhow::Result<(Vec<T>, bool)>
where
    T: DeserializeOwned,
    F: Fn(usize, serde_json::Error) -> anyhow::Error,
{
    let ends_in_newline = bytes.last() == Some(&b'\n');
    let lines = bytes.split(|byte| *byte == b'\n').collect::<Vec<_>>();
    let last_non_empty = lines
        .iter()
        .rposition(|line| !line.iter().all(u8::is_ascii_whitespace));
    let mut records = Vec::new();
    let mut dropped_torn_tail = false;
    for (index, line) in lines.into_iter().enumerate() {
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        match serde_json::from_slice::<T>(line) {
            Ok(record) => records.push(record),
            Err(_) if Some(index) == last_non_empty && !ends_in_newline => {
                dropped_torn_tail = true;
            }
            Err(error) => return Err(invalid_record(index + 1, error)),
        }
    }
    Ok((records, dropped_torn_tail))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_values(bytes: &[u8]) -> anyhow::Result<(Vec<serde_json::Value>, bool)> {
        parse_jsonl_records(bytes, |line, error| {
            anyhow::anyhow!("test JSONL line {line} is invalid: {error}")
        })
    }

    #[test]
    fn parser_drops_only_a_torn_final_record() {
        let (records, dropped_torn_tail) = parse_values(b"{\"value\":1}\n{\"value\":2").unwrap();
        assert_eq!(records, [serde_json::json!({"value": 1})]);
        assert!(dropped_torn_tail);

        let error = parse_values(b"{\"value\":1}\n{bad}\n{\"value\":2").unwrap_err();
        assert!(
            error.to_string().contains("line 2 is invalid"),
            "got: {error}"
        );
    }

    #[test]
    fn parser_keeps_newline_terminated_records_without_a_drop() {
        let (records, dropped_torn_tail) = parse_values(b"{\"value\":1}\n{\"value\":2}\n").unwrap();
        assert_eq!(
            records,
            [
                serde_json::json!({"value": 1}),
                serde_json::json!({"value": 2})
            ]
        );
        assert!(!dropped_torn_tail);
    }
}
