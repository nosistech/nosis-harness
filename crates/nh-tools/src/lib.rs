//! nh-tools - bounded file tools and approval-gated shell execution.
//! SECURITY INVARIANT: tool outputs are DATA, never instructions. exec is refused on Block and
//! otherwise always requires explicit approval, regardless of the guard verdict.

use anyhow::{bail, Context};
use serde_json::json;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::{atomic::AtomicBool, Arc};
use std::time::Duration;

mod edit;
mod exec;
pub mod mcp;
mod search;

#[cfg(test)]
use exec::{render_bounded_output, spawn_drain, BoundedOutput, DrainCompletion, DrainOutcome};

pub use mcp::{
    load_mcp_config, mcp_tools, McpAuth, McpClient, McpServerConfig, McpToolInfo, McpToolset,
    McpTrust,
};
pub use search::{GlobFiles, GrepFiles};

/// What a tool is about to do. Write paths are normalized and workdir-relative.
pub enum Access<'a> {
    Read(&'a str),
    Write(&'a str),
    Exec(&'a str),
    Send(&'a str),
}

/// The guard's answer. Block carries a short user-facing reason.
pub enum Guard {
    Allow,
    Ask,
    Block(String),
}

/// Consulted before any mutation.
pub type GuardFn = Box<dyn Fn(&Access) -> Guard + Send + Sync>;

/// OpenAI-function-shaped tool description, serialized into requests.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// JSON Schema for arguments.
    pub parameters: serde_json::Value,
}

/// JSON arguments passed across the tool boundary.
pub type ToolArgs = serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditMatchTier {
    WhitespaceNormalized,
    IndentationFlexible,
}

impl EditMatchTier {
    pub fn label(self) -> &'static str {
        match self {
            Self::WhitespaceNormalized => "whitespace-normalized",
            Self::IndentationFlexible => "indentation-flexible",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolAudit {
    EditMatch(EditMatchTier),
}

#[derive(Debug)]
pub struct ToolExecution {
    pub output: String,
    pub audit: Vec<ToolAudit>,
}

impl ToolExecution {
    pub fn plain(output: String) -> Self {
        Self {
            output,
            audit: Vec::new(),
        }
    }
}

pub struct ToolCtx {
    pub workdir: PathBuf,
    /// Approval gate: called with a human-readable action description before any exec.
    /// Returning false denies the action. UX: the description shown to the user must be
    /// short, concrete, and scannable (the command itself, not prose around it).
    pub approve: Box<dyn Fn(&str) -> bool + Send + Sync>,
    pub guard: GuardFn,
    pub scrubber: nh_vault::Scrubber,
    pub cancel: Arc<AtomicBool>,
}

impl ToolCtx {
    /// Default guard preserves M0/M1 behavior: edits proceed and exec asks.
    pub fn new(workdir: PathBuf, approve: Box<dyn Fn(&str) -> bool + Send + Sync>) -> Self {
        Self {
            workdir,
            approve,
            guard: Box::new(|access| match access {
                Access::Read(_) => Guard::Allow,
                Access::Write(_) => Guard::Allow,
                Access::Exec(_) => Guard::Ask,
                Access::Send(_) => Guard::Allow,
            }),
            scrubber: nh_vault::Scrubber::new(Vec::new()),
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Install a policy-backed guard.
    pub fn with_guard(mut self, guard: GuardFn) -> Self {
        self.guard = guard;
        self
    }

    /// Install the session scrubber so literal vault keys cannot leave through tools.
    pub fn with_scrubber(mut self, scrubber: nh_vault::Scrubber) -> Self {
        self.scrubber = scrubber;
        self
    }

    /// Install the observer for cancellation of the current turn.
    pub fn with_cancel(mut self, cancel: Arc<AtomicBool>) -> Self {
        self.cancel = cancel;
        self
    }
}

pub trait Tool: Send + Sync {
    fn spec(&self) -> ToolSpec;
    fn execute(&self, args: ToolArgs, ctx: &ToolCtx) -> anyhow::Result<String>;

    fn execute_with_audit(&self, args: ToolArgs, ctx: &ToolCtx) -> anyhow::Result<ToolExecution> {
        self.execute(args, ctx).map(ToolExecution::plain)
    }
}

/// args: {"path": string} - read file relative to workdir, refuse escapes above workdir.
pub struct ReadFile;

/// args: {"path", "content"} - create one new file without replacing an existing path
/// or creating parent directories.
pub struct WriteFile;

/// args: {"path", "old_string", "new_string"} - exact, unique match or a clear error
/// telling the model what to fix (not found / not unique).
pub struct EditFile;

/// args: {"command": string} - refused on Guard::Block and otherwise MUST call
/// ctx.approve(command), regardless of the guard verdict. Denial returns an Ok-shaped
/// result the model can read (`user denied: <command>`). Runs via the platform shell,
/// captures stdout+stderr+exit code.
pub struct ExecShell;

/// Product limit for one user message. This is deliberately smaller than any
/// provider-specific allowance so the harness has one predictable boundary.
pub const MAX_IMAGES_PER_MESSAGE: usize = 4;
/// 3.5 MiB raw stays below a 5 MiB provider cap even after base64 expansion.
const MAX_IMAGE_BYTES: usize = 3_670_016;
/// Maximum returned tool-result excerpt before head/tail elision.
const MAX_TOOL_RESULT_CHARS: usize = 32_000;
/// Maximum bytes retained from any one file or child-process stream before elision.
const MAX_TOOL_READ_BYTES: usize = 2 * 1024 * 1024;
/// Bytes checked at the start of a file before treating it as text.
const BINARY_SNIFF_BYTES: u64 = 8 * 1024;
const TOOL_BUFFER_BYTES: usize = 8 * 1024;
const EXEC_TIMEOUT: Duration = Duration::from_secs(300);
// A surviving descendant can hold a captured pipe open forever.
const DRAIN_GRACE: Duration = Duration::from_secs(5);
// Tree termination must be verified without replacing one hang with another.
const KILL_VERIFY_GRACE: Duration = Duration::from_secs(2);

pub(crate) struct ToolResultEnvelope {
    excerpt: String,
    digest: String,
    bytes: usize,
}

impl ToolResultEnvelope {
    pub(crate) fn new(content: String, scrubber: &nh_vault::Scrubber) -> Self {
        let bytes = content.len();
        let mut hasher = DefaultHasher::new();
        content.hash(&mut hasher);
        let digest = format!("{:016x}", hasher.finish());
        let scrubbed = scrubber.scrub(&content);
        let chars = scrubbed.chars().count();
        let excerpt = if chars <= MAX_TOOL_RESULT_CHARS {
            scrubbed
        } else {
            let head_chars = MAX_TOOL_RESULT_CHARS / 2;
            let tail_chars = MAX_TOOL_RESULT_CHARS - head_chars;
            let head: String = scrubbed.chars().take(head_chars).collect();
            let mut tail: Vec<char> = scrubbed.chars().rev().take(tail_chars).collect();
            tail.reverse();
            let tail: String = tail.into_iter().collect();
            format!(
                "{head}\n…[+{} chars elided; digest {digest}]\n{tail}",
                chars - MAX_TOOL_RESULT_CHARS
            )
        };
        Self {
            excerpt,
            digest,
            bytes,
        }
    }

    pub(crate) fn render(&self) -> String {
        let _metadata = (&self.digest, self.bytes);
        self.excerpt.clone()
    }
}

/// Child-process environment allowlist. Names are case-insensitive because
/// Windows environment variables are case-insensitive.
fn is_allowed_env_var(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    let base = matches!(
        upper.as_str(),
        "PATH"
            | "PATHEXT"
            | "SYSTEMROOT"
            | "WINDIR"
            | "COMSPEC"
            | "TEMP"
            | "TMP"
            | "TZ"
            | "LANG"
            | "TERM"
            | "USERPROFILE"
            | "HOMEDRIVE"
            | "HOMEPATH"
            | "HOME"
            | "APPDATA"
            | "LOCALAPPDATA"
            | "PROCESSOR_ARCHITECTURE"
            | "PROCESSOR_IDENTIFIER"
            | "NUMBER_OF_PROCESSORS"
            | "CARGO_HOME"
            | "RUSTUP_HOME"
    );
    #[cfg(not(windows))]
    let locale = upper.starts_with("LC_");
    #[cfg(windows)]
    let locale = false;
    base || locale
}

/// Pull a required string argument out of the tool-call args.
fn str_arg<'a>(args: &'a serde_json::Value, key: &str) -> anyhow::Result<&'a str> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing required argument: {key}"))
}

/// Collapse `.` and `..` components without touching the filesystem.
/// Used for paths that do not exist yet, where canonicalize cannot help.
fn normalize_lexically(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Resolve `rel` against the workdir. Refuses any path that escapes the workdir
/// (canonicalize + starts_with; lexical normalization for not-yet-existing paths,
/// so a missing file can still get a clear "file not found" from the caller).
/// The returned guard path uses the same resolved path, made relative and joined
/// with `/` on every platform.
///
/// The write-hold remains sound with case-sensitive globs on case-insensitive
/// filesystems because `EditFile` only mutates existing files: `canonicalize`
/// returns their real on-disk case for the guard (for example, `.GIT/config`
/// becomes `.git/config`). Missing paths retain typed case only in the lexical
/// branch, then fail with "file not found" before any write.
///
/// `WriteFile` closes the missing-path case-folding bypass in
/// `creation_guard_verdict`: it checks both the typed relative path and its
/// ASCII-lowercased form, so `.GIT/x` and `.ENV` cannot evade the write law.
fn resolve_in_workdir(workdir: &Path, rel: &str) -> anyhow::Result<(PathBuf, String)> {
    let root = workdir
        .canonicalize()
        .with_context(|| format!("working directory not found: {}", workdir.display()))?;
    let joined = root.join(rel);
    let resolved = match joined.canonicalize() {
        Ok(canon) => canon,
        // Path does not exist (yet); catch `..`/absolute escapes lexically.
        Err(_) => normalize_lexically(&joined),
    };
    if !resolved.starts_with(&root) {
        bail!("refused: {rel} escapes the working directory");
    }
    let relative = resolved
        .strip_prefix(&root)
        .map_err(|_| anyhow::anyhow!("resolved path escaped the working directory"))?
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    Ok((resolved, relative))
}

fn merge_guard_verdict(first: Guard, second: Guard) -> Guard {
    match (first, second) {
        (Guard::Block(reason), _) => Guard::Block(reason),
        (_, Guard::Block(reason)) => Guard::Block(reason),
        (Guard::Ask, _) | (_, Guard::Ask) => Guard::Ask,
        (Guard::Allow, Guard::Allow) => Guard::Allow,
    }
}

/// Check the typed creation path and its ASCII-folded form. If an existing
/// parent resolves through an in-workdir alias, check that actual path too.
fn creation_guard_verdict(ctx: &ToolCtx, relative: &str, actual_relative: Option<&str>) -> Guard {
    let folded = relative.to_ascii_lowercase();
    let typed = (ctx.guard)(&Access::Write(relative));
    let lowercase = (ctx.guard)(&Access::Write(&folded));
    let mut verdict = merge_guard_verdict(typed, lowercase);

    if let Some(actual) = actual_relative.filter(|actual| *actual != relative) {
        let actual_folded = actual.to_ascii_lowercase();
        let actual_verdict = (ctx.guard)(&Access::Write(actual));
        let actual_lowercase = (ctx.guard)(&Access::Write(&actual_folded));
        verdict = merge_guard_verdict(
            verdict,
            merge_guard_verdict(actual_verdict, actual_lowercase),
        );
    }
    verdict
}

fn path_exists_without_following(path: &Path, display_path: &str) -> anyhow::Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => {
            Err(error).with_context(|| format!("could not inspect destination {display_path}"))
        }
    }
}

fn relative_path(root: &Path, path: &Path) -> anyhow::Result<String> {
    Ok(path
        .strip_prefix(root)
        .map_err(|_| anyhow::anyhow!("resolved path escaped the working directory"))?
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/"))
}

fn destination_label(actual_relative: &str, requested_relative: &str) -> String {
    if actual_relative == requested_relative {
        actual_relative.to_owned()
    } else {
        format!("{actual_relative} (requested as {requested_relative})")
    }
}

fn parent_label(relative: &str) -> String {
    let parent = Path::new(relative)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let label = parent
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    if label.is_empty() {
        ".".to_owned()
    } else {
        label
    }
}

/// A validated image ready for a model-facing content part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedImage {
    pub media_type: String,
    pub data: String,
}

/// Load one user-selected image through the same read-law and workdir boundary
/// as `read_file`, then validate and base64-encode it for the wire layer.
pub fn load_image(path: &str, ctx: &ToolCtx) -> anyhow::Result<LoadedImage> {
    let (resolved, relative) = resolve_in_workdir(&ctx.workdir, path)?;
    match (ctx.guard)(&Access::Read(&relative)) {
        Guard::Block(reason) => bail!("blocked by law: {reason}"),
        Guard::Ask => {
            let action = format!("read {relative}");
            if !(ctx.approve)(&action) {
                bail!("user denied: {action}");
            }
        }
        Guard::Allow => {}
    }
    if !resolved.is_file() {
        bail!("image not found: {path} - check the path against the working directory");
    }

    let extension = Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    let (media_type, expected_magic): (&str, &[u8]) = match extension.as_deref() {
        Some("png") => ("image/png", b"\x89PNG\r\n\x1a\n"),
        Some("jpg" | "jpeg") => ("image/jpeg", b"\xff\xd8\xff"),
        _ => bail!("unsupported image format - use PNG or JPEG (.png, .jpg, .jpeg)"),
    };

    let file =
        std::fs::File::open(&resolved).with_context(|| format!("could not read image {path}"))?;
    let metadata = file
        .metadata()
        .with_context(|| format!("could not read image {path}"))?;
    if metadata.len() > MAX_IMAGE_BYTES as u64 {
        bail!("image is too large - maximum raw size is 3.5 MiB ({MAX_IMAGE_BYTES} bytes)");
    }
    let mut bytes = Vec::with_capacity(
        usize::try_from(metadata.len())
            .unwrap_or(MAX_IMAGE_BYTES)
            .min(MAX_IMAGE_BYTES),
    );
    file.take((MAX_IMAGE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .with_context(|| format!("could not read image {path}"))?;
    if bytes.len() > MAX_IMAGE_BYTES {
        bail!("image is too large - maximum raw size is 3.5 MiB ({MAX_IMAGE_BYTES} bytes)");
    }
    if !bytes.starts_with(expected_magic) {
        let extension = extension.as_deref().unwrap_or_default();
        bail!("image bytes do not match the .{extension} extension");
    }

    Ok(LoadedImage {
        media_type: media_type.to_owned(),
        data: encode_base64(&bytes),
    })
}

fn encode_base64(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(input.len().div_ceil(3) * 4);
    let mut chunks = input.chunks_exact(3);
    for chunk in &mut chunks {
        let bits = (u32::from(chunk[0]) << 16) | (u32::from(chunk[1]) << 8) | u32::from(chunk[2]);
        encoded.push(ALPHABET[((bits >> 18) & 0x3f) as usize] as char);
        encoded.push(ALPHABET[((bits >> 12) & 0x3f) as usize] as char);
        encoded.push(ALPHABET[((bits >> 6) & 0x3f) as usize] as char);
        encoded.push(ALPHABET[(bits & 0x3f) as usize] as char);
    }
    match chunks.remainder() {
        [first] => {
            let bits = u32::from(*first) << 16;
            encoded.push(ALPHABET[((bits >> 18) & 0x3f) as usize] as char);
            encoded.push(ALPHABET[((bits >> 12) & 0x3f) as usize] as char);
            encoded.push('=');
            encoded.push('=');
        }
        [first, second] => {
            let bits = (u32::from(*first) << 16) | (u32::from(*second) << 8);
            encoded.push(ALPHABET[((bits >> 18) & 0x3f) as usize] as char);
            encoded.push(ALPHABET[((bits >> 12) & 0x3f) as usize] as char);
            encoded.push(ALPHABET[((bits >> 6) & 0x3f) as usize] as char);
            encoded.push('=');
        }
        [] => {}
        _ => unreachable!("chunks_exact(3) leaves at most two bytes"),
    }
    encoded
}

impl Tool for ReadFile {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "read_file".into(),
            description: "Read a UTF-8 text file inside the working directory.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path, relative to the working directory."
                    }
                },
                "required": ["path"]
            }),
        }
    }

    fn execute(&self, args: serde_json::Value, ctx: &ToolCtx) -> anyhow::Result<String> {
        let path = str_arg(&args, "path")?;
        let (resolved, relative) = resolve_in_workdir(&ctx.workdir, path)?;
        match (ctx.guard)(&Access::Read(&relative)) {
            Guard::Block(reason) => return Ok(format!("blocked by law: {reason}")),
            Guard::Ask => {
                let action = format!("read {relative}");
                if !(ctx.approve)(&action) {
                    return Ok(format!("user denied: {action}"));
                }
            }
            Guard::Allow => {}
        }
        if !resolved.is_file() {
            bail!("file not found: {path} - check the path against the working directory");
        }
        let mut file =
            std::fs::File::open(&resolved).with_context(|| format!("could not read {path}"))?;
        let mut bytes = Vec::with_capacity(TOOL_BUFFER_BYTES);
        (&mut file)
            .take(BINARY_SNIFF_BYTES)
            .read_to_end(&mut bytes)
            .with_context(|| format!("could not read {path}"))?;
        if bytes.contains(&0) {
            bail!("file looks binary: {path} - choose a text file or use a binary-aware tool");
        }
        file.take((MAX_TOOL_READ_BYTES + 1 - bytes.len()) as u64)
            .read_to_end(&mut bytes)
            .with_context(|| format!("could not read {path}"))?;
        let truncated = bytes.len() > MAX_TOOL_READ_BYTES;
        bytes.truncate(MAX_TOOL_READ_BYTES);
        let (mut content, replaced_invalid_utf8) = match String::from_utf8(bytes) {
            Ok(content) => (content, false),
            Err(error) => {
                let utf8_error = error.utf8_error();
                let mut bytes = error.into_bytes();
                if truncated && utf8_error.error_len().is_none() {
                    bytes.truncate(utf8_error.valid_up_to());
                    (String::from_utf8_lossy(&bytes).into_owned(), false)
                } else {
                    (String::from_utf8_lossy(&bytes).into_owned(), true)
                }
            }
        };
        if replaced_invalid_utf8 {
            content.push_str("\n…[some bytes were not valid UTF-8 and were replaced]");
        }
        if truncated {
            content.push_str(&format!(
                "\n…[input truncated at {MAX_TOOL_READ_BYTES} bytes]"
            ));
        }
        Ok(ToolResultEnvelope::new(content, &ctx.scrubber).render())
    }
}

