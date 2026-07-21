# Nosis Harness — Project OS

**nh** is an honest, metered, multi-model terminal agent (Rust) for open-weight models — DeepSeek V4, Kimi K2.x, MiMo V2.5, GLM — with Claude, Codex, and Gemini as subscription-delegate peers. For every task it picks the **cheapest capable** route and hands you the **receipt**: cost, token usage, and savings. It is a harness with a meter, not a chat UI.

**Canonical spec:** `NOSIS_HARNESS_Master_Plan.md` (root). Appendices A/B supersede Sections 1 and 3 where they conflict. The folders below are the working layer on top of it.

## Install (from source)

Prerequisites: the Rust toolchain, version **1.96.0**. The repo pins it in `rust-toolchain.toml`, so `rustup` selects the right version automatically.

```sh
cargo build --release
```

The binary lands at `target/release/nh` (`target\release\nh.exe` on Windows). Add it to your `PATH`, or copy it somewhere already on it.

## Quickstart

- `nh init` — scaffold `.nosis/` in the current repo: the receipts dir, a `.gitignore`, a secret-pattern pre-commit hook, and a starter `catalog.toml`.
- `nh key add deepseek` — prompt for your DeepSeek API key and store it in the OS-native vault (never echoed, never written to files). For CI/headless use, the env fallback is `NH_<ENTRY>_KEY` with the entry uppercased — here, `NH_DEEPSEEK_KEY`.
- `nh run "fix the failing test" --model deepseek-v4-flash` — run one agent task. Every shell command stops at a y/N approval prompt (default **deny**), and each turn is logged to `.nosis/receipts.jsonl`. Defaults: `--model deepseek-v4-flash`, `--max-turns 20`, `--profile balanced`. Optional: `--think none|low|high|max` (absent = per-route-dialect default: High on always-thinking dialects, None on non-thinking) and `--autonomy ask|auto` (absent = the law-file default).
- `nh why "review the diff"` — explain the cheapest capable route for a rough token estimate of the task; add `--model <id>` to compare a specific route against it.
- `nh chat` — interactive session. `/model` and `/provider` switch routes mid-session (history and cumulative usage preserved); `/price` shows live peak/off-peak pricing.
- `nh profile` — list the execution profiles (frugal / balanced / max-quality) and their effective caps for a model.
- `nh tui` — full-screen terminal UI (`--model <id>`, `--budget <tokens>`, `--profile <p>`).
- `nh fleet run tasks.json` — run independent tasks in a durable, resumable worker fleet (`--max-workers <n>`, `--budget <tokens>`, `--escalate`, `--defer-offpeak`). `nh fleet resume` picks up the latest incomplete run.
- `nh mcp serve` — **PREVIEW**: serve the local MCP endpoint (default `--addr 127.0.0.1:8765`), loopback-only and bearer-token guarded (`--token-entry <entry>`). Tools: `why`, `route_cost`, and `receipts` (the metered-routing surface, with structured output), alongside `route_resolve`, `fleet_run`, and `fleet_status`. Do **not** expose it publicly before the MCP final spec lands on 2026-07-28.

## Privacy

Prompts and task text go only to the model provider you explicitly select, over TLS — nh adds no intermediary and does not phone home (no analytics, no beacons, no crash reporting). Receipts stay local in `.nosis/receipts.jsonl`. Details in [PRIVACY.md](./PRIVACY.md).

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
