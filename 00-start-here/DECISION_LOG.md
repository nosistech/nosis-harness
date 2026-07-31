# Decision Log

Use this for fast decisions. Large technical decisions live in `../02-architecture/ARCHITECTURE_DECISIONS.md`.

## 2026-07-31: Price freshness becomes provenance, not a deadline

Decision:

Replace `valid_until` with `verified_on` in every `catalog.toml` price block. Delete the recheck
deadline and the `stale` boolean that hangs off it. Receipts disclose **when a human last verified
the price**, and let the reader judge. Keep the separate **fx-rate staleness refusal** exactly as it
is. Scheduled as wave M3 "NO DEADLINES".

Why:

The owner was tired of it, and he was right. `catalog.toml:3` describes `valid_until` in the
project's own words as "Nosis's short recheck deadline, not a provider guarantee" — prices verified
2026-07-26 expiring 2026-08-02, **a seven-day window** for providers that change published prices
perhaps two to four times a year. The cadence modelled volatility that does not exist, and the cost
landed entirely on one person.

`valid_until` is a promise about the future: it expires, and it demands action. `verified_on` is a
fact about the past: it never expires and never asks for anything. The receipt goes from
"prices stale" to "price verified 2026-07-26", which carries strictly more information than a
boolean — so this **strengthens** the honest-cost claim while removing the recurring obligation.

Rejected alternatives:

- Widen the window to 90 or 180 days — reduces the toil without removing it. The owner asked for
  removal, not for a longer leash.
- Delete `valid_until` and change nothing else. **This is a trap:** `resolver.rs:116` reads
  `price.valid_until.is_none_or(|d| at.date_naive() > d)`, so an absent date makes every quote
  permanently stale. Deleting the field pins the flag on rather than removing it, and "stale" would
  lose all meaning by appearing everywhere forever.
- A scheduled CI price-watcher that diffs provider pricing pages and opens an issue on change. Good
  design, and it was offered — but it exists to buy down the risk of a long window. With no window,
  there is nothing to backstop, so it became optional and was dropped. Recorded here because it is
  the right answer if freshness ever needs a guarantee again. It must never write prices into the
  catalog: `price_confidence = "confirmed"` means a person checked, and a scraper that could set it
  would make the word a lie.
- Remove price disclosure entirely — rejected. Honest metering is the product.

Consequences:

- Immediate: no calendar, no expiry, no recurring task. One catalog edit and the flag is gone.
- Long-term: freshness is disclosed and ages visibly instead of flipping on a date nobody chose
  deliberately. The residual risk is stated plainly: a silent provider price change could go
  unnoticed indefinitely, with a receding `verified_on` date as the only signal. Accepted knowingly.
- **Unchanged:** fx staleness still refuses. A stale exchange rate silently mis-converts CNY to USD
  and yields a confidently wrong number. Price provenance is disclosure; fx staleness is arithmetic.

Review later:

If anyone ever runs a business on nosis receipts, revisit the watcher.

## 2026-07-31: An intra-workspace path edge is not a new dependency

Decision:

`crates/nh-tools` may depend on `crates/nh-law` by path. The standing "do not add dependencies" rule
governs **third-party** crates and is otherwise absolute.

Why:

Wave M2's `glob_files` needed glob matching. The audited, iterative, stack-safe matcher already
existed at `nh-law/src/matcher.rs`, but `nh-tools` could not see it, and the wave brief forbade
manifest edits — an unsatisfiable pair that made the executor stop clean, correctly. The rule exists
to protect the supply chain and the `cargo deny` surface. An intra-workspace edge touches neither:
`nh-law` is already a path dependency of nh-cli, nh-fleet, nh-mcp and nh-tui, depends only on
anyhow, serde and toml — all of which nh-tools already had — and no external version changed in
`Cargo.lock`. No cycle exists; nh-law depends on no workspace crate.

Rejected alternatives:

- Copy the matcher into nh-tools — creates a **second security-relevant glob surface** to audit and
  keep in sync, which is exactly the failure mode the reuse item was written to prevent.
- Drop `glob_files` — removes a third of the wave to preserve a rule that was never aimed at it.

Consequences:

- Immediate: one line in one manifest; one `pub(super)` widened to `pub` with a doc comment
  recording that the matcher is iterative and `**` spans segments. Matching behaviour unchanged.
- Long-term: future waves may reuse in-tree crates freely. Every third-party addition still needs an
  explicit owner decision.

Review later:

Never. The distinction is the rule now.

## 2026-07-30: Image generation declined — the two-wire rule holds

Decision:

Do not add image generation. `nh` accepts images as input (wave M1, `05c53cc`) and does not produce
them. If it is ever revisited, the least-damaging shape is an `nh-mcp` tool, which leaves the
router's wire rule intact.

Why:

Of the four providers, only Z.ai can generate images (`glm-image` $0.015/image, `cogview-4`
$0.01/image) and only through `POST /api/paas/v4/images/generations`. That is **a third wire**,
which violates the ratified two-wire rule (OpenAI-compatible and Anthropic Messages). Worse, the
endpoint returns **no `usage` object at all**, so every generated image would have to be metered
from an assumed per-image price rather than a reported one — cost we would be **fabricating**, which
is the exact behaviour this product exists to refuse. Z.ai Terms of Use §III.5(d) additionally place
an **affirmative AI-labelling duty on the operator**, a compliance obligation the harness has no
mechanism to discharge.

