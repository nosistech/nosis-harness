# Decisions — M4 + M5 Slices A–E (2026-07-15 → 2026-07-18)

Era file: M4 (fleet + swarm seam + scheduler + nh-mcp) and M5 Slices A–E ("The Honest Meter", pre-hardening). 14 entries, newest-first, bodies verbatim from the source draft.
Index: [`DECISION_LOG.md`](../../00-start-here/DECISION_LOG.md) (`00-start-here/`). Large technical decisions also carry numbered entries in [`ARCHITECTURE_DECISIONS.md`](../../02-architecture/ARCHITECTURE_DECISIONS.md).
Source abbreviations used in entries: `00-start-here/BUILD_LOG.md` (BL), `CONTRACTS_M4.md` (C4), `CONTRACTS_M5.md` (C5), `00-start-here/RESEARCH_2026-07_harness.md` (R), `04-research/_harness-research-2026-07/fable_K_product_cohesion_2026-07.md` (K).

---

## 2026-07-18: Slice E pulled forward piecemeal — formatting becomes the gate's job, not the builder's

**Decision:** Ship the M5 Slice E "LOOP" hygiene items early and mechanically, in two dedicated
commits: (1) a one-time `cargo fmt --all` normalization of the whole workspace plus a new `gate.ps1`
that mechanizes the three checks defining "clean" (`fmt --check`, `clippy -D warnings`,
`test --release`) with per-step exit-code aggregation (`68f71cd`); (2) `rust-toolchain.toml` pinning
1.96.0 (+ rustfmt/clippy), an explicit `.gitattributes` EOL policy, and a dormant `deny.toml`
supply-chain policy (`059a00e`). Standing process rule established at the same close: the code
implementer (Sol) must never run `cargo fmt` — formatting is now exclusively the gate's job
(`6a11f32` commit message).
**Alternatives considered:**
- Keep scoped `cargo fmt -p <crate>` inside each slice — rejected because the workspace was never
  fmt-clean, so any scoped fmt reflowed pre-existing code and polluted slice diffs; this "bit Slice A
  and Slice D" and had accumulated a 37-hunk / 7-file backlog (`68f71cd` commit message; the Slice A
  commit `68f91e6` records having to revert frozen crates "to fmt-clean HEAD").
- Piping gate output through `| tail` — rejected because tail's exit code 0 would mask a real failure
  (`68f71cd` commit message: "never `| tail`, whose 0 would mask a real failure").
