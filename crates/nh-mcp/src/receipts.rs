//! Receipt tail loading, JSONL recovery, and MCP projection.

use super::*;

#[derive(Deserialize)]
struct ReceiptsArgs {
    #[serde(default)]
    limit: Option<usize>,
}

pub(super) fn receipts(arguments: &Value, runtime: &Runtime) -> Value {
    let args: ReceiptsArgs = match serde_json::from_value(arguments.clone()) {
        Ok(args) => args,
        Err(error) => return tool_error(runtime, &error.to_string()),
    };
    let limit = args.limit.unwrap_or(10).clamp(1, 100);
    let bytes = match read_receipt_tail(&runtime.config.run_root) {
        Ok(bytes) => bytes,
        Err(error) => return tool_error(runtime, &format!("could not read receipts: {error}")),
    };
    let values = match parse_receipt_jsonl(&bytes, limit) {
        Ok(values) => values,
        Err(error) => return tool_error(runtime, &error.to_string()),
    };
    let text = receipts_text(&values);
    let structured = json!({
        "count": values.len(),
        "receipts": values
    });
    tool_result(runtime, &text, structured, false)
}

pub(super) fn read_receipt_tail(run_root: &Path) -> anyhow::Result<Vec<u8>> {
    let Some(nosis_dir) =
        nh_core::runtime_path::resolve_contained_dir(run_root, Path::new(".nosis"))?
    else {
        return Ok(Vec::new());
    };
    let path = nosis_dir.join("receipts.jsonl");
    nh_core::runtime_path::reject_symlink_or_special_file(&path, "receipts")?;
    let mut file = match File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error).with_context(|| format!("could not open {}", path.display()))
        }
    };
    let len = file
        .metadata()
        .with_context(|| format!("could not inspect {}", path.display()))?
        .len();
    let start = len.saturating_sub(MAX_RECEIPT_TAIL_BYTES as u64);
    let starts_mid_line = if start == 0 {
        false
    } else {
        file.seek(SeekFrom::Start(start - 1))?;
        let mut previous = [0_u8; 1];
        file.read_exact(&mut previous)?;
        previous[0] != b'\n'
    };
    file.seek(SeekFrom::Start(start))?;

    let mut bytes = Vec::new();
    file.take(MAX_RECEIPT_TAIL_BYTES as u64)
        .read_to_end(&mut bytes)
        .with_context(|| format!("could not read {}", path.display()))?;
    if starts_mid_line {
        let Some(newline) = bytes.iter().position(|byte| *byte == b'\n') else {
            bail!(
                "recent receipt exceeds the {}-byte safe read window",
                MAX_RECEIPT_TAIL_BYTES
            );
        };
        bytes.drain(..=newline);
    }
    Ok(bytes)
}

pub(super) fn parse_receipt_jsonl(bytes: &[u8], limit: usize) -> anyhow::Result<Vec<Value>> {
    let ends_in_newline = bytes.last() == Some(&b'\n');
    let lines: Vec<&[u8]> = bytes.split(|byte| *byte == b'\n').collect();
    let last_non_empty = lines
        .iter()
        .rposition(|line| !line.iter().all(|byte| byte.is_ascii_whitespace()));
    let mut receipts = VecDeque::with_capacity(limit);
    for (index, line) in lines.into_iter().enumerate() {
        if line.iter().all(|byte| byte.is_ascii_whitespace()) {
            continue;
        }
        let value: Value = match serde_json::from_slice(line) {
            Ok(value) => value,
            Err(_) if !ends_in_newline && Some(index) == last_non_empty => continue,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("receipts line {} is invalid", index + 1));
            }
        };
        let event: nh_fleet::LedgerEvent = serde_json::from_value(json!({
            "event": "task_receipt",
            "task_id": "nh-mcp-receipt",
            "attempt": 1,
            "receipt": value
        }))
        .with_context(|| format!("receipts line {} is invalid", index + 1))?;
        let nh_fleet::LedgerEvent::TaskReceipt { receipt, .. } = event else {
            unreachable!("static wrapper always selects task_receipt");
        };
        if receipts.len() == limit {
            receipts.pop_front();
        }
        receipts.push_back(
            serde_json::to_value(receipt).context("could not serialize a parsed receipt")?,
        );
    }
    Ok(receipts.into_iter().collect())
}

pub(super) fn receipts_text(receipts: &[Value]) -> String {
    if receipts.is_empty() {
        return "receipts: 0".into();
    }
    let rows = receipts
        .iter()
        .map(|receipt| {
            let ts = receipt.get("ts_utc").and_then(Value::as_str).unwrap_or("?");
            let model = receipt
                .get("model_id")
                .and_then(Value::as_str)
                .unwrap_or("?");
            let outcome = receipt
                .get("outcome")
                .and_then(Value::as_str)
                .unwrap_or("?");
            let turns = receipt.get("turns").and_then(Value::as_u64).unwrap_or(0);
            let tokens = receipt.get("usage").map_or_else(
                || "unmetered".to_string(),
                |usage| {
                    let prompt = usage
                        .get("prompt_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    let completion = usage
                        .get("completion_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    format!("{} tokens", prompt.saturating_add(completion))
                },
            );
            format!("{ts} | {model} | {outcome} | {turns} turns | {tokens}")
        })
        .collect::<Vec<_>>()
        .join(" || ");
    format!("receipts: {} | {rows}", receipts.len())
}
