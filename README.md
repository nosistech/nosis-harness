# Nosis Harness — Project OS

**nh** is an honest, metered, multi-model terminal agent (Rust) for open-weight models — DeepSeek V4, Kimi K2.x, MiMo V2.5, GLM, and explicitly configured local runtimes. You select the execution route explicitly; `nh why` independently estimates the **cheapest capable API** catalog route. Every accepted run produces a local receipt with reported token usage and price-derived cost context. It is a harness with a meter, not an automatic router.

**Canonical spec:** `NOSIS_HARNESS_Master_Plan.md` (root). Appendices A/B supersede Sections 1 and 3 where they conflict. The folders below are the working layer on top of it.

## What this program does on your computer

This section is for all readers. It uses short sentences and plain words on purpose.

- **It runs on your computer.** There is no account, no sign-up, and no server that we operate.
- **It sends no usage data to us.** There is no telemetry. We cannot see what you do.
- **It speaks only to the services that you configure.** These are your AI providers, and any tool
  servers that you add yourself. It contacts nothing else. Price data ships with the program; it is
  not fetched.
- **Your API keys go into the operating system credential store.** The program does not write them
  to files. It does not print them. It removes key-shaped text from what it shows you.
- **It asks before it runs a command.** The default answer is no. Piped input cannot approve a
  command for you.
- **It shows the cost of each call.** When a provider does not report a number, the program says so.
  It does not guess and it does not fill in a zero.
- **Files stay on your computer.** Receipts and run records go into a local `.nosis/` folder that
  you can delete at any time.
- **The optional MCP server is off by default.** When you start it, it listens only on your own
  machine and it needs a token.
