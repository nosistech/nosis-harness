# Current Task

## Immediate Goal

M0 implementation is done (pending live verification). Run the M0 exit criterion live test against DeepSeek, then start M1: RouteResolver full catalog.

## Why This Matters

The workspace is green offline (tests + clippy), but M0 only counts as done when the harness fixes a failing test in a real repo end-to-end via `deepseek-v4-flash`. M1's live route/pricing verification builds on that proven path.

## Current Status

Completed:

- Master Plan v0.1 + Appendix A (provider/access architecture) + Appendix B (verified model catalog), research through July 11, 2026.
- Project OS folder structure adapted from ProjectStarterTemplate (July 12).
- M0 implemented by the Fable 5 multi-agent workflow: all five crates, integration green, 53 tests passed + clippy `-D warnings` clean (see `BUILD_LOG.md`).
- M0 hardening pass: 6 adversarial review findings addressed.

In progress:

- M0 exit criterion live test (this task).

Blocked:

- Nothing.

## Next Action

`nh key add deepseek`, then `nh run "fix the failing test" --model deepseek-v4-flash` on a sample repo with a failing test. Confirm the approval prompt gates every shell command and receipts land in `.nosis/receipts.jsonl`. Then M1: RouteResolver full catalog.

## Do Not Do Yet

- TUI (M3), fleet/swarm (M4), nh-mcp server (M4).
- Buying GLM-5.2 credits or ANY Coding Plan subscription (GLM plan is supported-tools-only — unusable by this harness).
- Adding a 6th provider (GLM-5.2 is a post-v1 TOML entry).
- Trusting any Appendix B price as confirmed — all prices are `reported` until verified live at M1 integration.

## Definition Of Done

This task is done when:

- M0 exit criterion is met: the harness fixes a failing test in a sample repo end-to-end via `deepseek-v4-flash`, writing JSONL receipts, with `cargo test` and `cargo clippy -- -D warnings` clean.
- The live run is recorded in `BUILD_LOG.md` and M1 (RouteResolver full catalog) is picked up as the next task.
