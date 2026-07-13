# Product Brief

## Product Name

Nosis Harness

## One-Sentence Pitch

The first agent CLI that treats cost, clock, cache, and modality as routing inputs — open-weight frontier models do the bulk work, subscription delegates (Claude/Codex/Gemini) do what only they can.

## Problem

What painful problem does this solve?

- Existing CLIs (Claude Code, Codex, Gemini CLI, CodeWhale) have documented pain: approval fatigue, opaque cost/rate-limit shock, ambiguous status, context loss in long sessions, Windows instability.
- Nobody routes by price-at-this-hour or cache economics, even though cache-hit input is ~120× cheaper on DeepSeek V4-Pro and peak hours cost 2×.
- Multi-provider reality (API credits here, subscriptions there) has no harness that treats both as first-class routes.

## Customer

Who has this problem?

- Carlos/NosisTech first (dogfood: KORVIN, LECTOR, daily coding).
- Power users of open-weight models who juggle DeepSeek/Kimi/MiMo/GLM credits plus a Claude or ChatGPT subscription — especially on Windows.

## Solution

What will the product do? The 7 differentiators (plan §0):

1. Time-of-day cost routing (DeepSeek peak/off-peak, MiMo night discounts; La Ceiba daytime = DeepSeek off-peak).
2. Modality-aware dispatch (per-route flags; vision subtasks auto-delegate instead of erroring).
3. Thinking-budget governor (task complexity → per-provider reasoning dialect).
4. KV-cache-first context engine (stable prefix as invariant; cache-hit % in the status line).
5. MCP 2026-07-28 stateless-native, both client and server — no legacy to migrate.
6. UX that fixes the documented pain (semáforo status, cost HUD, trust dial, timeline scrubber, `?` palette, Windows-first).
7. Constitution-native: THE LAW + AGENTS.md enforced in code, never overridable by model text.

## Why Now

- DeepSeek V4 official launch (mid-July 2026) introduces peak/off-peak pricing — the routing lever exists for the first time.
- MCP 2026-07-28 spec finalizes in days; incumbents must migrate, a greenfield harness starts clean.
- GPT-5.6 (July 9) + Opus 4.8 make the Claude-plans/Codex-builds/Opus-gates loop strong enough to build this with one person.

## Success Criteria

This product is working if:

- Cache-hit % >60% on a 50-turn session (M2 exit).
- Deferrable jobs actually execute off-peak and the HUD shows the saving.
- A full native Windows session with zero renderer artifacts (M3 exit).
- Carlos uses it daily over the incumbents for open-model work.
