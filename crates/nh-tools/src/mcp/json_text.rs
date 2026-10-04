use nh_vault::Scrubber;
use serde::de::IgnoredAny;
use std::collections::BTreeMap;

pub(super) enum ScrubbedJson {
    NotJson,
    Unchanged,
    Rewritten(String),
    Collision,
    Unsafe,
}

enum Container {
    Object(BTreeMap<String, String>),
    Array,
}

/// Scrub every decoded JSON string while preserving all untouched source bytes.
///
/// The initial validation keeps this scanner small: delimiters outside strings
/// are trustworthy, while numbers, whitespace, ordering, and duplicate fields
/// can be copied without materializing the document as a `Value`.
pub(super) fn scrub_complete_json(text: &str, scrubbers: &[&Scrubber]) -> ScrubbedJson {
    if serde_json::from_str::<IgnoredAny>(text).is_err() {
        return ScrubbedJson::NotJson;
    }

    let bytes = text.as_bytes();
    let mut output = String::with_capacity(text.len());
    let mut copied_to = 0;
    let mut cursor = 0;
    let mut changed = false;
    let mut containers = Vec::new();

    while cursor < bytes.len() {
        match bytes[cursor] {
            b'{' => {
                containers.push(Container::Object(BTreeMap::new()));
                cursor += 1;
            }
            b'[' => {
                containers.push(Container::Array);
                cursor += 1;
            }
            b'}' | b']' => {
                if containers.pop().is_none() {
                    return ScrubbedJson::Unsafe;
                }
                cursor += 1;
            }
            b'"' => {
                let start = cursor;
                cursor += 1;
                while cursor < bytes.len() {
                    match bytes[cursor] {
                        b'\\' => cursor += 2,
                        b'"' => {
                            cursor += 1;
                            break;
                        }
                        _ => cursor += 1,
                    }
                }
                if cursor > bytes.len() || bytes.get(cursor.saturating_sub(1)) != Some(&b'"') {
                    return ScrubbedJson::Unsafe;
                }

                let token = &text[start..cursor];
                let Ok(decoded) = serde_json::from_str::<String>(token) else {
                    return ScrubbedJson::Unsafe;
                };
                let scrubbed = scrubbers
                    .iter()
                    .fold(decoded.clone(), |text, scrubber| scrubber.scrub(&text));
                let is_key = matches!(containers.last(), Some(Container::Object(_)))
                    && next_non_whitespace(bytes, cursor) == Some(b':');

                if is_key {
                    let Some(Container::Object(keys)) = containers.last_mut() else {
                        return ScrubbedJson::Unsafe;
                    };
                    if let Some(previous) = keys.get(&scrubbed) {
                        if previous != &decoded {
                            return ScrubbedJson::Collision;
                        }
                    } else {
                        keys.insert(scrubbed.clone(), decoded.clone());
                    }
                }

                if scrubbed != decoded {
                    output.push_str(&text[copied_to..start]);
                    let Ok(encoded) = serde_json::to_string(&scrubbed) else {
                        return ScrubbedJson::Unsafe;
                    };
                    output.push_str(&encoded);
                    copied_to = cursor;
                    changed = true;
                }
            }
            _ => cursor += 1,
        }
    }

    if !containers.is_empty() {
        return ScrubbedJson::Unsafe;
    }
    if !changed {
        return ScrubbedJson::Unchanged;
    }
    output.push_str(&text[copied_to..]);
    ScrubbedJson::Rewritten(output)
}

fn next_non_whitespace(bytes: &[u8], mut cursor: usize) -> Option<u8> {
    while let Some(byte) = bytes.get(cursor).copied() {
        if !matches!(byte, b' ' | b'\n' | b'\r' | b'\t') {
            return Some(byte);
        }
        cursor += 1;
    }
    None
}
