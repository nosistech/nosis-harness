# Master Context

This is the first file to read in any new session.

## Project

Name:

Nosis Harness

One-sentence description:

A Rust terminal agent harness that routes coding/agent work across open-weight model APIs (DeepSeek V4, Kimi K2.x, MiMo V2.5, GLM) and subscription delegates (Claude Code, Codex CLI, Antigravity), picking the cheapest capable route by clock, cache, modality, and thinking budget.

Owner:

Carlos Paredes Vargas / NosisTech LLC

## Mission

What is this project trying to achieve?

- Original-IP harness (House of Nosis portfolio, KORVIN posture: influenced-by CodeWhale, not forked) that is better than incumbents on 7 concrete differentiators.
- Structural cost advantage: time-of-day routing (La Ceiba daytime = DeepSeek off-peak), KV-cache-first context engine (~120× cheaper cache hits), free GLM routes for CI.
- Fix the documented CLI pain: approval fatigue, cost opacity, ambiguous status, context loss, Windows instability. Windows-first is a real wedge.

## Current State

Stage:

research → pre-prototype (planning complete, zero code)

Current status:

Master Plan v0.1 complete with Appendices A (provider access architecture) and B (full verified model catalog), research current through July 11, 2026. Project OS folder structure created July 12. Repo not yet `git init`-ed; M0 not started.

## What Matters Most

- Customer value: a harness that is measurably cheaper and calmer to use than Claude Code/Codex for open-model work.
- Speed: M0 in week 1; 10-week plan to M5 launch.
- Security: nh-vault from the first commit; tool outputs are always data; Lethal Trifecta never assembled.
- Simplicity: THE LAW governs every PR. 5 providers, 7 differentiators, nothing else in v1.
- Revenue: portfolio/launch asset for NosisTech (nosistech.com launch post at M5); not a paid product in v1.
- Maintainability: catalog is data (TOML), only 2 wire protocols (OpenAI + Anthropic Messages).

## Current Strategic Decision

Main direction:

Route A — greenfield, narrow scope. Steal patterns from CodeWhale (RouteResolver, nested constitution, fleet ledger), never code.

Why:

Forking CodeWhale (Route B) inherits 100k+ lines of moving upstream and isn't original IP. Route A keeps THE LAW satisfiable and the portfolio value intact.

## Read Next

1. `CURRENT_TASK.md`
2. `BUILD_LOG.md`
3. `ROADMAP.md`
4. `../02-architecture/ARCHITECTURE_DECISIONS.md`
5. `../05-ai-collaboration/AGENTS.md`
6. `../NOSIS_HARNESS_Master_Plan.md` — canonical spec (Appendices A/B supersede Sections 1/3)
