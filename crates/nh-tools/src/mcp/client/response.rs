//! Bounded validation for JSON and request-scoped SSE responses.

use super::{read_body_capped, MAX_MCP_BODY_BYTES};
use anyhow::{bail, Context as _};
use serde_json::Value;
use std::io::Read as _;

pub(super) enum RpcReply {
    Complete(Value),
    ServerError(String),
}

pub(super) fn read_rpc_reply(
    response: reqwest::blocking::Response,
    expected_id: &Value,
    url: &str,
) -> anyhow::Result<RpcReply> {
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .ok_or_else(|| anyhow::anyhow!("{url} omitted the MCP response Content-Type"))?
        .to_str()
        .with_context(|| format!("{url} sent an invalid MCP response Content-Type"))?;
    let media_type = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    match media_type.as_str() {
        "application/json" => {
            let text = read_body_capped(response, MAX_MCP_BODY_BYTES)?;
            let message: Value = serde_json::from_str(&text)
                .map_err(|_| anyhow::anyhow!("{url} sent invalid JSON - is it an MCP endpoint?"))?;
            validate_response(&message, expected_id)
        }
        "text/event-stream" => read_sse_reply(response, expected_id, url),
        other => bail!(
            "{url} sent unsupported MCP response Content-Type {other:?} - expected application/json or text/event-stream"
        ),
    }
}

fn read_sse_reply(
    response: reqwest::blocking::Response,
    expected_id: &Value,
    url: &str,
) -> anyhow::Result<RpcReply> {
    let mut reader = response.take((MAX_MCP_BODY_BYTES + 1) as u64);
    let mut buffer = [0_u8; 8 * 1024];
    let mut line = Vec::new();
    let mut event = SseEvent::default();
    let mut total = 0_usize;
    let mut skip_lf = false;

    loop {
        let read = reader
            .read(&mut buffer)
            .with_context(|| format!("could not read MCP SSE response from {url}"))?;
        if read == 0 {
            bail!("{url} ended its MCP SSE response before the matching final result");
        }
        total = total.saturating_add(read);
        if total > MAX_MCP_BODY_BYTES {
            bail!("mcp response exceeded cap of {MAX_MCP_BODY_BYTES} bytes");
        }

        for byte in &buffer[..read] {
            if skip_lf {
                skip_lf = false;
                if *byte == b'\n' {
                    continue;
                }
            }
            match *byte {
                b'\r' => {
                    if let Some(reply) = event.consume_line(&line, expected_id)? {
                        return Ok(reply);
                    }
                    line.clear();
                    skip_lf = true;
                }
                b'\n' => {
                    if let Some(reply) = event.consume_line(&line, expected_id)? {
                        return Ok(reply);
                    }
                    line.clear();
                }
                byte => line.push(byte),
            }
        }
    }
}

#[derive(Default)]
struct SseEvent {
    data: Vec<String>,
    saw_data: bool,
    first_line: bool,
}

impl SseEvent {
    fn consume_line(
        &mut self,
        raw_line: &[u8],
        expected_id: &Value,
    ) -> anyhow::Result<Option<RpcReply>> {
        let raw_line = if !self.first_line {
            self.first_line = true;
            raw_line.strip_prefix(b"\xef\xbb\xbf").unwrap_or(raw_line)
        } else {
            raw_line
        };
        let line = std::str::from_utf8(raw_line).context("MCP SSE response was not valid UTF-8")?;
        if line.is_empty() {
            return self.dispatch(expected_id);
        }
        if line.starts_with(':') {
            return Ok(None);
        }
        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);
        if field == "data" {
            self.saw_data = true;
            self.data.push(value.to_string());
        }
        Ok(None)
    }

    fn dispatch(&mut self, expected_id: &Value) -> anyhow::Result<Option<RpcReply>> {
        if !self.saw_data {
            return Ok(None);
        }
        self.saw_data = false;
        let data = self.data.join("\n");
        self.data.clear();
        let message: Value =
            serde_json::from_str(&data).context("MCP SSE event contained invalid JSON")?;
        classify_sse_message(&message, expected_id)
    }
}

fn classify_sse_message(message: &Value, expected_id: &Value) -> anyhow::Result<Option<RpcReply>> {
    let object = message
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("MCP SSE event was not a JSON-RPC object"))?;
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        bail!("MCP SSE event had an invalid JSON-RPC version");
    }
    if object.contains_key("method") {
        if object.contains_key("id") {
            bail!("MCP SSE response contained an independent server request");
        }
        if object.get("method").and_then(Value::as_str).is_none()
            || object.contains_key("result")
            || object.contains_key("error")
            || object
                .get("params")
                .is_some_and(|params| !params.is_object())
        {
            bail!("MCP SSE response contained a malformed notification");
        }
        return Ok(None);
    }
    validate_response(message, expected_id).map(Some)
}

fn validate_response(message: &Value, expected_id: &Value) -> anyhow::Result<RpcReply> {
    let object = message
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("MCP response was not a JSON-RPC object"))?;
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        bail!("MCP response had an invalid JSON-RPC version");
    }
    if object.contains_key("method") {
        bail!("MCP response contained a request or notification method");
    }
    let response_id = object
        .get("id")
        .ok_or_else(|| anyhow::anyhow!("MCP response omitted its request id"))?;
    if response_id != expected_id {
        bail!("MCP response id did not match the request");
    }
    let has_result = object.contains_key("result");
    let has_error = object.contains_key("error");
    if has_result == has_error {
        bail!("MCP response must contain exactly one of result or error");
    }
    if has_result {
        let result = object["result"]
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("MCP response result was not an object"))?;
        match result.get("resultType") {
            None => {}
            Some(Value::String(kind)) if kind == "complete" => {}
            Some(Value::String(kind)) => {
                bail!("MCP response used unsupported resultType {kind:?}")
            }
            Some(_) => bail!("MCP response resultType was not a string"),
        }
        return Ok(RpcReply::Complete(Value::Object(result.clone())));
    }

    let error = object["error"]
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("MCP response error was not an object"))?;
    if error.get("code").and_then(Value::as_i64).is_none() {
        bail!("MCP response error code was not an integer");
    }
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("MCP response error message was not a string"))?;
    let message = nh_vault::sanitize_untrusted_text(message).replace(['\r', '\n'], " ");
    Ok(RpcReply::ServerError(message))
}
