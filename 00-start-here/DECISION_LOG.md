# Decision Log

Use this for fast decisions. Large technical decisions live in `../02-architecture/ARCHITECTURE_DECISIONS.md`.

## 2026-07-31: A request timeout is never retried, because it may hide a billed response

Shipped as `76cbb54`, wave M4 "RESILIENCE". This is the decision that shaped the whole wave.

Decision:

Retry only on (a) transport failures that are **not** timeouts, and (b) HTTP 429, 500, 502, 503,
504. Never retry a request timeout. Never retry 400/401/403/404 or any other 4xx. Never retry a 2xx
whose body fails to read or parse.

Why:

A status code is **evidence about billing**. When a provider answers 429 or 503, it answered and
produced no completion, so that attempt cost nothing and retrying is free of double-charge risk. A
timeout is evidence of nothing at all: the 600-second request ceiling can expire while the provider
has already generated — and billed — a full response that never reached us. Retrying then charges
the user twice while the receipt reports one completion.

That asymmetry is the entire argument. This project's claim is an honest meter, and a silent
double-charge is precisely the failure the meter exists to prevent. A retry that *might* have been
paid for twice is worse than a failure the user can see, because the user cannot audit it. The same
reasoning excludes a 2xx that fails to parse: a 200 means we were billed, so the honest move is to
surface the parse failure, not to buy a second copy.

Rejected alternatives:

- Retry timeouts like any other transport failure. Recovers more real-world flakiness — the common
  choice in HTTP client libraries, which are not metering anything. Rejected: it trades an invisible
  monetary error for a visible one, in the wrong direction.
- Retry a timeout only when no usage block was seen. Unworkable: a timeout means no response body
  arrived at all, so there is nothing to inspect. The absence of evidence is the problem.

Consequences:

- Immediate: a genuinely hung connection still costs the user the full 600-second ceiling and one
  manual retry. Accepted knowingly.
- The reasoning is preserved as a comment beside `is_retryable` in `nh-core/src/wire/retry.rs`, not
  only here, because the next person to read that match arm will otherwise "fix" the omission.
- Long-term: this is the template for any future retry surface (fleet, MCP egress). The question is
  never "did it fail?" but "does the failure prove we were not billed?"

## 2026-07-31: Retry budget is four attempts and 45 seconds, with no configuration knob

Decision:

Four attempts maximum (one initial plus three retries), 2-second exponential backoff doubling to a
20-second per-delay cap, full jitter in `[0.5, 1.0]`, a hard 45-second total retry budget, and
`Retry-After` honored in its **delta-seconds form only**, clamped to the per-delay cap. No CLI flag,
no profile setting, no environment override.

Why:

The one piece of live evidence is the 2026-07-30 image probe, where free `glm-4.6v-flash` needed
four manual retries at 6/12/24/48s. A ladder sized to fully cover that case would freeze an
interactive turn for ninety seconds with no in-flight notice, which is a worse first-run experience
than an honest failure. 45 seconds bounds the silent wait while still covering the common transient
429. Where a provider sends `Retry-After`, its number beats our guess.

The HTTP-date form of `Retry-After` is ignored rather than parsed, because parsing it means either a
new dependency or a hand-rolled date parser, and the delta-seconds form is what these providers
actually send.

Rejected alternatives:

- Match the observed GLM ladder (5 attempts, ~90s). Recovers the free-tier on-ramp more often; costs
  a minute and a half of silent interactive freeze. Reconsider once the live working heartbeat
  exists — the tradeoff flips when the wait is visible.
- Add a config knob now. More surface across profiles, CLI and docs in a wave otherwise contained to
  one crate, to tune numbers nobody has field data for yet. Deferred, not refused.

Consequences:

- **Accepted tradeoff:** under a 45-second budget the observed GLM ladder only fully recovers if GLM
  sends `Retry-After`, which is unverified.
