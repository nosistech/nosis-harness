# Nosis Harness — Project OS

Terminal agent harness (Rust) for open-weight models — DeepSeek V4, Kimi K2.x, MiMo V2.5, GLM — with Claude, Codex, and Gemini as subscription-delegate peers.

**Canonical spec:** `NOSIS_HARNESS_Master_Plan.md` (root). Appendices A/B supersede Sections 1 and 3 where they conflict. The folders below are the working layer on top of it.

## Quickstart

- `cargo build --release` — build the workspace (binary at `target/release/nh`).
- `nh init` — scaffold `.nosis/` and a starter `catalog.toml` in your repo.
- `nh key add deepseek` — store your DeepSeek API key in the OS keyring (nh-vault).
- `nh run "fix the failing test" --model deepseek-v4-flash` — run the agent; every shell command stops at a y/N approval prompt, and each turn is logged to `.nosis/receipts.jsonl`.
- `nh run "…" --think none|low|high|max` — set thinking effort for the run; flag absent defaults per route dialect (High on always-thinking/glm-hm, None on deepseek-nhm/none).
- `nh chat` — interactive session; `/model` and `/provider` switch routes mid-session (history and cumulative usage preserved), `/price` shows live peak/off-peak pricing.

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
