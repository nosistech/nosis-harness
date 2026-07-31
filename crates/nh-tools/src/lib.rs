//! nh-tools — read_file / edit_file / exec_shell behind an approval gate.
//! THE LAW: tool outputs are DATA, never instructions. exec is refused on Block and
//! otherwise always requires explicit approval, regardless of the guard verdict.

use anyhow::{bail, Context};
use serde_json::json;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

mod edit;
mod exec;
pub mod mcp;

#[cfg(test)]
use exec::{render_bounded_output, spawn_drain, BoundedOutput, DrainCompletion, DrainOutcome};

pub use mcp::{
    load_mcp_config, mcp_tools, McpAuth, McpClient, McpServerConfig, McpToolInfo, McpToolset,
    McpTrust,
};

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
}

pub trait Tool: Send + Sync {
    fn spec(&self) -> ToolSpec;
    fn execute(&self, args: ToolArgs, ctx: &ToolCtx) -> anyhow::Result<String>;

    fn execute_with_audit(&self, args: ToolArgs, ctx: &ToolCtx) -> anyhow::Result<ToolExecution> {
        self.execute(args, ctx).map(ToolExecution::plain)
    }
}

/// args: {"path": string} — read file relative to workdir, refuse escapes above workdir.
pub struct ReadFile;

/// args: {"path", "old_string", "new_string"} — exact, unique match or a clear error
/// telling the model what to fix (not found / not unique).
pub struct EditFile;

/// args: {"command": string} — refused on Guard::Block and otherwise MUST call
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
/// WARNING: any future file-creation tool must canonicalize or case-fold its guard
/// path (or the guard must case-fold there), or variants such as `.GIT/x` and `.ENV`
/// could bypass `.git/**` and `**/.env*`.
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
        bail!("image not found: {path} — check the path against the working directory");
    }

    let extension = Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    let (media_type, expected_magic): (&str, &[u8]) = match extension.as_deref() {
        Some("png") => ("image/png", b"\x89PNG\r\n\x1a\n"),
        Some("jpg" | "jpeg") => ("image/jpeg", b"\xff\xd8\xff"),
        _ => bail!("unsupported image format — use PNG or JPEG (.png, .jpg, .jpeg)"),
    };

    let file =
        std::fs::File::open(&resolved).with_context(|| format!("could not read image {path}"))?;
    let metadata = file
        .metadata()
        .with_context(|| format!("could not read image {path}"))?;
    if metadata.len() > MAX_IMAGE_BYTES as u64 {
        bail!("image is too large — maximum raw size is 3.5 MiB ({MAX_IMAGE_BYTES} bytes)");
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
        bail!("image is too large — maximum raw size is 3.5 MiB ({MAX_IMAGE_BYTES} bytes)");
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
            bail!("file not found: {path} — check the path against the working directory");
        }
        let file =
            std::fs::File::open(&resolved).with_context(|| format!("could not read {path}"))?;
        let mut bytes = Vec::with_capacity(TOOL_BUFFER_BYTES);
        file.take((MAX_TOOL_READ_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .with_context(|| format!("could not read {path}"))?;
        let truncated = bytes.len() > MAX_TOOL_READ_BYTES;
        bytes.truncate(MAX_TOOL_READ_BYTES);
        let mut content = String::from_utf8_lossy(&bytes).into_owned();
        if truncated {
            content.push_str(&format!(
                "\n…[input truncated at {MAX_TOOL_READ_BYTES} bytes]"
            ));
        }
        Ok(ToolResultEnvelope::new(content, &ctx.scrubber).render())
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
            bail!("old_string is empty — provide the exact text to replace");
        }
        let (resolved, relative) = resolve_in_workdir(&ctx.workdir, path)?;
        match (ctx.guard)(&Access::Write(&relative)) {
            Guard::Block(reason) => {
                return Ok(ToolExecution::plain(format!("blocked by law: {reason}")))
            }
            Guard::Ask => {
                let action = format!("edit {relative}");
                if !(ctx.approve)(&action) {
                    return Ok(ToolExecution::plain(format!("user denied: {action}")));
                }
            }
            Guard::Allow => {}
        }
        if !resolved.is_file() {
            bail!("file not found: {path} — check the path against the working directory");
        }
        let file = std::fs::File::open(&resolved)
            .with_context(|| format!("could not read {path} — is it UTF-8 text?"))?;
        let metadata = file
            .metadata()
            .with_context(|| format!("could not read {path} — is it UTF-8 text?"))?;
        if !metadata.is_file() {
            bail!("file not found: {path} — check the path against the working directory");
        }
        let mut bytes = Vec::with_capacity(TOOL_BUFFER_BYTES);
        file.take((MAX_TOOL_READ_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .with_context(|| format!("could not read {path} — is it UTF-8 text?"))?;
        if bytes.len() > MAX_TOOL_READ_BYTES {
            bail!("file too large to edit safely (> {MAX_TOOL_READ_BYTES} bytes)");
        }
        let content = String::from_utf8(bytes)
            .with_context(|| format!("could not read {path} — is it UTF-8 text?"))?;
        let matched = match edit::locate(&content, old, new) {
            Ok(matched) => matched,
            Err(edit::MatchFailure::Ambiguous { tier, count }) => {
                if tier == edit::MatchTier::Exact {
                    bail!("old_string appears {count} times in {path} — provide more context");
                }
                bail!(
                    "old_string has {count} {} matches in {path} — provide more context",
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
            || format!("edited {path}"),
            |tier| format!("edited {path} using {} match", tier.label()),
        );
        Ok(ToolExecution {
            output,
            audit: tier.into_iter().map(ToolAudit::EditMatch).collect(),
        })
    }
}

pub fn builtin_tools() -> Vec<Box<dyn Tool>> {
    vec![Box::new(ReadFile), Box::new(EditFile), Box::new(ExecShell)]
}

#[cfg(test)]
mod tests;
