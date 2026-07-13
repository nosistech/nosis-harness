# Architecture Overview

## Current Architecture

A Rust workspace. An agent turn loop (nh-core) sends work to exactly one resolved route per turn. The RouteResolver (nh-routes) is the only component allowed to mint a resolved route: endpoint + wire protocol + model ID + context limit + price-at-current-clock + modality flags + thinking dialect. Two backend classes: direct API routes (DeepSeek, Kimi, MiMo, GLM - token-metered, keys in nh-vault) and delegate routes (Claude Code, Codex CLI, Antigravity CLI driven headless as subprocesses - quota-metered). Full spec: plan §2, §A.0.

## Main Components

- Frontend: nh-tui (ratatui; semáforo, cost HUD, timeline scrubber, trust dial, `?` palette) + nh-cli (headless `nh exec` for CI).
- Backend: nh-core (turn state machine, receipts), nh-routes (RouteResolver, catalog TOML, wire adapters: OpenAI + Anthropic Messages only), nh-context (budget, compaction, KV-cache prefix discipline, per-route `preserve_reasoning`).
- Database: SQLite for memory (retain/recall/reflect); append-only JSONL for receipts and fleet ledger.
- Background jobs: nh-fleet (workers, heartbeats, idempotent resume) + off-peak scheduler.
- External services: 5 provider APIs, MCP servers (client via nh-tools), KORVIN (as nh-mcp consumer), Telegram notify.
- Auth: nh-vault - OS-native secret stores (Windows Credential Manager/DPAPI, Keychain, secret-service), OAuth tokens for delegates/MCP.
- Billing: none in v1; cost accounting is internal (tokens × price(clock), delegate quota units).
- Observability: typed receipts per turn, failure classification (context/constraint/verification/planning), W3C Trace Context through MCP calls.

## Data Flow

1. Task arrives → classifier tags it `{modality, horizon, complexity, deferrable, secret-touching}`.
2. RouteResolver applies the policy table (plan §A.9) + clock + quota state → resolved route; deferrable work queues into off-peak windows.
3. nh-core runs the turn: stable prefix (law + AGENTS.md) → cached; dynamic content after the cache breakpoint; tools execute behind the trust dial.
4. Every turn: JSONL receipt + side-git snapshot (outside repo `.git`); verification policy gates phase advances; 2 failures at a tier → escalate one tier with receipt attached.

## Deployment Shape

Local:

Windows 11 native (ASUS build machine; verify over SSH on Predator). No sandbox parity pretense: Windows = approval-gating + restricted tokens/Job Objects; Linux = Landlock/seccomp; macOS = Seatbelt.

Staging:

n/a - it's a local CLI. CI runs headless `nh exec` with the free GLM-4.7-Flash route ($0 test suite).

Production:

Shipped binary + docs at M5; nh-mcp server exposed to KORVIN on the local network only in v1.
