# Decision Log

Use this for fast decisions. Large technical decisions live in `../02-architecture/ARCHITECTURE_DECISIONS.md`.

## 2026-07-29: Provider scope frozen at four open-weight providers plus local

Decision:

Support DeepSeek, Kimi, GLM, and MiMo — the four with working keys — plus local models through a
loopback OpenAI-compatible runtime. Do **not** add Anthropic, OpenAI, or Gemini API routes. Keep the
commented `class = "delegate"` stubs (`catalog.toml`), which drive a subscription child CLI rather
than a metered API route and therefore need no price verification.

Why:

Three independent research strands converged. Without keys, any frontier route ships unverified
prices, violating the catalog's own `price_confidence` rule. Anthropic and GPT-5.6 bill cache
*writes* (1.25–2×) and Gemini 3.1 Pro uses two-tier long-context pricing — **neither is expressible
in the `cache_hit`/`cache_miss`/`output` price schema**, so their cost could not be metered honestly.
Competitive analysis found the credibility gap was missing *local* support, not missing frontier.
Market analysis sized the open-weight segment at 61% of OpenRouter token consumption (Feb 2026) with
programming above 50% of platform tokens, served today by hacked competitor configs and no neutral
honestly-metered harness.

Rejected alternatives:

- Add frontier API routes — unverifiable prices, and the schema cannot express cache-write billing.
- Add frontier *price rows* only, as a `top_tier` anchor without routes. Verified at
  `resolver.rs:228-238` that one USD row would inflate every savings counterfactual 3–6×. Rejected as
  self-serving: manufacturing a larger savings number is the exact behaviour this product refuses.

Tradeoffs:

- Capability ceiling ≈82.6 vs ≈96 SWE-bench Verified. Never claim best results; claim best honesty
  about results.
- Loses users who want the harness to drive an existing Claude or GPT subscription.
- Savings counterfactuals stay honestly small.

Review later:

Only if a schema change for cache-write pricing is independently justified.

## 2026-07-29: Zero-price routes are a selectable tier, not a routing winner

Decision:

Add `class = "local"`. Local routes are explicitly selectable via `--model` and `/model` but excluded
from `resolve_capable`, cheapest-capable recommendation, provider defaults, automatic escalation, and
the `top_tier` cost anchor. Meter copy, fixed: `Local: no billed tokens; hardware and power are not
metered.` Local routes are additionally confined to the OpenAI wire and a loopback origin.

Why:

`resolve_capable` selects the cheapest fitting route, so a `0.0` route wins every comparison by
construction — `/why` would answer "local" every time and the skip-ladder would become a formality.
Two independent research strands recommended presenting local as a degraded tier. Separately, `$0.00`
is true in dollars and false in cost: electricity, VRAM, and latency are real and unmeterable here,
so the meter states that rather than implying free.

Rejected alternatives:

- Let local win cheapest-capable. Honest in dollars, misleading in substance.
- Model a quality axis so cheapest-capable means something. Rejected — it requires benchmark claims,
  and 0 of 104 small-model SWE-bench figures surveyed were independently verified.

Tradeoffs:

- A user wanting cost-minimal routing must select local explicitly.
- The same zero-price problem already exists for the free GLM tier, which *does* win cheapest-capable
  after the wave-1 `context` fix. That is deliberate (it is a real metered provider route) but it
  degraded the price ladder — see wave 4 item W4-6.

Review later:

Yes — revisit if a defensible non-benchmark quality signal appears.

## 2026-07-29: The "router inside the harness" moat claim is retired

Decision:

Retire the structural-moat claim. The defensible position is the **honest-meter bundle**: dated
first-party-verified prices with refuse-on-stale, append-only local receipts from provider-reported
usage, a keyless `why` explaining every skipped route, wire-correct open-weight dialect handling, and
metering exposed as MCP tools. Add "only router inside a harness", the unshipped `saved 93%` line,
"incumbents cannot show cost", off-peak fleet claims, and Linux/macOS support to the do-not-claim
list.

Why:

Falsified from both directions. Externally, AWS Kiro shipped "Auto", a cost router inside a harness,
and cache-aware/phase-aware routing proxies are commoditising. Internally and decisively,
**this harness does not auto-route at all**: `resolve_capable` is called only from `cmd_why.rs:51`,
TUI `input/commands.rs:190`, and MCP `route_tools.rs:101` — never from the agent execution path. The
README was already honest ("a harness with a meter, not an automatic router"); the moat claim was not.

Tradeoffs:

- Defensibility is moderate, not structural — the bundle is copyable in code. The real defence is
  that incumbent subscription/credit/markup business models disincentivise per-turn price honesty.
