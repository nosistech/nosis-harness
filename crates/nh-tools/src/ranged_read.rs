//! Operator-enabled, bounded line ranges for the existing `read_file` tool.

use super::{
    cancelled_before, render_tool_result, resolve_in_workdir, str_arg, Access, Guard, Tool,
    ToolCtx, ToolSpec, BINARY_SNIFF_BYTES, MAX_RANGED_FILE_BYTES, MAX_RANGED_LINE_BYTES,
    MAX_RANGED_LINE_COUNT, MAX_RANGED_START_LINE, MAX_TOOL_READ_BYTES, TOOL_BUFFER_BYTES,
};
use anyhow::{bail, Context as _};
use serde_json::json;
use std::io::{BufReader, Read as _, Seek as _, SeekFrom};
use std::sync::atomic::Ordering;

pub(super) struct RangedReadFile;

impl Tool for RangedReadFile {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "read_file".into(),
            description: "Read a UTF-8 text file inside the working directory. Optionally read a bounded 1-based line range by supplying both start_line and line_count.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path, relative to the working directory."
                    },
                    "start_line": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": MAX_RANGED_START_LINE,
                        "description": "First line to return (1-based); supply together with line_count."
                    },
                    "line_count": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": MAX_RANGED_LINE_COUNT,
                        "description": "Maximum lines to return; supply together with start_line."
                    }
                },
                "required": ["path"]
            }),
        }
    }

    fn execute(&self, args: serde_json::Value, ctx: &ToolCtx) -> anyhow::Result<String> {
        let path = str_arg(&args, "path")?;
        let Some(range) = parse_range(&args)? else {
            return super::ReadFile.execute(args, ctx);
        };
        if let Some(cancelled) = cancelled_before("file read", ctx) {
            return Ok(cancelled);
        }
        let (resolved, relative) = resolve_in_workdir(&ctx.workdir, path)?;
        match (ctx.guard)(&Access::Read(&relative)) {
            Guard::Block(reason) => {
                return Ok(render_tool_result(format!("blocked by law: {reason}"), ctx))
            }
            Guard::Ask => {
                let action = format!("read {relative}");
                if !(ctx.approve)(&action) {
                    return Ok(render_tool_result(format!("user denied: {action}"), ctx));
                }
            }
            Guard::Allow => {}
        }
        if let Some(cancelled) = cancelled_before("file read", ctx) {
            return Ok(cancelled);
        }
        if !resolved.is_file() {
            bail!("file not found: {path} - check the path against the working directory");
        }
        let file =
            std::fs::File::open(&resolved).with_context(|| format!("could not read {path}"))?;
        read_range(file, path, range, ctx)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReadRange {
    start_line: u64,
    line_count: u64,
}

fn parse_range(args: &serde_json::Value) -> anyhow::Result<Option<ReadRange>> {
    let start = args.get("start_line");
    let count = args.get("line_count");
    let (Some(start), Some(count)) = (start, count) else {
        if start.is_some() || count.is_some() {
            bail!("start_line and line_count must be supplied together");
        }
        return Ok(None);
    };
    let start_line = positive_integer(start, "start_line")?;
    let line_count = positive_integer(count, "line_count")?;
    if start_line > MAX_RANGED_START_LINE {
        bail!("start_line exceeds the {MAX_RANGED_START_LINE}-line scan limit");
    }
    if line_count > MAX_RANGED_LINE_COUNT {
        bail!("line_count exceeds the {MAX_RANGED_LINE_COUNT}-line result limit");
    }
    start_line
        .checked_add(line_count.saturating_sub(1))
        .ok_or_else(|| anyhow::anyhow!("requested line range overflows"))?;
    Ok(Some(ReadRange {
        start_line,
        line_count,
    }))
}

fn positive_integer(value: &serde_json::Value, name: &str) -> anyhow::Result<u64> {
    value
        .as_u64()
        .filter(|value| *value > 0)
        .ok_or_else(|| anyhow::anyhow!("{name} must be a positive whole number"))
}

fn read_range(
    mut file: std::fs::File,
    path: &str,
    range: ReadRange,
    ctx: &ToolCtx,
) -> anyhow::Result<String> {
    let file_bytes = file
        .metadata()
        .with_context(|| format!("could not inspect {path}"))?
        .len();
    if file_bytes > MAX_RANGED_FILE_BYTES {
        bail!("file is too large for a ranged read (> {MAX_RANGED_FILE_BYTES} bytes): {path}");
    }

    let mut sniff = Vec::with_capacity(BINARY_SNIFF_BYTES as usize);
    (&mut file)
        .take(BINARY_SNIFF_BYTES)
        .read_to_end(&mut sniff)
        .with_context(|| format!("could not read {path}"))?;
    if sniff.contains(&0) {
        bail!("file looks binary: {path} - choose a text file or use a binary-aware tool");
    }
    file.seek(SeekFrom::Start(0))
        .with_context(|| format!("could not seek {path}"))?;

    scan_range(file, path, range, ctx)
}

fn scan_range(
    file: std::fs::File,
    path: &str,
    range: ReadRange,
    ctx: &ToolCtx,
) -> anyhow::Result<String> {
    let requested_end = range
        .start_line
        .checked_add(range.line_count.saturating_sub(1))
        .ok_or_else(|| anyhow::anyhow!("requested line range overflows"))?;
    // The extra byte distinguishes a real EOF at the limit from a file that
    // grew after the metadata check. It is a sentinel and is never consumed.
    let limited = file.take(MAX_RANGED_FILE_BYTES.saturating_add(1));
    let mut reader = BufReader::with_capacity(TOOL_BUFFER_BYTES, limited);
    let mut line = Vec::with_capacity(TOOL_BUFFER_BYTES);
    let mut selected = Vec::new();
    let mut line_number = 0_u64;
    let mut scanned_bytes = 0_u64;
    let mut reached_eof = false;
    while line_number < requested_end {
        if ctx.cancel.load(Ordering::Acquire) {
            return Ok(render_tool_result(
                "turn cancelled during file read".into(),
                ctx,
            ));
        }
        let next_line = line_number.saturating_add(1);
        if !read_line_bounded(
            &mut reader,
            &mut line,
            path,
            next_line,
            MAX_RANGED_FILE_BYTES.saturating_sub(scanned_bytes),
        )? {
            reached_eof = true;
            break;
        }
        scanned_bytes = scanned_bytes.saturating_add(u64::try_from(line.len()).unwrap_or(u64::MAX));
        if scanned_bytes > MAX_RANGED_FILE_BYTES {
            bail!(
                "file grew beyond the {MAX_RANGED_FILE_BYTES}-byte ranged-read scan limit: {path}"
            );
        }
        line_number = next_line;
        if line_number >= range.start_line {
            if selected.len().saturating_add(line.len()) > MAX_TOOL_READ_BYTES {
                bail!(
                    "requested range exceeds the {MAX_TOOL_READ_BYTES}-byte result limit: {path}"
                );
            }
            selected.extend_from_slice(&line);
        }
    }
    if line_number < range.start_line {
        bail!(
            "start_line {} is past end of file ({line_number} lines): {path}",
            range.start_line
        );
    }

    let actual_end = line_number.min(requested_end);
    let (mut content, replaced_invalid_utf8) = decode_text(selected);
    if replaced_invalid_utf8 {
        content.push_str("\n...[some bytes were not valid UTF-8 and were replaced]");
    }
    let eof = if reached_eof && actual_end < requested_end {
        " (EOF reached)"
    } else {
        ""
    };
    Ok(render_tool_result(
        format!(
            "lines {}-{actual_end} of {path}{eof}:\n{content}",
            range.start_line
        ),
        ctx,
    ))
}

fn read_line_bounded(
    reader: &mut impl std::io::BufRead,
    line: &mut Vec<u8>,
    path: &str,
    line_number: u64,
    remaining_scan_bytes: u64,
) -> anyhow::Result<bool> {
    line.clear();
    loop {
        let available = reader
            .fill_buf()
            .with_context(|| format!("could not read {path}"))?;
        if available.is_empty() {
            return Ok(!line.is_empty());
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.map_or(available.len(), |index| index.saturating_add(1));
        if line.len().saturating_add(take) > MAX_RANGED_LINE_BYTES {
            bail!(
                "line {line_number} exceeds the {MAX_RANGED_LINE_BYTES}-byte ranged-read limit: {path}"
            );
        }
        let prospective_line_bytes =
            u64::try_from(line.len().saturating_add(take)).unwrap_or(u64::MAX);
        if prospective_line_bytes > remaining_scan_bytes {
            bail!(
                "file grew beyond the {MAX_RANGED_FILE_BYTES}-byte ranged-read scan limit: {path}"
            );
        }
        line.extend_from_slice(&available[..take]);
        let complete = newline.is_some();
        reader.consume(take);
        if complete {
            return Ok(true);
        }
    }
}

fn decode_text(bytes: Vec<u8>) -> (String, bool) {
    match String::from_utf8(bytes) {
        Ok(content) => (content, false),
        Err(error) => (
            String::from_utf8_lossy(&error.into_bytes()).into_owned(),
            true,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    #[test]
    fn scan_limit_refuses_growth_instead_of_reporting_a_false_eof() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("grown.txt");
        let mut file = std::io::BufWriter::new(std::fs::File::create(&path).unwrap());
        let mut line = vec![b'x'; 99];
        line.push(b'\n');
        for _ in 0..(MAX_RANGED_FILE_BYTES / 100 + 2) {
            file.write_all(&line).unwrap();
        }
        file.flush().unwrap();
        drop(file);
        let ctx = ToolCtx::new(
            dir.path().to_path_buf(),
            Box::new(|_| false),
            Box::new(|_| Guard::Allow),
            nh_vault::Scrubber::new(Vec::new()),
        );

        let error = scan_range(
            std::fs::File::open(path).unwrap(),
            "grown.txt",
            ReadRange {
                start_line: MAX_RANGED_START_LINE,
                line_count: 1,
            },
            &ctx,
        )
        .unwrap_err()
        .to_string();

        assert!(
            error.contains("grew beyond the 8388608-byte ranged-read scan limit"),
            "got: {error}"
        );
    }
}
