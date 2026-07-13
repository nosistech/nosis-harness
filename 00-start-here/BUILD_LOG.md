# Build Log

Record every meaningful session here.

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
