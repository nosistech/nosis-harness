//! Tool adapters that project remote MCP tools into the Nosis approval model.

use super::client::{McpClient, ToolEntry, ARGS_SUMMARY_MAX};
use super::config::{McpServerConfig, McpTrust};
use crate::{cancelled_before, render_tool_result, Tool, ToolCtx, ToolSpec};
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

const MAX_EXPOSED_TOOL_NAME_BYTES: usize = 64;
const MAX_DISCOVERY_QUERY_CHARS: usize = 128;
const MAX_DISCOVERY_RESULTS: usize = 8;
const MAX_DISCOVERY_ITEM_BYTES: usize = 8 * 1024;
const MAX_DISCOVERY_OUTPUT_BYTES: usize = 24 * 1024;
const MCP_DISCOVER: &str = "mcp_discover";
const MCP_INVOKE: &str = "mcp_invoke";

/// Adapters for every configured server, plus one friendly warning line per
/// server whose tools could not be listed (never a hard failure).
pub struct McpToolset {
    pub tools: Vec<Box<dyn Tool>>,
    pub warnings: Vec<String>,
}

/// Build one adapter per server tool, named `mcp__<server>__<tool>`.
/// `trust = "block"` servers are never contacted and offer no tools.
pub fn mcp_tools(configs: &[McpServerConfig], send_allowed: &dyn Fn(&str) -> bool) -> McpToolset {
    let (adapters, warnings) = collect_adapters(configs, send_allowed, false);
    let tools = adapters
        .into_iter()
        .map(|adapter| Box::new(adapter) as Box<dyn Tool>)
        .collect();
    McpToolset { tools, warnings }
}

/// Build two fixed tools over the same eagerly discovered MCP adapters. Remote
/// schemas become available only as bounded tool results, and invocation is
/// allowed only after that exact name was returned in this chat session.
pub fn mcp_discovery_tools(
    configs: &[McpServerConfig],
    send_allowed: &dyn Fn(&str) -> bool,
) -> McpToolset {
    let (adapters, warnings) = collect_adapters(configs, send_allowed, true);
    if adapters.is_empty() {
        return McpToolset {
            tools: Vec::new(),
            warnings,
        };
    }
    let registry = Arc::new(McpDiscoveryRegistry {
        adapters,
        discovered: Mutex::new(HashSet::new()),
    });
    let tools: Vec<Box<dyn Tool>> = vec![
        Box::new(McpDiscoverTool {
            registry: Arc::clone(&registry),
        }),
        Box::new(McpInvokeTool { registry }),
    ];
    McpToolset { tools, warnings }
}

fn collect_adapters(
    configs: &[McpServerConfig],
    send_allowed: &dyn Fn(&str) -> bool,
    report_blocked: bool,
) -> (Vec<McpToolAdapter>, Vec<String>) {
    let mut adapters = Vec::new();
    let mut warnings = Vec::new();
    for config in configs {
        if config.trust == McpTrust::Block {
            if report_blocked {
                warnings.push(format!(
                    "mcp server \"{}\": blocked by .nosis/mcp.toml - not contacted",
                    config.name
                ));
            }
            continue;
        }
        let server = config.name.clone();
        if !safe_name_component(&server) {
            warnings.push(
                "an MCP server name is not safe to expose as a model tool name; server not contacted"
                    .to_string(),
            );
            continue;
        }
        let Some(host) = nh_vault::host_of(&config.url) else {
            warnings.push(format!(
                "mcp server \"{server}\": could not parse a host from its url - not contacted"
            ));
            continue;
        };
        if !send_allowed(&host) {
            warnings.push(format!(
                "mcp server \"{server}\": destination {host} is blocked by law ([send]) - not contacted"
            ));
            continue;
        }
        let trust = config.trust;
        let client = match McpClient::new(config.clone()) {
            Ok(client) => Arc::new(client),
            Err(error) => {
                warnings.push(format!("mcp server \"{server}\": {error}"));
                continue;
            }
        };
        match client.list_tools_full() {
            Ok(entries) => {
                let mut excluded_names = 0_usize;
                for entry in entries {
                    let Some(exposed_name) = exposed_tool_name(&server, &entry.info.name) else {
                        excluded_names += 1;
                        continue;
                    };
                    adapters.push(McpToolAdapter {
                        server: server.clone(),
                        exposed_name,
                        trust,
                        entry,
                        client: Arc::clone(&client),
                    });
                }
                if excluded_names > 0 {
                    warnings.push(format!(
                        "mcp server \"{server}\": excluded {excluded_names} tools whose names cannot be exposed safely"
                    ));
                }
            }
            Err(e) => warnings.push(format!("mcp server \"{server}\": {e}")),
        }
    }
    adapters.sort_by(|left, right| left.exposed_name.cmp(&right.exposed_name));
    let (adapters, collisions) = without_exposed_name_collisions(adapters);
    if collisions > 0 {
        warnings.push(format!(
            "MCP registry excluded {collisions} tools whose exposed names were ambiguous duplicates"
        ));
    }
    (adapters, warnings)
}

