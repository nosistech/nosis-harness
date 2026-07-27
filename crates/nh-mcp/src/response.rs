//! Scrubbed MCP tool results and HTTP JSON responses.

use super::*;

pub(super) fn tool_error(runtime: &Runtime, message: &str) -> Value {
    tool_text(runtime, message, true)
}

pub(super) fn tool_result(
    runtime: &Runtime,
    text: &str,
    mut structured: Value,
    is_error: bool,
) -> Value {
    let text = nh_vault::safe_line(&runtime.scrubber, text);
    scrub_json(&mut structured, &runtime.scrubber);
    json!({
        "content": [{ "type": "text", "text": text }],
        "structuredContent": structured,
        "isError": is_error
    })
}

pub(super) fn tool_text(runtime: &Runtime, text: &str, is_error: bool) -> Value {
    let text = nh_vault::safe_line(&runtime.scrubber, text);
    json!({
        "content": [{ "type": "text", "text": text }],
        "isError": is_error
    })
}

pub(super) fn respond_json(request: Request, runtime: &Runtime, status: u16, value: &Value) {
    let mut value = value.clone();
    scrub_json(&mut value, &runtime.scrubber);
    let body = serde_json::to_string(&value).unwrap_or_else(|_| "{}".into());
    let body = runtime.scrubber.scrub(&body);
    let content_type =
        Header::from_bytes("Content-Type", "application/json").expect("static content-type header");
    let response = Response::from_string(body)
        .with_status_code(StatusCode(status))
        .with_header(content_type);
    let _ = request.respond(response);
}

pub(super) fn scrub_json(value: &mut Value, scrubber: &nh_vault::Scrubber) {
    match value {
        Value::String(text) => *text = scrubber.scrub(text),
        Value::Array(values) => {
            for value in values {
                scrub_json(value, scrubber);
            }
        }
        Value::Object(object) => {
            let fields = std::mem::take(object);
            for (key, mut value) in fields {
                scrub_json(&mut value, scrubber);
                object.insert(scrubber.scrub(&key), value);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}
