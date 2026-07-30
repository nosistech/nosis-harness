//! JSON-RPC method catalog and tool dispatch.

use crate::fleet_tools::{fleet_run, fleet_status};
use crate::receipts::receipts;
use crate::response::tool_error;
use crate::route_tools::{route_cost, route_resolve, why};
use crate::{Runtime, MAX_MCP_FLEET_BUDGET_TOKENS};
use serde_json::{json, Value};

pub(super) fn business_card() -> Value {
    json!({
        "name": "nh-mcp",
        "spec": "2026-07-28",
        "tools": ["fleet_run", "fleet_status", "receipts", "route_cost", "route_resolve", "why"],
        "notice": "local/preview only"
    })
}

pub(super) fn rpc_success(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

pub(super) fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
}

pub(super) fn tools_list() -> Value {
    json!({
        "tools": [
            {
                "name": "route_resolve",
                "description": "Resolve one catalog route with current peak status.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "model": { "type": "string" },
                        "prefer_offpeak": { "type": "boolean" }
                    }
                },
                "outputSchema": {
                    "type": "object",
                    "properties": {
                        "route": { "type": "object" },
                        "would_park_offpeak": { "type": "boolean" }
                    },
                    "required": ["route", "would_park_offpeak"]
                },
                "annotations": { "readOnlyHint": true }
            },
            {
                "name": "why",
                "description": "Choose the cheapest capable route and explain every skipped route.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "task": { "type": "string" },
                        "prompt_tokens": { "type": "integer", "minimum": 0 },
                        "output_tokens": { "type": "integer", "minimum": 0 },
                        "allowed": {
                            "type": "array",
                            "items": { "type": "string" }
                        },
                        "prefer_offpeak": { "type": "boolean" }
                    },
                    "required": ["prompt_tokens", "output_tokens"]
                },
                "outputSchema": {
                    "type": "object",
                    "properties": {
                        "route": { "type": "object" },
                        "cost": { "type": "object" },
                        "savings": { "type": "object" },
                        "rejected": { "type": "array", "items": { "type": "object" } }
                    },
                    "required": ["route", "cost", "rejected"]
                },
                "annotations": { "readOnlyHint": true }
            },
            {
                "name": "route_cost",
                "description": "Price one catalog route for explicit prompt, cache, and output tokens.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "model": { "type": "string" },
                        "prompt_tokens": { "type": "integer", "minimum": 0 },
                        "cached_tokens": { "type": "integer", "minimum": 0 },
                        "output_tokens": { "type": "integer", "minimum": 0 }
                    },
                    "required": ["prompt_tokens", "output_tokens"]
                },
                "outputSchema": {
                    "type": "object",
                    "properties": {
                        "route": { "type": "object" },
                        "quote": { "type": "object" },
                        "cost": { "type": "object" }
                    },
                    "required": ["route", "quote", "cost"]
                },
                "annotations": { "readOnlyHint": true }
            },
            {
                "name": "receipts",
                "description": "Read recent metered receipts from this server's repository root.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "limit": { "type": "integer", "minimum": 1, "maximum": 100 }
                    }
                },
                "outputSchema": {
                    "type": "object",
                    "properties": {
                        "count": { "type": "integer" },
                        "receipts": { "type": "array", "items": { "type": "object" } }
                    },
                    "required": ["count", "receipts"]
                },
                "annotations": { "readOnlyHint": true }
            },
            {
                "name": "fleet_run",
                "description": "Start a durable fleet run. A required observed-token budget stops new dispatch after reported usage reaches it.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "tasks": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "id": { "type": "string" },
                                    "task": { "type": "string" },
                                    "model": { "type": "string" },
                                    "defer_offpeak": { "type": "boolean" },
                                    "backend": { "type": "string" }
                                },
                                "required": ["task"]
                            }
                        },
                        "max_workers": { "type": "integer" },
                        "budget": {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": MAX_MCP_FLEET_BUDGET_TOKENS
                        },
                        "defer_offpeak": { "type": "boolean" }
                    },
                    "required": ["tasks", "budget"]
                },
                "outputSchema": {
                    "type": "object",
                    "properties": {
                        "run_id": { "type": "string" },
                        "task_count": { "type": "integer" }
                    },
                    "required": ["run_id", "task_count"]
                },
                "annotations": { "readOnlyHint": false }
            },
            {
                "name": "fleet_status",
                "description": "Read current counts from a durable fleet ledger.",
                "inputSchema": {
                    "type": "object",
                    "properties": { "run_id": { "type": "string" } },
                    "required": ["run_id"]
                },
                "outputSchema": {
                    "type": "object",
                    "properties": {
                        "run_id": { "type": "string" },
                        "state": {
                            "type": "string",
                            "enum": ["finished", "failed", "running", "starting", "unknown"]
                        },
                        "failed_reason": { "type": "string" },
                        "done": { "type": "integer" },
                        "failed": { "type": "integer" },
                        "gated": { "type": "integer" },
                        "pending": { "type": "integer" },
                        "unmetered": { "type": "integer" }
                    },
                    "required": ["run_id", "state", "done", "failed", "gated", "pending", "unmetered"]
                },
                "annotations": { "readOnlyHint": true }
            }
        ],
        "_meta": { "ttlMs": 60000 }
    })
}

pub(super) fn tools_call(params: &Value, runtime: &Runtime) -> Value {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return tool_error(runtime, "tools/call needs a tool name");
    };
    let empty_arguments = json!({});
    let arguments = params.get("arguments").unwrap_or(&empty_arguments);
    match name {
        "route_resolve" => route_resolve(arguments, runtime),
        "why" => why(arguments, runtime),
        "route_cost" => route_cost(arguments, runtime),
        "receipts" => receipts(arguments, runtime),
        "fleet_run" => fleet_run(arguments, runtime),
        "fleet_status" => fleet_status(arguments, runtime),
        other => tool_error(runtime, &format!("unknown tool '{other}' — use tools/list")),
    }
}