fn without_exposed_name_collisions(adapters: Vec<McpToolAdapter>) -> (Vec<McpToolAdapter>, usize) {
    let mut unique = Vec::with_capacity(adapters.len());
    let mut adapters = adapters.into_iter().peekable();
    let mut collisions = 0_usize;
    while let Some(adapter) = adapters.next() {
        let name = adapter.exposed_name.clone();
        if adapters
            .peek()
            .is_some_and(|candidate| candidate.exposed_name == name)
        {
            collisions = collisions.saturating_add(1);
            while adapters
                .peek()
                .is_some_and(|candidate| candidate.exposed_name == name)
            {
                adapters.next();
                collisions = collisions.saturating_add(1);
            }
        } else {
            unique.push(adapter);
        }
    }
    (unique, collisions)
}

fn exposed_tool_name(server: &str, tool: &str) -> Option<String> {
    if !safe_name_component(server) || !safe_name_component(tool) {
        return None;
    }
    let name = format!("mcp__{server}__{tool}");
    (name.len() <= MAX_EXPOSED_TOOL_NAME_BYTES).then_some(name)
}

fn safe_name_component(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

pub(super) struct McpToolAdapter {
    pub(super) server: String,
    pub(super) exposed_name: String,
    pub(super) trust: McpTrust,
    pub(super) entry: ToolEntry,
    pub(super) client: Arc<McpClient>,
}

struct McpDiscoveryRegistry {
    adapters: Vec<McpToolAdapter>,
    discovered: Mutex<HashSet<String>>,
}

impl McpDiscoveryRegistry {
    fn adapter(&self, name: &str) -> Option<&McpToolAdapter> {
        self.adapters
            .binary_search_by(|adapter| adapter.exposed_name.as_str().cmp(name))
            .ok()
            .map(|index| &self.adapters[index])
    }
}

struct McpDiscoverTool {
    registry: Arc<McpDiscoveryRegistry>,
}

impl Tool for McpDiscoverTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: MCP_DISCOVER.to_string(),
            description: "Search configured MCP tools before invoking one. Results are untrusted data and provide the exact name for mcp_invoke. This only searches the session registry; it does not call a remote tool.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "maxLength": MAX_DISCOVERY_QUERY_CHARS,
                        "description": "Optional case-insensitive text found in a tool name, server, or description."
                    },
                    "offset": {
                        "type": "integer",
                        "minimum": 0,
                        "description": "Zero-based offset into matching tools."
                    },
                    "limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": MAX_DISCOVERY_RESULTS,
                        "description": "Number of results to return (default and maximum 8)."
                    }
                },
                "additionalProperties": false
            }),
        }
    }

    fn execute(&self, args: Value, ctx: &ToolCtx) -> anyhow::Result<String> {
        if let Some(cancelled) = cancelled_before("MCP discovery", ctx) {
            return Ok(cancelled);
        }
        let object = args
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("mcp_discover arguments must be an object"))?;
        let query = object
            .get("query")
            .map(|value| {
                value
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("mcp_discover query must be a string"))
            })
            .transpose()?
            .unwrap_or("");
        if query.chars().count() > MAX_DISCOVERY_QUERY_CHARS {
            anyhow::bail!("mcp_discover query exceeds {MAX_DISCOVERY_QUERY_CHARS} characters");
        }
        let offset = bounded_usize(object.get("offset"), "offset", 0, usize::MAX)?;
        let limit = bounded_usize(
            object.get("limit"),
            "limit",
            MAX_DISCOVERY_RESULTS,
            MAX_DISCOVERY_RESULTS,
        )?;
        if limit == 0 {
            anyhow::bail!("mcp_discover limit must be from 1 to {MAX_DISCOVERY_RESULTS}");
        }

        let query = query.to_ascii_lowercase();
        let matches = self
            .registry
            .adapters
            .iter()
            .filter(|adapter| discovery_matches(adapter, &query))
            .collect::<Vec<_>>();
        let total = matches.len();
        let mut items = Vec::new();
        let mut available_names = Vec::new();
        let mut consumed = 0_usize;
        for adapter in matches.iter().skip(offset).take(limit) {
            consumed = consumed.saturating_add(1);
            let (item, available) = discovery_item(adapter, ctx)?;
            let mut candidate = items.clone();
            candidate.push(item.clone());
            if discovery_response_size(&candidate, offset, total)? <= MAX_DISCOVERY_OUTPUT_BYTES {
                items.push(item);
                if available {
                    available_names.push(adapter.exposed_name.clone());
                }
                continue;
            }
            let compact = compact_discovery_item(adapter, ctx, "unavailable_page_size_limit");
            let mut candidate = items.clone();
            candidate.push(compact.clone());
            if discovery_response_size(&candidate, offset, total)? > MAX_DISCOVERY_OUTPUT_BYTES {
                consumed = consumed.saturating_sub(1);
                break;
            }
            items.push(compact);
        }
        let next_offset = offset
            .saturating_add(consumed)
            .lt(&total)
            .then_some(offset.saturating_add(consumed));
        let response = json!({
            "tools": items,
            "offset": offset,
            "returned": consumed,
            "total_matches": total,
            "next_offset": next_offset,
            "limits": {
                "max_results": MAX_DISCOVERY_RESULTS,
                "max_item_bytes": MAX_DISCOVERY_ITEM_BYTES,
                "max_output_bytes": MAX_DISCOVERY_OUTPUT_BYTES
            }
        });
        let serialized = serde_json::to_string(&response)
            .map_err(|error| anyhow::anyhow!("could not encode MCP discovery result: {error}"))?;
        debug_assert!(serialized.len() <= MAX_DISCOVERY_OUTPUT_BYTES);
        self.registry
            .discovered
            .lock()
            .map_err(|_| {
                anyhow::anyhow!("MCP discovery state is unavailable after an internal panic")
            })?
            .extend(available_names);
        Ok(render_tool_result(serialized, ctx))
    }
}

