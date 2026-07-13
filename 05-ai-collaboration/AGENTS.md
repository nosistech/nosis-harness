# AGENTS.md

Project instructions for coding agents. (A repo-root AGENTS.md for the code workspace gets created at `git init`; it derives from this file and stays consistent with it.)

## First Read

1. `../00-start-here/MASTER_CONTEXT.md`
2. `../00-start-here/CURRENT_TASK.md`
3. `../00-start-here/BUILD_LOG.md`
4. `../02-architecture/ARCHITECTURE_DECISIONS.md`
5. `../NOSIS_HARNESS_Master_Plan.md` — canonical spec; Appendices A/B supersede §1 and §3

## THE LAW (top authority)

Small, simple, secure, safe, lightweight, readable, auditable, modular, congruent, harmonic. Every PR is judged against it. Scope not in v1 → `../03-execution/TASK_BACKLOG.md` under LATER.

## Build Loop Roles

- **Claude (Fable 5 / Opus, Claude Code)** — planner and spec owner. Writes/updates specs, briefs Codex, resolves ambiguity. Does not merge its own implementation unreviewed.
- **Codex (GPT-5.6)** — implementer. Terra by default; Sol (max effort, ultra mode) for M2 context engine and anything touching nh-law/security. Small PRs only. Must run `cargo test && cargo clippy -- -D warnings` before handoff. Never direct-to-main.
- **Opus 4.8 (Claude Code delegate)** — reviewer/gate. Checks THE LAW conformance, security posture, and spec match. May reject with a written receipt. Mandatory per PR. Send receipts + diffs, never raw transcripts (quota).
- Build on ASUS, verify over SSH on Predator.

## Behavior

- Preserve user work. Inspect before editing. Keep changes scoped.
- Prefer simple, auditable implementation.
- Update `../00-start-here/BUILD_LOG.md` after meaningful work; record architecture decisions.
- Catalog/pricing data is data (TOML) — never hard-code model IDs or prices in Rust.

## Safety

- Do not expose secrets — nh-vault rules (`../02-architecture/SECURITY_MODEL.md`) are non-negotiable.
- Do not make destructive changes without approval.
- Do not copy code from external repos (CodeWhale: patterns yes, code never).
- **Banned model strings (adapter-rejected, test-covered):** `deepseek-chat`, `deepseek-reasoner`, `mimo-v2-*`, `gpt-5.2*`, `gpt-5.3-codex`, `moonshot-v1-*`.
