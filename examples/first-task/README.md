# Your first small change

This disposable practice folder needs no programming tools. It contains a short
WELCOME.txt with one spelling mistake and EXPECTED.txt showing the intended result.
This is a guided exercise, not a benchmark or a real project.

Extract the package into a new folder **outside your existing projects and Git
repositories**. Initialization writes local configuration and, if a Git repository
is present, may install a pre-commit hook. Practicing outside one keeps that setup
away from your existing work.

In the extracted Windows package, open this folder in File Explorer, right-click
an empty area, then choose **Open in Terminal**. The commands below assume nh.exe
is one folder above this one. If you downloaded these files separately, use the
full path to your executable instead of `..\nh.exe`.

## 1. Try the free preview

Create the local practice configuration first. This does not contact a provider:

```powershell
..\nh.exe init
```

Then preview the task:

```powershell
..\nh.exe why 'Correct the spelling mistake in WELCOME.txt'
```

This shows a local route and price estimate. It does not read or edit the file,
contact a model or charge your provider account. You do not need an API key yet.

## 2. Choose a model

```powershell
..\nh.exe setup
```

Confirm that the displayed folder is this practice folder. Choose a model and
enter its API key only at the hidden prompt. If you want the next command to reuse
that model, accept the offer to save the selection. Decline the optional first
model task for now. Provider API access and credit are separate from a chat
subscription. Never paste your key into a task or save it in these files.

## 3. Make the change

```powershell
..\nh.exe tui
```

If you did not save a model, append `--model` followed by the route ID selected
in setup to the command above. Setup also prints a command for plain chat, which
uses different approval controls; the function-key instructions below apply to TUI.

Paste this message into Nosis, then press Enter. This step sends a model request
and may incur provider charges. Relevant file content can go to that provider.

```text
Read WELCOME.txt. Change only "smal" to "small" in that file. Do not change any other file and do not run shell commands. Explain what you changed.
```

File edits follow your policy and may occur without another approval. If a shell
request appears, press F4 to decline it; this exercise needs none. F1 opens help.
Esc stops active work but does not undo an edit already made. On some laptops,
hold Fn to send a function key. Ask for help if a shortcut does not work as shown.

## 4. Check it yourself

In rc.3 source, type `/review` to read the latest change details; PageUp/PageDown
scroll and Esc closes the view when idle. Older versions may not offer it. A model
saying it finished does not prove the file is correct. Type `/quit`, then inspect:

```powershell
Get-Content -LiteralPath WELCOME.txt
```

You should see: `This is a small practice project.` Compare all lines independently:

```powershell
Compare-Object -CaseSensitive -SyncWindow 0 (Get-Content -LiteralPath EXPECTED.txt) (Get-Content -LiteralPath WELCOME.txt)
```

No output means those text lines match in case and order. It does not check file
encoding, line-ending bytes or other files, or prove broader correctness.
Confirm that README.md and EXPECTED.txt were not changed.
If there is a difference, inspect it before asking Nosis to correct it.

## If something goes wrong

For a missing key, return to setup and use its hidden key prompt. For rejected
credentials or quota, check API access/credit with your selected provider. For a
timeout or interruption, inspect WELCOME.txt before trying again: a completed edit
can remain even when no final answer arrived. A retry may incur another charge.
Do not change antivirus settings to complete this exercise.

To repeat, extract a fresh copy into a new folder. Keep any results you want to
compare. When finished, you can remove this disposable practice folder; OS-vault
credentials are stored separately. Your own projects were not part of the exercise.