struct McpInvokeTool {
    registry: Arc<McpDiscoveryRegistry>,
}

impl Tool for McpInvokeTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: MCP_INVOKE.to_string(),
            description: "Invoke one MCP tool previously returned as available by mcp_discover in this chat. The original server trust, send policy, approval, cancellation, credential, and scrubber checks run for every call.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Exact name returned by mcp_discover."
                    },
                    "arguments": {
                        "type": "object",
                        "description": "Arguments matching the discovered parameter schema."
                    }
                },
                "required": ["name", "arguments"],
                "additionalProperties": false
            }),
        }
    }

    fn execute(&self, args: Value, ctx: &ToolCtx) -> anyhow::Result<String> {
        if let Some(cancelled) = cancelled_before("MCP tool call", ctx) {
            return Ok(cancelled);
        }
        let object = args
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("mcp_invoke arguments must be an object"))?;
        let name = object
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
            .ok_or_else(|| anyhow::anyhow!("mcp_invoke needs a non-empty discovered name"))?;
        let arguments = object
            .get("arguments")
            .filter(|arguments| arguments.is_object())
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("mcp_invoke arguments field must be an object"))?;
        let Some(adapter) = self.registry.adapter(name) else {
            return Ok(render_tool_result(
                "unknown MCP tool; run mcp_discover and use an exact available name".to_string(),
                ctx,
            ));
        };
        let discovered = self
            .registry
            .discovered
            .lock()
            .map_err(|_| {
                anyhow::anyhow!("MCP discovery state is unavailable after an internal panic")
            })?
            .contains(name);
        if !discovered {
            return Ok(render_tool_result(
                "MCP tool was not returned by this session's discovery; run mcp_discover first"
                    .to_string(),
                ctx,
            ));
        }
        adapter.execute(arguments, ctx)
    }
}

