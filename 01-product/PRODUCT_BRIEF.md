# Product Brief

## Product Name

Nosis Harness

## One-Sentence Pitch

The honest, visible, auditable *metered* agent for open-weight models — native on Windows: choose a route explicitly, inspect the cheapest-capable estimate, and get a local receipt.

> **Amended 2026-07-25.** This pitch previously read "...open-weight frontier models do the bulk work, subscription delegates (Claude/Codex/Gemini) do what only they can." The subscription-delegate backend class was **cut from v1** (2026-07-16/17); only a commented catalog schema stub remains. The pitch above now matches the shipped positioning in `WHY_BEST_IN_CATEGORY_2026.md`.

## Problem

What painful problem does this solve?

- Existing CLIs (Claude Code, Codex, Gemini CLI, CodeWhale) have documented pain: approval fatigue, opaque cost/rate-limit shock, ambiguous status, context loss in long sessions, Windows instability.
- Provider prices and limits change quickly, while existing CLIs rarely expose a reproducible local estimate tied to reported usage and freshness-dated catalog data.
- Multi-provider API users lack one small harness that applies the same approval, receipt, context, and credential rules across each direct route.

## Customer

Who has this problem?

- Carlos/NosisTech first (dogfood: KORVIN, LECTOR, daily coding).
- Power users of open-weight models who juggle DeepSeek/Kimi/MiMo/GLM credits plus a Claude or ChatGPT subscription — especially on Windows.

## Solution

What v0.1 does now:

1. Freshness-dated price estimates plus opt-in off-peak Fleet deferral when a trusted route actually defines a peak window.
2. Validated per-route modality data and an auditable `nh why` comparison; execution remains explicit.
3. Per-provider reasoning dialects controlled by bounded execution profiles.
4. KV-cache-first context engine (stable prefix as invariant; cache-hit % in the status line).
5. A loopback-only, bearer-guarded MCP preview; it is not a public network service.
6. UX that fixes the documented pain (semáforo status, cost HUD, trust dial, timeline scrubber, `?` palette, Windows-first).
7. Constitution-native: THE LAW + AGENTS.md enforced in code, never overridable by model text.

## Why Now

- Provider pricing and model limits are changing quickly; a data-driven catalog with short recheck deadlines avoids hard-coded cost claims.
- MCP 2026-07-28 spec finalizes in days; incumbents must migrate, a greenfield harness starts clean.
- GPT-5.6 (July 9) + Opus 5 make the Claude-plans/Codex-builds/Opus-gates loop strong enough to build this with one person.

## Success Criteria

This product is working if:

- Cache-hit % >60% on a 50-turn session (M2 exit).
- A synthetic peak-window route parks and resumes correctly; any future production peak window must be reverified before it enters the trusted catalog.
- A full native Windows session with zero renderer artifacts (M3 exit).
- Carlos uses it daily over the incumbents for open-model work.
