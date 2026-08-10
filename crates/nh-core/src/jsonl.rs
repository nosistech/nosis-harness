//! Shared durability primitives for append-only JSONL files.

use anyhow::Context as _;
use serde::de::DeserializeOwned;
use std::io::Write as _;
use std::path::Path;

/// SECURITY INVARIANT: each complete line is locked, flushed, and synced before return.
pub(crate) fn append_locked_line(path: &Path, line: &str) -> anyhow::Result<()> {
    // read(true): Windows LockFileEx requires read/write DATA access on the
    // handle; a pure append-only handle (FILE_APPEND_DATA) fails file.lock()
    // with ACCESS_DENIED. Append semantics are preserved.
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .create(true)
        .append(true)
        .open(path)
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
