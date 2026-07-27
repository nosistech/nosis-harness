# RECONSTRUCTED RATIONALE — the 27 decisions with no contemporaneous "why"

**Status: ANSWERED 2026-07-25.** Originally a list of open questions merged from the five decision-record
drafts (A: M2–M3, B: M4–M5 A–E, C: Slice F, D: Release Slice + Slice G, E: standing cross-cutting).

## How to read this file — the labelling contract

On 2026-07-25 the owner reviewed all 27 items and answered: **"I don't remember any, to be honest."**
That is itself a finding, and it is recorded rather than papered over.

This file therefore contains **two different kinds of statement**, and they are never mixed:

- **Known (sourced).** Traceable to a commit, contract, BUILD_LOG entry, or code. Carried over verbatim
  from the original question.
- **Rationale (reconstructed 2026-07-25).** NOT a memory and NOT a transcript. It is the justification
  that holds *today*, derived from the code, the constraints, and THE LAW. It explains why the decision
  is defensible — it does **not** claim this is what anyone was thinking at the time.

**This distinction is load-bearing and must survive into any launch article.** nosis's entire pitch is
that it does not fabricate: the meter prints "unpriced" rather than `0.00`, `nh why` refuses to compare
¥ against $, and W7's exec reports "could NOT be killed" rather than claiming a kill it did not verify.
A decision log containing invented deliberation would fail the same test the product passes. Where the
honest finding is *"the constraints decided it and there was no real deliberation"*, that is written
plainly — and it is usually the better story.

**The five DECISIONS_*.md files in this directory remain a purely sourced record.** Reconstructions are
deliberately quarantined here so the boundary between evidence and inference stays legible.

---

## Era: M2–M3 (draft A, 5 items)

### 1. ratatui + crossterm choice

**Known:** CONTRACTS_M3.md §0/§5.3 authorizes exactly these two crates; no source records any compared
alternative TUI stacks.

**Rationale (reconstructed 2026-07-25):** There was almost certainly no contest, and the absence of a
recorded comparison is consistent with that. nosis is Windows-first, which disqualifies **termion**
outright (Unix-only). **cursive**'s mainstream backends pull ncurses C bindings — foreign code that sits
badly with THE LAW's *auditable* and *lightweight* clauses, and which would later have collided with
`unsafe_code = "forbid"` (`d1f9ad0`). **ratatui** is the maintained successor to the archived tui-rs, and
**crossterm** is its default cross-platform backend. Under the dependency budget, ratatui+crossterm was
the only stack satisfying every constraint.

**Article angle:** "We didn't hold a bake-off — the constraints left one candidate" is a stronger claim
than a fabricated comparison, and it demonstrates that a tight dependency budget does real work.

### 2. Compaction constants 70/50/2

**Known:** `COMPACT_AT = 0.70`, target `0.50`, `KEEP_RECENT = 2`, recorded only as "pinned defaults;
revisit if live sessions thrash" (CONTRACTS_M2.md:417).

**Rationale (reconstructed 2026-07-25):** The recorded note is itself evidence these were **chosen by
judgement, not measurement** — "revisit if live sessions thrash" is what you write when you have not
measured. They are nonetheless well-formed:

- **0.70 trigger** leaves 30% headroom so a single large tool result can land without overflowing before
  compaction runs. Slice F W3-2 later reinforced exactly this by making the trigger compute
  `input_tokens = max(provider prompt count, fresh local estimate)` so a just-appended tool result is
  actually seen.
- **0.50 target, not 0.60** creates **hysteresis**. The 20-point gap between trigger and target is the
  work you can do before recompacting; a 0.70→0.60 pair would re-trigger almost immediately and thrash.
- **KEEP_RECENT = 2** preserves the current turn *and* its predecessor, so an in-flight tool-call /
  tool-result pair can never be split across a compaction boundary.

**Not recorded:** whether any of the three was ever tuned against a live session. No source shows a
revisit, so the "pinned defaults" appear to have simply held.

### 3. The "CodeWhale bar" for Slice D

