//! Validation for the modern per-request MCP HTTP envelope.

use crate::protocol::{rpc_error, rpc_error_data};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Map, Value};
use tiny_http::Request;

pub(super) const PROTOCOL_VERSION: &str = "2026-07-28";

pub(super) struct ValidatedRequest<'a> {
    pub(super) id: Value,
    pub(super) method: &'a str,
    pub(super) params: &'a Value,
}

pub(super) struct Rejection {
    pub(super) status: u16,
    pub(super) body: Value,
}

pub(super) fn validate<'a>(
    request: &Request,
    message: &'a Value,
) -> Result<ValidatedRequest<'a>, Rejection> {
    let Some(object) = message.as_object() else {
        return Err(invalid_request(Value::Null));
    };
    let id = readable_id(object).unwrap_or(Value::Null);
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
        || object.contains_key("result")
        || object.contains_key("error")
    {
        return Err(invalid_request(id));
    }
    let Some(request_id) = readable_id(object) else {
        return Err(invalid_request(Value::Null));
    };
    let Some(method) = object.get("method").and_then(Value::as_str) else {
        return Err(invalid_request(request_id));
    };
    let Some(params) = object.get("params").filter(|params| params.is_object()) else {
        return Err(invalid_params(request_id));
    };
    let Some(meta) = params.get("_meta").and_then(Value::as_object) else {
        return Err(invalid_params(request_id));
    };
    let Some(version) = meta
        .get("io.modelcontextprotocol/protocolVersion")
        .and_then(Value::as_str)
    else {
        return Err(invalid_params(request_id));
    };
    if !meta
        .get("io.modelcontextprotocol/clientCapabilities")
        .is_some_and(Value::is_object)
    {
        return Err(invalid_params(request_id));
    }
    if meta
        .get("io.modelcontextprotocol/clientInfo")
        .is_some_and(|info| {
            let Some(info) = info.as_object() else {
                return true;
            };
            info.get("name").and_then(Value::as_str).is_none()
                || info.get("version").and_then(Value::as_str).is_none()
        })
    {
        return Err(invalid_params(request_id));
    }

    let protocol_header = required_header(request, "MCP-Protocol-Version", &request_id)?;
    if protocol_header != version {
        return Err(header_mismatch(request_id));
    }
    if version != PROTOCOL_VERSION {
        return Err(Rejection {
            status: 400,
            body: rpc_error_data(
                request_id,
                -32022,
                "unsupported protocol version",
                json!({ "supported": [PROTOCOL_VERSION], "requested": version }),
            ),
        });
    }
    let method_header = required_header(request, "Mcp-Method", &request_id)?;
    if method_header != method {
        return Err(header_mismatch(request_id));
    }
    if method == "tools/call" {
        let Some(name) = params
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
        else {
            return Err(invalid_params(request_id));
        };
        let encoded_name = required_header(request, "Mcp-Name", &request_id)?;
        let decoded_name =
            decode_header_value(encoded_name).map_err(|()| header_mismatch(request_id.clone()))?;
        if decoded_name != name {
            return Err(header_mismatch(request_id));
        }
    }

    Ok(ValidatedRequest {
        id: request_id,
        method,
        params,
    })
}

fn readable_id(object: &Map<String, Value>) -> Option<Value> {
    object
        .get("id")
        .filter(|id| id.is_string() || id.as_i64().is_some() || id.as_u64().is_some())
        .cloned()
}

fn required_header<'a>(request: &'a Request, name: &str, id: &Value) -> Result<&'a str, Rejection> {
    let mut values = request
        .headers()
        .iter()
        .filter(|header| header.field.as_str().as_str().eq_ignore_ascii_case(name));
    let Some(value) = values.next() else {
        return Err(header_mismatch(id.clone()));
    };
    if values.next().is_some() {
        return Err(header_mismatch(id.clone()));
    }
    Ok(value.value.as_str())
}

fn decode_header_value(value: &str) -> Result<String, ()> {
    if let Some(encoded) = value
        .strip_prefix("=?base64?")
        .and_then(|value| value.strip_suffix("?="))
    {
        let decoded = STANDARD.decode(encoded).map_err(|_| ())?;
        return String::from_utf8(decoded).map_err(|_| ());
    }
    let bytes = value.as_bytes();
    let plain = !bytes.is_empty()
        && bytes
            .iter()
            .all(|byte| *byte == b'\t' || (b' '..=b'~').contains(byte))
        && !bytes.first().is_some_and(u8::is_ascii_whitespace)
        && !bytes.last().is_some_and(u8::is_ascii_whitespace);
    plain.then(|| value.to_string()).ok_or(())
}

fn invalid_request(id: Value) -> Rejection {
    Rejection {
        status: 400,
        body: rpc_error(id, -32600, "invalid request"),
    }
}

fn invalid_params(id: Value) -> Rejection {
    Rejection {
        status: 400,
        body: rpc_error(id, -32602, "invalid params"),
    }
}

fn header_mismatch(id: Value) -> Rejection {
    Rejection {
        status: 400,
        body: rpc_error(
            id,
            -32020,
            "required HTTP headers do not match the request body",
        ),
    }
}
