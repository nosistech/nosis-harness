# Master Context

This is the first file to read in any new session.

## Project

Name:

Nosis Harness

One-sentence description:

A Rust terminal agent harness that executes an explicitly selected open-weight model route, meters reported usage, and explains the cheapest capable catalog route without silently dispatching it.

Owner:

Carlos Paredes Vargas / NosisTech LLC

## Mission

What is this project trying to achieve?

- Original-IP harness (House of Nosis portfolio, KORVIN posture: influenced-by CodeWhale, not forked) that is better than incumbents on 7 concrete differentiators.
- Structural cost controls: explicit route selection, a separate `nh why` cheapest-capable
  explanation, stable-prefix context handling, and clock-aware pricing only when a trusted catalog
  entry actually defines a peak window. The current first-party catalog has no peak windows.
- Fix the documented CLI pain: approval fatigue, cost opacity, ambiguous status, context loss, Windows instability. Windows-first is a real wedge.

## Current State

Stage:

public-v0.1 hardening; Windows implementation and test suite exist, release gates remain

Current status:

Nine-crate Rust workspace implemented. Windows tests are active; Linux and macOS remain unverified. Public release is blocked on the documented release/FEEL/platform gates.

## What Matters Most

- Customer value: a harness that is measurably cheaper and calmer to use than Claude Code/Codex for open-model work.
- Speed: M0 in week 1; 10-week plan to M5 launch.
- Security: nh-vault from the first commit; tool outputs are data; shell execution is always
  approval-gated; outbound credentials are scoped to exact configured origins.
- Simplicity: THE LAW governs every change. Public v0.1 is deliberately narrower than the original
  seven-differentiator plan; current behavior is described by `README.md`, `SECURITY.md`, and the
  architecture overview.
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
