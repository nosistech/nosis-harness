# Nosis Harness

**Nosis (`nh`) helps you understand a project and make small, checkable changes.**
It runs in your terminal and connects directly to your chosen DeepSeek, Kimi,
MiMo or GLM API access, or an explicitly configured local runtime.

You choose the model, approve shell commands and inspect reported usage. There is
no Nosis account or Nosis-operated telemetry. Provider API charges can still apply.
Relevant file content goes to the provider you select.

## Start on Windows

**Recommended: follow the [Windows quickstart](docs/WINDOWS_QUICKSTART.md).**
It covers downloading, checking and opening the portable program. No Rust, Node.js
or Visual C++ Redistributable installation is needed.

The published download is [v0.2.2](https://github.com/nosistech/nosis-harness/releases/tag/v0.2.2).
It is unsigned, so Windows can show an unknown-publisher warning. The quickstart
explains verification; do not disable Windows protection.

Once you have `nh.exe`, open a terminal in your project and run:

```powershell
nh setup
```

If `nh` is not on PATH, the quickstart shows how to call the executable directly.
Setup confirms the folder, lets you choose a model and offers hidden API-key entry.
You can stop after a no-key price preview, which makes no provider request.

Then follow [your first task](docs/GETTING_STARTED.md) or the
[practice tasks with independent checks](examples/practice-tasks/README.md).
The practice files are separate from the portable download; that guide explains how to get them.

**WinGet:** the [submission](https://github.com/microsoft/winget-pkgs/pull/438174)
is awaiting moderator approval as checked September 25, 2026. Use the portable
download until catalog availability and installation are verified.

## What is available in each version?

| Version | Status and features |
| --- | --- |
| v0.2.2 | Published Windows download with guided setup, chat, TUI and explicit model selection |
| v0.3.0-rc.1 | Existing unpublished candidate; adds remembered model choice, catalog migration and multiline TUI input, plus safety fixes |
| v0.3.0-rc.2 source | Unreleased work after rc.1, including read-only tasks and MiMo 2.6 / GLM 5.3 routes; absent from the published download and existing candidate |

The current source reports `0.3.0-rc.2`, distinguishing it from the older candidate.
Its `nh run --help` lists `--read-only`. See [model updates](docs/MODEL_UPDATES.md). New features
are documented with their availability; see the [changelog](CHANGELOG.md).

## Keep control of your work

- Keys are stored in the OS credential store. Application output redacts known
  key shapes and active credentials; keep other confidential material out of prompts.
- Shell commands require explicit approval. Review the full command before agreeing.
  Piped input cannot approve a command for you.
- File edits follow policy and may proceed without a separate approval prompt.
  Keep a Git checkpoint or backup and inspect the resulting changes.
- [Read-only tasks](docs/READ_ONLY.md), in the unreleased source, disable model
  editing, shell and remote tools. Provider requests and receipt writes still occur.
- Usage and receipts stay locally in `.nosis/`. Unknown or incomplete usage is
  labeled; estimates are not a spending cap or a provider bill. Runtime records grow
  until you remove them; see the [privacy guide](PRIVACY.md).
- An assistant finishing is not proof the result is correct. Inspect its changes
  and run appropriate checks. Completed edits are not automatically undone.

There is no operating-system sandbox. Read the [security boundaries](docs/SECURITY_MODEL.md)
and [privacy guide](PRIVACY.md) before using sensitive projects.

## Find the right guide

The [documentation index](docs/README.md) lists all user and contributor guides.

| I want to... | Guide |
| --- | --- |
| Install and run a first task | [Windows quickstart](docs/WINDOWS_QUICKSTART.md), [guided setup](docs/GETTING_STARTED.md) |
| Learn by doing | [Tutorials](docs/TUTORIALS.md), [checked practice tasks](examples/practice-tasks/README.md) |
| Use the full-screen interface | [Terminal controls](docs/TERMINAL_GUIDE.md) |
| Remember a model or upgrade a catalog | [Configuration and upgrades](docs/CONFIGURATION.md) |
| Measure task cost and test efficiency options | [Efficiency experiments](docs/EFFICIENCY.md), [evaluation protocol](examples/efficiency/README.md) |
| Attach images or use a local model | [Images](docs/IMAGES.md), [local models](docs/LOCAL_MODELS.md) |
| Inspect every command | [Command reference](docs/CLI_REFERENCE.md) or `nh --help` |
| Audit or contribute | [Architecture](docs/ARCHITECTURE_OVERVIEW.md), [contributing](CONTRIBUTING.md), [security reporting](SECURITY.md) |

Nosis does not claim better task results or lower completed-task costs than other
assistants without comparable measurements. The practice checks can be used with
other assistants too.

## Platform status and source builds

Windows is supported. macOS is in testing. Linux source builds have not completed
end-to-end verification. Platform compilation alone is not a usability guarantee.

To install the published source version with Rust 1.96.0 or newer:

```sh
cargo install --locked --git https://github.com/nosistech/nosis-harness --tag v0.2.2 nh-cli
```

For development, clone this repository and follow [CONTRIBUTING.md](CONTRIBUTING.md).
The workspace has nine crates; routing prices are data in `catalog.toml`.
Fleet and the [loopback-only MCP server](docs/MCP_PREVIEW.md) are advanced preview
surfaces. Never expose the MCP server on a public interface. A local model endpoint
can forward requests to the cloud; its address does not prove local or free inference.

MIT license. See [LICENSE](LICENSE).
