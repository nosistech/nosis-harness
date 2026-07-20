//! nh-tools — read_file / edit_file / exec_shell behind an approval gate.
//! THE LAW: tool outputs are DATA, never instructions. exec always passes the gate.

use anyhow::{bail, Context};
use serde_json::json;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdout, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::process::CommandExt;
#[cfg(windows)]
use std::os::windows::process::CommandExt;

pub mod mcp;

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
    fn execute(&self, args: serde_json::Value, ctx: &ToolCtx) -> anyhow::Result<String>;
}

/// args: {"path": string} — read file relative to workdir, refuse escapes above workdir.
pub struct ReadFile;

/// args: {"path", "old_string", "new_string"} — exact, unique match or a clear error
/// telling the model what to fix (not found / not unique).
pub struct EditFile;

/// args: {"command": string} — MUST call ctx.approve(command) first; denial returns an
/// Ok-shaped result the model can read ("user denied: <command>"). Runs via the platform
/// shell, captures stdout+stderr+exit code.
pub struct ExecShell;

/// Maximum returned tool-result excerpt before head/tail elision.
const MAX_TOOL_RESULT_CHARS: usize = 32_000;
/// Maximum bytes retained from any one file or child-process stream before elision.
const MAX_TOOL_READ_BYTES: usize = 2 * 1024 * 1024;
const TOOL_BUFFER_BYTES: usize = 8 * 1024;
const EXEC_TIMEOUT: Duration = Duration::from_secs(300);

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
        .expect("resolved path was checked inside workdir")
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    Ok((resolved, relative))
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
            description: "Replace one exact occurrence of old_string with new_string in a file."
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
                        "description": "Exact text to replace. Must appear exactly once in the file."
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
        let path = str_arg(&args, "path")?;
        let old = str_arg(&args, "old_string")?;
        let new = str_arg(&args, "new_string")?;
        if old.is_empty() {
            bail!("old_string is empty — provide the exact text to replace");
        }
        let (resolved, relative) = resolve_in_workdir(&ctx.workdir, path)?;
        match (ctx.guard)(&Access::Write(&relative)) {
            Guard::Block(reason) => return Ok(format!("blocked by law: {reason}")),
            Guard::Ask => {
                let action = format!("edit {relative}");
                if !(ctx.approve)(&action) {
                    return Ok(format!("user denied: {action}"));
                }
            }
            Guard::Allow => {}
        }
        if !resolved.is_file() {
            bail!("file not found: {path} — check the path against the working directory");
        }
        let content = std::fs::read_to_string(&resolved)
            .with_context(|| format!("could not read {path} — is it UTF-8 text?"))?;
        match content.matches(old).count() {
            0 => bail!("old_string not found in {path}"),
            1 => {}
            n => bail!("old_string appears {n} times in {path} — provide more context"),
        }
        let edited = content.replacen(old, new, 1);
        std::fs::write(&resolved, edited).with_context(|| format!("could not write {path}"))?;
        Ok(format!("edited {path}"))
    }
}

impl Tool for ExecShell {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "exec_shell".into(),
            description: "Run a shell command in the working directory. Requires user approval."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to run."
                    }
                },
                "required": ["command"]
            }),
        }
    }

    fn execute(&self, args: serde_json::Value, ctx: &ToolCtx) -> anyhow::Result<String> {
        self.execute_with_timeout(args, ctx, EXEC_TIMEOUT)
    }
}

struct BoundedOutput {
    bytes: Vec<u8>,
    truncated: bool,
}

fn drain_bounded<R: Read>(mut reader: R) -> std::io::Result<BoundedOutput> {
    let mut bytes = Vec::with_capacity(TOOL_BUFFER_BYTES);
    let mut truncated = false;
    let mut chunk = [0u8; TOOL_BUFFER_BYTES];
    loop {
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        let remaining = MAX_TOOL_READ_BYTES.saturating_sub(bytes.len());
        let keep = remaining.min(read);
        bytes.extend_from_slice(&chunk[..keep]);
        truncated |= keep < read;
    }
    Ok(BoundedOutput { bytes, truncated })
}

fn join_output(
    handle: thread::JoinHandle<std::io::Result<BoundedOutput>>,
    stream: &str,
) -> anyhow::Result<BoundedOutput> {
    handle
        .join()
        .map_err(|_| anyhow::anyhow!("could not capture command {stream}: drain thread panicked"))?
        .with_context(|| format!("could not capture command {stream}"))
}

