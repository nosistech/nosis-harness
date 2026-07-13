# Current Task

## Immediate Goal

M1 is done (pending live provider tests). Run the live verification pass — a keyed DeepSeek session and a GLM free-route session — then start M2: context engine + law.

## Why This Matters

The workspace is green (176 tests + clippy `-D warnings` clean, 180 after hardening) and every catalog price is first-party confirmed, but the wire clients, mid-session switching, and MCP client have only been proven against loopback mocks. One live keyed session (which also completes M0's live exit criterion) and one GLM free-route session (costs nothing) turn "mock-verified" into "done". M2's byte-stable cache discipline builds directly on the request paths those live runs exercise.

## Current Status

Completed:

- Master Plan v0.1 + Appendices A/B; Project OS folder structure (July 12).
- M0: all five crates — turn loop, tools behind the approval gate, vault, routes, CLI, JSONL receipts — plus hardening (see `BUILD_LOG.md`).
- M1: full 5-provider catalog with clock-aware pricing, Anthropic Messages wire, thinking dialects + `nh run --think`, stateless MCP client, `nh chat` with mid-session `/model`/`/provider` switching and cost HUD. Integration green; 4 adversarial findings hardened.
- Price verification pass: all four providers confirmed first-party; MiMo B.3 conflict resolved (see `../04-research/SOURCE_INDEX.md`).

In progress:

- Live provider verification (this task).

Blocked:

- Nothing.

## Next Action

`nh key add deepseek`, then a live `nh chat` session: mid-session `/model` + `/provider` switch preserving history and cumulative usage, `/price` peak/off-peak against the real clock, one stateless MCP call with handle passback against a real 2026-07-28 server. Repeat the wire check on a GLM free route (`glm-4.7-flash` — free tier, zero cost). Then M2: context engine + law.

## Do Not Do Yet

- TUI (M3), fleet/swarm (M4), nh-mcp server (M4).
- Buying GLM-5.2 credits or ANY Coding Plan subscription (GLM plan is supported-tools-only — unusable by this harness).
- Trusting the DeepSeek peak 2x windows as confirmed — announcement-only today; re-verify on/around 2026-07-24 (`valid_until`).

## Definition Of Done

This task is done when:

- A live keyed session completes a mid-session `/model` + `/provider` switch and one real MCP tool call with handle passthrough (no session header on the wire), with receipts in `.nosis/receipts.jsonl`.
- A GLM free route answers a real request end-to-end.
- The runs are recorded in `BUILD_LOG.md`, the open verify-live ledger rows (CONTRACTS_M1.md §6: `reasoning_effort`, `ttlMs`, GLM rate limits) are updated, and M2 (context engine + law) is picked up as the next task.
