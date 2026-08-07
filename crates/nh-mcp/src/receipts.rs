//! Receipt tail loading, JSONL recovery, and MCP projection.

use crate::response::{tool_error, tool_result};
use crate::Runtime;
use anyhow::Context as _;
use nh_core::receipt::{parse_receipt_jsonl, read_receipt_tail};
use nh_core::wire::{Usage, UsageEvidence};
use serde::Deserialize;
use serde_json::{json, Value};

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
    let receipts = match parse_receipt_jsonl(&bytes, limit) {
        Ok(receipts) => receipts,
        Err(error) => return tool_error(runtime, &error.to_string()),
    };
    let values = match receipts
        .into_iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()
        .context("could not serialize a parsed receipt")
    {
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
            let tokens = receipt_usage_text(receipt);
            format!("{ts} | {model} | {outcome} | {turns} turns | {tokens}")
        })
        .collect::<Vec<_>>()
        .join(" || ");
    format!("receipts: {} | {rows}", receipts.len())
}

fn receipt_usage_text(receipt: &Value) -> String {
    let Some(value) = receipt.get("usage") else {
        return "unmetered".into();
    };
    let Ok(usage) = serde_json::from_value::<Usage>(value.clone()) else {
        return "usage unavailable".into();
    };
    match usage.evidence {
        UsageEvidence::Measured => usage
            .prompt_tokens
            .checked_add(usage.completion_tokens)
            .map_or_else(
                || "token total unavailable (overflow)".into(),
                |tokens| format!("{tokens} tokens"),
            ),
        UsageEvidence::Partial => usage
            .prompt_tokens
            .checked_add(usage.completion_tokens)
            .map_or_else(
                || "token lower bound unavailable (overflow)".into(),
                |tokens| format!("~{tokens} tokens (lower bound)"),
            ),
        UsageEvidence::Unknown => "usage unknown".into(),
    }
}