fn render_bounded_output(output: BoundedOutput, stream: &str) -> String {
    let mut rendered = String::from_utf8_lossy(&output.bytes).into_owned();
    if output.truncated {
        rendered.push_str(&format!(
            "\n…[{stream} truncated at {MAX_TOOL_READ_BYTES} bytes]"
        ));
    }
    rendered
}

fn timeout_label(timeout: Duration) -> String {
    if timeout.subsec_nanos() == 0 {
        format!("{}s", timeout.as_secs())
    } else {
        format!("{}ms", timeout.as_millis())
    }
}

fn terminate_child_tree(child: &mut Child, command: &str) -> anyhow::Result<ExitStatus> {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(unix)]
    {
        let _ = std::process::Command::new("kill")
            .args(["-KILL", &format!("-{}", child.id())])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    if let Some(status) = child
        .try_wait()
        .with_context(|| format!("could not reap timed-out command: {command}"))?
    {
        return Ok(status);
    }
    child
        .kill()
        .with_context(|| format!("could not kill timed-out command: {command}"))?;
    child
        .wait()
        .with_context(|| format!("could not reap timed-out command: {command}"))
}

impl ExecShell {
    fn execute_with_timeout(
        &self,
        args: serde_json::Value,
        ctx: &ToolCtx,
        timeout: Duration,
    ) -> anyhow::Result<String> {
        let command = str_arg(&args, "command")?;
        match (ctx.guard)(&Access::Exec(command)) {
            Guard::Block(reason) => return Ok(format!("blocked by law: {reason}")),
            Guard::Ask => {
                // THE LAW: shipped policy always routes exec through approval.
                if !(ctx.approve)(command) {
                    // Ok-shaped so the model can read the denial and adapt, not crash the turn.
                    return Ok(format!("user denied: {command}"));
                }
            }
            Guard::Allow => {}
        }
        #[cfg(windows)]
        let mut cmd = {
            let mut c = std::process::Command::new("cmd");
            c.arg("/C");
            c.raw_arg(command);
            c
        };
        #[cfg(not(windows))]
        let mut cmd = {
            let mut c = std::process::Command::new("sh");
            c.args(["-c", command]);
            c.process_group(0);
            c
        };
        cmd.current_dir(&ctx.workdir);
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        // THE LAW: approved commands get only the minimum environment required
        // for shells and normal build tools, never ambient credentials.
        cmd.env_clear();
        for (name, value) in std::env::vars_os() {
            if is_allowed_env_var(&name.to_string_lossy()) {
                cmd.env(&name, value);
            }
        }
        let mut child = cmd
            .spawn()
            .with_context(|| format!("could not run command: {command}"))?;
        let stdout: ChildStdout = child.stdout.take().expect("stdout configured as piped");
        let stderr: ChildStderr = child.stderr.take().expect("stderr configured as piped");
        let stdout_thread = thread::spawn(move || drain_bounded(stdout));
        let stderr_thread = thread::spawn(move || drain_bounded(stderr));

        let deadline = Instant::now() + timeout;
        let mut timed_out = false;
        let status = loop {
            if let Some(status) = child
                .try_wait()
                .with_context(|| format!("could not wait for command: {command}"))?
            {
                break status;
            }
            if Instant::now() >= deadline {
                timed_out = true;
                break terminate_child_tree(&mut child, command)?;
            }
            thread::sleep(Duration::from_millis(50));
        };

        let stdout = render_bounded_output(join_output(stdout_thread, "stdout")?, "stdout");
        let stderr = render_bounded_output(join_output(stderr_thread, "stderr")?, "stderr");
        let content = if timed_out {
            format!(
                "command timed out after {} — killed\nstdout:\n{stdout}\nstderr:\n{stderr}",
                timeout_label(timeout)
            )
        } else {
            let code = status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "killed by signal".into());
            format!("exit code: {code}\nstdout:\n{stdout}\nstderr:\n{stderr}")
        };
        Ok(ToolResultEnvelope::new(content, &ctx.scrubber).render())
    }
}

