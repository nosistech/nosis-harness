//! Deterministic, bounded repository search without shell execution.

use crate::{
    relative_path, render_tool_result, resolve_in_workdir, str_arg, Access, Guard, Tool, ToolCtx,
    ToolSpec, BINARY_SNIFF_BYTES, MAX_TOOL_READ_BYTES,
};
use anyhow::{bail, Context as _};
use nh_law::glob_matches;
use serde_json::json;
use std::ffi::OsStr;
use std::io::Read as _;
use std::path::Path;

const DEFAULT_PRUNED_DIRS: [&str; 5] = ["target", "node_modules", ".venv", "dist", "build"];
const MAX_SEARCH_LINE_CHARS: usize = 300;
const MAX_WALK_FILES: usize = 20_000;
const MAX_SEARCH_MATCHES: usize = 500;

/// Find files whose workdir-relative paths match a segment-wise glob.
pub struct GlobFiles;

/// Find literal substrings in text files under the working directory.
pub struct GrepFiles;

#[derive(Clone, Copy)]
pub(super) struct SearchLimits {
    pub(super) files: usize,
    pub(super) matches: usize,
}

impl Default for SearchLimits {
    fn default() -> Self {
        Self {
            files: MAX_WALK_FILES,
            matches: MAX_SEARCH_MATCHES,
        }
    }
}

#[derive(Clone, Copy)]
enum StopReason {
    FileCap(usize),
    MatchCap(usize),
}

#[derive(Default)]
struct WalkStats {
    files_visited: usize,
    files_excluded_by_law: usize,
    symlinks_skipped: usize,
    law_directories_pruned: usize,
    default_directories_pruned: usize,
    special_entries_skipped: usize,
    stopped: Option<StopReason>,
}

enum WalkControl {
    Continue,
    MatchCap,
}

#[derive(Default)]
struct GrepStats {
    matches: usize,
    matching_files: usize,
    binary_files_skipped: usize,
    oversized_files_skipped: usize,
}

enum SearchFile {
    Text(String),
    Binary,
    Oversized,
}

impl Tool for GlobFiles {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "glob_files".into(),
            description: "Find files by workdir-relative glob. Matching is segment-wise and ** spans directory segments. Skips symlinks and prunes target, node_modules, .venv, dist, and build directories by default.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Glob pattern matched against forward-slash-joined workdir-relative paths."
                    },
                    "path": {
                        "type": "string",
                        "description": "Optional subdirectory to search, relative to the working directory."
                    }
                },
                "required": ["pattern"]
            }),
        }
    }

    fn execute(&self, args: serde_json::Value, ctx: &ToolCtx) -> anyhow::Result<String> {
        glob_files_with_limits(&args, ctx, SearchLimits::default())
    }
}

impl Tool for GrepFiles {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "grep_files".into(),
            description: "Search text files for a literal substring, not a regular expression. Skips binary and oversized files, skips symlinks, and prunes target, node_modules, .venv, dist, and build directories by default.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Literal substring to find; this is not a regular expression."
                    },
                    "path": {
                        "type": "string",
                        "description": "Optional subdirectory to search, relative to the working directory."
                    },
                    "glob": {
                        "type": "string",
                        "description": "Optional glob matched against each workdir-relative file path."
                    },
                    "case_insensitive": {
                        "type": "boolean",
                        "description": "Match without case distinctions. Defaults to false."
                    }
                },
                "required": ["pattern"]
            }),
        }
    }

    fn execute(&self, args: serde_json::Value, ctx: &ToolCtx) -> anyhow::Result<String> {
        grep_files_with_limits(&args, ctx, SearchLimits::default())
    }
}

pub(super) fn glob_files_with_limits(
    args: &serde_json::Value,
    ctx: &ToolCtx,
    limits: SearchLimits,
) -> anyhow::Result<String> {
    let pattern = str_arg(args, "pattern")?;
    let scope = optional_str_arg(args, "path")?;
    validate_limits(limits)?;

    let mut matches = Vec::new();
    let walk = walk_files(scope, ctx, limits, |_, relative| {
        if glob_matches(pattern, relative) {
            matches.push(relative.to_owned());
            if matches.len() >= limits.matches {
                return Ok(WalkControl::MatchCap);
            }
        }
        Ok(WalkControl::Continue)
    })?;
    matches.sort();

    let footer = format!(
        "- {} matches; {} files visited; {} files excluded by law; {} symlinks skipped; {} directories pruned by law{}{}{}",
        matches.len(),
        walk.files_visited,
        walk.files_excluded_by_law,
        walk.symlinks_skipped,
        walk.law_directories_pruned,
        default_prune_footer(&walk),
        special_entry_footer(&walk),
        stop_footer(walk.stopped)
    );
    matches.push(footer);
    Ok(render_tool_result(matches.join("\n"), ctx))
}

