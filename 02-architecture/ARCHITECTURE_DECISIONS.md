# Architecture Decisions

## Decision 1: Route A — greenfield, patterns not code

Decision:

Build original IP. Adopt CodeWhale's proven *patterns* (RouteResolver, nested constitution, fleet ledger, side-git snapshots, honest-cost rule) without forking its ~4,300-commit Rust codebase.

Why:

Forking inherits 100k+ lines and a moving upstream, and kills portfolio value. Rebuilding everything violates THE LAW — so scope narrows to 5 providers + 7 differentiators.

Alternatives considered:

- Route B: fork CodeWhale (MIT permits). Fastest to parity; rejected for IP and maintenance reasons.

Tradeoffs:

- Slower to feature parity; some CodeWhale features never arrive (accepted).

How to revisit:

- If v1 slips badly past week 10, reconsider B for specific subsystems.

## Decision 2: Single RouteResolver mints all resolved routes

Decision:

One component owns route resolution; a resolved route carries endpoint, wire, model ID, context limit, price at current clock, modality flags, thinking dialect.

Why:

Central choke point makes clock pricing, modality dispatch, banned-string rejection, and quota accounting enforceable and testable in one place.

Tradeoffs:

- All routing features serialize through one crate's API — design it early (M1).

## Decision 3: Catalog is data; exactly 2 wire protocols

Decision:

Models/prices live in `catalog.toml` (with `valid_until`, `price_confidence: confirmed|reported|verify_live`, per-route modality and output caps). Adapters exist only for OpenAI wire and Anthropic Messages wire.

Why:

Catalogs rot (K3, V5, Opus 5 will land); new models must be a TOML entry, not a release. Every provider in scope speaks one of the two wires.

Tradeoffs:

- Exotic provider features not expressible in 2 wires get dropped (accepted).

## Decision 4: Two backend classes — API routes vs delegate routes

Decision:

Class 1: direct API (DeepSeek/Kimi/MiMo/GLM), token-metered. Class 2: delegates (Claude Code `claude -p`, Codex `codex exec`, Antigravity CLI) driven headless as subprocesses, quota-metered, treated as zero-marginal-cost but quota-scarce.

Why:

Matches actual access (API credits vs subscriptions, no API keys for Claude/OpenAI/Google). Delegates are reserved for what they're uniquely good at: Opus = review gate, Codex = implementation bursts, Gemini = search-grounded research (best-effort only).

Tradeoffs:

- Cost HUD needs dual units; delegate output capture is subprocess-fragile — wrap defensively, write normal receipts.

## Decision 5: MCP targets the 2026-07-28 stateless spec from commit one

Decision:

Client + (later) server built against the frozen RC of MCP 2026-07-28: stateless core, explicit state handles surfaced in receipts, `.well-known` discovery, `ttlMs` caching, OAuth 2.1. 2025-11-25 supported as fallback client only. Pin SDK version; conformance check in CI; don't ship nh-mcp server publicly until the final lands.

Why:

Incumbents must migrate off session-based MCP; a greenfield harness never carries that debt. Handles-in-receipts satisfies THE LAW's auditability.

Tradeoffs:

- Small spec deltas possible between RC and final (July 28) — pinned SDK + conformance suite absorbs them.

## Decision 6: nh-vault in M0, OS-native secret stores

Decision:

No plaintext keys at rest anywhere; `keyring` crate over Windows Credential Manager/Keychain/secret-service; memory-only injection at spawn, `zeroize` after use; redaction scrubber on every output path; per-route key scoping; MCP header lint; git guard pre-commit hook.

Why:

Keys exist from the first commit. A leaked key in a stack trace is defined as a test failure. Full rules: plan §A.8 and `SECURITY_MODEL.md`.

Tradeoffs:

- LiteLLM gateway mode kept as a flag (simpler key surface, but loses Anthropic-wire features and native clock pricing). Default = direct + vault.

## Decision 7: Context engine gets per-route `preserve_reasoning`

Decision:

Compaction strips reasoning by default, but Kimi K2.7 (always-thinking, `preserve_thinking` forced ON) and MiMo thinking+tools mode require reasoning history persisted across turns — a per-route boolean the compactor must respect.

Why:

Opposite behaviors are both mandatory: stripping saves tokens on DeepSeek; stripping *degrades* Kimi/MiMo. Route-level flag is the only correct home (same logic as modality flags living on routes, not models).

## Decision 8: Windows-first

Decision:

First-class native Windows 11 (crossterm, tested renderer matrix, Job Objects + restricted tokens), honest docs that full syscall sandboxing is Linux-only.

Why:

Both dev machines run Windows 11; incumbent sandboxes still don't support native Windows — a real wedge.

Tradeoffs:

- Weaker containment on Windows in v1, stated honestly rather than faked.