Rejected alternatives:

- Add the generations endpoint as a third wire — breaks the two-wire rule for one provider and one
  feature, and the wire has no usage block to meter.
- Hard-code a per-image price and present it as measured — dishonest metering; refused outright.
- Hard-code the price and label it an estimate — still puts a number we did not receive into a
  receipt, next to numbers we did. Receipts stay one kind of thing.

Consequences:

- Immediate: no code, no catalog rows, no new dependency. Wave M1 shipped input-only.
- Long-term: the two-wire rule remains the load-bearing constraint that keeps the meter honest. Any
  future capability that needs a third wire must clear the same test — a real `usage` block, or it
  does not ship inside the router.

Review later:

Only if Z.ai adds a `usage` block to the generations response **and** the feature is worth a wire.

## 2026-07-30: MiMo off-peak 0.8× refuted — it is not available to us

Decision:

Do not implement a MiMo off-peak multiplier. `catalog.toml` has no `off_peak` key, so **it is
already correct by omission — change nothing.** Remove it from the backlog as refuted, not deferred.

Why:

The July research listed it as ready to build. Verification against first-party documentation killed
it. The 0.8× exists **only on the prepaid Token Plan**, as a Credits consumption coefficient — not a
pay-as-you-go discount. Both pay-as-you-go pages (English and zh-CN, dated 2026-07-15) contain zero
off-peak language. Worse, Token Plan quota is contractually **coding-tools-only** and expressly
forbids API use by automation scripts and application backends, which is precisely what `nh` is.
Implementing it would have written a **discount into the meter that we never receive**, understating
real spend — the single worst failure mode this product has.

Rejected alternatives:

- Implement it behind a flag for Token Plan holders — the plan's terms forbid our access pattern, so
  the flag would only ever be set by someone violating them.
- Implement it and label it an estimate — an estimate that systematically understates cost is worse
  than no feature.

Consequences:

- Immediate: no catalog schema change, no time-of-day logic in the pricer, no clock dependency.
- Long-term: reinforces the standing rule that a discount enters the meter only with first-party
  documentation **for our own billing relationship**, not for an adjacent product tier.

Review later:

Only if MiMo publishes off-peak pricing on a pay-as-you-go page.

## 2026-07-30: Kimi Batch API 0.6× refuted — not adoptable now

Decision:

Do not adopt the Kimi Batch API. Plausible **only** for fleet mode, and only after a live probe.

Why:

Two independent blockers. First, `completion_window` has a **12-hour minimum**, so every call is a
≥12h asynchronous job (upload → submit → poll → download → rejoin by `custom_id`). That rules out
`nh run` and `nh chat` categorically; they are interactive. Second, the documented batch `usage`
block has **no `cached_tokens` field**, while batch bills cache-hit ($0.10/1M) and cache-miss
($0.57/1M) **5.7× apart**. Cost could therefore only be guessed, which is a **REFUSE** condition for
this product — the harness declines to report a number it cannot derive.

Two traps recorded for anyone who revisits this:

- The 0.6× multiplier does **not** reconcile for `kimi-k2.6` cached input: published batch price is
  $0.10, not 0.6 × $0.16 = $0.096. Do not derive batch prices by multiplication.
- The pricing page lists `kimi-k2.7-code` as batch-eligible, while the API guide's normative warning
  says the model must be k2.6 or k2.5. The two first-party sources disagree; a live probe is the
  only tiebreak.

Rejected alternatives:

- Adopt for `nh run` with a progress spinner — a 12-hour floor is not a spinner, it is a different
  product.
- Adopt and meter cache-hit optimistically or pessimistically — a 5.7× spread makes either choice a
  fabrication.

Consequences:

- Immediate: no batch client, no job store, no polling loop, no `custom_id` rejoin logic. Nothing
  built.
- Long-term: the fleet path keeps batch as a genuine future option, but the entry condition is now
  written down — a live probe that shows a `cached_tokens` field, or it stays out.

Review later:

If Moonshot adds `cached_tokens` to the batch usage block, or shortens `completion_window`.

## 2026-07-30: Verified leads, not research specifications

Decision:

Treat `00-start-here/RESEARCH_2026-07_harness.md` (~90 items, 10 tiers) and the 14 raw files in
`04-research/_harness-research-2026-07/` as **July-2026 leads**. Verify every item against
first-party documentation **and** a live probe before briefing it to an executor.

Why:

Three items were taken to verification on 2026-07-30. **Two were refuted outright** (MiMo off-peak,
Kimi batch, both above) and the third — multimodal — needed five corrections before it was safe to
build, including one catalog claim (`mimo-v2.5-pro` modality) that was false and **protected by a
green test**. A 2-in-3 refutation rate on items marked ready-to-build is the evidence.

Rejected alternatives:

- Brief research items to Sol directly — two of three would have produced shipped code implementing
  something that does not exist.

Consequences:

- Immediate: Tier 2 is marked stale (it assumes auto-routing, which the harness does not do). Tier 4,
  5 and 8 remain unbuilt but unverified.
- Long-term: verification cost is now part of every wave's budget, not an optional preflight.

Review later:

Never. This is a standing rule.

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