pub(super) fn grep_files_with_limits(
    args: &serde_json::Value,
    ctx: &ToolCtx,
    limits: SearchLimits,
) -> anyhow::Result<String> {
    let pattern = str_arg(args, "pattern")?;
    let scope = optional_str_arg(args, "path")?;
    let glob = optional_str_arg(args, "glob")?;
    let case_insensitive = optional_bool_arg(args, "case_insensitive")?;
    validate_limits(limits)?;

    let folded_pattern = case_insensitive.then(|| pattern.to_lowercase());
    let mut output = Vec::new();
    let mut grep = GrepStats::default();
    let walk = walk_files(scope, ctx, limits, |path, relative| {
        if glob.is_some_and(|filter| !glob_matches(filter, relative)) {
            return Ok(WalkControl::Continue);
        }
        let content = match read_search_file(path, relative)? {
            SearchFile::Text(content) => content,
            SearchFile::Binary => {
                grep.binary_files_skipped += 1;
                return Ok(WalkControl::Continue);
            }
            SearchFile::Oversized => {
                grep.oversized_files_skipped += 1;
                return Ok(WalkControl::Continue);
            }
        };

        let mut file_matched = false;
        for (line_index, line) in content.lines().enumerate() {
            let matched = if let Some(needle) = &folded_pattern {
                line.to_lowercase().contains(needle)
            } else {
                line.contains(pattern)
            };
            if !matched {
                continue;
            }
            if !file_matched {
                grep.matching_files += 1;
                file_matched = true;
            }
            grep.matches += 1;
            let result_line = ctx
                .scrubber
                .scrub(&format!("{relative}:{}:{line}", line_index + 1));
            output.push(truncate_search_line(&result_line));
            if grep.matches >= limits.matches {
                return Ok(WalkControl::MatchCap);
            }
        }
        Ok(WalkControl::Continue)
    })?;

    let footer = format!(
        "- {} matches in {} files; {} files visited; {} files excluded by law; {} binary files skipped; {} oversized files skipped; {} symlinks skipped; {} directories pruned by law{}{}{}",
        grep.matches,
        grep.matching_files,
        walk.files_visited,
        walk.files_excluded_by_law,
        grep.binary_files_skipped,
        grep.oversized_files_skipped,
        walk.symlinks_skipped,
        walk.law_directories_pruned,
        default_prune_footer(&walk),
        special_entry_footer(&walk),
        stop_footer(walk.stopped)
    );
    output.push(footer);
    Ok(render_tool_result(output.join("\n"), ctx))
}

fn validate_limits(limits: SearchLimits) -> anyhow::Result<()> {
    if limits.files == 0 || limits.matches == 0 {
        bail!("internal search limits must be positive");
    }
    Ok(())
}

fn optional_str_arg<'a>(args: &'a serde_json::Value, key: &str) -> anyhow::Result<Option<&'a str>> {
    match args.get(key) {
        None => Ok(None),
        Some(value) => value
            .as_str()
            .map(Some)
            .ok_or_else(|| anyhow::anyhow!("argument must be a string: {key}")),
    }
}

fn optional_bool_arg(args: &serde_json::Value, key: &str) -> anyhow::Result<bool> {
    match args.get(key) {
        None => Ok(false),
        Some(value) => value
            .as_bool()
            .ok_or_else(|| anyhow::anyhow!("argument must be a boolean: {key}")),
    }
}

