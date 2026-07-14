# AUTONOMOUS HANDOFF — read this FIRST on resume

**Written 2026-07-13 by Fable 5 just before a `/clear`. The session that reads this is the ORCHESTRATOR and runs autonomously until the project is done.**

## Roles (fixed by Carlos)

- **Orchestrator = Opus 4.8, high effort** (this Claude session). Plans, writes per-milestone briefs + locked contracts, runs verification gates, adversarially reviews, gates, commits, writes docs. Does NOT hand-write milestone implementation code.
- **Executor = GPT-5.6 Sol, xhigh effort**, driven headless via Codex CLI. Writes all M2–M5 implementation code.
- Verified working 2026-07-13: `codex exec -m gpt-5.6-sol -c model_reasoning_effort=xhigh` returned SOL-READY. Codex CLI 0.144.1, logged in via ChatGPT.

## Exact executor invocation

Run from the repo root, one milestone (or milestone-slice) per call. Codex needs write access + workspace-write sandbox to implement:

```
codex exec --skip-git-repo-check -s workspace-write \
  -m gpt-5.6-sol -c model_reasoning_effort=xhigh \
  "<brief: read AGENTS.md + CONTRACTS_<Mx>.md, implement <scope>, run cargo test + clippy -D warnings before finishing>"
```

Give Sol a written brief + a locked `CONTRACTS_<Mx>.md` (same pattern as `CONTRACTS_M1.md`). Keep scopes small; large milestones (M3 TUI, M4 fleet) split into 2–3 codex calls. Long calls: run in background (`run_in_background: true`) and poll.

## The build loop (repeat per milestone M2 → M5)

1. Orchestrator writes `CONTRACTS_<Mx>.md` (locked public APIs) + a brief, consistent with the master plan §6 and THE LAW.
2. Executor (Sol xhigh) implements via `codex exec`.
3. Orchestrator verifies: `cargo test --workspace` + `cargo clippy --workspace --all-targets -- -D warnings` + the milestone's exit criteria (see `MILESTONES.md`).
4. Orchestrator adversarially reviews vs THE LAW + `02-architecture/SECURITY_MODEL.md`; sends confirmed findings back to Sol to fix (hardening pass).
5. Update `BUILD_LOG.md` + `CURRENT_TASK.md`; `git commit` (trailer: `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>` + note Sol as implementer in body).
6. Next milestone. Do not skip the review gate.

## Guardrails (non-negotiable)

- **UX IS THE PRODUCT** (Carlos's #1 rule): every user-facing line short, concrete, actionable; no stack traces; drop-if-hard. TUI (M3) is where this matters most.
- **THE LAW**: small, simple, secure, safe, lightweight, readable, auditable, modular, congruent, harmonic.
- If `gpt-5.6-sol` stops resolving in Codex → **STOP and tell Carlos**, do not silently fall back to Terra.
- If Sol fails the same gate twice → **STOP and report** (plan's escalation rule), don't loop.
- Banned model strings (adapter-rejected): `deepseek-chat`, `deepseek-reasoner`, `mimo-v2-*` (v2.5 OK), `gpt-5.2*`, `gpt-5.3-codex`, `moonshot-v1-*`.
- No plaintext secrets; every output path through the Scrubber; approval gate before exec/state-mutating MCP.

## State at handoff

- **M0: DONE** (commits `51aef97`→`1a0eaa5`). 62 tests, hardened.
- **M1: DONE and VERIFIED GREEN** (commits `0ed3d6d` integration → `bfdfc59` hardening → `96c46b1` docs). Full 13-route catalog, clock pricing (Beijing peak windows, all 8 boundary tests), Anthropic Messages wire, thinking dialects, stateless MCP client (no session header on wire — exit criterion met), `nh chat` REPL with /model /provider /price. **180 tests passing, 1 ignored (live-keyring), 0 failed across 10 binaries; clippy clean.** Confirmed by the orchestrator at handoff, not just the workflow's self-report. Live-pending only: real provider calls, a real 2026-07-28 MCP server, DeepSeek peak-window re-verify ~2026-07-24 (see MILESTONES.md).
- `CONTRACTS_M1.md` exists at repo root (pattern to copy for M2+). M1 contract deviations are logged in its §7 + DECISION_LOG 2026-07-13.

## FIRST ACTIONS on resume

1. `git log --oneline | head -6` — confirm HEAD is `96c46b1` (or later). M1 is already verified green, so no need to re-fix it.
2. `cargo test --workspace` + `cargo clippy --workspace --all-targets -- -D warnings` — quick sanity re-check (expect 180 passing / 1 ignored / clean), then move to M2.
3. Read the latest `00-start-here/BUILD_LOG.md` + `CURRENT_TASK.md` for the M1 review outcome.
4. Begin **M2** (context engine + nested constitution loader + mechanical write-holds; exit: cache-hit % >60% on a 50-turn session, protected path blocked in max autonomy — `MILESTONES.md`). Write `CONTRACTS_M2.md`, brief Sol, run the loop.
5. Continue autonomously through M5. Report to Carlos at each milestone boundary; only stop for the guardrail conditions above.

Full spec: `NOSIS_HARNESS_Master_Plan.md` (Appendices A/B supersede §1/§3). Memory: `m2-m5-codex-sol-directive`, `ux-first-and-the-law`.