- The budget counts measured attempt time as well as sleeps, so a provider that answers 503 slowly
  consumes its own budget and effectively gets fewer retries. Defensible — it bounds total
  wall-clock — but it is not what "45 seconds of retry budget" sounds like, so it is written down.
- No in-flight "retrying in 6s" notice exists: the wire clients have no progress sink, and threading
  one through `credential.rs` would touch every frontend. The hard budget is what bounds the silence.

## 2026-07-31: Wave M4 is retry only — failover, re-resolve and cooldown stay unbuilt

Decision:

Ship bounded retry and nothing else from research Tier 4. No availability re-resolve to the cheapest
capable route, no provider cooldown or circuit breaker.

Why:

Retry is contained to `nh-core`'s wire layer and reviewable in one sitting. Availability re-resolve
pulls in `nh-routes` and the resolver and changes **which provider gets the user's data and money**
on failure — a different question, with a privacy dimension, that deserves its own ratification. One
wave, one concern, is also the shape that has produced every clean executor run on this repo.

Rejected alternative: implement all three together, as the research groups them. Rejected for review
surface, not for merit; all three remain High-value leads.

Consequences: a dead provider still ends the turn after the retry budget. The research's Tier-4
"availability re-resolve" and "provider cooldown" rows are unchanged and remain the next candidates.
Note the related standing decision that **the harness does not auto-route**, which constrains what
re-resolve is allowed to do without asking.

## 2026-07-31: A defect found in review is fixed before the commit, not after it

Decision:

Wave M4b — the jitter-domain fix and the error-wording fix — was squashed into the M4 commit rather
than landing as a follow-up `fix:` commit. History records one correct wave.

Why:

The M4b work corrected code that had never been published. Committing the defect first and the fix
second would have put a backoff that silently ran at half its ratified length into permanent
history, for the sole benefit of preserving a review narrative that the `BUILD_LOG` already records
in full. Nobody was ever exposed to the bug.

The bug is worth recording even though it never shipped, because of *how* it survived: `next_delay`
divided its jitter sample by `u32::MAX`, while both production callers supplied
`SystemTime`-derived `subsec_nanos`, bounded at 999,999,999 — about 23% of that divisor. The
specified `[0.5, 1.0]` jitter span was really `[0.5, 0.616]` in production. **The unit tests were
green because they passed `u32::MAX`, a value the real caller can never produce.** The pure function
and its only caller disagreed about the domain, and nothing asserted the caller's side.

Rejected alternative: two commits preserving the exact review sequence. Rejected — the `BUILD_LOG`
keeps both wave entries, so nothing about the review is lost.

Consequences:

- The general lesson, now the third instance of the repo's own rule that a passing test proves
  consistency rather than truth: **a pure function tested only at values its real caller cannot
  supply is not tested.** The fix therefore names the domain (`JITTER_SCALE`), collapses the
  duplicated inline closure into one `system_jitter()` source, and adds the assertion that the
  *source* stays inside the domain — the check whose absence let this through.
- An out-of-domain jitter value now trips a `debug_assert` rather than being clamped, so a future
  contract mismatch is exposed instead of silently distorted. Note this compiles out in release; the
  domain test covers the only production caller.
- `RetryExhausted` remains the terminal error for non-retryable failures, because uniform receipt
  recovery is worth it, but its `Display` renders the bare provider failure when `attempts == 1`. A
  rejected API key reads exactly as it did before wave M4 instead of gaining an "after 1 attempt
  over 0.4s" preamble in front of the one actionable sentence.

## 2026-07-31: Route prices do not expire — the freshness apparatus is deleted

**This entry supersedes the `verified_on` design drafted earlier the same day. `verified_on` was
never implemented.** The owner, asked to choose, rejected a per-route date as well: any date in a
file he owns still reads as something to keep current, and the whole point was to stop being pulled
back to it. Shipped as `223217a`, wave M3 "PRICES DON'T EXPIRE".

