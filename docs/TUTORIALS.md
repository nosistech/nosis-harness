# Three small tasks to learn Nosis

Start with the [Windows quickstart](WINDOWS_QUICKSTART.md). These examples assume
`nh` is on your user PATH; otherwise use the full path to `nh.exe`. Run PowerShell
commands in your terminal, and task messages inside Nosis when instructed.

Examples 1 and 2 work with v0.2.2. Commands marked **v0.3.0-rc.1 candidate** require that source build; they are not in the downloadable v0.2.2 package.
Output panels illustrate what to look for, not a recorded model response or exact price.

For a small bug fix with executable acceptance checks, use the
[practice project](../examples/practice-tasks/README.md). The unreleased source build
also supports [read-only tasks](READ_ONLY.md) for asking about files without giving
the model editing or command-execution tools.

## 1. Try a preview without a key or a charge

Create a new practice folder outside your existing Git repositories, somewhere you
can write (for example, in Documents). Pick a different name if it
already exists; do not use a folder containing work you want to keep unchanged.

```powershell
New-Item -ItemType Directory nh-practice
Set-Location nh-practice
nh init
nh doctor
nh why "Explain the purpose of a README" --model deepseek-v4-flash
```

`init` creates project configuration and may install a Git pre-commit hook when
Git is present. `doctor` reports setup issues; its exit code is not a readiness
guarantee. `why` reads the local catalog and estimates tokens and cost. It does
not contact a model, answer the question or inspect a repository for you.

```text
You type a task
      |
      v
Local catalog + estimated token counts
      |
      v
Route and price explanation       No model request, no API key
```

**Done when:** you can find the selected route and estimate in the explanation.
Prices may be stale or incomplete, and a real multi-turn task can cost more.
For `nh not found`, use the full executable path or reopen the terminal after
changing PATH. For an untrusted catalog, see example 3; do not blindly trust it.

## 2. Make one small change and inspect it

Use the practice folder from example 1. This example also requires Git, a chosen
provider API key, and API credit. A chat subscription may not provide API credit.
The task text and any files the model reads can be sent to that provider.

Create a harmless file and save a comparison point in Git's index:

```powershell
git init
Set-Content -LiteralPath README.md -Value '# Practice project','This is a smal example.'
git add README.md
nh setup
```

In setup, confirm the displayed folder, choose a model, and enter a key only at
the hidden key prompt. Start chat only when you are ready for a paid request.
Then paste this single-line message at the Nosis prompt:

```text
In README.md, change "smal" to "small". Make no other edits. Afterward, ask to run git diff --check and explain the change.
```

Read any proposed command before approving it. Shell approval defaults to **no**.
If the command differs from the requested check, decline and ask why it is needed.
File tools follow your policy; do not assume every file edit has its own approval
screen. A prompt limiting changes is an instruction, not an enforced read-only mode.

When the task finishes, type `/quit` in chat. Back in PowerShell:

```powershell
git status --short
git diff -- README.md
git diff --check
```

Expected change illustration:

```diff
-This is a smal example.
+This is a small example.
```

**Done when:** the intended word is corrected, `git status` shows no unexpected
file changes, and the final check reports no whitespace errors. Inspect extra
changes before accepting them. Nosis does not automatically undo a completed edit.
`git diff` does not display untracked files, so always inspect `git status` too.

If the key is rejected, check API access and credit with the provider. `nh doctor`
checks local configuration, not whether the provider accepts requests. Replace an
invalid vault entry using `nh key add <entry>`. Never paste a key into the conversation.
If the provider times out, inspect the files before repeating the task: cancellation
does not undo earlier tools, and an in-flight request may still incur charges.

## 3. Return to work and upgrade without losing configuration

Return to the same project folder. For v0.2.2, explicitly select the same model:

```powershell
nh chat --model deepseek-v4-flash
```

**v0.3.0-rc.1 candidate: remember a model by choice.** Setup can offer to save your selection;
declining keeps it session-only. You can also use:

```powershell
nh model set deepseek-v4-flash
nh model show
nh chat
nh model clear
```

The preference lives in your user configuration, applies across projects, and
contains a route ID only. Explicit `--model` takes precedence. It grants no new
permissions and stores no API key. A missing saved route stops with recovery
instructions instead of silently choosing another provider.

To find an interrupted session, run:

```powershell
nh resume
```

Copy a listed session ID into `nh resume <session-id>` without the angle brackets.
If it says there are no interrupted sessions, start a new chat. A cleanly exited
session is not listed as interrupted. Inspect project files before continuing a
task interrupted during an edit or command. Resume is not file rollback.
After upgrading from v0.2.2, older TUI sessions without saved budget metadata are
refused. Start a new `nh tui --model <route-id> --budget <tokens>` session with an
explicit limit, or omit `--budget` deliberately. Keep the old transcript for review.

For a portable upgrade, close Nosis, download and verify the newer executable,
then replace the executable in its program folder. Keep your project files and
user configuration. Run `nh --version` and `nh doctor` to confirm the copy in use.

**v0.3.0-rc.1 candidate: migrate a recognized older bundled catalog.** From the project folder:

```powershell
nh catalog migrate
```

Read the route/destination changes and backup path before answering yes. Enter
declines. Customized or unrecognized catalogs are refused and remain unchanged;
review those manually. The migration keeps a backup, but is not crash-atomic.
If interrupted, follow its backup recovery message before running another task.
If a previous attempt left a backup, review and move that backup to a different
safe name before retrying; migration will not overwrite it. Correct the reported
permissions or filesystem problem first.
See [guided setup](GETTING_STARTED.md) for the v0.2.2 manual recovery path.

```text
Verify new download -> Replace executable -> Check version
                                              |
                                              v
                                   Review catalog changes
                                              |
                                  Consent -> Backup -> Migrate
```

The WinGet submission is still pending. Use the portable path until the listing
is approved and installation is verified. Removing the executable leaves project
history and OS-vault credentials in place; see [privacy and removal](../PRIVACY.md).

For terminal controls, see [terminal guide](TERMINAL_GUIDE.md). For screenshots as
model input, see [image input](IMAGES.md). For local inference, see
[local models](LOCAL_MODELS.md).
