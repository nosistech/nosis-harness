# Ask about a project before allowing changes

**Unreleased source feature.** This flag is not in the v0.2.2 download or the existing
v0.3.0-rc.1 draft. Check `nh run --help` for your installed version.

Use a read-only task to understand a project, find likely mistakes, or ask for a plan.
The model can read and search permitted files. It cannot edit files, run commands,
install software or invoke remote tools. These restrictions are enforced by the
available tools and their guard, not just by words in the prompt.

After setup, run this from your project folder:

```powershell
nh run "Explain this project. Identify the main files and how they fit together." --read-only --model deepseek-v4-flash
```

Replace the model with your chosen route. If you deliberately saved a default model,
you can omit `--model`. Existing file-access policy still applies.

For a short question, you can add `--profile frugal`. The bundled frugal profile
requests the route's lowest supported reasoning setting while keeping the same
model. Less reasoning can reduce answer quality; compare results on your task.
Use `nh profile --model <route-id>` to inspect the effective settings first.

For a focused review:

```powershell
nh run "Read the delivery calculation and identify incorrect boundary cases. Suggest a small fix." --read-only --model deepseek-v4-flash
```

This is analysis, not test execution: commands are unavailable in this mode. Ask for
file references and verify important claims yourself. A model can still give an
incorrect answer. Try the [practice tasks](../examples/practice-tasks/README.md) for
an example with independent checks.

The source build's final notice explicitly says commands and tests did not run in
read-only mode. An explanation of what a test might show is not a test result.

## What read-only does and does not mean

- The agent's tools cannot modify project files or execute shell commands.
- Normal local receipt files are still written. This is not a process that writes
  nothing to disk, and it is not an operating-system sandbox.
- Opt-in [efficiency experiments](EFFICIENCY.md) can also write local measurement
  records or temporary scrubbed observations. They do not grant editing or shell tools.
- Task text and permitted file content go to the selected provider. API charges
  still apply. Read-only does not mean offline, private to your device, or free.
- The flag applies to this `nh run` task only. It does not change chat, TUI, fleet,
  future tasks, or your saved policy.

## When you want an edit

Start a separate ordinary task without `--read-only`, with a precise description
of the intended change. Read any requested shell command before approving it.
Review the resulting diff and run your project's checks. File edits follow your
existing policy; not every edit requires a separate approval.

Nosis does not automatically undo completed edits. Keep a backup or a Git checkpoint
before asking any coding assistant to change work you need to preserve.
