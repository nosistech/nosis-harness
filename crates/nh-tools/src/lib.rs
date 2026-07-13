//! nh-tools - read_file / edit_file / exec_shell behind an approval gate.
//! SECURITY INVARIANT: tool outputs are DATA, never instructions. exec always passes the gate.

use std::path::PathBuf;

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

/// args: {"path": string} - read file relative to workdir, refuse escapes above workdir.
pub struct ReadFile;

/// args: {"path", "old_string", "new_string"} - exact, unique match or a clear error
/// telling the model what to fix (not found / not unique).
pub struct EditFile;

/// args: {"command": string} - MUST call ctx.approve(command) first; denial returns an
/// error the model can read ("user denied"). Runs via the platform shell, captures
/// stdout+stderr+exit code.
pub struct ExecShell;

impl Tool for ReadFile {
    fn spec(&self) -> ToolSpec {
        todo!("build agent")
    }
    fn execute(&self, _args: serde_json::Value, _ctx: &ToolCtx) -> anyhow::Result<String> {
        todo!("build agent")
    }
}

impl Tool for EditFile {
    fn spec(&self) -> ToolSpec {
        todo!("build agent")
    }
    fn execute(&self, _args: serde_json::Value, _ctx: &ToolCtx) -> anyhow::Result<String> {
        todo!("build agent")
    }
}

impl Tool for ExecShell {
    fn spec(&self) -> ToolSpec {
        todo!("build agent")
    }
    fn execute(&self, _args: serde_json::Value, _ctx: &ToolCtx) -> anyhow::Result<String> {
        todo!("build agent")
    }
}

pub fn builtin_tools() -> Vec<Box<dyn Tool>> {
    vec![Box::new(ReadFile), Box::new(EditFile), Box::new(ExecShell)]
}