- `01-product/WHY_BEST_IN_CATEGORY_2026.md` still contains the retired claim plus a stale
  Anthropic-June-15 credit-split claim (announced, then paused, not shipped) and needs a
  re-derivation pass before any launch post is written.

Review later:

Yes — before writing launch posts.

## 2026-07-29: DeepSeek Anthropic-wire routes removed from the catalog

Decision:

Remove `deepseek-v4-flash-anthropic` and `deepseek-v4-pro-anthropic`. Retain
`crates/nh-core/src/wire/anthropic.rs` and the `Wire` enum unchanged.

Why:

Owner FEEL finding — four DeepSeek rows in the `/model` picker where two would do, and the
`-anthropic` variants are indistinguishable to a user from the models they duplicate. They were
wire-parity routes, not user choices. Research advised against adding Kimi's or GLM's
Anthropic-compatible endpoints (that client drops reasoning content, violating Kimi's replay
contract), so no other first-party Anthropic-wire route is planned.

Tradeoffs:

- The Anthropic wire client becomes unused capability with no reachable route. Retained deliberately:
  deleting it means touching the `Wire` enum, catalog schema, `effort_for`, dialect handling, and
  their tests — a multi-file refactor rejected this close to release.
- Removes the pending live probe about whether that endpoint silently downgrades `deepseek-v4-pro`
  to flash.

Review later:

Yes — decide post-v0.1.0 whether to delete the Anthropic client entirely.

## 2026-07-26: Remote notifications removed from public v0.1

Decision:

Remove Telegram configuration, credential access, sender thread, HTTP code, tests, and TUI
dependencies. Keep the local approval bell. Leave remote notifications open only as a future,
separately reviewed explicit opt-in integration.

Why:

Telegram is not part of the harness's central invariant. The implementation added credential,
destination, privacy, thread, dependency, and outbound-network surface; its strongest walk-away
justification was not wired to headless Fleet, and the real send remained unverified.

Tradeoffs:

- Public v0.1 has no phone alert for unattended work.
- Reintroduction requires a fresh owner decision and security review, preferably behind an isolated
  adapter/plugin boundary.

Review later:

yes — only when a concrete walk-away workflow justifies the attack surface.

## 2026-07-13: M1 contract amendments ratified with orchestrator authority

Decision:

CONTRACTS_M1.md froze the M1 public surface with amendments through the architect only, but four integration/hardening deviations were ratified with orchestrator authority instead and recorded in CONTRACTS_M1.md §7: keyless `nh chat` startup (warning + REPL, exit 0 on `/quit`), the `peak <multiplier>x until HH:MM` footer format, nh-tools crate-root re-exports of the MCP items, and `nh run --think none|low|high|max` with per-dialect defaults.

Why:

The gaps surfaced after the architect's pass finished; blocking a green integration to re-convene the architect adds process without value (THE LAW: simple). All four are additive-only and written into the contract itself, so the frozen-surface audit trail stays intact.

Tradeoffs:

- Two ratification authorities for one contract — rule: orchestrator amendments must be additive-only and land in §7 with a date.

Review later:

no

## 2026-07-12: ProjectStarterTemplate adopted as project OS

Decision:

Adapt the standard NosisTech starter template into this folder; Master Plan stays at root as canonical spec, folders are the working layer.

Why:

Explicit context per session; every agent (Claude/Codex/Opus) gets the same read order.

Tradeoffs:

- Two places to update (plan vs working docs) — rule: docs point to plan sections instead of duplicating them.

Review later:

no

## 2026-07-11: Implementer re-pointed to GPT-5.6

Decision:

Build-loop implementer = GPT-5.6 Terra by default; Sol (max effort, ultra mode) for M2 context engine and anything touching nh-law/security. "Codex 5.5" references are legacy.

Why:

GPT-5.6 GA July 9; OpenAI guidance: Terra succeeds GPT-5.5-class work at half price. `gpt-5.2*`/`gpt-5.3-codex` deprecated under ChatGPT sign-in.

Tradeoffs:

- Requires updated Codex CLI binary or 5.6 won't appear.

Review later:

yes — when quotas or a newer family land.

## 2026-07-11: nh-vault lands in M0, not later

Decision:

Key vault crate ships in the first milestone.

Why:

Keys exist from the first commit, so security must too (THE LAW: secure). Plan §A.8/§A.10.7.

Review later:

no

## 2026-07-09: Route A — greenfield, not CodeWhale fork

Decision:

Original IP, 5 providers, patterns-not-code from CodeWhale.

Why / tradeoffs:

See `../02-architecture/ARCHITECTURE_DECISIONS.md` Decision 1.

Review later:

no