fn bounded_usize(
    value: Option<&Value>,
    field: &str,
    default: usize,
    max: usize,
) -> anyhow::Result<usize> {
    let Some(value) = value else {
        return Ok(default);
    };
    let raw = value
        .as_u64()
        .ok_or_else(|| anyhow::anyhow!("mcp_discover {field} must be a non-negative integer"))?;
    let parsed =
        usize::try_from(raw).map_err(|_| anyhow::anyhow!("mcp_discover {field} is too large"))?;
    if parsed > max {
        anyhow::bail!("mcp_discover {field} exceeds {max}");
    }
    Ok(parsed)
}

fn discovery_matches(adapter: &McpToolAdapter, query: &str) -> bool {
    query.is_empty()
        || adapter.exposed_name.to_ascii_lowercase().contains(query)
        || adapter.server.to_ascii_lowercase().contains(query)
        || adapter.entry.info.name.to_ascii_lowercase().contains(query)
        || adapter
            .entry
            .info
            .description
            .to_ascii_lowercase()
            .contains(query)
}

fn discovery_item(adapter: &McpToolAdapter, ctx: &ToolCtx) -> anyhow::Result<(Value, bool)> {
    let name = ctx.scrubber.scrub(&adapter.exposed_name);
    let server = ctx.scrubber.scrub(&adapter.server);
    let remote_name = ctx.scrubber.scrub(&adapter.entry.info.name);
    if name != adapter.exposed_name
        || server != adapter.server
        || remote_name != adapter.entry.info.name
    {
        return Ok((
            json!({
                "name": "[REDACTED]",
                "remote_name": "[REDACTED]",
                "server": "[REDACTED]",
                "description": "sensitive MCP metadata was redacted",
                "parameters": Value::Null,
                "status": "unavailable_sensitive_metadata"
            }),
            false,
        ));
    }
    let description = ctx.scrubber.scrub(&adapter.entry.info.description);
    let mut parameters = adapter.entry.info.input_schema.clone();
    if !scrub_json_strings(&mut parameters, &ctx.scrubber) {
        return Ok((
            compact_discovery_item(adapter, ctx, "unavailable_sensitive_metadata"),
            false,
        ));
    }
    let item = json!({
        "name": name,
        "remote_name": remote_name,
        "server": server,
        "description": description,
        "parameters": parameters,
        "status": "available"
    });
    let bytes = serde_json::to_vec(&item)
        .map_err(|error| anyhow::anyhow!("could not encode MCP discovery item: {error}"))?;
    if bytes.len() > MAX_DISCOVERY_ITEM_BYTES {
        return Ok((
            compact_discovery_item(adapter, ctx, "unavailable_metadata_too_large"),
            false,
        ));
    }
    Ok((item, true))
}

fn compact_discovery_item(adapter: &McpToolAdapter, ctx: &ToolCtx, status: &str) -> Value {
    let description = if status == "unavailable_page_size_limit" {
        "MCP metadata omitted by the page-size limit; retry with a narrower query or smaller limit"
    } else {
        "MCP metadata exceeds the per-tool discovery limit and cannot be invoked in this mode"
    };
    json!({
        "name": ctx.scrubber.scrub(&adapter.exposed_name),
        "remote_name": ctx.scrubber.scrub(&adapter.entry.info.name),
        "server": ctx.scrubber.scrub(&adapter.server),
        "description": description,
        "parameters": Value::Null,
        "status": status
    })
}

