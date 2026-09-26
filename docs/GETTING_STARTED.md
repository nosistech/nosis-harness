# Your first task with Nosis

Install using the [Windows quickstart](WINDOWS_QUICKSTART.md), then return here.
Follow the four steps below with the published v0.2.2 download. Differences in
unreleased builds are collected at the end.

You need a provider API key for an AI answer. This is a private access code from
the company supplying your model; a chat-app subscription may not include API
access or credit. You can explore setup without one. The no-key price preview
does not contact a model or produce an AI answer.

## 1. Open your project

In File Explorer, open the project folder and choose **Open in Terminal**.
For your first attempt, use a practice folder without sensitive files. Add a short
`README.md` describing a project you know; the first question below will read it.

```powershell
nh setup
```

If that command is not found, use the executable's actual path, for example:

```powershell
& 'C:\Users\you\Apps\Nosis\nh.exe' setup
```

Run setup in an interactive terminal. Piped answers cannot authorize setup.

## 2. Follow setup

Confirm the displayed folder. Choose a provider you have API access to and a model
you want to try. Nosis does not silently select another provider for you.

If you already have provider API access, use that provider for your first task.
This avoids creating another account just to try Nosis. Model prices are rates,
not a task's total price: `1M` means one million tokens, or pieces of text. Prices
can change, and a cheaper rate does not guarantee a cheaper result.

You can inspect a price preview without a key. It is an estimate, not an AI answer.
To connect, obtain an API key from the provider's own API dashboard and enter it
only at Nosis's hidden key prompt. It is stored in the OS credential store.
Existing keys are retained. Never paste a key into a task or shell command.

Use your provider's official instructions to create the key, then return to Nosis:
[DeepSeek](https://api-docs.deepseek.com/),
[Kimi](https://platform.kimi.ai/docs/overview),
[MiMo](https://mimo.mi.com/docs/), or
[GLM / Z.AI](https://docs.z.ai/guides/overview/quick-start).
You do not need to run their programming examples to use Nosis.

Press Enter to decline a yes/no question. Type `cancel` to stop. Changes you already
confirmed remain; existing configuration and Git hooks are preserved, while missing
ignore entries may be added.

## 3. Complete one small task

In published v0.2.2, setup can open chat. Ask a narrow question such as:

```text
Read README.md and explain this project's purpose in three sentences. Do not edit files or run commands.
```

Choose a file that exists in your practice project. This instruction alone does
not remove tools in v0.2.2; file edits still follow policy. Decline unexpected shell
commands. Task text and file content can be sent to the provider and incur charges.

**Done when:** the answer identifies the actual project and agrees with the file.
Check that yourself; the model can be wrong. For an editing task with executable
checks, continue with the [practice project](../examples/practice-tasks/README.md).
Its files are separate from the portable download; follow its file-download steps.

## 4. Return later

Use the command setup prints for your selected model. In chat, `/quit` exits.
Run `nh doctor` if something stops working; it reports configuration without
printing key values. It does not test whether your provider account accepts calls.

| Problem | Next action |
| --- | --- |
| `nh` is not found | Use the full executable path or reopen the terminal after changing user PATH |
| Credential store unavailable | Run `nh doctor` and resolve the credential-store error. Do not save your key in a file as a workaround |
| No API key | Rerun setup and use the hidden key prompt |
| Provider rejects the request | Check that provider's API access and credit; `doctor` only confirms stored configuration |
| Provider times out | Inspect files before retrying; earlier actions may have completed |
| Catalog is untrusted | Follow [catalog upgrade guidance](CONFIGURATION.md); do not grant trust just to dismiss the error |

If the saved API key is wrong or expired, replace it at the hidden prompt with
`nh key add <entry>`, using the credential entry shown by setup or doctor. This
updates that entry; you do not need to delete it first. Rerunning setup preserves
an existing key, so it will not repair an invalid key by itself.

Plain chat accepts one line per message. For pasting or editing a multiline task,
use `nh tui --model <route-id>` and see the [terminal controls](TERMINAL_GUIDE.md).
Use the model ID printed by setup. In the current candidate/source composer,
pasting does not send the task; review it and press Enter when ready.

## Additional controls

**Using a candidate or source build?** Current unreleased setup groups models by
provider, shows catalog input/output prices and includes **All models**. It also
offers a read-only first task. When `nh run --help` lists `--read-only`, use
[enforced read-only tasks](READ_ONLY.md) to remove editing and command tools for
that task. Those protections are not in the published v0.2.2 download.

[Remember a model or upgrade a catalog](CONFIGURATION.md) in candidate/source builds.
For v0.2.2 catalog replacement, retain a backup outside the project before intentionally
removing the old `catalog.toml` and rerunning setup; review provider destinations first.
For local inference, follow [local models](LOCAL_MODELS.md).
For keyboard help and change review, see [terminal controls](TERMINAL_GUIDE.md).
