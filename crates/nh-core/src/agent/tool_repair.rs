use serde_json::Value;

#[derive(Debug)]
pub struct ParsedArguments {
    pub value: Value,
    pub notes: Vec<String>,
}

#[derive(Debug)]
pub struct ParseFailure {
    pub error: serde_json::Error,
    pub notes: Vec<String>,
}

pub fn canonical_tool_name(name: &str) -> Option<&'static str> {
    match name {
        "bash" | "shell" | "run_command" => Some("exec_shell"),
        "str_replace_editor" | "replace_in_file" => Some("edit_file"),
        "read" | "view_file" => Some("read_file"),
        _ => None,
    }
}

pub fn parse_arguments(raw: &str) -> Result<ParsedArguments, ParseFailure> {
    if let Ok(value) = serde_json::from_str(raw) {
        return Ok(ParsedArguments {
            value,
            notes: Vec::new(),
        });
    }

    let mut candidate = raw.to_string();
    let mut notes = Vec::new();
    if let Some(unfenced) = strip_json_fence(&candidate) {
        candidate = unfenced;
        notes.push("stripped JSON markdown fence".to_string());
        if let Ok(value) = serde_json::from_str(&candidate) {
            return Ok(ParsedArguments { value, notes });
        }
    }

    let (repaired, changes) = mechanically_repair_json(&candidate);
    notes.extend(changes);
    if notes.is_empty() {
        notes.push("mechanical JSON repair found no safe syntax change".to_string());
    }
    match serde_json::from_str(&repaired) {
        Ok(value) => Ok(ParsedArguments { value, notes }),
        Err(error) => Err(ParseFailure { error, notes }),
    }
}

fn strip_json_fence(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let after_open = trimmed.strip_prefix("```")?;
    let header_end = after_open.find('\n')?;
    let language = after_open[..header_end].trim();
    if !language.is_empty() && !language.eq_ignore_ascii_case("json") {
        return None;
    }
    let body = after_open[header_end + 1..].strip_suffix("```")?;
    Some(body.trim().to_string())
}

fn mechanically_repair_json(input: &str) -> (String, Vec<String>) {
    let (quoted, quote_changes) = normalize_quotes(input);
    let (keyed, quoted_keys) = quote_unquoted_keys(&quoted);
    let (commas_removed, trailing_commas) = remove_trailing_commas(&keyed);
    let mut notes = Vec::new();
    if quote_changes.single_quotes {
        notes.push("normalized single-quoted JSON strings".to_string());
    }
    if quote_changes.unterminated {
        notes.push("closed an unterminated JSON string".to_string());
    }
    if quoted_keys {
        notes.push("quoted unquoted JSON object keys".to_string());
    }
    if trailing_commas {
        notes.push("removed trailing JSON commas".to_string());
    }
    (commas_removed, notes)
}

#[derive(Default)]
struct QuoteChanges {
    single_quotes: bool,
    unterminated: bool,
}

#[derive(Clone, Copy)]
enum Quote {
    Double,
    Single,
}

fn normalize_quotes(input: &str) -> (String, QuoteChanges) {
    let mut output = String::with_capacity(input.len() + 1);
    let chars = input.char_indices();
    let mut quote = None;
    let mut escaped = false;
    let mut changes = QuoteChanges::default();
    let mut containers = Vec::new();

    for (_, ch) in chars {
        match quote {
            Some(Quote::Double) => {
                output.push(ch);
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    quote = None;
                }
            }
            Some(Quote::Single) => {
                changes.single_quotes = true;
                if escaped {
                    if ch == '\'' {
                        output.push('\'');
                    } else {
                        output.push('\\');
                        output.push(ch);
                    }
                    escaped = false;
                } else {
                    match ch {
                        '\\' => escaped = true,
                        '\'' => {
                            output.push('"');
                            quote = None;
                        }
                        '"' => output.push_str("\\\""),
                        _ => output.push(ch),
                    }
                }
            }
            None => match ch {
                '"' => {
                    output.push(ch);
                    quote = Some(Quote::Double);
                }
                '\'' => {
                    output.push('"');
                    quote = Some(Quote::Single);
                    changes.single_quotes = true;
                }
                '{' | '[' => {
                    containers.push(ch);
                    output.push(ch);
                }
                '}' => {
                    if containers.last() == Some(&'{') {
                        containers.pop();
                    }
                    output.push(ch);
                }
                ']' => {
                    if containers.last() == Some(&'[') {
                        containers.pop();
                    }
                    output.push(ch);
                }
                _ => output.push(ch),
            },
        }
    }

    if let Some(open_quote) = quote {
        changes.unterminated = true;
        let insertion =
            closing_container_suffix_start(&output, &containers).unwrap_or(output.len());
        let closing = match open_quote {
            Quote::Double | Quote::Single => '"',
        };
        output.insert(insertion, closing);
    }
    (output, changes)
}

fn closing_container_suffix_start(output: &str, containers: &[char]) -> Option<usize> {
    if containers.is_empty() {
        return None;
    }
    let mut expected = containers.iter().rev();
    let mut first_closer = None;
    for (index, ch) in output.char_indices().rev() {
        if ch.is_whitespace() {
            continue;
        }
        let Some(container) = expected.next() else {
            break;
        };
        let matching = matches!((container, ch), ('{', '}') | ('[', ']'));
        if !matching {
            return None;
        }
        first_closer = Some(index);
    }
    if expected.next().is_none() {
        first_closer
    } else {
        None
    }
}