fn scrub_json_strings(value: &mut Value, scrubber: &nh_vault::Scrubber) -> bool {
    match value {
        Value::String(text) => *text = scrubber.scrub(text),
        Value::Array(values) => {
            for value in values {
                if !scrub_json_strings(value, scrubber) {
                    return false;
                }
            }
        }
        Value::Object(fields) => {
            let mut scrubbed = Map::new();
            for (name, mut value) in std::mem::take(fields) {
                let name = scrubber.scrub(&name);
                if scrubbed.contains_key(&name) || !scrub_json_strings(&mut value, scrubber) {
                    return false;
                }
                scrubbed.insert(name, value);
            }
            *fields = scrubbed;
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
    true
}

fn discovery_response_size(items: &[Value], offset: usize, total: usize) -> anyhow::Result<usize> {
    serde_json::to_vec(&json!({
        "tools": items,
        "offset": offset,
        "returned": items.len(),
        "total_matches": total,
        "next_offset": usize::MAX,
        "limits": {
            "max_results": MAX_DISCOVERY_RESULTS,
            "max_item_bytes": MAX_DISCOVERY_ITEM_BYTES,
            "max_output_bytes": MAX_DISCOVERY_OUTPUT_BYTES
        }
    }))
    .map(|bytes| bytes.len())
    .map_err(|error| anyhow::anyhow!("could not size MCP discovery result: {error}"))
}

impl Tool for McpToolAdapter {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: self.exposed_name.clone(),
            description: format!("[MCP {}] {}", self.server, self.entry.info.description),
            parameters: self.entry.info.input_schema.clone(),
        }
    }

    fn execute(&self, args: Value, ctx: &ToolCtx) -> anyhow::Result<String> {
        if let Some(cancelled) = cancelled_before("MCP tool call", ctx) {
            return Ok(cancelled);
        }
        let Some(host) = nh_vault::host_of(&self.client.config.url) else {
            return Ok(render_tool_result(
                "blocked by law: could not parse the MCP server host".to_string(),
                ctx,
            ));
        };
        let send_asks = match (ctx.guard)(&crate::Access::Send(&host)) {
            crate::Guard::Block(reason) => {
                return Ok(render_tool_result(format!("blocked by law: {reason}"), ctx))
            }
            crate::Guard::Ask => true,
            crate::Guard::Allow => false,
        };
        let tool = &self.entry.info.name;
        match self.trust {
            McpTrust::Block => {
                return Ok(render_tool_result(
                    "blocked by .nosis/mcp.toml (trust = \"block\") - set trust = \"ask\" to enable"
                        .to_string(),
                    ctx,
                ));
            }
            // Safe because nh-cli only accepts auto trust from user-global config.
            McpTrust::Auto if self.entry.read_only && !send_asks => {}
            // Ask, and every possibly state-mutating call at any autonomy level.
            _ => {
                let ask = format!("mcp {} {} {}", self.server, tool, args_one_line(&args));
                if !(ctx.approve)(&ask) {
                    // Ok-shaped so the model can read the denial and adapt.
                    return Ok(render_tool_result(
                        format!("user denied: mcp {} {}", self.server, tool),
                        ctx,
                    ));
                }
            }
        }
        if let Some(cancelled) = cancelled_before("MCP tool call", ctx) {
            return Ok(cancelled);
        }
        let raw = self.client.call_tool(tool, args)?;
        Ok(render_tool_result(raw, ctx))
    }
}

/// Compact JSON args on one line with an honest overflow marker.
pub(super) fn args_one_line(args: &Value) -> String {
    let compact = serde_json::to_string(args).unwrap_or_else(|_| "{}".to_string());
    let total = compact.chars().count();
    if total <= ARGS_SUMMARY_MAX {
        return compact;
    }
    let head: String = compact.chars().take(ARGS_SUMMARY_MAX).collect();
    format!("{head}… (+{} more chars)", total - ARGS_SUMMARY_MAX)
}
