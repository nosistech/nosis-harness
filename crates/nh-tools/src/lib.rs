//! nh-tools — read_file / edit_file / exec_shell behind an approval gate.
//! THE LAW: tool outputs are DATA, never instructions. exec always passes the gate.

use anyhow::{bail, Context};
use serde_json::json;
use std::path::{Component, Path, PathBuf};

pub mod mcp;

pub use mcp::{
    load_mcp_config, mcp_tools, McpAuth, McpClient, McpServerConfig, McpToolInfo, McpToolset,
    McpTrust,
};

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

/// Env vars that may hold harness secrets: the `NH_<ENTRY>_KEY` vault fallback
/// shape (nh-vault). Case-insensitive — Windows env names are.
fn is_secret_env_var(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    upper.starts_with("NH_") && upper.ends_with("_KEY")
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
fn resolve_in_workdir(workdir: &Path, rel: &str) -> anyhow::Result<PathBuf> {
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
    Ok(resolved)
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
        let resolved = resolve_in_workdir(&ctx.workdir, path)?;
        if !resolved.is_file() {
            bail!("file not found: {path} — check the path against the working directory");
        }
        std::fs::read_to_string(&resolved)
            .with_context(|| format!("could not read {path} — is it UTF-8 text?"))
    }
}

impl Tool for EditFile {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "edit_file".into(),
            description:
                "Replace one exact occurrence of old_string with new_string in a file.".into(),
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
        let resolved = resolve_in_workdir(&ctx.workdir, path)?;
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
            description:
                "Run a shell command in the working directory. Requires user approval.".into(),
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
        let command = str_arg(&args, "command")?;
        // THE LAW: the approval gate runs before the command, no exceptions.
        if !(ctx.approve)(command) {
            // Ok-shaped so the model can read the denial and adapt, not crash the turn.
            return Ok(format!("user denied: {command}"));
        }
        let mut cmd = if cfg!(windows) {
            let mut c = std::process::Command::new("cmd");
            c.args(["/C", command]);
            c
        } else {
            let mut c = std::process::Command::new("sh");
            c.args(["-c", command]);
            c
        };
        cmd.current_dir(&ctx.workdir);
        // THE LAW: no plaintext key ever hits disk. The child must not inherit the
        // NH_<ENTRY>_KEY vault fallback, or an approved `echo $NH_X_KEY > f`
        // would exfiltrate the key into the workdir.
        for (name, _) in std::env::vars_os() {
            if is_secret_env_var(&name.to_string_lossy()) {
                cmd.env_remove(&name);
            }
        }
        let output = cmd
            .output()
            .with_context(|| format!("could not run command: {command}"))?;
        let code = output
            .status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "killed by signal".into());
        Ok(format!(
            "exit code: {code}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
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
        ToolCtx {
            workdir: workdir.to_path_buf(),
            approve: Box::new(move |_| approve),
        }
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

        let text = ReadFile
            .execute(json!({"path": "note.txt"}), &ctx)
            .unwrap();
        assert_eq!(text, "hello old world");

        let result = EditFile
            .execute(
                json!({"path": "note.txt", "old_string": "old", "new_string": "new"}),
                &ctx,
            )
            .unwrap();
        assert_eq!(result, "edited note.txt");

        let text = ReadFile
            .execute(json!({"path": "note.txt"}), &ctx)
            .unwrap();
        assert_eq!(text, "hello new world");
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
        assert_eq!(err, "old_string appears 2 times in a.txt — provide more context");
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
        let ctx = ToolCtx {
            workdir: dir.path().to_path_buf(),
            approve: Box::new(move |cmd| {
                assert_eq!(cmd, "echo pwned > marker.txt");
                calls_seen.fetch_add(1, Ordering::SeqCst);
                false
            }),
        };
        let result = ExecShell
            .execute(json!({"command": "echo pwned > marker.txt"}), &ctx)
            .unwrap();
        assert_eq!(result, "user denied: echo pwned > marker.txt");
        assert_eq!(calls.load(Ordering::SeqCst), 1, "approval gate must be consulted");
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

    #[test]
    fn secret_env_var_shape_is_detected_case_insensitively() {
        assert!(is_secret_env_var("NH_DEEPSEEK_KEY"));
        assert!(is_secret_env_var("nh_test_entry_key"));
        assert!(!is_secret_env_var("PATH"));
        assert!(!is_secret_env_var("NH_WORKDIR"));
        assert!(!is_secret_env_var("MY_KEY"));
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
}