Decision:

Delete `valid_until` from all 12 route-price blocks and add nothing in its place. Delete
`PriceQuote.stale`, the staleness computation, the catalog parsing, the `stale` parameter on
`usd_compare_key`, all six `*price stale` display markers, and the `stale` member of the MCP
`route_cost` payload. **Prices themselves are untouched** — metering, receipts, `nh why` and
cheapest-capable selection all work exactly as before. Keep `price_confidence` and the per-route
first-party citation comments: static claims that never expire and cost nothing. Keep the **fx-rate
staleness refusal** exactly as it is.

Why:

`catalog.toml:3` described the key in the project's own words as "Nosis's short recheck deadline,
not a provider guarantee" — prices verified 2026-07-26, expiring 2026-08-02, **a seven-day window**
for providers that change published prices perhaps two to four times a year. The cadence modelled
volatility that does not exist, and the entire cost landed on one person, who said plainly that it
would keep dragging him back to the product.

The deadline also lived in **process, not just code**: `RELEASE_CHECKLIST.md:73` made rechecking
prices a release blocker, and `PROMPT_LIBRARY.md:21` instructed future research agents to record a
`valid_until` for every route — which would have quietly rebuilt the machinery after it was deleted.
Removing the field without sweeping the documents would have removed the enforcement and kept the
obligation.

Rejected alternatives:

- `verified_on`, a provenance date replacing the deadline. Genuinely better than an expiry — a date
  carries more information than a boolean — but still a per-route date in a file the owner owns.
  Rejected by the owner after being drafted and briefed.
- Widen the window to 90 or 180 days — reduces toil without removing it.
- Delete `valid_until` and change nothing else. **A trap:** `resolver.rs:116` read
  `price.valid_until.is_none_or(|d| at.date_naive() > d)`, so an absent date made every quote
  permanently stale. Deleting the field alone would have pinned the flag ON.
- A scheduled CI watcher diffing provider pricing pages. Good design, offered, dropped: it existed
  to backstop a long window, and with no window there is nothing to backstop. Recorded because it is
  the right answer if freshness ever needs a guarantee again — and it must never write prices into
  the catalog, since `price_confidence = "confirmed"` means a person checked, and a scraper that
  could set it would make the word a lie.
- Remove prices entirely — never on the table. Honest metering is the product.

Consequences:

- Immediate: no calendar, no expiry, no recurring task, in code or in process. The
  release-checklist item now states in bold that it can never block a release again.
- **Accepted tradeoff, stated plainly:** receipts carry no freshness signal at all, so a silent
  provider price change is metered wrong until a human notices and edits `catalog.toml`. This is a
  real regression in the honesty story and was chosen knowingly, three times, in exchange for never
  maintaining a price calendar.
- A test now pins that a price block with no freshness key loads normally, so an expiry cannot be
  reintroduced by accident.
- **Unchanged:** fx staleness still refuses. An old price is a number a reader can judge; an old
  exchange rate silently mis-converts CNY to USD and yields a confidently wrong number the reader
  cannot judge at all. Different failure, different answer. It costs nothing to keep — there is no
  `[fx]` block in `catalog.toml`, so the path is dormant, held for a future CNY route.

Review later:

Only if someone starts running a business on nosis receipts.

## 2026-07-31: Superseded — price freshness as provenance (`verified_on`)

Superseded the same day by the entry above, and recorded because the reasoning is still sound and
the trap it documents is still real. The design: replace `valid_until` with `verified_on`, so
receipts disclose when a human last verified a price instead of flagging a boolean. `valid_until` is
a promise about the future that expires and demands action; `verified_on` is a fact about the past
that never does. It was briefed to the executor, which correctly refused to backdate one route's
provenance (Kimi K3 was verified 2026-07-28, not 2026-07-26) — a reminder that a brief written from
reading can be wrong about facts the file already records. The owner then rejected per-route dates
outright.

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
