# Decision Log

Use this for fast decisions. Large technical decisions live in `../02-architecture/ARCHITECTURE_DECISIONS.md`.

## 2026-07-13: M1 contract amendments ratified with orchestrator authority

Decision:

CONTRACTS_M1.md froze the M1 public surface with amendments through the architect only, but four integration/hardening deviations were ratified with orchestrator authority instead and recorded in CONTRACTS_M1.md §7: keyless `nh chat` startup (warning + REPL, exit 0 on `/quit`), the `peak <multiplier>x until HH:MM` footer format, nh-tools crate-root re-exports of the MCP items, and `nh run --think none|low|high|max` with per-dialect defaults.

Why:

The gaps surfaced after the architect's pass finished; blocking a green integration to re-convene the architect adds process without value (THE LAW: simple). All four are additive-only and written into the contract itself, so the frozen-surface audit trail stays intact.

Tradeoffs:

- Two ratification authorities for one contract — rule: orchestrator amendments must be additive-only and land in §7 with a date.

Review later:

no

## 2026-07-12: ProjectStarterTemplate adopted as project OS

Decision:

Adapt the standard NosisTech starter template into this folder; Master Plan stays at root as canonical spec, folders are the working layer.

Why:

Explicit context per session; every agent (Claude/Codex/Opus) gets the same read order.

Tradeoffs:

- Two places to update (plan vs working docs) — rule: docs point to plan sections instead of duplicating them.

Review later:

no

## 2026-07-11: Implementer re-pointed to GPT-5.6

Decision:

Build-loop implementer = GPT-5.6 Terra by default; Sol (max effort, ultra mode) for M2 context engine and anything touching nh-law/security. "Codex 5.5" references are legacy.

Why:

GPT-5.6 GA July 9; OpenAI guidance: Terra succeeds GPT-5.5-class work at half price. `gpt-5.2*`/`gpt-5.3-codex` deprecated under ChatGPT sign-in.

Tradeoffs:

- Requires updated Codex CLI binary or 5.6 won't appear.

Review later:

yes — when quotas or a newer family land.

## 2026-07-11: nh-vault lands in M0, not later

Decision:

Key vault crate ships in the first milestone.

Why:

Keys exist from the first commit, so security must too (THE LAW: secure). Plan §A.8/§A.10.7.

Review later:

no

## 2026-07-09: Route A — greenfield, not CodeWhale fork

Decision:

Original IP, 5 providers, patterns-not-code from CodeWhale.

Why / tradeoffs:

See `../02-architecture/ARCHITECTURE_DECISIONS.md` Decision 1.

Review later:

no