fn walk_files<F>(
    scope: Option<&str>,
    ctx: &ToolCtx,
    limits: SearchLimits,
    mut visit: F,
) -> anyhow::Result<WalkStats>
where
    F: FnMut(&Path, &str) -> anyhow::Result<WalkControl>,
{
    let root = ctx
        .workdir
        .canonicalize()
        .with_context(|| format!("working directory not found: {}", ctx.workdir.display()))?;
    let mut stats = WalkStats::default();
    let (scope_path, scope_relative) = if let Some(path) = scope {
        let (resolved, relative) = resolve_in_workdir(&ctx.workdir, path)?;
        if !path.is_empty() {
            let requested = root.join(path);
            match std::fs::symlink_metadata(&requested) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    stats.symlinks_skipped = 1;
                    return Ok(stats);
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("could not inspect search scope {path}"))
                }
            }
        }
        (resolved, relative)
    } else {
        (root.clone(), String::new())
    };
    if !scope_path.is_dir() {
        let label = scope.unwrap_or(".");
        bail!("search scope is not a directory: {label}");
    }
    if !scope_relative.is_empty() {
        if is_default_pruned(scope_path.file_name()) {
            stats.default_directories_pruned = 1;
            return Ok(stats);
        }
        if matches!((ctx.guard)(&Access::Read(&scope_relative)), Guard::Block(_)) {
            stats.law_directories_pruned = 1;
            return Ok(stats);
        }
    }

    let mut stack = vec![scope_path];
    while let Some(directory) = stack.pop() {
        let mut entries = std::fs::read_dir(&directory)
            .with_context(|| format!("could not read directory {}", directory.display()))?
            .collect::<Result<Vec<_>, _>>()
            .with_context(|| format!("could not read directory {}", directory.display()))?;
        entries.sort_by_key(|entry| entry.file_name().to_string_lossy().into_owned());

        let mut directories = Vec::new();
        for entry in entries {
            let file_type = entry
                .file_type()
                .with_context(|| format!("could not inspect {}", entry.path().display()))?;
            if file_type.is_symlink() {
                stats.symlinks_skipped += 1;
                continue;
            }
            if file_type.is_dir() {
                let file_name = entry.file_name();
                if is_default_pruned(Some(file_name.as_os_str())) {
                    stats.default_directories_pruned += 1;
                    continue;
                }
                let relative = relative_path(&root, &entry.path())?;
                if matches!((ctx.guard)(&Access::Read(&relative)), Guard::Block(_)) {
                    stats.law_directories_pruned += 1;
                    continue;
                }
                directories.push(entry.path());
                continue;
            }
            if !file_type.is_file() {
                stats.special_entries_skipped += 1;
                continue;
            }

            stats.files_visited += 1;
            let relative = relative_path(&root, &entry.path())?;
            match (ctx.guard)(&Access::Read(&relative)) {
                Guard::Allow => {
                    if matches!(visit(&entry.path(), &relative)?, WalkControl::MatchCap) {
                        stats.stopped = Some(StopReason::MatchCap(limits.matches));
                    }
                }
                Guard::Ask | Guard::Block(_) => {
                    stats.files_excluded_by_law += 1;
                }
            }
            if stats.stopped.is_none() && stats.files_visited >= limits.files {
                stats.stopped = Some(StopReason::FileCap(limits.files));
            }
            if stats.stopped.is_some() {
                break;
            }
        }
        if stats.stopped.is_some() {
            break;
        }
        for child in directories.into_iter().rev() {
            stack.push(child);
        }
    }
    Ok(stats)
}

fn is_default_pruned(name: Option<&OsStr>) -> bool {
    name.and_then(OsStr::to_str)
        .is_some_and(|name| DEFAULT_PRUNED_DIRS.contains(&name))
}

fn read_search_file(path: &Path, relative: &str) -> anyhow::Result<SearchFile> {
    let mut file =
        std::fs::File::open(path).with_context(|| format!("could not read {relative}"))?;
    let metadata = file
        .metadata()
        .with_context(|| format!("could not read {relative}"))?;
    if metadata.len() > MAX_TOOL_READ_BYTES as u64 {
        return Ok(SearchFile::Oversized);
    }

    let mut prefix = Vec::with_capacity(BINARY_SNIFF_BYTES as usize);
    (&mut file)
        .take(BINARY_SNIFF_BYTES)
        .read_to_end(&mut prefix)
        .with_context(|| format!("could not read {relative}"))?;
    if prefix.contains(&0) {
        return Ok(SearchFile::Binary);
    }

    let mut bytes = prefix;
    file.take((MAX_TOOL_READ_BYTES + 1 - bytes.len()) as u64)
        .read_to_end(&mut bytes)
        .with_context(|| format!("could not read {relative}"))?;
    if bytes.len() > MAX_TOOL_READ_BYTES {
        return Ok(SearchFile::Oversized);
    }
    Ok(SearchFile::Text(
        String::from_utf8_lossy(&bytes).into_owned(),
    ))
}

fn truncate_search_line(line: &str) -> String {
    let chars = line.chars().count();
    if chars <= MAX_SEARCH_LINE_CHARS {
        return line.to_owned();
    }
    let head: String = line.chars().take(MAX_SEARCH_LINE_CHARS).collect();
    format!("{head}…(+{} more chars)", chars - MAX_SEARCH_LINE_CHARS)
}

fn default_prune_footer(stats: &WalkStats) -> String {
    if stats.default_directories_pruned == 0 {
        String::new()
    } else {
        format!(
            "; {} build/vendor directories pruned by default ({})",
            stats.default_directories_pruned,
            DEFAULT_PRUNED_DIRS.join(", ")
        )
    }
}

fn special_entry_footer(stats: &WalkStats) -> String {
    if stats.special_entries_skipped == 0 {
        String::new()
    } else {
        format!(
            "; {} non-file entries skipped",
            stats.special_entries_skipped
        )
    }
}

fn stop_footer(reason: Option<StopReason>) -> String {
    match reason {
        None => String::new(),
        Some(StopReason::MatchCap(limit)) => format!(
            "; stopped after the {}-match cap; narrow the pattern or scope",
            grouped_number(limit)
        ),
        Some(StopReason::FileCap(limit)) => format!(
            "; stopped after the {}-file visit cap; narrow the pattern or scope",
            grouped_number(limit)
        ),
    }
}

fn grouped_number(value: usize) -> String {
    let digits = value.to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, character) in digits.chars().enumerate() {
        if index != 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(character);
    }
    grouped
}