pub fn builtin_tools() -> Vec<Box<dyn Tool>> {
    vec![Box::new(ReadFile), Box::new(EditFile), Box::new(ExecShell)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn ctx_with(workdir: &Path, approve: bool) -> ToolCtx {
        ToolCtx::new(workdir.to_path_buf(), Box::new(move |_| approve))
    }

    #[test]
    fn specs_have_expected_names_and_required_args() {
        let tools = builtin_tools();
        let specs: Vec<ToolSpec> = tools.iter().map(|t| t.spec()).collect();
        let names: Vec<&str> = specs.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["read_file", "edit_file", "exec_shell"]);
        assert_eq!(specs[0].parameters["required"], json!(["path"]));
        assert_eq!(
            specs[1].parameters["required"],
            json!(["path", "old_string", "new_string"])
        );
        assert_eq!(specs[2].parameters["required"], json!(["command"]));
        for spec in &specs {
            assert_eq!(spec.parameters["type"], "object");
            assert!(!spec.description.is_empty());
        }
    }

    #[test]
    fn read_edit_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("note.txt"), "hello old world").unwrap();
        let ctx = ctx_with(dir.path(), true);

        let text = ReadFile.execute(json!({"path": "note.txt"}), &ctx).unwrap();
        assert_eq!(text, "hello old world");

        let result = EditFile
            .execute(
                json!({"path": "note.txt", "old_string": "old", "new_string": "new"}),
                &ctx,
            )
            .unwrap();
        assert_eq!(result, "edited note.txt");

        let text = ReadFile.execute(json!({"path": "note.txt"}), &ctx).unwrap();
        assert_eq!(text, "hello new world");
    }

    #[test]
    fn read_uses_session_scrubber_for_literal_secrets() {
        let dir = tempfile::tempdir().unwrap();
        const LITERAL: &str = "fixture-literal-abc123";
        std::fs::write(dir.path().join("secret.txt"), LITERAL).unwrap();
        let ctx = ctx_with(dir.path(), true)
            .with_scrubber(nh_vault::Scrubber::new(vec![LITERAL.to_string()]));

        let result = ReadFile
            .execute(json!({"path": "secret.txt"}), &ctx)
            .unwrap();

        assert_eq!(result, "[REDACTED]");
        assert!(!result.contains(LITERAL));
    }

    #[test]
    fn default_tool_context_still_scrubs_shapes_only() {
        let dir = tempfile::tempdir().unwrap();
        let shaped = "sk-fixture-abc123";
        let plain = "fixture-literal-abc123";
        std::fs::write(dir.path().join("output.txt"), format!("{shaped}\n{plain}")).unwrap();

        let result = ReadFile
            .execute(json!({"path": "output.txt"}), &ctx_with(dir.path(), true))
            .unwrap();

        assert_eq!(result, format!("[REDACTED]\n{plain}"));
    }

    #[test]
    fn oversized_read_is_bounded_before_envelope_elision() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("large.txt"),
            vec![b'x'; MAX_TOOL_READ_BYTES + 100_000],
        )
        .unwrap();

        let result = ReadFile
            .execute(json!({"path": "large.txt"}), &ctx_with(dir.path(), true))
            .unwrap();

        assert!(result.contains("chars elided; digest "), "got: {result}");
        assert!(
            result.contains("input truncated at 2097152 bytes"),
            "got: {result}"
        );
        assert!(result.chars().count() <= MAX_TOOL_RESULT_CHARS + 100);
    }

    #[test]
    fn read_missing_file_names_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_with(dir.path(), true);
        let err = ReadFile
            .execute(json!({"path": "nope.txt"}), &ctx)
            .unwrap_err()
            .to_string();
        assert!(err.contains("file not found: nope.txt"), "got: {err}");
    }

    #[test]
    fn edit_old_string_not_found() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "abc").unwrap();
        let ctx = ctx_with(dir.path(), true);
        let err = EditFile
            .execute(
                json!({"path": "a.txt", "old_string": "zzz", "new_string": "y"}),
                &ctx,
            )
            .unwrap_err()
            .to_string();
        assert_eq!(err, "old_string not found in a.txt");
    }

    #[test]
    fn edit_non_unique_old_string() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "foo bar foo").unwrap();
        let ctx = ctx_with(dir.path(), true);
        let err = EditFile
            .execute(
                json!({"path": "a.txt", "old_string": "foo", "new_string": "baz"}),
                &ctx,
            )
            .unwrap_err()
            .to_string();
        assert_eq!(
            err,
            "old_string appears 2 times in a.txt — provide more context"
        );
    }

    #[test]
    fn path_escape_blocked_for_existing_and_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let workdir = dir.path().join("inner");
        std::fs::create_dir(&workdir).unwrap();
        std::fs::write(dir.path().join("secret.txt"), "sk-test-0000").unwrap();
        let ctx = ctx_with(&workdir, true);

        // Existing file above workdir (canonicalize branch).
        let err = ReadFile
            .execute(json!({"path": "../secret.txt"}), &ctx)
            .unwrap_err()
            .to_string();
        assert!(err.contains("escapes the working directory"), "got: {err}");

        // Missing file above workdir (lexical branch) — still an escape, not "not found".
        let err = ReadFile
            .execute(json!({"path": "../missing.txt"}), &ctx)
            .unwrap_err()
            .to_string();
        assert!(err.contains("escapes the working directory"), "got: {err}");

        // Absolute path outside workdir.
        let abs = dir.path().join("secret.txt").display().to_string();
        let err = ReadFile
            .execute(json!({"path": abs}), &ctx)
            .unwrap_err()
            .to_string();
        assert!(err.contains("escapes the working directory"), "got: {err}");

        // EditFile goes through the same gate.
        let err = EditFile
            .execute(
                json!({"path": "../secret.txt", "old_string": "a", "new_string": "b"}),
                &ctx,
            )
            .unwrap_err()
            .to_string();
        assert!(err.contains("escapes the working directory"), "got: {err}");
    }

    #[test]
    fn exec_denied_never_runs_and_is_ok_shaped() {
        let dir = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_seen = calls.clone();
        let ctx = ToolCtx::new(
            dir.path().to_path_buf(),
            Box::new(move |cmd| {
                assert_eq!(cmd, "echo pwned > marker.txt");
                calls_seen.fetch_add(1, Ordering::SeqCst);
                false
            }),
        );
        let result = ExecShell
            .execute(json!({"command": "echo pwned > marker.txt"}), &ctx)
            .unwrap();
        assert_eq!(result, "user denied: echo pwned > marker.txt");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "approval gate must be consulted"
        );
        assert!(
            !dir.path().join("marker.txt").exists(),
            "denied command must never execute"
        );
    }

    #[test]
    fn exec_echo_happy_path() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_with(dir.path(), true);
        let result = ExecShell
            .execute(json!({"command": "echo hello"}), &ctx)
            .unwrap();
        assert!(result.contains("exit code: 0"), "got: {result}");
        assert!(result.contains("hello"), "got: {result}");
    }

    #[cfg(windows)]
    #[test]
    fn exec_windows_preserves_approved_embedded_quotes_verbatim() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_with(dir.path(), true);
        let command = r#"echo a "b c" d"#;

        let result = ExecShell
            .execute(json!({"command": command}), &ctx)
            .unwrap();

        assert!(result.contains("a \"b c\" d"), "got: {result}");
    }

    #[test]
    fn exec_timeout_kills_child_and_prevents_late_marker() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_with(dir.path(), true);
        let command = if cfg!(windows) {
            "ping -n 6 127.0.0.1 > nul && echo late > marker.txt"
        } else {
            "sleep 5; echo late > marker.txt"
        };

        let started = Instant::now();
        let result = ExecShell
            .execute_with_timeout(
                json!({"command": command}),
                &ctx,
                Duration::from_millis(100),
            )
            .unwrap();
        let elapsed = started.elapsed();
        thread::sleep(Duration::from_millis(1_200));

        assert!(
            result.contains("command timed out after 100ms — killed"),
            "got: {result}"
        );
        assert!(
            elapsed < Duration::from_secs(2),
            "timeout waited for a descendant instead of killing its process tree: {elapsed:?}"
        );
        assert!(
            !dir.path().join("marker.txt").exists(),
            "timed-out command continued after its shell was killed"
        );
    }

    #[test]
    fn child_env_allowlist_is_case_insensitive_and_minimal() {
        assert!(is_allowed_env_var("PATH"));
        assert!(is_allowed_env_var("path"));
        assert!(is_allowed_env_var("CARGO_HOME"));
        assert!(!is_allowed_env_var("NH_DEEPSEEK_KEY"));
        assert!(!is_allowed_env_var("GITHUB_TOKEN"));
        assert!(!is_allowed_env_var("OPENAI_API_KEY"));
    }

    #[test]
    fn exec_child_never_sees_nh_key_env_fallback() {
        std::env::set_var("NH_EXECTEST_KEY", "sk-test-0000-exec");
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_with(dir.path(), true);
        let command = if cfg!(windows) {
            "echo [%NH_EXECTEST_KEY%]"
        } else {
            "echo [${NH_EXECTEST_KEY:-unset}]"
        };
        let result = ExecShell
            .execute(json!({"command": command}), &ctx)
            .unwrap();
        std::env::remove_var("NH_EXECTEST_KEY");
        assert!(
            !result.contains("sk-test-0000-exec"),
            "child must not inherit NH_*_KEY: {result}"
        );
    }

    #[test]
    fn exec_child_never_sees_ambient_github_token() {
        const SECRET: &str = "ambient-secret-must-not-pass";
        let previous = std::env::var_os("GITHUB_TOKEN");
        std::env::set_var("GITHUB_TOKEN", SECRET);
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_with(dir.path(), true);
        let command = if cfg!(windows) {
            "echo [%GITHUB_TOKEN%]"
        } else {
            "echo [${GITHUB_TOKEN:-unset}]"
        };
        let result = ExecShell
            .execute(json!({"command": command}), &ctx)
            .unwrap();
        match previous {
            Some(value) => std::env::set_var("GITHUB_TOKEN", value),
            None => std::env::remove_var("GITHUB_TOKEN"),
        }
        assert!(
            !result.contains(SECRET),
            "ambient credential leaked: {result}"
        );
    }

    #[test]
    fn over_cap_exec_result_is_bounded_with_digest_marker() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_with(dir.path(), true);
        let command = if cfg!(windows) {
            "for /L %i in (1,1,40000) do @echo x"
        } else {
            "yes x | head -n 40000"
        };
        let raw_chars = 80_000;
        let result = ExecShell
            .execute(json!({"command": command}), &ctx)
            .unwrap();
        assert!(result.contains("chars elided; digest "), "got: {result}");
        assert!(result.chars().count() < raw_chars);
        assert!(result.chars().count() <= MAX_TOOL_RESULT_CHARS + 100);
    }

    #[test]
    fn exec_stream_is_bounded_before_envelope_elision() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_with(dir.path(), true);
        let command = if cfg!(windows) {
            "powershell -NoProfile -Command \"[Console]::Out.Write('x' * 2200000)\""
        } else {
            "yes x | head -c 2200000"
        };

        let result = ExecShell
            .execute(json!({"command": command}), &ctx)
            .unwrap();

        assert!(result.contains("chars elided; digest "), "got: {result}");
        assert!(
            result.contains("stdout truncated at 2097152 bytes"),
            "got: {result}"
        );
        assert!(result.chars().count() <= MAX_TOOL_RESULT_CHARS + 100);
    }

    #[test]
    fn exec_reports_nonzero_exit_code() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_with(dir.path(), true);
        let result = ExecShell
            .execute(json!({"command": "exit 7"}), &ctx)
            .unwrap();
        assert!(result.contains("exit code: 7"), "got: {result}");
    }

    #[test]
    fn missing_argument_is_actionable() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_with(dir.path(), true);
        let err = ReadFile.execute(json!({}), &ctx).unwrap_err().to_string();
        assert_eq!(err, "missing required argument: path");
    }

    #[test]
    fn protected_edit_is_ok_shaped_and_leaves_file_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let protected = dir.path().join(".nosis").join("law.toml");
        std::fs::create_dir_all(protected.parent().unwrap()).unwrap();
        std::fs::write(&protected, "before").unwrap();
        let ctx = ToolCtx::new(dir.path().to_path_buf(), Box::new(|_| true)).with_guard(Box::new(
            |access| match access {
                Access::Write(path) if *path == ".nosis/law.toml" => {
                    Guard::Block("protected path (.nosis/**)".into())
                }
                _ => Guard::Allow,
            },
        ));

        let result = EditFile
            .execute(
                json!({"path": ".nosis/law.toml", "old_string": "before", "new_string": "after"}),
                &ctx,
            )
            .unwrap();

        assert_eq!(result, "blocked by law: protected path (.nosis/**)");
        assert_eq!(std::fs::read_to_string(protected).unwrap(), "before");
    }

    #[test]
    fn protected_read_is_blocked_before_io_and_normal_source_is_allowed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src").join("lib.rs"), "pub fn safe() {}").unwrap();
        let ctx = ToolCtx::new(dir.path().to_path_buf(), Box::new(|_| true)).with_guard(Box::new(
            |access| match access {
                Access::Read(".env") => Guard::Block("protected read (**/.env*)".into()),
                _ => Guard::Allow,
            },
        ));

        let blocked = ReadFile.execute(json!({"path": ".env"}), &ctx).unwrap();
        assert_eq!(blocked, "blocked by law: protected read (**/.env*)");
        let allowed = ReadFile
            .execute(json!({"path": "src/lib.rs"}), &ctx)
            .unwrap();
        assert_eq!(allowed, "pub fn safe() {}");
    }

    #[test]
    fn tool_result_redacts_key_shapes_before_egress() {
        let dir = tempfile::tempdir().unwrap();
        let fake = format!("ghp_{}", "A".repeat(36));
        std::fs::write(dir.path().join("output.txt"), &fake).unwrap();
        let result = ReadFile
            .execute(json!({"path": "output.txt"}), &ctx_with(dir.path(), true))
            .unwrap();
        assert_eq!(result, "[REDACTED]");
        assert!(!result.contains(&fake));
    }

    #[test]
    fn protected_missing_edit_is_blocked_before_file_check() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolCtx::new(dir.path().to_path_buf(), Box::new(|_| true)).with_guard(Box::new(
            |access| match access {
                Access::Write(".nosis/new.toml") => Guard::Block("protected path".into()),
                _ => Guard::Allow,
            },
        ));

        let result = EditFile
            .execute(
                json!({"path": ".nosis/new.toml", "old_string": "before", "new_string": "after"}),
                &ctx,
            )
            .unwrap();

        assert_eq!(result, "blocked by law: protected path");
    }

    #[test]
    fn edit_ask_uses_normalized_relative_path_and_denial_is_ok_shaped() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("nested")).unwrap();
        std::fs::write(dir.path().join("note.txt"), "before").unwrap();
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let approvals = Arc::clone(&seen);
        let ctx = ToolCtx::new(
            dir.path().to_path_buf(),
            Box::new(move |action| {
                approvals.lock().unwrap().push(action.to_string());
                false
            }),
        )
        .with_guard(Box::new(|access| match access {
            Access::Write("note.txt") => Guard::Ask,
            _ => Guard::Allow,
        }));

        let result = EditFile
            .execute(
                json!({"path": "nested/../note.txt", "old_string": "before", "new_string": "after"}),
                &ctx,
            )
            .unwrap();

        assert_eq!(result, "user denied: edit note.txt");
        assert_eq!(*seen.lock().unwrap(), ["edit note.txt"]);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("note.txt")).unwrap(),
            "before"
        );
    }

    #[test]
    fn default_context_allows_edit_and_routes_exec_through_approval() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("note.txt"), "before").unwrap();
        let approvals = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&approvals);
        let ctx = ToolCtx::new(
            dir.path().to_path_buf(),
            Box::new(move |_| {
                seen.fetch_add(1, Ordering::SeqCst);
                false
            }),
        );

        let edited = EditFile
            .execute(
                json!({"path": "note.txt", "old_string": "before", "new_string": "after"}),
                &ctx,
            )
            .unwrap();
        let denied = ExecShell
            .execute(json!({"command": "echo should-not-run"}), &ctx)
            .unwrap();

        assert_eq!(edited, "edited note.txt");
        assert_eq!(denied, "user denied: echo should-not-run");
        assert_eq!(approvals.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn blocked_exec_never_runs_or_asks() {
        let dir = tempfile::tempdir().unwrap();
        let approvals = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&approvals);
        let ctx = ToolCtx::new(
            dir.path().to_path_buf(),
            Box::new(move |_| {
                seen.fetch_add(1, Ordering::SeqCst);
                true
            }),
        )
        .with_guard(Box::new(|_| Guard::Block("blocked command".into())));

        let result = ExecShell
            .execute(json!({"command": "echo pwned > marker.txt"}), &ctx)
            .unwrap();

        assert_eq!(result, "blocked by law: blocked command");
        assert_eq!(approvals.load(Ordering::SeqCst), 0);
        assert!(!dir.path().join("marker.txt").exists());
    }
}
