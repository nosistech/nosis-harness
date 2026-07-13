# Context Handoff

Use this when moving to a new chat, CLI, model, or teammate.

## Summary

Project:

Nosis Harness — Rust terminal agent harness routing across open-weight APIs (DeepSeek/Kimi/MiMo/GLM) + subscription delegates (Claude/Codex/Gemini) by clock, cache, modality, and thinking budget.

Current goal:

Pre-M0 setup: `git init`, repo-root AGENTS.md, first commit, then hand M0 to Codex (GPT-5.6). See `../00-start-here/CURRENT_TASK.md`.

What was built:

Nothing in code yet. Master Plan v0.1 + Appendices A/B (root), and this project OS folder (July 12, 2026).

What is next:

M0 skeleton (see `CODEX.md` for the exact prompt), then M1 RouteResolver + catalog.

## Important Files

- `../NOSIS_HARNESS_Master_Plan.md` — canonical spec; Appendices A/B supersede §1/§3
- `../00-start-here/CURRENT_TASK.md`, `BUILD_LOG.md`, `MILESTONES.md`
- `../02-architecture/ARCHITECTURE_DECISIONS.md`, `SECURITY_MODEL.md`
- `AGENTS.md` (roles + THE LAW + banned strings), `CODEX.md` (M0 prompt)

## Decisions

- Route A greenfield; single RouteResolver; catalog = TOML data; 2 wire protocols only.
- Two backend classes (API vs delegate routes); nh-vault in M0; MCP 2026-07-28 stateless-native.
- Implementer = GPT-5.6 Terra/Sol (not "Codex 5.5" — legacy).

## Risks

- All Appendix B prices `reported` until verified live at M1; MiMo first-party pricing sources conflict.
- DeepSeek legacy aliases die July 24; MiMo V2 series already dead (June 30) — audit KORVIN/LiteLLM configs.
- MCP final spec lands July 28 — pin the frozen RC SDK; don't ship nh-mcp server publicly before final.

## Exact Next Prompt

```text
Read 00-start-here/MASTER_CONTEXT.md, 00-start-here/CURRENT_TASK.md, and 00-start-here/BUILD_LOG.md, then NOSIS_HARNESS_Master_Plan.md as needed. Continue from the latest next step. Do not restart from scratch.
```
