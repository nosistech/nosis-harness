//! Approved shell execution with bounded capture, timeout, and process-tree termination.

use crate::{
    is_allowed_env_var, render_tool_result, str_arg, Access, ExecShell, Guard, Tool, ToolCtx,
    ToolSpec, DRAIN_GRACE, EXEC_TIMEOUT, KILL_VERIFY_GRACE, MAX_TOOL_READ_BYTES, TOOL_BUFFER_BYTES,
};
use anyhow::Context as _;
use serde_json::json;
use std::io::Read;
use std::process::{Child, ChildStderr, ChildStdout, ExitStatus, Stdio};
use std::sync::{atomic::Ordering, mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::process::CommandExt as _;
#[cfg(windows)]
use std::os::windows::process::CommandExt as _;

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

#[derive(Clone)]
pub(super) struct BoundedOutput {
    pub(super) bytes: Vec<u8>,
    pub(super) truncated: bool,
}

pub(super) enum DrainCompletion {
    Complete,
    Failed(String),
    Incomplete(Duration),
    Panicked,
}

pub(super) struct DrainOutcome {
    pub(super) output: BoundedOutput,
    pub(super) completion: DrainCompletion,
}

pub(super) struct DrainHandle {
    pub(super) shared: Arc<Mutex<BoundedOutput>>,
    pub(super) done: mpsc::Receiver<std::io::Result<()>>,
}

impl DrainHandle {
    pub(super) fn finish(self, deadline: Instant, grace: Duration) -> DrainOutcome {
        let completion = match self
            .done
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        {
            Ok(Ok(())) => DrainCompletion::Complete,
            Ok(Err(error)) => DrainCompletion::Failed(error.to_string()),
            Err(mpsc::RecvTimeoutError::Timeout) => DrainCompletion::Incomplete(grace),
            Err(mpsc::RecvTimeoutError::Disconnected) => DrainCompletion::Panicked,
        };
        let output = match self.shared.lock() {
            Ok(output) => output.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        DrainOutcome { output, completion }
    }
}

pub(super) fn drain_bounded<R: Read>(
    mut reader: R,
    shared: &Mutex<BoundedOutput>,
) -> std::io::Result<()> {
    let mut chunk = [0u8; TOOL_BUFFER_BYTES];
    loop {
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        let mut output = shared
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let remaining = MAX_TOOL_READ_BYTES.saturating_sub(output.bytes.len());
        let keep = remaining.min(read);
        output.bytes.extend_from_slice(&chunk[..keep]);
        output.truncated |= keep < read;
    }
    Ok(())
}

pub(super) fn spawn_drain<R: Read + Send + 'static>(reader: R) -> DrainHandle {
    let shared = Arc::new(Mutex::new(BoundedOutput {
        bytes: Vec::with_capacity(TOOL_BUFFER_BYTES),
        truncated: false,
    }));
    let thread_shared = Arc::clone(&shared);
    let (done_tx, done) = mpsc::channel();
    let _drain_thread = thread::spawn(move || {
        let result = drain_bounded(reader, &thread_shared);
        let _ = done_tx.send(result);
    });
    DrainHandle { shared, done }
}

pub(super) fn render_bounded_output(outcome: DrainOutcome, stream: &str) -> String {
    let DrainOutcome { output, completion } = outcome;
    let mut rendered = String::from_utf8_lossy(&output.bytes).into_owned();
    if output.truncated {
        rendered.push_str(&format!(
            "\n…[{stream} truncated at {MAX_TOOL_READ_BYTES} bytes]"
        ));
    }
    match completion {
        DrainCompletion::Complete => {}
        DrainCompletion::Failed(error) => {
            rendered.push_str(&format!("\n…[{stream} capture failed: {error}]"));
        }
        DrainCompletion::Incomplete(grace) => {
            rendered.push_str(&format!(
                "\n…[{stream} capture incomplete after {} - a surviving child process may still hold the pipe]",
                timeout_label(grace)
            ));
        }
        DrainCompletion::Panicked => {
            rendered.push_str(&format!(
                "\n…[{stream} capture incomplete - drain thread panicked]"
            ));
        }
    }
    rendered
}

pub(super) fn timeout_label(timeout: Duration) -> String {
    if timeout.subsec_nanos() == 0 {
        format!("{}s", timeout.as_secs())
    } else {
        format!("{}ms", timeout.as_millis())
    }
}

#[derive(Clone, Copy)]
pub(super) enum TerminationReason {
    Timeout(Duration),
    Cancelled,
}

pub(super) enum Termination {
    Reaped {
        _status: ExitStatus,
        reason: TerminationReason,
    },
    Survived {
        detail: String,
        reason: TerminationReason,
    },
}

pub(super) fn poll_child_reaped(
    child: &mut Child,
    command: &str,
    deadline: Instant,
) -> anyhow::Result<Option<ExitStatus>> {
    loop {
        if let Some(status) = child
            .try_wait()
            .with_context(|| format!("could not reap terminated command: {command}"))?
        {
            return Ok(Some(status));
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(None);
        }
        thread::sleep(remaining.min(Duration::from_millis(50)));
    }
}

pub(super) fn terminate_child_tree(
    child: &mut Child,
    command: &str,
    reason: TerminationReason,
) -> anyhow::Result<Termination> {
    #[cfg(windows)]
    let tree_kill_status = std::process::Command::new("taskkill")
        .args(["/PID", &child.id().to_string(), "/T", "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    #[cfg(unix)]
    let tree_kill_status = std::process::Command::new("kill")
        .args(["-KILL", &format!("-{}", child.id())])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let tree_kill_succeeded = matches!(&tree_kill_status, Ok(status) if status.success());
    let tree_kill_detail = match &tree_kill_status {
        Ok(status) if status.success() => format!("tree-kill exit status: {status}"),
        Ok(status) => format!("tree-kill non-success exit status: {status}"),
        Err(error) => format!("tree-kill failed to start: {error}"),
    };

    let verification_started = Instant::now();
    let verification_deadline = verification_started + KILL_VERIFY_GRACE;
    let tree_kill_deadline = if tree_kill_succeeded {
        verification_started + KILL_VERIFY_GRACE / 2
    } else {
        verification_started
    };
    if let Some(status) = poll_child_reaped(child, command, tree_kill_deadline)? {
        return Ok(Termination::Reaped {
            _status: status,
            reason,
        });
    }
    child
        .kill()
        .with_context(|| format!("could not kill terminated command: {command}"))?;
    if let Some(status) = poll_child_reaped(child, command, verification_deadline)? {
        return Ok(Termination::Reaped {
            _status: status,
            reason,
        });
    }
    Ok(Termination::Survived {
        detail: format!(
            "{tree_kill_detail}; child did not reap within {} across tree-kill verification and the direct-kill fallback",
            timeout_label(KILL_VERIFY_GRACE)
        ),
        reason,
    })
}

impl ExecShell {
    pub(super) fn execute_with_timeout(
        &self,
        args: serde_json::Value,
        ctx: &ToolCtx,
        timeout: Duration,
    ) -> anyhow::Result<String> {
        self.execute_with_deadlines(args, ctx, timeout, DRAIN_GRACE)
    }

    pub(super) fn execute_with_deadlines(
        &self,
        args: serde_json::Value,
        ctx: &ToolCtx,
        timeout: Duration,
        drain_grace: Duration,
    ) -> anyhow::Result<String> {
        let command = str_arg(&args, "command")?;
        if let Guard::Block(reason) = (ctx.guard)(&Access::Exec(command)) {
            return Ok(render_tool_result(format!("blocked by law: {reason}"), ctx));
        }
        // SECURITY INVARIANT: this boundary requires explicit approval for every non-blocked exec,
        // regardless of which non-Block verdict the guard returned.
        if !(ctx.approve)(command) {
            // Ok-shaped so the model can read the denial and adapt, not crash the turn.
            return Ok(render_tool_result(format!("user denied: {command}"), ctx));
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
        // SECURITY INVARIANT: approved commands get only the minimum environment required
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
        let stdout: ChildStdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("command stdout pipe was not created"))?;
        let stderr: ChildStderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow::anyhow!("command stderr pipe was not created"))?;
        let stdout_drain = spawn_drain(stdout);
        let stderr_drain = spawn_drain(stderr);

        let deadline = Instant::now() + timeout;
        let (status, termination) = loop {
            if let Some(status) = child
                .try_wait()
                .with_context(|| format!("could not wait for command: {command}"))?
            {
                break (Some(status), None);
            }
            if ctx.cancel.load(Ordering::Acquire) {
                break (
                    None,
                    Some(terminate_child_tree(
                        &mut child,
                        command,
                        TerminationReason::Cancelled,
                    )?),
                );
            }
            if Instant::now() >= deadline {
                break (
                    None,
                    Some(terminate_child_tree(
                        &mut child,
                        command,
                        TerminationReason::Timeout(timeout),
                    )?),
                );
            }
            thread::sleep(Duration::from_millis(50));
        };

        let drain_deadline = Instant::now() + drain_grace;
        let stdout =
            render_bounded_output(stdout_drain.finish(drain_deadline, drain_grace), "stdout");
        let stderr =
            render_bounded_output(stderr_drain.finish(drain_deadline, drain_grace), "stderr");
        let content = match termination {
            Some(Termination::Reaped { reason, .. }) => match reason {
                TerminationReason::Timeout(timeout) => format!(
                    "command timed out after {} - killed\nstdout:\n{stdout}\nstderr:\n{stderr}",
                    timeout_label(timeout)
                ),
                TerminationReason::Cancelled => {
                    format!("command cancelled - killed\nstdout:\n{stdout}\nstderr:\n{stderr}")
                }
            },
            Some(Termination::Survived { detail, reason }) => match reason {
                TerminationReason::Timeout(timeout) => format!(
                    "command timed out after {} - could NOT be killed: {detail}\nstdout:\n{stdout}\nstderr:\n{stderr}",
                    timeout_label(timeout)
                ),
                TerminationReason::Cancelled => format!(
                    "command cancelled - could NOT be killed: {detail}\nstdout:\n{stdout}\nstderr:\n{stderr}"
                ),
            },
            None => {
                let status = status
                    .ok_or_else(|| anyhow::anyhow!("command completed without an exit status"))?;
                let code = status
                    .code()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "killed by signal".into());
                format!("exit code: {code}\nstdout:\n{stdout}\nstderr:\n{stderr}")
            }
        };
        Ok(render_tool_result(content, ctx))
    }
}