- **Not yet tested on Linux or macOS.** See [Platform status](#platform-status) below. We do not
  claim what we have not tested.

For the technical detail behind each point, read [SECURITY.md](./SECURITY.md) (the security model,
the audits, and how to report a problem) and [PRIVACY.md](./PRIVACY.md) (what leaves your machine).

## Install (from source)

Prerequisites: the Rust toolchain, version **1.96.0**. The repo pins it in `rust-toolchain.toml`, so `rustup` selects the right version automatically.

```sh
cargo build --release
```

The binary lands at `target/release/nh` (`target\release\nh.exe` on Windows). Add it to your `PATH`, or copy it somewhere already on it.

## Quickstart

- `nh init` — scaffold `.nosis/` in the current repo: the receipts dir, a `.gitignore`, a secret-pattern pre-commit hook, and the trusted bundled `catalog.toml`. A changed repository catalog is refused unless the operator has placed an exact reviewed copy at `~/.nosis/catalog.toml`. Existing Git hooks are preserved and reported for manual chaining.
- `nh key add deepseek` — prompt for your DeepSeek API key and store it in the OS-native vault (never echoed, never written to files). For CI/headless use, the env fallback is `NH_<ENTRY>_KEY` with the entry uppercased — here, `NH_DEEPSEEK_KEY`.
- `nh key remove deepseek` — remove that entry from the OS-native vault. Environment fallbacks must be unset separately.
- Local Ollama and llama.cpp routes are user-filled, loopback-only, and selected only through
  `--model` or `/model`; they never become cheapest-capable candidates. See
  [Local models](./06-operations/LOCAL_MODELS.md) for setup, the Ollama truncation warning, model
  verification, licensing, and hardware sizing.
- `nh run "fix the failing test" --model deepseek-v4-flash` — run one agent task. Every shell command stops at a y/N approval prompt (default **deny**), and each turn is logged to `.nosis/receipts.jsonl`. Defaults: `--model deepseek-v4-flash`, `--max-turns 20`, `--profile balanced`. Optional: `--think none|low|high|max` (absent = per-route-dialect default: High on always-thinking dialects, None on non-thinking) and `--autonomy ask|auto` (absent = the law-file default).
- `nh why "review the diff"` — explain the cheapest capable route for a rough token estimate of the task; add `--model <id>` to compare a specific route against it.
- `nh chat` — interactive session. `/model` and `/provider` switch routes mid-session (history and cumulative usage preserved); `/price` evaluates the catalog price at the current clock time and flags stale data.
- `nh profile` — list the execution profiles (frugal / balanced / max-quality) and their effective caps for a model.
- `nh tui` — full-screen terminal UI (`--model <id>`, `--budget <tokens>`, `--profile <p>`).
- `nh fleet run tasks.json` — run independent tasks in a durable, resumable worker fleet (`--max-workers <n>`, required `--budget <tokens>` unless `budget_tokens` is in the file, `--escalate`, `--defer-offpeak`). The observed-token budget stops new dispatch after completed receipts reach it; already-running calls can finish. Off-peak deferral activates only for a trusted route whose catalog entry currently defines peak windows. `nh fleet resume` picks up the latest incomplete run.
- `nh mcp serve` — **PREVIEW**: serve the local MCP endpoint (default `--addr 127.0.0.1:8765`), loopback-only and bearer-token guarded (`--token-entry <entry>`). Tools: `why`, `route_cost`, and `receipts` (the metered-routing surface, with structured output), alongside `route_resolve`, budget-required `fleet_run`, and `fleet_status`. Do **not** expose it publicly before the MCP final spec lands on 2026-07-28.

## Platform status

Windows is built and tested. Linux and macOS paths exist but have not yet been executed on those platforms; this release does not claim them as verified.

## Privacy

`nh` has no Nosis-operated telemetry, analytics, beacons, or crash reporting. Model requests go directly to the provider route you select; approved MCP calls and shell commands can create additional network traffic. Receipts are local by default. Exact boundaries and deletion steps are in [PRIVACY.md](./PRIVACY.md).

## Runtime files

Apart from the configuration and Git hook created by an explicit `nh init`, the harness
does not generate source code or caches of its own. Normal runs append the intentional,
gitignored audit state in `.nosis/receipts.jsonl`; Fleet additionally uses `.nosis/fleet/`.
These append-only records grow until the operator deletes them. Agent-requested edits and
approved shell commands can change other workspace files by design.

## License & contributing

MIT © nosistech LLC — see [LICENSE](./LICENSE). Security policy and reporting: [SECURITY.md](./SECURITY.md). How to contribute (including the workspace gate): [CONTRIBUTING.md](./CONTRIBUTING.md). Release history: [CHANGELOG.md](./CHANGELOG.md).

## First Read Order

1. `00-start-here/MASTER_CONTEXT.md`
2. `00-start-here/CURRENT_TASK.md`
3. `00-start-here/BUILD_LOG.md`
4. `00-start-here/ROADMAP.md`
5. `02-architecture/ARCHITECTURE_DECISIONS.md`
6. `05-ai-collaboration/AGENTS.md`
7. `NOSIS_HARNESS_Master_Plan.md` (full spec, when depth is needed)

## Folder Purpose

- `00-start-here`: continuity, roadmap, current state, and decisions
- `01-product`: differentiators, positioning, pricing, and ideas
- `02-architecture`: crate design, routing brain, security, MCP, and integrations
- `03-execution`: milestone tasks, tests, releases, and quality gates
- `04-research`: model/provider research, CodeWhale analysis, sources
- `05-ai-collaboration`: roles and prompts for Claude (plan), Codex (build), Opus (gate)
- `06-operations`: API keys/access map, environments, costs, incidents, vendors
- `07-assets`: brand, screenshots, diagrams, exports
- `08-decisions-and-risk`: risks, assumptions, open questions, tradeoffs
- `09-customer-learning`: user feedback once it ships
- `10-knowledge-system`: lessons, glossary, patterns, playbooks
- `11-automation`: recurring tasks (e.g. price-catalog verification), checklists
- `12-executive`: one-page summary, pitch notes, market map

## Rule — THE LAW

Small, simple, secure, safe, lightweight, readable, auditable, modular, congruent, harmonic. Add complexity only when it creates real value. Every feature request that isn't in v1 scope goes to `03-execution/TASK_BACKLOG.md` under LATER.
