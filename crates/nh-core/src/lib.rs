//! nh-core - agent turn loop, wire client, receipts.
//! Every turn writes a scrubbed JSONL receipt to .nosis/receipts.jsonl (append-only).

pub mod credential;
pub mod runtime_path;

pub mod agent;
pub mod receipt;
pub mod wire;
