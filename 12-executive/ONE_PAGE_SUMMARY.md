# One Page Summary

## Project

Nosis Harness (NosisTech LLC)

## What It Is

A Rust terminal agent harness — like Claude Code or Codex CLI, but provider-plural: it routes each task to the cheapest capable model across DeepSeek V4, Kimi K2.x, MiMo V2.5, and GLM APIs, while driving Claude, Codex, and Gemini headless through existing subscriptions for what only they do best.

## Who It Serves

Carlos/NosisTech first (KORVIN, LECTOR, daily coding); then power users of open-weight models on Windows.

## Problem

Every incumbent CLI is single-provider-biased, cost-opaque, approval-fatiguing, and unstable on Windows — and none of them route by price-at-this-hour or cache economics, the two biggest cost levers of 2026.

## Solution

Seven differentiators: time-of-day cost routing, modality-aware dispatch, thinking-budget governor, KV-cache-first context engine, MCP 2026-07-28 stateless-native (client + server), pain-fixing UX (semáforo, cost HUD, trust dial, timeline scrubber), constitution-native governance (THE LAW enforced in code).

## Why It Can Win

- Structural cost edge: La Ceiba daytime = DeepSeek off-peak; cache hits ~120× cheaper; free GLM routes for CI.
- Timing: DeepSeek peak/off-peak pricing and the MCP stateless spec both land in July 2026 — greenfield starts clean while incumbents migrate.
- Windows-first native support — a wedge nobody serves.
- Build leverage: Claude plans → GPT-5.6 implements → Opus 4.8 gates, one human directing.

## Current Status

Planning complete (Master Plan v0.1 + verified catalog, July 11, 2026). Project OS in place. Repo init + M0 next.

## Next Milestone

M0 (week 1): workspace skeleton, turn loop on deepseek-v4-flash, read/edit/exec tools, JSONL receipts, nh-vault. Exit: fixes a failing test in a sample repo end-to-end.