fn quote_unquoted_keys(input: &str) -> (String, bool) {
    let mut output = String::with_capacity(input.len() + 4);
    let mut index = 0;
    let mut in_string = false;
    let mut escaped = false;
    let mut last_significant = None;
    let bytes = input.as_bytes();
    let mut changed = false;

    while index < bytes.len() {
        let ch = input[index..].chars().next().expect("valid char boundary");
        if in_string {
            output.push(ch);
            index += ch.len_utf8();
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
                last_significant = Some('"');
            }
            continue;
        }

        if ch == '"' {
            output.push(ch);
            index += 1;
            in_string = true;
            continue;
        }

        if matches!(last_significant, Some('{') | Some(','))
            && (ch.is_ascii_alphabetic() || ch == '_')
        {
            let key_start = index;
            index += ch.len_utf8();
            while index < bytes.len() {
                let next = input[index..].chars().next().expect("valid char boundary");
                if next.is_ascii_alphanumeric() || matches!(next, '_' | '-') {
                    index += next.len_utf8();
                } else {
                    break;
                }
            }
            let mut lookahead = index;
            while lookahead < bytes.len() {
                let next = input[lookahead..]
                    .chars()
                    .next()
                    .expect("valid char boundary");
                if next.is_whitespace() {
                    lookahead += next.len_utf8();
                } else {
                    break;
                }
            }
            if input[lookahead..].starts_with(':') {
                output.push('"');
                output.push_str(&input[key_start..index]);
                output.push('"');
                changed = true;
                last_significant = Some('"');
                continue;
            }
            output.push_str(&input[key_start..index]);
            last_significant = input[key_start..index].chars().last();
            continue;
        }

        output.push(ch);
        index += ch.len_utf8();
        if !ch.is_whitespace() {
            last_significant = Some(ch);
        }
    }
    (output, changed)
}

fn remove_trailing_commas(input: &str) -> (String, bool) {
    let mut output = String::with_capacity(input.len());
    let mut index = 0;
    let mut in_string = false;
    let mut escaped = false;
    let bytes = input.as_bytes();
    let mut changed = false;

    while index < bytes.len() {
        let ch = input[index..].chars().next().expect("valid char boundary");
        if in_string {
            output.push(ch);
            index += ch.len_utf8();
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        if ch == '"' {
            in_string = true;
            output.push(ch);
            index += 1;
            continue;
        }
        if ch == ',' {
            let mut lookahead = index + 1;
            while lookahead < bytes.len() {
                let next = input[lookahead..]
                    .chars()
                    .next()
                    .expect("valid char boundary");
                if next.is_whitespace() {
                    lookahead += next.len_utf8();
                } else {
                    break;
                }
            }
            if matches!(input[lookahead..].chars().next(), Some('}' | ']')) {
                changed = true;
                index += 1;
                continue;
            }
        }
        output.push(ch);
        index += ch.len_utf8();
    }
    (output, changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn valid_json_is_unchanged_and_uncounted() {
        let parsed = parse_arguments(r#"{"path":"a.txt"}"#).unwrap();
        assert_eq!(parsed.value, json!({"path": "a.txt"}));
        assert!(parsed.notes.is_empty());
    }

    #[test]
    fn fenced_json_is_stripped_once() {
        let parsed = parse_arguments("```json\n{\"path\":\"a.txt\"}\n```").unwrap();
        assert_eq!(parsed.value, json!({"path": "a.txt"}));
        assert_eq!(parsed.notes, ["stripped JSON markdown fence"]);
    }

    #[test]
    fn mechanical_repairs_cover_only_the_named_syntax_classes() {
        let parsed = parse_arguments("{path: 'a\\'b.txt', new_string: \"done\",}").unwrap();
        assert_eq!(
            parsed.value,
            json!({"path": "a'b.txt", "new_string": "done"})
        );
        assert_eq!(
            parsed.notes,
            [
                "normalized single-quoted JSON strings",
                "quoted unquoted JSON object keys",
                "removed trailing JSON commas",
            ]
        );

        let unterminated = parse_arguments(r#"{"path":"a.txt}"#).unwrap();
        assert_eq!(unterminated.value, json!({"path": "a.txt"}));
        assert_eq!(unterminated.notes, ["closed an unterminated JSON string"]);
    }

    #[test]
    fn unsafe_or_unknown_damage_gets_one_bounded_failure() {
        let failure = parse_arguments("{path => value").unwrap_err();
        assert!(!failure.notes.is_empty());
        assert!(!failure.error.to_string().is_empty());
    }

    #[test]
    fn aliases_are_closed_and_explicit() {
        assert_eq!(canonical_tool_name("shell"), Some("exec_shell"));
        assert_eq!(canonical_tool_name("str_replace_editor"), Some("edit_file"));
        assert_eq!(canonical_tool_name("view_file"), Some("read_file"));
        assert_eq!(canonical_tool_name("unknown"), None);
    }
}
