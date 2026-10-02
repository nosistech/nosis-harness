//! Shared durability primitives for append-only JSONL files.

use anyhow::Context as _;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::io::Write as _;
use std::path::Path;

const OMITTED_TOOL_ARGUMENTS: &str = r#"{"_nosis":"tool arguments omitted from durable record"}"#;

/// Serialize durable JSON only after scrubbing decoded string values.
///
/// Tool-call arguments are JSON encoded inside a string, so they receive a
/// second structural pass before the outer record is encoded. Invalid nested
/// JSON is replaced rather than persisted without proven redaction.
pub(crate) fn serialize_scrubbed<T: Serialize>(
    value: &T,
    scrubber: &nh_vault::Scrubber,
    record_name: &str,
) -> anyhow::Result<String> {
    let mut value = serde_json::to_value(value)
        .with_context(|| format!("could not serialize {record_name}"))?;
    scrub_tool_arguments(&mut value, scrubber)?;
    scrub_json_value(&mut value, scrubber)?;
    serde_json::to_string(&value).with_context(|| format!("could not serialize {record_name}"))
}

/// Parse and structurally scrub one already-serialized JSON value.
pub(crate) fn scrub_json_text(text: &str, scrubber: &nh_vault::Scrubber) -> anyhow::Result<String> {
    let mut value = serde_json::from_str(text).context("could not parse JSON for redaction")?;
    scrub_json_value(&mut value, scrubber)?;
    serde_json::to_string(&value).context("could not serialize scrubbed JSON")
}

fn scrub_tool_arguments(
    value: &mut serde_json::Value,
    scrubber: &nh_vault::Scrubber,
) -> anyhow::Result<()> {
    match value {
        serde_json::Value::Array(values) => {
            for value in values {
                scrub_tool_arguments(value, scrubber)?;
            }
        }
        serde_json::Value::Object(fields) => {
            if let Some(serde_json::Value::Array(calls)) = fields.get_mut("tool_calls") {
                for call in calls {
                    let Some(arguments) = call
                        .as_object_mut()
                        .and_then(|call| call.get_mut("arguments"))
                        .and_then(|arguments| arguments.as_str())
                    else {
                        continue;
                    };
                    let scrubbed = scrub_argument_string(arguments, scrubber)
                        .unwrap_or_else(|_| OMITTED_TOOL_ARGUMENTS.to_owned());
                    call["arguments"] = serde_json::Value::String(scrubbed);
                }
            }
            for value in fields.values_mut() {
                scrub_tool_arguments(value, scrubber)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn scrub_argument_string(arguments: &str, scrubber: &nh_vault::Scrubber) -> anyhow::Result<String> {
    if arguments.trim().is_empty() {
        return Ok(arguments.to_owned());
    }
    let mut nested = serde_json::from_str::<serde_json::Value>(arguments)
        .context("could not parse tool arguments for redaction")?;
    let original = nested.clone();
    scrub_json_value(&mut nested, scrubber)?;
    if nested == original && !arguments.contains('\\') {
        return Ok(arguments.to_owned());
    }
    serde_json::to_string(&nested).context("could not serialize scrubbed tool arguments")
}

fn scrub_json_value(
    value: &mut serde_json::Value,
    scrubber: &nh_vault::Scrubber,
) -> anyhow::Result<()> {
    match value {
        serde_json::Value::String(text) => *text = scrubber.scrub(text),
        serde_json::Value::Array(values) => {
            for value in values {
                scrub_json_value(value, scrubber)?;
            }
        }
        serde_json::Value::Object(fields) => {
            let original = std::mem::take(fields);
            for (name, mut field) in original {
                scrub_json_value(&mut field, scrubber)?;
                let name = scrubber.scrub(&name);
                if fields.insert(name, field).is_some() {
                    anyhow::bail!("scrubbing produced duplicate JSON object keys");
                }
            }
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {}
    }
    Ok(())
}

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
    let mut options = std::fs::OpenOptions::new();
    options.read(true).create(true).append(true);
    set_private_create_mode(&mut options);
    let mut file = options
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

#[cfg(unix)]
fn set_private_create_mode(options: &mut std::fs::OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt as _;

    options.mode(0o600);
}

#[cfg(not(unix))]
fn set_private_create_mode(_options: &mut std::fs::OpenOptions) {}

/// Visit complete JSONL records while tolerating only one torn final append.
pub(crate) fn for_each_jsonl_record<T, F, V>(
    bytes: &[u8],
    invalid_record: F,
    mut visitor: V,
) -> anyhow::Result<bool>
where
    T: DeserializeOwned,
    F: Fn(usize, serde_json::Error) -> anyhow::Error,
    V: FnMut(T) -> anyhow::Result<()>,
{
    let ends_in_newline = bytes.last() == Some(&b'\n');
    let last_non_empty = bytes
        .split(|byte| *byte == b'\n')
        .enumerate()
        .filter(|(_, line)| !line.iter().all(u8::is_ascii_whitespace))
        .map(|(index, _)| index)
        .last();
    let mut dropped_torn_tail = false;
    for (index, line) in bytes.split(|byte| *byte == b'\n').enumerate() {
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        match serde_json::from_slice::<T>(line) {
            Ok(record) => visitor(record)?,
            Err(_) if Some(index) == last_non_empty && !ends_in_newline => {
                dropped_torn_tail = true;
            }
            Err(error) => return Err(invalid_record(index + 1, error)),
        }
    }
    Ok(dropped_torn_tail)
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
    let mut records = Vec::new();
    let dropped_torn_tail = for_each_jsonl_record(bytes, invalid_record, |record| {
        records.push(record);
        Ok(())
    })?;
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

    #[cfg(unix)]
    #[test]
    fn append_creates_private_files_without_chmodding_existing_files() {
        use std::os::unix::fs::PermissionsExt as _;

        let directory = tempfile::tempdir().unwrap();
        let created = directory.path().join("created.jsonl");
        append_locked_line(&created, "{}").unwrap();
        assert_eq!(
            std::fs::metadata(&created).unwrap().permissions().mode() & 0o777,
            0o600
        );

        let existing = directory.path().join("existing.jsonl");
        std::fs::write(&existing, b"{}\n").unwrap();
        std::fs::set_permissions(&existing, std::fs::Permissions::from_mode(0o640)).unwrap();
        append_locked_line(&existing, "{}").unwrap();
        assert_eq!(
            std::fs::metadata(&existing).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }
}
