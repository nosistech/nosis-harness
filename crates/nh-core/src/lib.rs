//! nh-core - agent turn loop, wire client, receipts.
//! Every turn writes a scrubbed JSONL receipt to .nosis/receipts.jsonl (append-only).

pub mod context_experiment;
pub mod cost;
pub mod credential;
pub mod efficiency;
mod jsonl;
pub mod runtime_path;
pub mod terminal_capability;

pub mod agent;
pub mod receipt;
pub mod session_ledger;
pub mod wire;

/// Serialize a workspace durable record after structurally scrubbing decoded
/// JSON keys and string values. OpenAI-style `tool_calls[].arguments` strings
/// are decoded and scrubbed too; other strings that happen to contain encoded
/// JSON are treated as ordinary strings.
pub fn serialize_scrubbed_json<T: serde::Serialize>(
    value: &T,
    scrubber: &nh_vault::Scrubber,
    record_name: &str,
) -> anyhow::Result<String> {
    jsonl::serialize_scrubbed(value, scrubber, record_name)
}