**Known:** CONTRACTS_M3.md:332 says the re-skin must "match the CodeWhale bar the owner referenced";
what concretely defined that bar is unrecorded.

**Rationale (reconstructed 2026-07-25):** The bar is recoverable from what the re-skin actually shipped
in response to it (commit `3fcd00e`): a bordered frame, a chat transcript with distinct you/nosis roles
and turn separation, framed centered modals with Clear-before-draw anti-bleed, a welcome screen, a
key-hint strip, and type-freely slash commands with a live menu. Read backwards, the bar was **"it must
read as a modern chat TUI, not a log dump."**

Critically, the commit message records that the owner **rejected a content-complete TUI on UX grounds and
reopened M3** — so the bar was defined by *rejection*, not by specification. See item 26: this is the
origin of the FEEL gate.

### 4. Slice F's mouse-wheel tradeoff

**Known:** Mouse-capture removal is sourced (commit `3fcd00e`, BUILD_LOG:637-640) and reversed
CONTRACTS_M3 §9.2; no dated contract amendment recorded it.

**RESOLVED 2026-07-25.** A retroactive amendment was written into `CONTRACTS_M3.md` §9.2. Sourced reason
from the commit: capture was removed so **native click-drag copy works with no Shift held** — with
capture on, the terminal never sees the drag and text cannot be selected out of the transcript. The
wheel-scroll loss is covered by the keyboard scroll shipped in Slice E (`↑↓`, PageUp/PageDown, End) plus
the `↑ more`/`↓ more` hints, so no scrolling *capability* was lost — only the input device.

**Not recorded:** whether the wheel loss was explicitly weighed in the moment or simply accepted as the
price of restoring copy. The owner has no recollection.

### 5. M2 contract draft-vs-amendment history

**Known:** CONTRACTS_M2.md §7 reads "(none yet)" — M2 shipped with zero integration amendments, the only
milestone contract of its era needing none.

