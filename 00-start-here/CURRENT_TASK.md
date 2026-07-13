# Current Task

## Immediate Goal

Pre-M0 setup: initialize the git repo, write the repo-root `AGENTS.md` (THE LAW + build-loop roles), then hand Milestone M0 to Codex (GPT-5.6).

## Why This Matters

The build loop (Claude plans → Codex implements → Opus 4.8 gates) can't start until the repo and AGENTS.md exist — both the M0 prompt and the Opus review gate assume them.

## Current Status

Completed:

- Master Plan v0.1 + Appendix A (provider/access architecture) + Appendix B (verified model catalog), research through July 11, 2026.
- Project OS folder structure adapted from ProjectStarterTemplate (July 12).
- M0 handoff prompt re-pointed from "Codex 5.5" to GPT-5.6 (see `../05-ai-collaboration/CODEX.md`).

In progress:

- Pre-M0 setup (this task).

Blocked:

- Nothing.

## Next Action

`git init` in this folder, add `.gitignore` (`.nosis/`, secrets patterns), write repo-root `AGENTS.md`, first commit. Then paste the M0 prompt from `../05-ai-collaboration/CODEX.md` into Codex CLI.

## Do Not Do Yet

- TUI (M3), fleet/swarm (M4), nh-mcp server (M4).
- Buying GLM-5.2 credits or ANY Coding Plan subscription (GLM plan is supported-tools-only — unusable by this harness).
- Adding a 6th provider (GLM-5.2 is a post-v1 TOML entry).
- Trusting any Appendix B price as confirmed — all prices are `reported` until verified live at M1 integration.

## Definition Of Done

This task is done when:

- Repo is initialized with root AGENTS.md and a first commit.
- Codex has the M0 brief and starts implementing.
- M0 exit criterion is met: the harness fixes a failing test in a sample repo end-to-end via `deepseek-v4-flash`, writing JSONL receipts, with `cargo test` and `cargo clippy -- -D warnings` clean.
