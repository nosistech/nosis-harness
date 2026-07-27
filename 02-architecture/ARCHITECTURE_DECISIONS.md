# Architecture Decisions

*Last audited 2026-07-24 against the current repo. Decisions 4, 5, 7, and 8 carry dated amendments below; Decisions 1, 2, 3, and 6 stand as written.*

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

**Amendment 2026-07-24:** The delegate-adapter class (Class 2) was CUT from v1 and demoted to an escalation-gate footnote; only a commented catalog schema stub remains (`[routes.claude-opus-4-8]` in `catalog.toml`) so the class can return if a measured workload proves the escalation gate insufficient — the economics broke (Anthropic moved programmatic Claude use to API pricing 2026-06-15; Gemini CLI died as an open delegate 2026-06-18) and open-weight parity made the class unnecessary — evidence: `00-start-here/RESEARCH_2026-07_harness.md`:37, :61, :314 (ratified in the `d3cac39` research cycle), `00-start-here/CURRENT_TASK.md`:13 ("the delegate class is CUT from v1"), revisit trigger at `RESEARCH_2026-07_harness.md`:101 ("no OpenAI/Anthropic/Google key until a measured workload proves the delegate insufficient").

## Decision 5: MCP targets the 2026-07-28 stateless spec from commit one

Decision:

Client + (later) server built against the frozen RC of MCP 2026-07-28: stateless core, explicit state handles surfaced in receipts, `.well-known` discovery, `ttlMs` caching, OAuth 2.1. 2025-11-25 supported as fallback client only. Pin SDK version; conformance check in CI; don't ship nh-mcp server publicly until the final lands.

Why:

Incumbents must migrate off session-based MCP; a greenfield harness never carries that debt. Handles-in-receipts satisfies THE LAW's auditability.

Tradeoffs:

- Small spec deltas possible between RC and final (July 28) — pinned SDK + conformance suite absorbs them.

**Amendment 2026-07-24:** The "pin SDK version; conformance check in CI" mitigation never existed — the shipped implementation is SDK-free (server = `tiny_http`, `crates/nh-mcp/Cargo.toml`:15; client hand-rolled in `crates/nh-tools/src/mcp.rs`) and the CI added in `d1f9ad0` has no MCP conformance job — the actual posture is: SDK-free implementation, loopback-only + bearer-token server, hardened in Slice F W2, the Release MCP wave (`1d04871`), and Slice G W6a–c; the core of the decision held (stateless-native, handles in receipts, no public server before the final spec lands 2026-07-28) and must be re-verified against the final spec text before any public exposure — evidence: `crates/nh-mcp/Cargo.toml`:15, `crates/nh-tools/src/mcp.rs`, `.github/workflows/ci.yml` (`d1f9ad0`).

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

**Amendment 2026-07-24:** Naming drift only — the flag shipped as `preserve_when_thinking`, not `preserve_reasoning` — evidence: `crates/nh-routes/src/lib.rs`:308, :364 (added via contract amendment A-M5-1).

## Decision 8: Windows-first

Decision:

First-class native Windows 11 (crossterm, tested renderer matrix, Job Objects + restricted tokens), honest docs that full syscall sandboxing is Linux-only.

Why:

Both dev machines run Windows 11; incumbent sandboxes still don't support native Windows — a real wedge.

Tradeoffs:

- Weaker containment on Windows in v1, stated honestly rather than faked.

**Amendment 2026-07-24:** Windows process-tree containment for v1 is verified `taskkill /PID <id> /T /F` (Unix: `kill -KILL -<pid>` against a real process group) — capture the kill's exit status, poll `try_wait()` within a bounded verification grace, fall back to `child.kill()`, and if the child still has not reaped, report "could NOT be killed" honestly instead of claiming success; the originally promised kill-on-close Job Object was REJECTED because it requires raw Win32 calls, i.e. a new dependency plus `unsafe`, which breaks the workspace-wide `unsafe_code = "forbid"` lint shipped in commit `d1f9ad0` (2026-07-20) — ratified by the owner 2026-07-24 (Slice G Wave 7, decision R1), keeping this decision's own tradeoff line true to itself ("weaker containment on Windows in v1, stated honestly rather than faked"). Accepted residual risk (stated by the owner): "a grandchild that re-parents while taskkill walks the tree may survive — we REPORT that honestly instead of pretending we killed it." "Restricted tokens" remain UNRATIFIED: not implemented and with no ratified decision either way (the same forbid-unsafe reasoning likely applies; owner to confirm intent or strike the phrase). Revisit only via a deliberate owner decision to admit a vetted unsafe-bearing dependency for Windows containment (reopens the forbid-unsafe decision, `d1f9ad0`) — evidence: `C:\Users\capv2\AppData\Local\Temp\sol_wave7_prompt.txt`:119-127 (R1) and :20-23 (forbid-unsafe constraint); commit `d1f9ad0`; `Cargo.toml`:20-21.
