//! Pure path globbing and shell-command pattern matching.

pub(super) fn first_match<'a>(patterns: &'a [String], value: &str) -> Option<&'a str> {
    patterns
        .iter()
        .find(|pattern| glob_matches(pattern, value))
        .map(String::as_str)
}

/// Keep the exec first-token/whole-command rule in this one function.
pub(super) fn exec_pattern_matches(pattern: &str, command: &str) -> bool {
    if glob_matches(pattern, command) {
        return true;
    }

    command
        .split(['&', ';', '|'])
        .filter_map(exec_command_fragment)
        .any(|fragment| {
            let tokens: Vec<&str> = fragment.split_whitespace().collect();
            tokens
                .first()
                .is_some_and(|token| exec_token_matches(pattern, token))
                || command_token(&tokens, 0).is_some_and(|token| exec_token_matches(pattern, token))
                || glob_matches(pattern, fragment)
        })
}

pub(super) fn exec_command_fragment(fragment: &str) -> Option<&str> {
    let fragment = fragment.trim().trim_start_matches(['\'', '"']);
    (!fragment.is_empty()).then_some(fragment)
}

pub(super) fn command_token<'a>(tokens: &'a [&'a str], depth: usize) -> Option<&'a str> {
    const MAX_WRAPPER_DEPTH: usize = 4;

    if depth >= MAX_WRAPPER_DEPTH {
        return None;
    }
    let mut start = 0;
    while tokens
        .get(start)
        .is_some_and(|token| is_environment_assignment(token))
    {
        start += 1;
    }
    let tokens = tokens.get(start..)?;
    let first = *tokens.first()?;
    let wrapper = executable_name(first);

    if wrapper.eq_ignore_ascii_case("env") {
        let mut index = 1;
        while let Some(token) = tokens.get(index).copied() {
            let token = token.trim_matches(['\'', '"']);
            if token == "--" {
                index += 1;
                break;
            }
            if matches!(token, "-u" | "--unset" | "-C" | "--chdir") {
                index += 2;
                continue;
            }
            if token.starts_with('-') || is_environment_assignment(token) {
                index += 1;
                continue;
            }
            break;
        }
        return command_token(tokens.get(index..)?, depth + 1);
    }

    if wrapper.eq_ignore_ascii_case("command") || wrapper.eq_ignore_ascii_case("nohup") {
        let mut index = 1;
        while tokens
            .get(index)
            .is_some_and(|token| token.trim_matches(['\'', '"']).starts_with('-'))
        {
            index += 1;
        }
        return command_token(tokens.get(index..)?, depth + 1);
    }

    let is_shell_name = wrapper.eq_ignore_ascii_case("cmd")
        || matches!(wrapper, "sh" | "bash" | "dash" | "zsh" | "fish")
        || wrapper.eq_ignore_ascii_case("powershell")
        || wrapper.eq_ignore_ascii_case("pwsh");
    if !is_shell_name {
        return Some(first);
    }

    let option = tokens.get(1)?.trim_matches(['\'', '"']);
    let shell_wrapper = wrapper.eq_ignore_ascii_case("cmd")
        && (option.eq_ignore_ascii_case("/c") || option.eq_ignore_ascii_case("/k"));
    let posix_wrapper = matches!(wrapper, "sh" | "bash" | "dash" | "zsh" | "fish")
        && option.starts_with('-')
        && option.contains('c');
    let powershell_wrapper = (wrapper.eq_ignore_ascii_case("powershell")
        || wrapper.eq_ignore_ascii_case("pwsh"))
        && (option.eq_ignore_ascii_case("-command")
            || option.eq_ignore_ascii_case("-c")
            || option.eq_ignore_ascii_case("/c"));
    if shell_wrapper || posix_wrapper || powershell_wrapper {
        return command_token(tokens.get(2..)?, depth + 1);
    }

    None
}

pub(super) fn is_environment_assignment(token: &str) -> bool {
    let token = token.trim_matches(['\'', '"']);
    let Some((name, _)) = token.split_once('=') else {
        return false;
    };
    !name.is_empty()
        && name
            .chars()
            .all(|character| character == '_' || character.is_ascii_alphanumeric())
}

pub(super) fn executable_name(token: &str) -> &str {
    let token = token.trim_matches(['\'', '"']);
    let name = token.rsplit(['/', '\\']).next().unwrap_or(token);
    name.strip_suffix(".exe").unwrap_or(name)
}

pub(super) fn exec_token_matches(pattern: &str, token: &str) -> bool {
    let token = token.trim_start_matches(['\'', '"']);
    let executable = executable_name(token);
    #[cfg(windows)]
    {
        let pattern = pattern.to_ascii_lowercase();
        glob_matches(&pattern, &token.to_ascii_lowercase())
            || glob_matches(&pattern, &executable.to_ascii_lowercase())
    }
    #[cfg(not(windows))]
    {
        glob_matches(pattern, token) || glob_matches(pattern, executable)
    }
}

pub(super) fn glob_matches(pattern: &str, value: &str) -> bool {
    let pattern_segments: Vec<&str> = pattern.split('/').collect();
    let value_segments: Vec<&str> = value.split('/').collect();
    let mut pattern_index = 0;
    let mut value_index = 0;
    let mut wildcard_index = None;
    let mut wildcard_value_index = 0;

    while value_index < value_segments.len() {
        if pattern_index < pattern_segments.len()
            && pattern_segments[pattern_index] != "**"
            && segment_matches(pattern_segments[pattern_index], value_segments[value_index])
        {
            pattern_index += 1;
            value_index += 1;
        } else if pattern_index < pattern_segments.len() && pattern_segments[pattern_index] == "**"
        {
            wildcard_index = Some(pattern_index);
            pattern_index += 1;
            wildcard_value_index = value_index;
        } else if let Some(wildcard) = wildcard_index {
            wildcard_value_index += 1;
            value_index = wildcard_value_index;
            pattern_index = wildcard + 1;
        } else {
            return false;
        }
    }

    while pattern_segments.get(pattern_index) == Some(&"**") {
        pattern_index += 1;
    }
    pattern_index == pattern_segments.len()
}

pub(super) fn segment_matches(pattern: &str, value: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let value: Vec<char> = value.chars().collect();
    let mut pattern_index = 0;
    let mut value_index = 0;
    let mut wildcard_index = None;
    let mut wildcard_value_index = 0;

    while value_index < value.len() {
        match pattern.get(pattern_index) {
            Some('?') => {
                pattern_index += 1;
                value_index += 1;
            }
            Some('*') => {
                wildcard_index = Some(pattern_index);
                pattern_index += 1;
                wildcard_value_index = value_index;
            }
            Some(expected) if value.get(value_index) == Some(expected) => {
                pattern_index += 1;
                value_index += 1;
            }
            _ => {
                let Some(wildcard) = wildcard_index else {
                    return false;
                };
                wildcard_value_index += 1;
                value_index = wildcard_value_index;
                pattern_index = wildcard + 1;
            }
        }
    }

    while pattern.get(pattern_index) == Some(&'*') {
        pattern_index += 1;
    }
    pattern_index == pattern.len()
}