**Why:** Reproducibility and diff-cleanliness for a multi-agent build loop where a different model
writes the code than gates it (THE LAW: auditable, simple — the CONTRACTS_M5 Slice E rationale is
that "the M4 finale nearly lived only in Temp", C5 §Slice E, line ~493). The pin exists so "a future
rustfmt can't silently re-introduce the reflow drift" (`059a00e` commit message).
**Immediate effect on the harness:** No crate source behavior changed (both commits are
behavior-preserving; gate re-verified 357/0/1 `--release`, clippy clean after the normalize,
`68f71cd`). The build loop gained a single authoritative pass/fail command.
**Long-term consequence:** Every subsequent wave (all of Slice F, the Release Slice) is gated by
`gate.ps1`; the "Sol never runs fmt / orchestrator runs the normalizing fmt post-run" division shows
up in every later BUILD_LOG entry (e.g. BL:314-316, BL:174-176). Not everything in the Slice E spec
shipped here: `deny.toml` was dormant ("cargo-deny is not installed yet", `059a00e`), and
cargo-nextest + the AV canary + CI were deferred — CI/cargo-deny activation landed later in the
Release Slice Section B (`cccb2dc`, other writer's era), nextest/AV-canary went to backlog (BL:77-78).
**Evidence:** commits `68f71cd`, `059a00e`, `6a11f32`; C5 §Slice E (lines ~491-509); C5 §0.1
"Repo tooling (Slice E)" (line ~111); BL:404 (357/0/1); Slice A commit `68f91e6` fmt note.
**Article angle:** A two-model build loop forced the project to make formatting a mechanical gate
step because letting the implementer format kept contaminating auditable diffs.
**Review later:** no (the pitfall is recorded as RESOLVED; the deferred CI/nextest items were picked
up in a later era).

## 2026-07-18: Slice D LEVER — profiles clamp exactly two levers on the chosen route; `balanced` must equal prior behavior byte-for-byte

**Decision:** Ship execution profiles (`frugal` / `balanced` / `max-quality`) as a `profiles.toml`
layered bundled→user→repo like law, where a profile clamps only **thinking tier + output cap** on the
route the user already chose; a repo profile may only tighten, never loosen; `balanced` reproduces
pre-profile behavior exactly. Route *selection* by profile and any currency session hard-stop were
explicitly held out. The effective profile is recorded on every receipt and shown as a HUD chip, and
the displays show the route-RESOLVED effort (`none/low/high/max`), not the abstract posture.
**Alternatives considered:**
- Profiles that also pick the route — deferred to the M6 auto-router by owner-ratified design call
  ("route *selection* by profile → M6 auto-router", C5 §8 A-M5-7).
- A currency session hard-stop inside Slice D — rejected for D ("No currency session hard-stop in D
  (held to a separate lever / M6)", C5 §8 A-M5-7).
- Displaying the abstract posture in `nh profile`/`/profile` — rejected during the slice: displays
  were refined to the route-resolved effective effort because "the meter must not lie on
  always-/no-thinking routes" (`2564476` commit message; BL:394-409).
**Why:** One owner of every user-selectable cost lever ("The single owner of every user-selectable
cost lever — no cost knob lives anywhere else", C5 §Slice D, line ~482); tighten-only repo layering
reuses the law model (THE LAW: congruent). Clamps honor route floors/ceilings so "an always-thinking
route can't be silenced, a no-toggle route can't be forced to think" (`2564476`).
**Immediate effect on the harness:** New `nh-routes::profiles` module (`Profiles`,
`EffectiveExecutionPolicy`, `ThinkingPosture`), `nh-core::resolve_effort`,
`Receipt.effective_profile` + `AgentLoop.profile` (additive, old receipts still parse), `--profile`
on run/chat/tui, keyless `nh profile`, `/profile` live switch + HUD chip. Frozen nh-fleet gained
exactly the pre-authorized compile glue (`profile: None`, A-M5-7 + addendum). Gate 357/0/1
`--release`, clippy clean; FEEL-approved by the owner (`2564476`).
**Long-term consequence:** E4 exit met (frugal↔max-quality produce different built bodies on the
same route, pinned by tests); the receipt-carried profile becomes the audit trail for "which lever
was active when this money was spent". The M6 auto-router inherits a clean seam. The A-M5-7 addendum
also created the "blanket glue" precedent: trivial exhaustive-literal ripples of an authorized field
addition no longer force a stop (C5 §8 A-M5-7 addendum, "Blanket" paragraph).
**Evidence:** commit `2564476`; C5 §Slice D (lines ~472-487) + §8 A-M5-7 and addendum (lines
~658-705); BL:378-416; docs-close `6a11f32`.
**Article angle:** The cost knobs were deliberately reduced to two clamps on an already-chosen route,
with the default profile contractually required to change nothing.
**Review later:** yes — when the M6 auto-router takes over route selection and the deferred currency
hard-stop lever is designed (both named as M6 in A-M5-7).

## 2026-07-18: Slice C VISIBLE — native currency is the billed truth; USD is an ≈gloss that only prints on fresh FX; the savings headline baseline is no-cache

**Decision:** Build the money HUD + counterfactual savings line + `/why` under a set of ratified
honesty rules (amendment A-M5-6): the native currency (mostly CNY) stays the billed source of truth;
an approximate USD gloss (`¥0.11 (≈$0.02)`) prints only when the catalog `[fx]` rate is fresh, is
always ≈-marked, and is never used to sum across currencies (per-currency subtotals only); a
stale/absent rate means the gloss is omitted, never guessed. The savings headline baseline is
**no-cache** (same model, zero cache); peak and top-tier are breakdown context, never the headline.
Adaptive precision means a real sub-cent spend never renders as $0.00. The approval cluster became
y/a/n/Esc-only with any other key a no-op, and "Esc to stop" while working was dropped rather than
shipped as a false claim.
**Alternatives considered:**
- FX-summing sessions into one USD number — prohibited by the ratified rule ("sessions **never**
  FX-sum across currencies... the gloss is display, never a summation basis", C5 §8 A-M5-6).
- Peak or top-tier as the savings headline — rejected; ratified FEEL call fixed the headline to
  no-cache as "the honest 'our caching saved you N%'" with top-tier/peak as breakdown only (C5 §8
  A-M5-6, "Ratified FEEL/format calls").
- Shipping "Esc to stop" while working — dropped under drop-if-hard because there was "no truthful
  cooperative cancel path... rather than claim an interrupt that does not work" (`a0f77be` commit
  message).
- `/profile` + profile HUD chip in Slice C — held to Slice D because the `Profiles` module didn't
  exist yet (C5 §8 A-M5-6, last paragraph).
**Why:** "The meter-must-not-lie invariant governs it" (C5 §8 A-M5-6) — the gloss exists because a
Western user "has no gut feel for ¥", but honesty rules cap what the convenience may claim. The
counterfactual line is the product's identified 60-second aha, honest by construction because both
numbers come from the same catalog price data and the same JSONL token counts (K §2, items 1-2).
**Immediate effect on the harness:** New `[fx]` catalog block + `Fx` type, pure
`cost_of`/`naive_cost`/`saved_pct` in nh-routes, the money HUD replacing the token-only line, `/why`
+ `nh why` backed by Slice A's `RejectionTrace`, the L6 approval fix, working heartbeat, OSC 9;4
taskbar semáforo. Gate 339/0/1 `--release`; FEEL-approved and live-verified with a real GLM key
(`a0f77be`; docs-close `213ed0a`).
**Long-term consequence:** These exact rules were later verified live as launch evidence
(cross-currency refusal, usd_approx-on-fresh-fx — BL:24-39, other writers' era) and were extended by
Slice F W3 into the resolver (stale FX → refuse to compare). The savings line is the standing
launch-screenshot asset (K §2.3).
**Evidence:** commit `a0f77be`; C5 §Slice C (lines ~443-468) + §8 A-M5-6 (lines ~639-656); K §2
(lines 44-61); docs-close `213ed0a` ("FEEL-approved + live-verified with a real GLM key. Gate 339
pass / 0 fail").
**Article angle:** The team wrote formal rules for when a currency conversion is allowed to appear
at all, and deleted a keyboard hint rather than ship an interrupt that didn't work.
**Review later:** no (the rules were subsequently hardened, not revisited).

## 2026-07-18: Slice B FLOOR — law is the only trusted source of credential audiences; a repo checkout can never grant one

**Decision:** Close the credential-redirect hole with an audience broker (`get_scoped`) that refuses
before a secret materializes, and make **law (bundled/user layers only)** the trusted source of an
entry's approved audience hosts — a repository's law/config cannot add or widen an audience
(amendment A-M5-5, owner-ratified 2026-07-18, reusing the existing `repo_tries_to_weaken` layering
"exactly as `write.auto` is repo-refused"). In the same slice: `Access::Read`/`Send` law classes with
the read guard live on every path including the unattended fleet path (A-M5-4), nh-mcp made
fail-closed (default-minted OS-seeded bearer token + strict loopback Host/Origin), tool results
bounded by `ToolResultEnvelope`, min-env exec allowlist, scrubber shape widening, OAuth RFC 8707
`resource`, and MCP description sanitize-only.
**Alternatives considered:**
- Catalog or `.nosis/mcp.toml` as the audience source — rejected because both are repo-controlled;
  the hole being closed is precisely that "`find_catalog` walks up to a repo-controlled
  `catalog.toml`; `.nosis/mcp.toml` is repo-controlled" (C5 §8 A-M5-5).
- Wiring a live `Access::Send` consult into the MCP adapter in M5 — deferred: "The **broad** egress
  consult site is M6's privacy-router... M5 wires no live `Access::Send` producer into the MCP
  adapter" (C5 §8 A-M5-5; the mechanism shipped, the broad producer waited — Slice F W2 later wired
  it, other writers' era).
- MCP TOFU/hash-pinning in M5 — deferred to M7 by the M5 scope ruling (`extensions.lock` does it
  provenance-wide; M5 keeps only sanitize — C5 header ruling 3a).
**Why:** The Lethal-Trifecta read leg and the credential-audience exfil were two of the ~12 live
issues the July research surfaced (BL:466-469); "a repo checkout can no longer redirect a real vault
credential to an attacker origin" (C5 §Slice B, L4). Host-only comparison was chosen so DeepSeek's
dual wires satisfy one audience entry, pinned by a test (C5 §8 A-M5-5, last paragraph). THE LAW:
secure, congruent (reuses the law layering instead of a new trust mechanism).
**Immediate effect on the harness:** `read_file` consults the guard before I/O; bundled law blocks
env files/keys/certs from reads; unauthenticated or cross-Origin `fleet_run` refused ("it spends
money"); gate 319/0/1 `--release` (`edfcd62`; BL:418-449).
**Long-term consequence:** The broker became the seam Slice F W1 hardened (url-crate host parity,
fail-closed undeclared entries) — the era that followed built on this floor rather than redesigning
it. The deferred broad-egress consult and privacy-router remained M6 commitments.
**Evidence:** commit `edfcd62`; C5 §Slice B (lines ~411-439) + §8 A-M5-4 (lines ~598-613) and
A-M5-5 (lines ~615-637); BL:418-449; docs-close `918989a`.
**Article angle:** The trust question "who may say where a credential is allowed to go" was answered
with the same layered-law machinery that already refused repo-granted write autonomy.
**Review later:** yes — the M6 privacy-router is the named owner of the broad egress consult site
(C5 §8 A-M5-5).

## 2026-07-17→18: Slice A TRUTH — the wire must match the display, compaction must not break the cache, and enum ripples get pre-authorized instead of improvised

**Decision:** Fix the meter-math so every number and the routing choice is provable: explicit
`thinking:{type:disabled}` for None/Low on disable-capable dialects (instead of omission, which the
provider auto-escalated — a cost bug), a new `kimi-toggle` dialect, reasoning replay conditional on
effective thinking state, compaction that appends the elision note as a NEW message so the retained
prefix stays byte-identical (cache HIT, not a ~120× miss), `PrefixSeal` enforced in all builds,
output caps on both wires, and the thin honest resolver `resolve_capable` + `RejectionTrace`
(cheapest context-fitting route with an auditable per-route skip trace; explicitly no jurisdiction,
no learning — M6). Process decision embedded in the same slice: three amendments (A-M5-1/2/3)
formalized the pattern that a public enum/field addition's compile ripple into frozen crates is
enumerated and pre-authorized as behavior-preserving glue, and that adversarial-review regressions
get their own amendment with a failing test as proof.
**Alternatives considered:**
- Letting Sol either break scope or duplicate parsing when the brief hit un-enumerated catalog
  schema — rejected; A-M5-1 exists "so Sol never faces a break-scope-or-duplicate choice (the
  A-M4-1 lesson)" (C5 §8 A-M5-1).
- Editing frozen crates ad hoc for the KimiToggle compile ripple — rejected; Sol "correctly STOPPED
  at the frozen boundary" and the orchestrator ratified the exact three one-token arms (A-M5-2).
- Shipping the L7 compaction fix as-is on the Anthropic wire — rejected in adversarial review: the
  separate elision note produced two consecutive `user` messages, which the Anthropic Messages API
  rejects; fixed by the A-M5-3 consecutive-user merge, pinned by the orchestrator-authored failing
  test `anthropic_body_roles_alternate_after_compaction` (C5 §8 A-M5-3).
**Why:** The M5 thesis — the meter must be true before anything else is added (C5 header, "M5
thesis"); the compaction fix alone converts a ~120× cache-miss cost bug into a cache hit (R-derived
L7, `68f91e6` commit message). THE LAW: honest/auditable (the RejectionTrace is the audit artifact
`/why` reads).
**Immediate effect on the harness:** nh-core + nh-routes changes confined to the §0.1 seams;
+14 tests; gate 306/0/1 `--release`, clippy clean; no FEEL gate (no human-facing surface); two wire
shapes (DeepSeek disable, Kimi toggle) entered the VERIFY-LIVE §7 ledger as guesses pending a real
key (`68f91e6`; docs-close `7404878`).
**Long-term consequence:** `resolve_capable`/`RejectionTrace` became the engine behind `/why`,
`nh why`, and later the MCP `why` tool; the two wire guesses were confirmed live on 2026-07-20
(BL:38-39, other writers' era). The amendment-ripple pattern (A-M5-2 style) was reused by
A-M5-4/-7/-9 and became the project's standard way to touch frozen code.
**Evidence:** commit `68f91e6`; C5 §Slice A (lines ~359-407) + §8 A-M5-1/2/3 (lines ~547-596);
BL:451-477 (research grounding); docs-close `7404878` (306/0/1; "two [VERIFY-LIVE §7] wire shapes
pending a live key").
**Article angle:** The most expensive bug in the harness was a compaction routine that silently
invalidated its own prompt cache, and the fix was constrained to keep every retained byte identical.
**Review later:** no (the live verification the slice demanded was completed on 2026-07-20).

## 2026-07-17: M5 scope ratified — thin honest routing in, forecast out; two named defers; "additive-only" abandoned for enumerated behavior corrections

**Decision:** Owner ratified four scope rulings for M5 (C5 header, lines 20-29): (1) five slices A–E
(TRUTH/FLOOR/VISIBLE/LEVER/LOOP) re-slotted by seam; (2) thin honest-routing IS in Slice A — "makes
'cheapest capable' true, not aspirational; powers `/why`" — but "One addition, not two:" the pre-run
forecast / `cost_estimate` are OUT (M6-adjacent); (3) two defers held out of M5: MCP TOFU/
hash-pinning → M7, jurisdiction routing + governance metadata + privacy-router filter → M6 (M5 ships
only the `[read]`/`[send]` law class); (4) behavior-corrections authorized and enumerated up front in
§0.1 — M5 "canNOT stay 'additive only' like M4 — fixing the meter bugs *changes what the wire
sends*", with public type signatures staying source-compatible and wire behavior changing only at
enumerated seams, each pinned by a new test.
**Alternatives considered:**
- Keeping M4's freeze-whole-crates + additive-only discipline — rejected explicitly in ruling 4 (the
  meter bugs live inside frozen code; you cannot fix a wire lie additively).
- Shipping the pre-run cost forecast alongside the resolver — rejected as M6-adjacent (ruling 2).
- Including jurisdiction/privacy routing or TOFU pinning now — deferred with named future owners
  (ruling 3; the privacy-aware-routing differentiator itself was identified in the research,
  BL:469-470, but its mechanism was split from its policy).
**Why:** The A-M4-1 lesson: M4 discovered its one frozen-crate need mid-milestone and had to stop for
an amendment; M5 put the entire mutable surface and amendment list UP FRONT (§0.1 title: "UP FRONT —
the A-M4-1 lesson"). THE LAW: auditable (every behavior change is a tagged [Δ] row with a pinning
test), small (one verb — "*meter*. No second verb", C5 header).
**Immediate effect on the harness:** CONTRACTS_M5 locked (`9e36a94`) with per-crate seam tables,
E1–E5 exit criteria mapped to real tests, nh-fleet kept frozen; positioning doc
`01-product/WHY_BEST_IN_CATEGORY_2026.md` written in the same commit.
**Long-term consequence:** The seam-table + amendment mechanism carried the whole rest of the project
(Slice F's §0.1-F tables are the same instrument). The M6/M7 defers (learning router, privacy
routing, TOFU) became the named backlog. nh-fleet's continued freeze is what later made W5's
reopening (A-M5-8) a formal event.
**Evidence:** C5 header lines 1-29 ("owner scope-ratified 2026-07-17", the four rulings) + §0.1
(lines 33-116); commit `9e36a94`; BL:473-477.
**Article angle:** After one milestone of freezing crates whole, the project switched to freezing
everything except an enumerated, test-pinned list of seams — published before any code was written.
**Review later:** no (the mechanism itself; the deferred features have their own M6/M7 triggers).

## 2026-07-17: M5 direction chosen — "The Honest Meter": fix truth, safety, and visibility before autonomy or providers

**Decision:** Close M4 and set the M5 direction to making the meter TRUE (fix the cost/correctness
bugs), SAFE (security floor), and VISIBLE (money HUD + counterfactual savings line + `/why` +
profiles) before adding any autonomy, learning, or providers. Basis: an owner-commissioned
"deepest + richest" research pass run on TWO models — Fable 5 (high) web-cited across 13 lenses and
GPT-5.6 Sol (xhigh) over the crate code (60-item backlog), 265 unique sources — which
**independently converged** on the product identity ("the metered harness") and the top priority
("make the meter true + visible + safe before adding autonomy or providers").
**Alternatives considered:** The research's ranked backlog contained competing next moves that were
explicitly sequenced OUT of M5: the learning router (moat, "off receipts already written"), privacy-
aware routing, reliability, and ecosystem work all moved to the M6–M7 arc (`0039cc4` commit message);
the report's LAW-rejection list also names whole approaches rejected outright (neural/learned router,
a third wire, gateway dependency, LLMLingua-style compression — R:305-314).
**Why:** The research surfaced ~12 live code issues in the shipped meter (thinking-defaults cost bug,
120× cache-miss compaction, unguarded `read_file`, credential-audience exfil, nh-mcp no-auth, etc. —
BL:466-469); shipping intelligence on top of a lying meter would compound every one. The two-model
convergence "is the spine" (C5 header). THE LAW: honest/congruent — every existing crate already
serves the meter, so adopting the identity required zero new machinery (K §1, congruence test).
**Immediate effect on the harness:** None (docs/research only — `a2c2b83`, `0039cc4`); the identity
sentence became the CONTRACTS_M5 preamble every seam had to be congruent with.
**Long-term consequence:** Defined the beachhead-vs-moat split ("M5 wins the beachhead... M6 wins
the moat", C5 header) and produced the durable research corpus (`00-start-here/
RESEARCH_2026-07_harness.md`, 14 raw files) that later eras (audit, positioning posts) cite.
**Evidence:** commits `a2c2b83`, `0039cc4`; BL:451-477; C5 header lines 7-18; K §1.
**Article angle:** Two different frontier models were pointed at the same product from different
angles — web landscape vs code — and the milestone was chosen from where their conclusions overlapped.
**Review later:** no.

## 2026-07-17: The delegate-adapter class is cut from v1 — repositioned as "open-weight-first harness with a frontier review gate"

**Decision:** Cut the entire subscription-delegate adapter class (Claude/Codex/Gemini child-CLI
routes) from v1/M5 scope, keep the commented `class = "delegate"` schema in `catalog.toml`, and
reposition the product from "open-weight frontier models do the bulk work, subscription delegates
do what only they can" to **"open-weight-first harness with a frontier review gate."** Delegates
"return post-launch only if the economics return" (K F4).
**Alternatives considered:**
- Keeping delegates as a marquee pillar (the standing one-sentence pitch,
  `01-product/PRODUCT_BRIEF.md:9`) — rejected because the pillar's economics broke (below).
- Building "one safe delegate seam... not 3 wrappers" — the research backlog carried this as a
  Tier-7 item marked `delegate=none` (R:246), i.e. the seam idea survived as backlog, not v1 scope.
- A Gemini delegate specifically — deferred separately on reliability grounds ("Antigravity headless
  CLI is unreliable (no `--model`, drops stdout, times out) → **defer**", R:99).
**Why — the two external events that forced it (as cited by the sources):**
1. **Anthropic, 2026-06-15:** moved ALL programmatic Claude Code use (`claude -p`, Agent SDK, GitHub
   Actions) off the flat subscription onto standard API pricing — so headless delegation "now burns
   metered dollars, not subscription slack" (K F4 and K §1; source cited there:
   https://ccforeveryone.com/guides/claude-code-limits-and-pricing).
2. **Google, 2026-06-18:** Gemini CLI killed as an open delegate (open source + free tier ended →
   closed Antigravity binary, ~20 req/day) (K §3 W3 and K F4; source cited there:
   https://inventivehq.com/blog/terminal-ai-coding-clis-compared-2026).
   Additional context the sources attach: subscription quota opacity was already the incumbents' pain
   (March 2026 rate-limit-drain complaints — K §1, citing
   https://www.macrumors.com/2026/03/26/claude-code-users-rapid-rate-limit-drain-bug/), and
   open-weight capability parity made the pillar unnecessary (DeepSeek V4 83.7% SWE-bench Verified —
   K F4, citing https://spectrumailab.com/blog/best-open-source-coding-model-2026 and
   https://www.contextstudios.ai/comparisons/kimi-k2-7-vs-deepseek-v4).
   The cut also made positioning match code that already existed: the Opus gate had shipped as a
   review-pause, not a live delegate, and delegate routes were already commented out (K F4: "the code
   already made the right call"; `catalog.toml:356-377`). THE LAW fit as scored by the source:
   "small, congruent (positioning = code), honest... negative-cost scope reduction that sharpens
   identity" (K F4).
**Immediate effect on the harness:** None in code — the delegate adapter had never been built; the
commented catalog schema stayed (`catalog.toml:5` documents `class = "delegate"`; `catalog.toml:
356-377` holds the commented Claude/Codex route stanzas). The cut was recorded in the consolidated
report as both a scope-discipline ruling (R:37) and item (5) of the v1 cut list (R:61), and in the
LAW-rejection list ("Full delegate adapter class in v1 — the economics broke (2026-06
Anthropic/Google changes)", R:314). The new category positioning was committed with the M5 lock:
"the honest, visible, auditable *metered* agent for open-weight models — native on Windows"
(`01-product/WHY_BEST_IN_CATEGORY_2026.md:18`, committed in `9e36a94`).
**Long-term consequence:** Deletes an entire adapter class from the launch surface and from the
frozen-wire risk budget (no third process-spawning client class to secure); locks the product's
frontier-model relationship to the review role only; the key-acquisition rule became "no
OpenAI/Anthropic/Google key until a measured workload proves the delegate insufficient" (R:101).
Re-entry condition is explicit: the economics returning (K F4).
**Evidence:** K F4 (fable_K_product_cohesion_2026-07.md:90-92, with the five external links above);
K §3 W3 (line 70); R:37, R:61, R:97-101, R:246-247, R:314; `catalog.toml:5,356-377`;
`WHY_BEST_IN_CATEGORY_2026.md:18`; commits `a2c2b83` (research), `9e36a94` (positioning lock);
`PRODUCT_BRIEF.md:9` (the superseded pitch — see UNSOURCED for its un-updated state).
**Article angle:** Two vendor pricing moves in one June week made "borrow your subscription's agent"
economically dead, and the project's response was to delete the feature class and rename what it was.
**Review later:** yes — trigger stated in the source: delegates return post-launch only if the
subscription economics return (K F4).

**Also recorded by:** the standing cross-cutting record ([`DECISIONS_STANDING.md`](DECISIONS_STANDING.md), product-identity entry) lists the cut as a rejected alternative; unique citation merged from it: `00-start-here/CURRENT_TASK.md`:13 ("the delegate class is CUT from v1"). See also `ARCHITECTURE_DECISIONS.md` Decision 4 (amended 2026-07-24).

---

## 2026-07-16: M4 Slice B — the escalation ladder climbs model failures, not infrastructure faults; failures travel as receipts, never transcripts

**Decision:** Implement the escalation ladder with these semantics: each tier gets ≤2 attempts;
`Outcome::{Fail,Timeout}` receipts climb the ladder (each `TaskEscalated` carries a typed reason,
and the failure Receipt is already durable as the preceding `TaskReceipt` — never a raw transcript);
an **infrastructure `Err` terminates immediately** ("the ladder climbs model failures, not faults");
`--escalate` is opt-in and per-task `model` is rejected with one friendly line when laddering; a
killed escalation run resumes mid-ladder via a pure `ladder_position` fold. The off-peak scheduler
(E2) reuses the frozen `nh_routes::ResolvedRoute::price_at`/`peak_status` through an injected
`Clock` trait, parking peak tasks on a 100 ms `recv_timeout` tick (no busy-spin); routes with no
peak data always dispatch ("Honest: never fabricate a window", C4 §B.1).
**Alternatives considered:**
- Re-implementing clock pricing in the fleet — prohibited by contract ("the SAME helper the M3 HUD
  uses — do NOT reimplement clock pricing", C4 §B.1). No alternative ladder semantics appear in the
  sources; the receipts-not-transcripts rule is inherited from the Master Plan ("receipts + typed
  reason, NEVER raw transcripts — plan line 297", C4 §B.2).
**Why:** THE LAW: congruent (scheduler reuses peak logic; ladder reuses `Receipt` outcomes — C4
§0.2) and honest (no fabricated windows; typed reasons). The infra-vs-model distinction keeps retry
spend pointed at problems a better model can fix.
**Immediate effect on the harness:** `next_step` pure seam (`Retry|Escalate|Gate|Done`) unit-tested
across the whole ladder; default ladder `flash/none → k2.7/high → v4-pro/high → v4-pro/max → Opus
review-pause GATE`; resume-continues-the-climb via an additive `#[serde(default)] escalate` flag on
`RunStarted`. Gate 284/0/1 (+11), E1 kill-9 test unmodified and green; FEEL owner-approved with live
parking captured during the actual Beijing peak window (BL:512-553; commit `25bd5b3`).
**Long-term consequence:** Exactly-one-terminal + at-least-once invariants held through every later
fleet change; the ladder was deliberately NOT touched by M5's resolver ("M5's resolver is *initial*
cheapest-capable selection, a different concern from fleet fallback", C5 §0.1 frozen note) — the two
routing mechanisms stayed separate concerns.
**Evidence:** C4 §B.1-B.2 (lines 188-214); BL:512-553; commit `25bd5b3`; docs-close `f439e17`.
**Article angle:** The retry policy distinguishes "the model failed" from "the infrastructure
failed" and only spends escalation money on the former, with the evidence carried as typed receipts.
**Review later:** no.

## 2026-07-15: The Opus review gate ships as a review-pause, not a live delegate (owner ruling #2)

**Decision:** The terminal tier of the M4 escalation ladder is **GATE(opus-4.8) = review-pause**:
when a task exhausts the ladder, it stops with `TaskGate{reason}`, the accumulated failure receipts
attach, and `RunReport.gated` surfaces it for the human/orchestrator to review. No live Opus call is
made by the harness. Recorded as owner ruling #2 of the four M4 scope rulings.
**Alternatives considered:**
- **A live delegate call** (`claude -p` headless driven as a `ChatClient`) — rejected for M4,
  explicitly: "the *live* delegate route (`claude -p` headless) is explicitly OUT of M4 — no
  delegate `ChatClient` adapter, no client-factory (frozen nh-routes) touch this cycle" (C4 §B.2).
  The contract's stated reasons: nosis has no first-party Opus API route ("Opus is the reviewer, not
  a fleet worker") and a delegate adapter would have forced a frozen-crate write in nh-routes.
  C4 §7 logs the live path as a deliberate defer: "Delegate/Opus-gate live — OUT of M4 (gate is
  review-pause). Revisit for a later milestone."
**Why:** Keeps M4's frozen-crate discipline intact (only A-M4-1 was sanctioned) and keeps the
harness from autonomously spending frontier-model money at the top of an *unattended* ladder — the
gate is where the human re-enters. THE LAW: small/secure (no new adapter class; no unattended
frontier spend). Note the timing: Anthropic had already moved programmatic Claude Code to API
pricing a month earlier (2026-06-15, per K F4's citations), so the subscription-slack rationale for
a live delegate was already dead when the ruling was made — though the contract itself argues from
scope and architecture, not from that market event; the market framing was added two days later by
the research pass, which observed "the code already made the right call" (K F4).
**Immediate effect on the harness:** `TaskGate` became a terminal ledger event (C4 §A.2) and
`RunReport.gated` a first-class count; the ladder's top tier costs nothing until a human looks.
Shipped and gated in Slice B (`25bd5b3`, BL:529-535).
**Long-term consequence:** Enabled the "frontier review gate" half of the post-cut positioning (K
F4) — the review-pause is the only frontier-model touchpoint in the product, and it is human-mediated.
Forecloses unattended frontier escalation unless a later milestone reverses it (C4 §7's "revisit").
**Evidence:** C4:12 (ruling #2), C4 §B.2 (lines 208-214), C4 §7 (line 313); BL:565-570 (Carlos ruled
the four decisions; line 568: "Opus 4.8 gate = **review-pause** (no live delegate)"); commit
`25bd5b3`; K F4 (lines 90-92).
**Article angle:** At the exact moment vendors were re-pricing programmatic frontier access, the
ladder's top rung shipped as a pause-for-human instead of an API call — for architecture reasons
that the market news then retroactively vindicated.
**Review later:** yes — C4 §7 explicitly marks the live delegate/gate "Revisit for a later
milestone", and the delegate-cut entry ties re-entry to the economics returning.

## 2026-07-15: nh-mcp is built on tiny_http — blocking, no tokio (owner ruling #3) — speaking the same stateless wire the client already speaks

**Decision:** The nh-mcp HTTP server uses **`tiny_http`** (blocking, ~zero transitive deps, no
tokio), binds `127.0.0.1` by default with a "local/preview only" banner, and speaks the **stateless
2026-07-28 JSON-RPC wire** that `nh_tools::mcp::McpClient` already speaks: no `initialize`
handshake, never an `Mcp-Session-Id` header; the E3 exit test drives the new server with the
existing client acting as KORVIN. `fleet_run` returns a `run_id` as a stateless passthrough handle
(run state lives in the Slice-A ledger, not a session). Recorded as owner ruling #3.
**Alternatives considered (both recorded in the contract with reasons):**
- Hand-rolled `std::net::TcpListener` — rejected: "more parsing code to harden" (C4 §0.4).
- `axum` — rejected: "pulls tokio, breaks the no-async posture" (C4 §0.4; the whole workspace is std
  threads + channels, a stance carried from M3 — C4 §0.4 nh-fleet row: "No async runtime").
**Why:** THE LAW: congruent + small — "congruent with the no-async-runtime stance from M3" (C4
§0.4), and the server mirroring the client's wire is named as "the congruence lever" (C4 §Slice C
preamble): the E3 test gets a real client for free and the stateless invariant is proven from both
sides ("Assert NO session header on any request — stateless invariant holds server-side too",
C4 §C.4). The 127.0.0.1 + banner rule enforces the standing "no public MCP before the 2026-07-28
final spec" security invariant (C4 §0.3).
**Immediate effect on the harness:** New `crates/nh-mcp` with three tools
(`route_resolve`/`fleet_run`/`fleet_status`), `nh mcp serve` with the two-line preview banner;
`tiny_http 0.12.0` compiled against the Cargo.lock checksum by the orchestrator on a clean registry
(Sol's sandbox lacked network — BL:502-504). Gate 292/0/1 (+8); FEEL driven through the real binary
over live HTTP; non-loopback `--addr` hard-rejected (BL:479-510; commit `ece6bb0`).
**Long-term consequence:** Zero-migration-debt posture for the MCP 2026-07-28 final ("nosis is
stateless-native... while every incumbent migrates", K F8); the blocking/no-tokio choice held
through the MCP metered-service expansion (later era) without a runtime rewrite; the plan's
`receipts_query`/`cost_estimate` tools were honestly deferred rather than silently dropped ("NOT in
the exit criteria; add only if cheap, else note as deferred (honest, not silent)", C4 §C.2) — they
returned as `receipts`/`route_cost` in the Release Slice (other writers' era).
**Evidence:** C4:13 (ruling #3), C4 §0.4 (lines 58-61), C4 §Slice C (lines 227-263), C4 §0.3
(lines 50-52); BL:479-510, BL:568; commit `ece6bb0`.
**Article angle:** The MCP server was written to the not-yet-final stateless spec using the
project's own MCP client as its conformance test, on a blocking HTTP library chosen to keep tokio
out of the workspace.
**Review later:** no (the 2026-07-28 public-exposure decision is a separate, already-scheduled
gate, not a revisit of tiny_http).

## 2026-07-15: One sanctioned frozen-crate write — OAuth2 via amendment A-M4-1/A-M4-2; everything else stops for an amendment (owner ruling #1)

**Decision:** M4 froze five crates whole (nh-core, nh-tools, nh-law, nh-routes, nh-vault) and the
owner authorized exactly ONE frozen-crate write: OAuth2 in `nh_tools::mcp` (amendment A-M4-1, plus
an additive nh-vault keyring setter as A-M4-2), because exit criterion E4 ("OAuth refresh survives a
forced expiry mid-session") was impossible without touching the `bail!("oauth2 arrives in M4")`
site. "Every other frozen need STOPS for an amendment" (C4:10-11). Secrets come from the vault,
never TOML; refresh-on-401 retries exactly once.
**Alternatives considered:**
- An encrypted-at-rest token cache under `.nosis/` instead of the keyring — recorded as the fallback
  "if the owner prefers zero nh-vault change... but keyring is the secure default" (C4 §D.1); moot in
  the end because **A-M4-2 turned out to be a NO-OP** — `Vault::set` had existed since M0, so Slice D
  touched no nh-vault code at all (C4 §8, A-M4-1 clarification).
**Why:** THE LAW: secure + auditable — the freeze protects proven code; the single exception is
written into the contract with its justification ("authorized because E4 cannot be met otherwise",
C4 §8) and its as-implemented deviations were logged the day they landed (the OAuth2 config became a
struct variant, forcing an authorized 2-line adaptation of non-frozen `nh_tui::mcp_state`; tokens
held as `String` in a `Mutex<OAuthState>` to avoid a Cargo edit — C4 §8 clarification, 2026-07-16).
**Immediate effect on the harness:** `McpAuth::OAuth2 { token_url, client_id, vault_entry }` with
refresh + single 401 retry; E4 test `oauth2_refreshes_on_absence_expiry_and_one_401_retry` replaced
the deferral test; nh-tools 56/0, clippy clean; committed `9344251` (BL:451-458).
**Long-term consequence:** Established the amendment discipline that M5 §0.1 then inverted into
up-front seam tables ("the A-M4-1 lesson") — the project's entire subsequent change-control model
descends from this ruling and from watching Sol correctly stop at frozen boundaries.
**Evidence:** C4:10-11 (ruling #1), C4 §Slice D (lines 267-289), C4 §8 (lines 315-333); BL:451-458,
BL:565-567; commit `9344251`.
**Article angle:** The milestone allowed itself exactly one edit to frozen code, wrote the
justification into the contract before the edit, and logged that half the authorization turned out
to be unnecessary.
**Review later:** no.

## 2026-07-15: Kimi Swarm ships as a minimal honest seam, not a client (owner ruling #4 — "don't overdo it, budget")

**Decision:** Implement `Backend { Native, KimiSwarm }` with `Native` fully done and `KimiSwarm` as
the smallest honest seam: a `SwarmClient` trait, ONE mock-receipt test proving the submit→collect
shape produces a typed receipt, and an honest `PendingSwarmClient` stub that `bail!`s "arrives live
in M6". No polling, no retry/streaming machinery, no frozen-wire touch; "If a real swarm client
would need a wire change, STOP — it waits for M6" (C4 §B.3).
**Alternatives considered:**
- A fuller swarm client (polling/streaming, live endpoint) — rejected by owner directive, quoted in
  both the contract and the build log: "don't overdo it, budget" / "Budget-minimal by directive"
  (C4:14-15, §B.3; BL:540-542).
**Why:** THE LAW: small + honest — the seam is real (typed receipt shape proven by test) and the gap
is stated in the failure message rather than hidden. The live Agent-Swarm endpoint + Kimi key were
logged as live-pending in the verify ledger (C4 §7).
**Immediate effect on the harness:** The `Backend` enum + serde round-trip landed in Slice B
(`25bd5b3`); fleet tasks could *name* the backend without the product pretending it worked.
**Long-term consequence:** The research's v1 cut list later LOCKED this state ("Kimi Swarm stays the
minimal seam + honest stub it already is (owner ruling #4)", K F9); W5 (later era) preserved and
persisted the `backend` field through resume rather than expanding it.
**Evidence:** C4:14-15 (ruling #4), C4 §B.3 (lines 216-224), C4 §7 (line 312); BL:540-542, BL:569;
commit `25bd5b3`; K F9 (line 111).
**Article angle:** A speculative integration was shipped as one enum variant, one mock test, and an
error message that names the milestone it actually arrives in.
**Review later:** yes — trigger: M6, per the stub's own message ("arrives live in M6") and C4 §7's
live-pending entry.

## 2026-07-15: M4 Slice A — the fleet is an fsync-durable append-only ledger with one writer, std threads, and a pure resume fold; no async, no new dependencies

**Decision:** Build `nh-fleet` as: an append-only `ledger.jsonl` per run where ONE mutex-guarded
writer does write→flush→`sync_all()` per event before acknowledging ("A `kill -9` cannot lose a
committed event... the ledger is the one source of truth", C4 §A.2), every line Scrubber-scrubbed;
`TaskStarted` fsync-committed BEFORE the task runs (worker blocks on a coordinator ack) so any
observable side-effect implies a durable start record (BL:580-582); stable task ids (caller `id` or
deterministic `t{index:03}-{hash8}`) as the idempotency key, collisions rejected pre-run; resume as
a **pure fold** `plan_from_ledger` (terminal → never re-run; started-without-terminal → re-run at
attempt+1; the guarantee is exactly one terminal record per task); a bounded std-thread worker pool
reusing the exact `nh run` construction recipe so fleet behavior is identical to single-task runs;
budget as a hard stop that sums prior receipts across resume. Dependency ruling: "no new external
crates — std threads + channels... No async runtime" (C4 §0.4).
**Alternatives considered:**
- An async runtime for the fleet — foreclosed by the §0.4 ruling (mirror the nh-tui `Worker` shape);
  no competing ledger design appears in the sources. Heartbeats were included but explicitly
  demoted: "drop-if-hard (the resume model in A.5 does not depend on heartbeats for correctness)"
  (C4 §A.4).
- Re-running completed tasks on resume — foreclosed by design: the contract's E1 test asserts
  pre-kill `TaskDone` tasks have NO second `TaskStarted` and the mock proves each completed task
  executed once (C4 §A.8).
**Why:** E1 is the milestone crux ("a 10-task fleet run survives `kill -9` and resumes
idempotently", C4 §0.5); durability-before-side-effects and pure folds make the property testable
headlessly. THE LAW: auditable (the ledger reuses `nh_core::receipt` typed receipts), congruent
(worker mirrors nh-tui's `Worker`), small (zero new deps).
**Immediate effect on the harness:** `crates/nh-fleet` + `nh fleet run/resume` CLI; the E1
integration test spawns the real `nh` binary against an inert env-gated echo provider, kills it
mid-run after ≥3 durable `TaskDone`, resumes, and proves all 10 tasks reach exactly one terminal
with no committed task re-run. Gate 273/0/1 `--release` (+12); committed `96db4f7` (BL:555-613).
Two non-blocking notes were recorded honestly (the echo seam ships in the binary but is inert and
cannot bypass the law gate; keyless fleet exits with an actionable line rather than opening).
**Long-term consequence:** The ledger became the substrate for everything after it: the scheduler
and ladder append to it (Slice B), nh-mcp's `fleet_run`/`fleet_status` hand out and fold it
(Slice C), and the M4-freeze of this proven core is what made W5's later reopening a formal
amendment event (A-M5-8, other writers' era). The exactly-one-terminal invariant survived every
subsequent change.
**Evidence:** C4 §A.2-A.8 (lines 91-181), §0.4 (lines 55-57), §0.5 (line 64); BL:555-613 (gate
273/0/1; E1 PASSES); commit `96db4f7`; docs-close `a4dd198`.
**Article angle:** The fleet's crash-safety story is one fsync-per-event writer plus a pure function
over the ledger — proven by a test that kills the real binary mid-run and checks nothing ran twice.
**Review later:** no.