impl Tool for WriteFile {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "write_file".into(),
            description: "Create a new UTF-8 text file inside the working directory. Refuses existing files and missing parent directories.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "New file path, relative to the working directory."
                    },
                    "content": {
                        "type": "string",
                        "description": "Complete UTF-8 content for the new file."
                    }
                },
                "required": ["path", "content"]
            }),
        }
    }

    fn execute(&self, args: serde_json::Value, ctx: &ToolCtx) -> anyhow::Result<String> {
        let path = str_arg(&args, "path")?;
        let content = str_arg(&args, "content")?;
        let (resolved, relative) = resolve_in_workdir(&ctx.workdir, path)?;
        let parent = resolved.parent().ok_or_else(|| {
            anyhow::anyhow!("could not write {path}: file has no parent directory")
        })?;
        let parent_is_dir = parent.is_dir();
        let initially_exists = path_exists_without_following(&resolved, path)?;

        let (destination, actual_relative) = if !initially_exists && parent_is_dir {
            let root = ctx.workdir.canonicalize().with_context(|| {
                format!("working directory not found: {}", ctx.workdir.display())
            })?;
            let canonical_parent = parent
                .canonicalize()
                .with_context(|| format!("could not resolve parent directory for {path}"))?;
            if !canonical_parent.starts_with(&root) {
                bail!("refused: {path} escapes the working directory");
            }
            let file_name = resolved
                .file_name()
                .ok_or_else(|| anyhow::anyhow!("could not write {path}: invalid file name"))?;
            let destination = canonical_parent.join(file_name);
            let actual_relative = relative_path(&root, &destination)?;
            (destination, Some(actual_relative))
        } else {
            (resolved.clone(), None)
        };

        let verdict = creation_guard_verdict(ctx, &relative, actual_relative.as_deref());
        if let Guard::Block(reason) = &verdict {
            return Ok(
                ToolResultEnvelope::new(format!("blocked by law: {reason}"), &ctx.scrubber)
                    .render(),
            );
        }

        if initially_exists || path_exists_without_following(&destination, path)? {
            return Ok(ToolResultEnvelope::new(
                format!("refused: {path} already exists - use edit_file to change it"),
                &ctx.scrubber,
            )
            .render());
        }
        if content.len() > MAX_TOOL_READ_BYTES {
            bail!("content too large to write safely (> {MAX_TOOL_READ_BYTES} bytes)");
        }
        if !parent_is_dir {
            return Ok(ToolResultEnvelope::new(
                format!(
                    "refused: parent directory does not exist: {} - create it first",
                    parent_label(&relative)
                ),
                &ctx.scrubber,
            )
            .render());
        }
        let actual_relative = actual_relative.as_deref().unwrap_or(&relative);
        let destination_label = destination_label(actual_relative, &relative);
        if matches!(verdict, Guard::Ask) {
            let action = format!("create {destination_label}");
            if !(ctx.approve)(&action) {
                return Ok(ToolResultEnvelope::new(
                    format!("user denied: {action}"),
                    &ctx.scrubber,
                )
                .render());
            }
        }

        let destination_parent = destination.parent().ok_or_else(|| {
            anyhow::anyhow!("could not write {path}: file has no parent directory")
        })?;
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let mut attempt = 0_u16;
        let (temp_path, mut temp_file) = loop {
            if attempt == 1000 {
                bail!("could not create temporary file for {path}");
            }
            let candidate = destination_parent.join(format!(
                ".nh-write-{}-{nonce}-{attempt}.tmp",
                std::process::id()
            ));
            match std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&candidate)
            {
                Ok(file) => break (candidate, file),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    attempt += 1;
                }
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("could not create temporary file for {path}"))
                }
            }
        };

        let write_result = (|| -> anyhow::Result<()> {
            use std::io::Write as _;

            temp_file
                .write_all(content.as_bytes())
                .with_context(|| format!("could not write {path}"))?;
            // A new file deliberately keeps the platform-default mode assigned
            // when the temporary file is created; there is no prior mode to copy.
            temp_file
                .flush()
                .with_context(|| format!("could not flush temporary file for {path}"))?;
            temp_file
                .sync_all()
                .with_context(|| format!("could not fsync temporary file for {path}"))?;
            Ok(())
        })();
        drop(temp_file);
        if let Err(error) = write_result {
            let _ = std::fs::remove_file(&temp_path);
            return Err(error);
        }

        match path_exists_without_following(&destination, path) {
            Ok(false) => {}
            Ok(true) => {
                let _ = std::fs::remove_file(&temp_path);
                return Ok(ToolResultEnvelope::new(
                    format!("refused: {path} already exists - use edit_file to change it"),
                    &ctx.scrubber,
                )
                .render());
            }
            Err(error) => {
                let _ = std::fs::remove_file(&temp_path);
                return Err(error);
            }
        }
        if let Err(error) = std::fs::rename(&temp_path, &destination) {
            let _ = std::fs::remove_file(&temp_path);
            if error.kind() == std::io::ErrorKind::AlreadyExists
                || path_exists_without_following(&destination, path).unwrap_or(false)
            {
                return Ok(ToolResultEnvelope::new(
                    format!("refused: {path} already exists - use edit_file to change it"),
                    &ctx.scrubber,
                )
                .render());
            }
            return Err(error).with_context(|| format!("could not create {path}"));
        }
        Ok(ToolResultEnvelope::new(
            format!("created {destination_label} ({} bytes)", content.len()),
            &ctx.scrubber,
        )
        .render())
    }
}

