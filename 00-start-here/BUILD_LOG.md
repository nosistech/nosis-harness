# Build Log

Record every meaningful session here.

## 2026-07-13: M0 hardening (adversarial review fixes)

Builder:

- Claude (Fable 5, Claude Code) — hardening agent

What changed:

- nh-cli: every stderr path now passes the Scrubber — progress lines, the approval prompt, and the final `nh:` error line; model-supplied text is also control-char-escaped (`sanitize_line`) so \r/ANSI cannot spoof the approval gate, with a visible truncation marker past 500 chars.
- nh-tools: `exec_shell` strips `NH_*_KEY` env vars from the child, closing the key-exfiltration-to-disk path via the env fallback.
- nh-routes: `from_toml` rejects banned route keys AND banned `model_id` values (clean alias can no longer smuggle a dead id onto the wire).
- nh-cli: `nh init` writes a starter catalog.toml (embedded repo-root catalog — still data), so `nh run` works in a fresh repo; missing-catalog error now says "run `nh init` to create one".

Tests/checks run:

- `cargo test --workspace` (62 passed, 1 ignored keyring round-trip), `cargo clippy --workspace --all-targets -- -D warnings` — green. Manual: `nh init` + `nh run` flow in a fresh temp dir reaches the key prompt.

Next step:

- M1: live route/pricing verification against providers.

## 2026-07-12: M0 build finalized (Fable 5 multi-agent workflow)

Builder:

- Claude (Fable 5, Claude Code) — multi-agent workflow: 5 parallel crate builders, integrator, 3 adversarial reviewers, hardening pass

What changed:

- M0 implemented end-to-end across all five crates via the multi-agent workflow; integration green after merging the crate builders' outputs.
- 6 adversarial review findings addressed (fixes detailed in the M0 hardening entry above).

Tests/checks run:

- 53 passed; 0 failed; 1 ignored (keyring_round_trip) across 6 test binaries: nh-core unit 12, nh-cli 8, nh-core integration 3, nh-routes 10, nh-tools 10, nh-vault 10 (+1 ignored); doc-tests 0; clippy -D warnings clean.

Next step:

- Verify live against DeepSeek (`nh key add deepseek`, then `nh run` on a sample repo), then M1.

## 2026-07-12: M0 implemented (turn loop, tools, vault, routes, CLI)

Builder:

- Claude (Fable 5, Claude Code) — 5 parallel crate builders + integrator

What changed:

- Implemented all locked `todo!()` contracts across `nh-core` (AgentLoop, OpenAiCompatClient, receipts), `nh-routes` (RouteResolver, catalog parsing, banned-string rejection), `nh-tools` (read_file / edit_file / exec_shell behind approval gate; denial is an Ok-shaped "user denied: <command>" tool result), `nh-vault` (OS keyring + env fallback + Scrubber), `nh-cli` (init / key / run).
- Sanctioned contract addition: `AgentLoop.on_event: Option<Box<dyn Fn(&str) + Send>>` for progress lines; nh-cli wires it to stderr. Field set is now frozen.

Tests/checks run:

- `cargo build --workspace`, `cargo test --workspace` (53 passed, 1 ignored keyring round-trip), `cargo clippy --workspace --all-targets -- -D warnings` — all green.

Next step:

- M1: live route/pricing verification against providers.

## 2026-07-12: Project OS created

Builder:

- Carlos + Claude (Fable 5, Claude Code)

What changed:

- Adapted ProjectStarterTemplate into this folder as the project operating system.
- Filled core docs from Master Plan v0.1: master context, current task, roadmap, milestones, decision log, product brief, architecture overview/decisions, security model, AI-collaboration set (AGENTS/CLAUDE/CODEX/MODEL_ROLES/CONTEXT_HANDOFF/PROMPT_LIBRARY), risk register, one-page summary.
- Re-pointed the M0 implementer prompt from "Codex 5.5" to GPT-5.6 (Terra default / Sol for hardest), added nh-vault to M0 scope per plan §A.10.7.

Files changed:

- All of `00-start-here/`, `05-ai-collaboration/`, plus README, PRODUCT_BRIEF, ARCHITECTURE_OVERVIEW, ARCHITECTURE_DECISIONS, SECURITY_MODEL, RISK_REGISTER, ONE_PAGE_SUMMARY.

Decisions made:

- Adopted the template as project OS (see DECISION_LOG 2026-07-12).

Tests/checks run:

- None (no code yet).

Next step:

- `git init`, root AGENTS.md, first commit, hand M0 to Codex (prompt in `../05-ai-collaboration/CODEX.md`).

Risks:

- All Appendix B prices are `reported`, not confirmed — verify live at M1. MiMo first-party pricing sources conflict.

## 2026-07-09 → 2026-07-11: Master Plan v0.1 + Appendices A/B

Builder:

- Carlos + Claude (research/planning)

What changed:

- Master Plan v0.1 written (verdict, capability matrix, architecture, routing brain, fleet, MCP 2026-07-28 strategy, UX, build plan M0–M5, risks, Codex first prompt).
- Appendix A: two-backend access architecture (API routes vs subscription delegates), per-provider deep dives, nh-vault spec.
- Appendix B: complete verified model catalog (July 11) with delta logs.

Next step:

- Organize project folder, then pre-M0 setup.