**Rationale (reconstructed 2026-07-25):** Not reconstructable from the repo, and marked as such. The
plausible structural reason: M2 was the context engine — the most spec-heavy and least externally-coupled
milestone, with no provider wire formats, no terminal behaviour, and no OS surfaces to collide with.
Milestones that later needed amendments (M5's A-M5-1, A-M5-8, A-M5-9) all did so because reality outside
the crate — a provider's wire shape, a frozen crate boundary — differed from the spec. M2 had little
such outside.

**Caution for article use:** zero amendments is weak evidence of good specification, not proof. It is
equally consistent with the contract having been written late.

---

## Era: M4–M5 Slices A–E (draft B, 4 items)

### 6. `PRODUCT_BRIEF.md:9` carried the superseded delegate pitch

**RESOLVED 2026-07-25 — it was a pending edit, not a deliberate hold.** Owner ratified the fix. The pitch
now reads as the shipped positioning ("the honest, visible, auditable *metered* agent for open-weight
models — native on Windows"), with a dated amendment note preserving what it previously claimed and why
it changed.

### 7. No explicit owner-ratification line for the delegate cut (K F4)

**Known:** The cut is thoroughly adopted (RESEARCH:37, RESEARCH:61 v1 cut list item 5, RESEARCH:314
LAW-rejection list; repositioning shipped in `e2b2f02`), but there is no dated "owner ratified F4"
sentence.

**Rationale (reconstructed 2026-07-25):** The cut is attributable by **evidence convergence**, not by a
ratification moment: it appears independently in the v1 cut list, the LAW-rejection list, and the shipped
repositioning commit. The 2026-07-16/17 scope session is the plausible locus but remains inferred.

**Guidance for launch posts: attribute the delegate cut to the v1 cut list, never to a dated decision
moment.** Claiming a moment that cannot be produced is the failure mode this whole file exists to avoid.

### 8. The `wip/<slice>` commit rule

**RESOLVED 2026-07-25 — never adopted; superseded by the gate.** Owner ratified. A dated amendment is now
in `CONTRACTS_M5.md` §Slice E (governing the duplicate clause at §6). What actually delivered the
durability the rule was meant to provide: the 4-step `gate.ps1` floor, the mandatory per-wave adversarial
review, and Sol's retained per-wave brief + self-report. On a solo single-working-tree project a `wip/`
branch added ceremony without adding protection.

**Article angle:** process that earned its keep (the gate) versus process that was specified and quietly
never used (the branch rule) — and the value of auditing your own contracts against `git log`.

### 9. BUILD_LOG gaps for M5 Slices A, C, E

**RESOLVED 2026-07-25 — backfilled.** Owner ratified backfill so BUILD_LOG is the complete public record
before the v0.1.0 tag. Entries reconstructed strictly from commits `9c96259`, `a0a4036`, `bc2a1b1`,
`a71eb23` and the docs commits `70a2f9d`, `3a5df91`, `0c14743`; any field not recoverable from a primary
source is marked "not recorded at the time" rather than guessed.

---

## Era: M5 Slice F (draft C, 4 items)

### 10. Why W3 (meter) outranked W2 (egress) in the ratified order

**Known:** The ratified order is the labeled line in CURRENT_TASK.md:261-262 ("W1 security → W3 meter →
W2 egress → W5 fleet → W4 surfaces"); "security floor first" and "surfaces last" are sourced; no primary
source argues W3-over-W2. The existing draft's "the meter is the product thesis" was the writer's
inference.

**Rationale (reconstructed 2026-07-25) — and it is a better argument than the thesis one:** the order is
**dependency order, not importance order.**

- W1 had already landed the security floor, so the acute egress risk was reduced *before* W2 was due.
- W3 touched `nh-core` + `nh-routes` — the deepest layers. W2 (`nh-tools`/`nh-mcp`) and W4 (surfaces)
  both **consume** those types.
- W3 changed signatures, not just behaviour: it folded `effort_for` to take `Wire`, and changed the
  resolver's cross-currency comparison semantics. Had W2 or W4 been written first, both would have been
  refactored a second time against the new shapes.

Sequencing the meter first meant every later wave built against final signatures. The thesis framing is a
consequence, not the cause.

### 11. Why option (a) — cost check inside candidate selection — was rejected for W3-1

**Known:** The audit (high-1) offered folding the cost check into candidate selection; the ratified call
was the simpler full drop, recorded as "compaction only runs post-trigger; overflow-avoidance beats
cache-warmth" (CONTRACTS_M5.md W3-1).

**Rationale (reconstructed 2026-07-25):** Two independent reasons.

1. **Separation of concerns.** Option (a) pushes compaction policy into the router's candidate-selection
   path, coupling two subsystems that otherwise share nothing. The router's job is choosing the cheapest
   capable route; whether history needs compacting is not an input to that.
2. **The guard was fail-open, so it was wrong in principle, not merely complex.** It protected cache
   warmth *at the cost of permitting context overflow*. Under the Q3 ratification — fail-closed
   everywhere — a guard that trades a correctness failure for a performance win is the wrong shape
   regardless of where it lives. Deleting it was not the cheap option; it was the correct one.

### 12. W4 FEEL outcome

**ANSWERED 2026-07-25: no verdict was ever delivered — held, then superseded.** W4 was held at the FEEL
gate on 2026-07-21 with a 19-step test script prepared. The owner never drove it, and Slice G then
rewrote the surfaces underneath: W2 rebuilt terminal restore and worker shutdown, W6c changed redaction
and approval display. The `nh.exe` that would have been driven no longer reflects the tree.

**Recorded resolution:** the W4 FEEL is folded into a single fresh FEEL gate on the current tree, run
against a new `--release` build before the combined W4 + Slice-G commit. Re-running the old script
against the stale binary would have proven nothing.

### 13. Origin of the `[send]`-before-trust ordering

**Known:** Sourced as an owner-ratified design call (BUILD_LOG W2, call 1) with its safety property
(default `Allow` byte-identical); no competing ordering is recorded.

**Rationale (reconstructed 2026-07-25):** The only real alternative is trust-then-law, and it fails on
two counts. The trust machinery can **prompt the user**; if the law would return `Block`, that prompt is
(a) a UX defect — asking permission for something that can never proceed — and (b) an injection surface,
since a hostile MCP server entry could induce a prompt purely to farm an approval habit. Law-first means
a `Block` short-circuits before anything user-visible or network-visible happens. The byte-identical
default `Allow` path was the property that made the reordering safe to ship rather than the motivation
for it.

---

## Era: Release Slice + Slice G (draft D, 9 items)

### 14. Why nosistech LLC (not the founder) holds the MIT copyright

**Known:** The directive "(not Carlos personally)" is verbatim (CURRENT_TASK.md:3) and shipped
(LICENSE:1-3, `b43b023`); no reason is recorded.

**Rationale (reconstructed 2026-07-25) — general practice, not a claim about the owner's reasoning:**
entity-held copyright is conventional for three reasons: it keeps the project's IP transferable without
involving the founder personally, it makes future contribution and relicensing mechanics run through one
stable holder, and it keeps project liability at the entity. Which of these motivated the call is
unrecorded, and the owner does not recall.

**This is not legal advice and should not be presented as a legal rationale in an article.**

### 15. Why ASD-STE100 for SECURITY.md

**Known:** The assignment and execution are sourced (CURRENT_TASK.md:3; `b43b023`).

**Rationale (reconstructed 2026-07-25):** ASD-STE100 is a controlled English developed for aerospace
maintenance documentation, where a misread instruction is a safety event. It restricts vocabulary to one
meaning per word, bans synonyms, and forbids complex constructions. A SECURITY.md is the same class of
document: "you must", "do not", and scope statements have to survive a hurried read by a non-native
speaker under stress. It is also directly congruent with THE LAW's *readable* clause.

**Not recorded:** whether the motivation was non-native readers, controlled-language principle, or prior
familiarity.

### 16. Why $2 as the per-provider live-test hard cap

**Known:** "<$2/provider HARD CAP" is verbatim and repeatedly sourced (CURRENT_TASK.md:3, :75); the
number is unexplained.

**Rationale (reconstructed 2026-07-25):** The number was almost certainly arbitrary-but-safe, and the
outcome proves the point: total real spend across all four providers was **≈$0.0014** — roughly 1400×
under a single provider's cap. The cap's function was never budgetary. It was a **commitment device**: a
hard stop that bounds the blast radius of a bug, a retry loop, or a misconfigured max_tokens, chosen high
enough never to interfere with legitimate tiny prompts and low enough that hitting it is unmistakably a
fault rather than a cost.

**Article angle:** the cap that never bound is the one worth writing about — its value was that it made
"how much could this possibly cost?" a question with a written answer before the first key was used.

### 17. The specific W6b cap values

**Known:** 8 MiB provider / 4 MiB MCP / 256 KiB OAuth / 24h ttl / 512 tools / 4 active runs / 32-byte
token floor were orchestrator-recommended and owner-confirmed (CURRENT_TASK.md:31, :19); the per-number
"why" was delivered in-session and never written down.

**Rationale (reconstructed 2026-07-25 by the orchestrator).** Every value follows one rule: **reject only
pathology, never legitimate use.** Each is set well above the largest plausible honest case so that
tripping a limit is unambiguous evidence of a fault or an attack, not of a big-but-normal workload.

| Cap | Value | Why this number |
|---|---|---|
| Provider response body | 8 MiB | A completion at a 32k-token output limit is well under 1 MiB of JSON. 8 MiB is ~8× the largest plausible legitimate response. |
| MCP response body | 4 MiB | MCP results are tool output, structurally smaller than completions — and everything past the 32k-char `ToolResultEnvelope` is discarded anyway, so a larger cap would buy nothing. |
| OAuth document | 256 KiB | Metadata and token documents are a few KB. 256 KiB is ~50× headroom while staying trivially small. |
| `ttlMs` clamp | ≤ 24h | A remote-supplied TTL beyond a day is indistinguishable from an attempt to pin stale state; one day bounds cache poisoning to a single usage cycle. |
| `tools/list` | ≤ 512 | No legitimate server exposes that many. Truncate-and-warn keeps the session usable instead of failing it. |
| Concurrent fleet runs | 4 | Matches a small-machine core budget and bounds worst-case concurrent provider spend. |
| Caller token floor | ≥ 32 bytes | 256 bits, far beyond brute force — and it matches the 32-byte `getrandom` mint from Slice F W2, so the floor can never reject nosis's own token. |

**Design note worth keeping:** all seven **reject rather than truncate** (except `tools/list`, which
truncates loudly). Silent truncation would turn an attack into a subtly wrong answer — the failure mode
this project treats as worse than an error.

### 18. Longest-first sort vs a multi-pattern matcher (H-14)

**Known:** The ratified call is recorded (CURRENT_TASK.md:9); the comparative reasoning is not.

**Rationale (reconstructed 2026-07-25):** The bug is that replacing a shorter secret first can reveal the
remainder of a longer secret that contains it. A simultaneous multi-pattern matcher (Aho-Corasick) is the
textbook fix, and it was the wrong trade here: it means either a **new dependency** — against the
dependency budget — or a **hand-rolled automaton**, which is precisely the kind of clever security-path
code THE LAW's *auditable* clause exists to prevent. Sorting literals longest-first is roughly two lines,
provably eliminates the failure mode, and is testable in both insertion orders with overlapping secrets
(which the shipped tests do). The cost is O(n·m) instead of O(n+m) — irrelevant at the handful-of-keys
scale this operates on.

**The general principle:** prefer the boring fix whose correctness you can see, unless the input size
makes the clever one necessary.

### 19. Why the digest-bound approval renderer was deferred (H-11)

**Known:** Only the ratified label "minimal-honest approval (defer digest renderer)" exists
(CURRENT_TASK.md:27).

**Rationale (reconstructed 2026-07-25):** The digest renderer solves "the string shown at approval is
provably the string executed" by hash-binding the two. The actual finding was narrower — the approval
display **truncated the dangerous tail** of a long command. The minimal fix (cap 120→500, honest
"(+N more chars)", bidi/zero-width escaping) closes that finding completely, without introducing a
rendering abstraction shared across three surfaces (`nh-tools/src/mcp.rs`, `cmd_run`, `nh-tui`). At a
release gate, drop-if-hard favours the smaller correct fix over a cross-surface refactor.

**W7 later strengthened this from the other direction:** exec now requires explicit approval for *every*
non-`Block` verdict at the op boundary, so the display-binding carries less weight than the gate does.
The digest renderer remains a legitimate post-1.0 item, not a known hole.

### 20. The rejected variants of Q1 / Q2 / Q3

**Known:** The ratified outcomes are verbatim (CURRENT_TASK.md:59); the in-session alternatives were not
written down.

**Rationale (reconstructed 2026-07-25 from the shape of what was chosen — the in-session wording is not
recoverable):**

- **Q1 (credential binding).** Rejected: *host-only matching*, which ignores scheme and port, so a
  downgrade to plain `http` or a different port on the same host would silently inherit the credential.
  Also rejected: *allowing `http` generally*, which puts credentials on the wire in cleartext. Chosen:
  `https` + exact origin (scheme+host+port), with plain `http` permitted only for **literal** loopback —
  literal specifically because a DNS-based loopback check is attacker-influenceable.
- **Q2 (config trust).** Rejected: letting repo-local `.nosis/*.toml` **add** trust. Under that shape,
  cloning a repository could grant itself a credential audience, auto-trust an MCP server, or add a
  notify destination — a supply-chain foothold from a `git clone`. Chosen: repo config is
  **restrict-only** (may tighten, never introduce), with new trust only from user-global `~/.nosis`.
- **Q3 (unknown states).** Rejected: fail-open / best-effort — render `0.00` when cost is unknown, treat
  an unrecognized `finish_reason` as success. This converts an unknown into a confident false claim,
  which is the exact inversion of the product thesis. Chosen: fail-closed everywhere.

### 21. The pre-build "scoped MCP recommendation"

**Known:** The orchestrator was required to bring a scoped recommendation before building; the owner
ratified "MCP = FULL expansion" (CURRENT_TASK.md:3, :85). The scoped option's shape is unrecorded.

**Rationale (reconstructed 2026-07-25):** The scoped alternative would have hardened the existing three
tools (`route_resolve`, `fleet_run`, `fleet_status`) and stopped there — no `why`, no `route_cost`, no
`receipts`, no `structuredContent`. It was rejected because MCP is the surface where nosis's actual
differentiator becomes consumable by *other* agents: the meter and the receipt. Shipping only the fleet
plumbing would have exposed the least distinctive third of the product. The expansion also cost nothing
in public exposure, since `nh mcp serve` remains a loopback-only preview either way and is gated from
public release until the final spec lands 2026-07-28.

### 22. Why SSRF filtering stops at link-local + metadata literals

**Known:** Ratified label only (CURRENT_TASK.md:27).

**Rationale (reconstructed 2026-07-25):** The line is drawn where the **false-positive rate is zero**.

- *Blocking all private ranges* (RFC1918) was rejected because it breaks legitimate use: an MCP server on
  a LAN box or a corporate internal host is a normal, intended deployment. A security control that blocks
  the intended use gets disabled by users, which is worse than not having it.
- *DNS-time checks* were rejected because deciding after resolution introduces **TOCTOU** — resolve,
  decide, then connect can re-resolve to a different address — and puts a DNS dependency in the security
  path.
- **Link-local (169.254/16) and the cloud metadata address (169.254.169.254)** have no legitimate use as
  a user-configured MCP endpoint and are the highest-value target in the class, since metadata endpoints
  vend instance credentials.

So the filter covers the range where blocking is free and the payoff is highest, and declines to guess
elsewhere. Consistent with the same fail-closed-without-false-positives rule behind item 17.

---

## Era: standing / cross-cutting (draft E, 5 items)

### 23. Origin of THE LAW's ten words

**Known:** Every in-repo source presents the ten words (small, simple, secure, safe, lightweight,
readable, auditable, modular, congruent, harmonic) as already settled; it predates this repo. The
authoring moment and any rejected wording are unrecorded, and the owner does not recall.

**Rationale (reconstructed 2026-07-25) — what *is* verifiable is more useful than the origin story:** the
ten words demonstrably **decide things**, which is rare for a principles list. The record shows them
rejecting real proposals: Job Objects rejected to preserve `forbid(unsafe_code)` (*auditable*);
Aho-Corasick rejected for a two-line sort (*small*, *auditable*); new runtime crates refused except where
already compiled transitively (*lightweight*); features dropped at the gate under drop-if-hard (*simple*).

**Article angle:** a values list that has never rejected anything is decoration. The test of one is its
kill list — and this one has a documented one.

### 24. "NEVER two nosis codexes" — founding incident

**Known:** The rule and its operational context are well recorded (CURRENT_TASK.md:186); no source states
whether a concrete collision caused it or it was preventive. The owner does not recall.

**Rationale (reconstructed 2026-07-25) — the rule is load-bearing regardless of origin:** two `codex exec`
sessions running `-s workspace-write` against one working tree would interleave edits to the same files
with no locking. Worse, it would destroy the orchestrator's **verification method**: after every wave the
orchestrator confirms scope by checking file mtimes to prove only the authorized files were touched. With
two concurrent writers that check becomes meaningless, and "Sol stayed in scope" becomes unprovable. The
rule protects the audit trail, not just the bytes.

### 25. Restricted tokens (Decision 8, second half)

**RESOLVED 2026-07-25 — struck.** Owner ratified: remove the promise and state what actually exists.
`RISK_REGISTER.md` now describes the real containment (exec refused on `Block` and requiring explicit
approval for every other verdict at the op boundary, law `exec_block` patterns, null stdin, 300s
deadline, env allowlist, verified `taskkill /T /F` with honest survival reporting, filesystem
symlink-rejection + canonical containment, `[send]`-gated egress with https+exact-origin audiences, and
`unsafe_code = "forbid"` workspace-wide) and states plainly that **v1 ships no OS-level sandbox**, that
Job Objects were rejected 2026-07-24 for requiring `unsafe`, and that restricted tokens are not
implemented.

### 26. When the FEEL gate became a formal, named gate

**Known:** The practice is documented from M3 onward; the term "FEEL gate" appears from M5 Slice C/D
onward; no single document declares it.

**Rationale (reconstructed 2026-07-25):** The gate was **created by an act and named later.** It was
created at M3 when the owner rejected a content-complete, test-passing, artifact-clean TUI purely on how
it felt to use — reopening M3 into Slices D/E/F (`3fcd00e`). That rejection established the standing rule
that green tests are necessary and not sufficient. The *name* was applied retroactively once the pattern
had repeated enough to need one. See item 3 — the "CodeWhale bar" and the FEEL gate are the same event
viewed from two sides.

**Article angle:** the most important gate in the project has no specification, was never designed, and
exists because a human said "no" to something that passed every automated check.

### 27. Fable 5's docs role — ratification moment

**Known:** The role is well evidenced in practice (commit `202bdca`; memory: build-loop-resume) but no
dated owner directive establishes it, unlike the 2026-07-13 Sol directive.

**Rationale (reconstructed 2026-07-25):** The division **emerged from capability fit rather than being
ratified**: code changes require a gate-verifiable diff against a frozen contract (Sol, one wave at a
time, serialized), while docs are parallelizable, individually verifiable against the CLI, and carry no
merge conflicts (Fable 5, fan-out). The clearest evidence is `202bdca`, where five writers each
independently accuracy-verified their output against the real CLI — a shape that only works for docs.

**For a launch post: describe this as an emergent division of labour, not a decision.** There is no
moment to point at.

---

## Flagged doc conflicts — status as of 2026-07-25

| Conflict | Status |
|---|---|
| `AGENTS.md:20` / `CODEX.md:7` "Terra by default" | **FIXED 2026-07-25** — both now state `gpt-5.6-sol` at `max` as the default implementer, noting the 2026-07-13 supersession. |
| `PRODUCT_BRIEF.md:9` delegate pitch | **FIXED 2026-07-25** — see item 6. |
| `ARCHITECTURE_DECISIONS.md` Decision 8 (Job Objects + restricted tokens) | **CLOSED 2026-07-25** — dated amendment applied 2026-07-24; `RISK_REGISTER.md:11` now also corrected (see item 25). |
| `RISK_REGISTER.md` review-debt row ("no direct-to-main") | **FIXED 2026-07-25** — rule was never adopted; now states the gate + per-wave adversarial review as the real mitigation. See item 8. |
| `RISK_REGISTER.md` MCP row ("Pin SDK to frozen RC, conformance check in CI") | **FIXED 2026-07-25** — no SDK exists to pin (hand-rolled client, `tiny_http` server) and CI has no conformance job. Now states the real mitigation: loopback-only preview, no public server before 2026-07-28. |
| `RISK_REGISTER.md` two delegate-quota rows | **FIXED 2026-07-25** — closed as "feature cut from v1". |
| `ARCHITECTURE_DECISIONS.md` Decisions 4, 5, 7 | Dated amendments applied 2026-07-24. |
| `SECURITY.md` "no critical problems" | **STILL OPEN — release blocker.** Written 2026-07-20, outdated one day later by the 2026-07-21 audit (2 critical / 14 high). Must be rewritten before the tag to name both audits, their real findings, and the remediation commit — and only after C-01/C-02 are verified closed by Slice G. |
| `nh mcp serve` `print_banner` still hints 3 tools | **STILL OPEN** — cosmetic, audit nit N-01. `tools/list` is authoritative and returns all 6. |
| `CONTRACTS_M3.md` §9.2 mouse capture | **CLOSED 2026-07-25** — retroactive amendment written. See item 4. |
| `CONTRACTS_M5.md` §6 / §Slice E `wip/<slice>` | **CLOSED 2026-07-25** — amendment records it as never adopted, superseded by the gate. See item 8. |