impl Tool for EditFile {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "edit_file".into(),
            description:
                "Replace one unique occurrence of old_string with new_string in a file. Exact matching is preferred; whitespace-only drift may be tolerated and reported."
                    .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path, relative to the working directory."
                    },
                    "old_string": {
                        "type": "string",
                        "description": "Text to replace. It must identify exactly one region in the file."
                    },
                    "new_string": {
                        "type": "string",
                        "description": "Replacement text."
                    }
                },
                "required": ["path", "old_string", "new_string"]
            }),
        }
    }

    fn execute(&self, args: serde_json::Value, ctx: &ToolCtx) -> anyhow::Result<String> {
        self.execute_with_audit(args, ctx)
            .map(|execution| execution.output)
    }

    fn execute_with_audit(&self, args: ToolArgs, ctx: &ToolCtx) -> anyhow::Result<ToolExecution> {
        let path = str_arg(&args, "path")?;
        let old = str_arg(&args, "old_string")?;
        let new = str_arg(&args, "new_string")?;
        if old.is_empty() {
            bail!("old_string is empty - provide the exact text to replace");
        }
        let (resolved, relative) = resolve_in_workdir(&ctx.workdir, path)?;
        let destination_label = destination_label(&relative, path);
        match (ctx.guard)(&Access::Write(&relative)) {
            Guard::Block(reason) => {
                return Ok(ToolExecution::plain(format!("blocked by law: {reason}")))
            }
            Guard::Ask => {
                let action = format!("edit {destination_label}");
                if !(ctx.approve)(&action) {
                    return Ok(ToolExecution::plain(format!("user denied: {action}")));
                }
            }
            Guard::Allow => {}
        }
        if !resolved.is_file() {
            bail!("file not found: {path} - check the path against the working directory");
        }
        let file = std::fs::File::open(&resolved)
            .with_context(|| format!("could not read {path} - is it UTF-8 text?"))?;
        let metadata = file
            .metadata()
            .with_context(|| format!("could not read {path} - is it UTF-8 text?"))?;
        if !metadata.is_file() {
            bail!("file not found: {path} - check the path against the working directory");
        }
        let mut bytes = Vec::with_capacity(TOOL_BUFFER_BYTES);
        file.take((MAX_TOOL_READ_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .with_context(|| format!("could not read {path} - is it UTF-8 text?"))?;
        if bytes.len() > MAX_TOOL_READ_BYTES {
            bail!("file too large to edit safely (> {MAX_TOOL_READ_BYTES} bytes)");
        }
        let content = String::from_utf8(bytes)
            .with_context(|| format!("could not read {path} - is it UTF-8 text?"))?;
        let matched = match edit::locate(&content, old, new) {
            Ok(matched) => matched,
            Err(edit::MatchFailure::Ambiguous { tier, count }) => {
                if tier == edit::MatchTier::Exact {
                    bail!("old_string appears {count} times in {path} - provide more context");
                }
                bail!(
                    "old_string has {count} {} matches in {path} - provide more context",
                    tier.label()
                );
            }
            Err(edit::MatchFailure::NotFound(candidate)) => {
                let actual = ctx.scrubber.scrub(&candidate.text);
                bail!(
                    "old_string not found in {path}\nnearest candidate: {path}:{}-{}\nactual text:\n{actual}",
                    candidate.first_line,
                    candidate.last_line
                )
            }
        };
        let mut edited = String::with_capacity(
            content.len() - (matched.range.end - matched.range.start) + matched.replacement.len(),
        );
        edited.push_str(&content[..matched.range.start]);
        edited.push_str(&matched.replacement);
        edited.push_str(&content[matched.range.end..]);
        let parent = resolved.parent().ok_or_else(|| {
            anyhow::anyhow!("could not write {path}: file has no parent directory")
        })?;
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let mut attempt = 0_u16;
        let (temp_path, mut temp_file) = loop {
            if attempt == 1000 {
                bail!("could not create temporary file for {path}");
            }
            let candidate = parent.join(format!(
                ".nh-edit-{}-{nonce}-{attempt}.tmp",
                std::process::id()
            ));
            match std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&candidate)
            {
                Ok(file) => break (candidate, file),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    attempt += 1;
                }
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("could not create temporary file for {path}"))
                }
            }
        };

        let write_result = (|| -> anyhow::Result<()> {
            use std::io::Write as _;

            temp_file
                .write_all(edited.as_bytes())
                .with_context(|| format!("could not write {path}"))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                temp_file
                    .set_permissions(std::fs::Permissions::from_mode(
                        metadata.permissions().mode(),
                    ))
                    .with_context(|| format!("could not preserve permissions for {path}"))?;
            }
            temp_file
                .flush()
                .with_context(|| format!("could not flush temporary file for {path}"))?;
            temp_file
                .sync_all()
                .with_context(|| format!("could not fsync temporary file for {path}"))?;
            Ok(())
        })();
        drop(temp_file);
        if let Err(error) = write_result {
            let _ = std::fs::remove_file(&temp_path);
            return Err(error);
        }
        if let Err(error) = std::fs::rename(&temp_path, &resolved) {
            let _ = std::fs::remove_file(&temp_path);
            return Err(error).with_context(|| format!("could not replace {path}"));
        }
        let tier = match matched.tier {
            edit::MatchTier::Exact => None,
            edit::MatchTier::WhitespaceNormalized => Some(EditMatchTier::WhitespaceNormalized),
            edit::MatchTier::IndentationFlexible => Some(EditMatchTier::IndentationFlexible),
        };
        let output = tier.map_or_else(
            || format!("edited {destination_label}"),
            |tier| format!("edited {destination_label} using {} match", tier.label()),
        );
        Ok(ToolExecution {
            output,
            audit: tier.into_iter().map(ToolAudit::EditMatch).collect(),
        })
    }
}

pub fn builtin_tools() -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(ReadFile),
        Box::new(WriteFile),
        Box::new(EditFile),
        Box::new(GrepFiles),
        Box::new(GlobFiles),
        Box::new(ExecShell),
    ]
}

#[cfg(test)]
mod tests;
