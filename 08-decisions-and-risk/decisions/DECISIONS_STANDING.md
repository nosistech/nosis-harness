# DECISIONS — Standing, Cross-Cutting (2026-07-12 → present)

Era covered: standing decisions that shape the whole project — product/positioning, process/build model, and the engineering constitution (the draft's three thematic groups, flattened here). 13 entries; entries are newest-first by the date in each heading ("standing since" entries sort by origin date). Index: see `../../00-start-here/DECISION_LOG.md`.
Conventions: file paths are repo-relative to `C:\Users\capv2\Desktop\nosis-Harness` unless absolute; "memory:" cites a file in `C:\Users\capv2\.claude\projects\C--Users-capv2-Desktop-nosis-Harness\memory\`. Memory files are point-in-time; where they conflict with the repo, the repo is preferred and the conflict is flagged.

---

## 2026-07-19, escalated to HARD rule 2026-07-23: every options-presentation carries a recommendation + why + the tradeoff — never a bare menu, never a silent decision

**Decision:** Project-wide hard rule: EVERY time options are presented to the owner — prose forks,
question menus, authorization asks — the presenter MUST state its recommendation, the reasoning,
and the tradeoff accepted. No bare menus; equally, no deciding silently. The owner is the design
authority and ratifies; the orchestrator recommends.
**Alternatives considered:** the two failure modes the rule excludes are named in the source:
a neutral option list, and a fait accompli (memory: decisions-explain-rec-then-owner-decides.md).
**Why:** Owner's stated reason: he "is the design authority and wants to actually decide, but with
your reasoning in front of them." First given 2026-07-19; escalated 2026-07-23 with the quote
"every time you present me options to chose from, you have to tell me your recommendation and why."
**Immediate effect on the harness:** Every Slice G design call was run this way — e.g. the Wave 6
split (Q1 finest-split, Q2 MCP trust, Q3 SSRF depth, Q4 approval rendering) and Wave 7's R1/R2 all
record "ratified (recommended option)" (memory: slice-g-audit-remediation.md:29;
`Temp/sol_wave7_prompt.txt`:119-136 shows the resulting ratified-decision format, including the
rejected alternative and accepted residual risk in writing).
**Long-term consequence:** Every ratified decision now carries its rejected alternative and its
accepted residual risk as a written artifact — which is precisely what makes a decision record
like this one reconstructable.
**Evidence:** memory: decisions-explain-rec-then-owner-decides.md (dates + owner quote);
`Temp/sol_wave7_prompt.txt` RATIFIED DECISIONS section (the format in practice).
**Article angle:** The human stays the design authority by rule, but the machine is required to
argue for one option and name its cost before the human chooses.
**Review later:** no.

---

## 2026-07-20: `unsafe_code = "forbid"` workspace-wide — zero in-tree unsafe

**Decision:** `[workspace.lints.rust] unsafe_code = "forbid"` in the root `Cargo.toml`, inherited
by all 9 crates via `[lints] workspace = true`, shipped in Release Slice Section B. There is zero
in-tree unsafe; the lint makes adding any a hard compile error.
**Alternatives considered:** none recorded at adoption time (the workspace already had zero
unsafe; the lint codified the status quo — memory MEMORY.md Section B). The decision's real test
came later: see the Job Object rejection below.
**Why:** Part of the Section B "engineering tail" hardening for the public release (forbid-unsafe +
workspace lints + MIT license + cargo-deny gate + keyless CI in one commit). Serves THE LAW's
"secure, safe, auditable": the property "no unsafe anywhere" is machine-checked, not asserted.
**Immediate effect on the harness:** Every Sol brief since carries it as a hard constraint
("ZERO unsafe. There is no in-tree unsafe and this wave must not add the first" —
`Temp/sol_wave7_prompt.txt`:22-23).
**Long-term consequence:** Forecloses whole solution classes that need raw OS APIs — proven
consequential on 2026-07-24 when a kill-on-close Windows Job Object was rejected specifically
because it "needs raw Win32 calls, which means a new dependency AND `unsafe`, and that breaks the
workspace `unsafe_code = "forbid"` law shipped in d1f9ad0" (`Temp/sol_wave7_prompt.txt`:121-127,
ratified decision R1, with the accepted residual risk stated in writing).
**Evidence:** `Cargo.toml`:20-21; commit `d1f9ad0` (2026-07-20); `Temp/sol_wave7_prompt.txt`:20-23,
119-127.
**Article angle:** The no-unsafe rule was cheap to adopt and expensive to keep — four days later it
forced the project to reject the textbook Windows containment primitive and document the residual
risk instead.
**Review later:** yes — trigger: if a future Windows containment need (Job Objects, restricted
tokens) is judged worth a vetted unsafe-bearing dependency, this lint is the decision to reopen,
with the owner.

---

## standing since ~2026-07-20: the 250k-context checkpoint protocol

**Decision:** At roughly 250k context, the orchestrator STOPS and hands the owner a self-contained
continuation prompt (a checkpoint block written into `CURRENT_TASK.md` plus refreshed memory);
the owner saves it, runs `/clear`, and types `continue` — the fresh session resumes seamlessly
from the recorded state.
**Alternatives considered:** none recorded in sources.
**Why (as sourced):** Recorded as an owner-defined protocol: "Checkpoint protocol (owner): at
~250k context orchestrator STOPS + hands a self-contained continuation prompt; owner saves +
`/clear` + types `continue` → seamless resume" (`00-start-here/CURRENT_TASK.md`:3). The underlying
rationale (context-window exhaustion mid-wave) is implied by the mechanism, not separately argued.
**Immediate effect on the harness:** `CURRENT_TASK.md` accumulated a dated stack of checkpoint
blocks (sessions 4, 5, 5b, 6, 7, 7b, 7c — :5-79), each the actual resume anchor for the next
session; the pattern extends the older per-milestone resume-anchor practice
(`00-start-here/AUTONOMOUS_HANDOFF.md`, written 2026-07-13 "just before a /clear", is the
protocol's ancestor).
**Long-term consequence:** Multi-day, multi-session builds (Slice G ran across at least sessions
5–7c) proceed without state loss; project state lives in files, not in any one conversation.
**Evidence:** `00-start-here/CURRENT_TASK.md`:3, 5-79 (checkpoint blocks);
`00-start-here/AUTONOMOUS_HANDOFF.md`:1-3; MEMORY.md index ("250k-checkpoint protocol active").
**Article angle:** The orchestrator treats its own context window as a resource to be checkpointed
like any other, writing a self-contained resume file before every clear.
**Review later:** no.

---

## 2026-07-18 (fmt/pin) + 2026-07-20 (deny): the gate as sole arbiter — 4 steps, pinned toolchain, and Sol never runs `cargo fmt`

**Decision:** One script, `gate.ps1`, defines "clean" for the workspace and must pass before every
commit: (1) `cargo fmt --all --check`, (2) `cargo clippy --workspace --all-targets --release --
-D warnings`, (3) `cargo deny check`, (4) `cargo test --workspace --release`. Exit codes are
captured per step via `$LASTEXITCODE` and aggregated — never piped through `tail`. The toolchain
is pinned to 1.96.0 (`rust-toolchain.toml`) so fmt/clippy results are identical everywhere.
Standing rule: Sol must NEVER run `cargo fmt` — formatting is the orchestrator/gate's job; the
orchestrator runs any normalizing fmt after a Sol wave and re-gates.
**Alternatives considered:**
- Letting the implementer format its own diffs — rejected after a real incident: a
  `cargo fmt --all` inside an M5 Slice A gate command reformatted frozen crates and blew a ~6-file
  changeset up to ~19 files; even scoped `cargo fmt -p` reflowed pre-existing code because the
  workspace had never been fmt-clean (memory: cargo-fmt-all-pitfall.md).
- Living with fmt drift — rejected: root-fixed by a one-time `cargo fmt --all` normalization
  (commit `bc2a1b1`), then locked with the fmt-check gate step and the 1.96.0 pin (commit `a71eb23`).
- Suppressing cargo-deny findings to get green — not done: "nothing suppressed — only added
  CDLA-Permissive-2.0 for webpki-roots" (memory MEMORY.md, Section B; commit `d1f9ad0`).
**Why:** The gate mechanizes THE LAW's mechanical half so the human FEEL gate is the only
subjective step; the pipe rule exists because "a pipeline's exit code is the last command's, so
`| tail` would mask a real failure with tail's 0" (`gate.ps1`:9-11); the pin exists because "a
newer rustfmt" could "silently re-introduce the reflow drift we just cleared"
(`rust-toolchain.toml`:1-4).
**Immediate effect on the harness:** Every wave since reports a gate verdict as its headline
(363/0/1 → … → 493/0/1 `--release`); fmt drift from Sol's hand-written code is caught by step 1
and normalized by the orchestrator, never by Sol (recorded per-wave in memory:
build-loop-resume.md and slice-g-audit-remediation.md). CI mirrors the gate keylessly on
windows + ubuntu (commit `d1f9ad0`, `.github/workflows/ci.yml`).
**Long-term consequence:** "Gated PASS" is a reproducible, single-command claim; supply-chain
policy (advisories/bans/licenses/sources) is enforced on every commit, not just at release.
**Evidence:** `gate.ps1`:1-51 (4 steps at :36-39); `rust-toolchain.toml`:1-8; commits `bc2a1b1`,
`a71eb23`, `d1f9ad0` (adds the deny step, `gate.ps1` +1 line); memory: cargo-fmt-all-pitfall.md.
**Article angle:** A formatting incident that polluted one diff got a root-cause fix — normalize
once, gate forever, pin the toolchain, and ban the implementer from ever running the formatter.
**Review later:** no (the pin will need a deliberate bump eventually; that is its purpose, not a
defect).

---

## standing (recorded from Slice F onward): NEVER two nosis codexes at once

**Decision:** At most ONE nosis-owned `codex exec` process runs at a time. Before launching a Sol
wave, confirm which codex sessions belong to the orchestrator; the owner's own codex TUI sessions
(observed as PIDs 19820, 50340, later 47112) are his and must be left alone.
**Alternatives considered:** none recorded (parallel Sol waves were never attempted; waves are
explicitly "ONE wave at a time" — memory: build-loop-resume.md session-4 exec model).
**Why (as sourced):** The rule pairs with the one-wave-at-a-time execution model — each Sol wave
owns an enumerated mutable surface of the single shared working tree, and scope verification after
each run (`git diff --numstat`, file mtimes) assumes exactly one writer. The recorded operational
hazard is misidentifying the owner's own codex processes as orchestrator runs
(`00-start-here/CURRENT_TASK.md`:186: "confirm which sessions are yours before launching a second
codex; NEVER two *nosis* codexes at once"). A fuller incident-style rationale is not written down
(see UNSOURCED).
**Immediate effect on the harness:** Every launch checklist and checkpoint block carries the rule
(`CURRENT_TASK.md`:182-186, 452; memory: build-loop-resume.md, slice-g-audit-remediation.md:39).
**Long-term consequence:** Serializes implementation waves; parallelism is achieved only across
disjoint roles (Sol code ∥ Fable docs), never across two code writers.
**Evidence:** `00-start-here/CURRENT_TASK.md`:182-186, 452; memory: build-loop-resume.md;
memory: slice-g-audit-remediation.md:39.
**Article angle:** The pipeline enforces a single-writer rule on the working tree: one implementing
model at a time, so every diff has exactly one author to hold accountable.
**Review later:** no.

---

## 2026-07-17: the best-in-category thesis — honest metered agent for open-weight models on Windows; moat = the router inside the harness

**Decision:** The category nosis competes in (and defines) is stated precisely: "the honest,
visible, auditable metered agent for open-weight models — native on Windows." The claimed moat is
structural, not a feature: the counterfactual savings line can only be printed by a harness whose
router lives inside it and sees its own cache warmth, clock window, thinking budget, and running
spend. Deliverable: ≥5 launch posts, written only when M5 is shipped + FEEL-approved, with every
published claim gated by an evidence matrix (live/mock/security/cost/UX/Windows).
**Alternatives considered:**
- Competing as "best AI agent" — rejected (benchmark-ceiling incumbents' game;
  `WHY_BEST_IN_CATEGORY_2026.md`:15-17).
- Pulling M6 features (learning router, resilience, resume) into M5 to be "more best" — rejected:
  "exactly the mess to avoid — and it would delay the beachhead" (:52-54).
**Why:** Claude Code/Codex have cost opacity and can't print the savings line; OpenRouter/proxies
aggregate access but "aren't the harness, so they can't see cache warmth, clock windows, or
budget." Both numbers in the savings line come from one `catalog.toml` and one `receipts.jsonl` —
"honest by construction" (:29-36). The explicit caveat: never oversell, "honesty is the brand" —
M5 claims best only on honesty + visibility + safety (:47-50).
**Immediate effect on the harness:** Shaped the M5 build order — "the win is Slice C (the meter
made visible) sitting on Slice B (a floor you can trust)"; the thin routing (Slice A) only closes
an integrity gap (:38-42). The file doubles as a living article-seed backlog.
**Long-term consequence:** Locks a two-milestone arc (M5 beachhead, M6 moat) and an evidence
discipline for all marketing: aspirational claims stay in the seed backlog until demonstrable
(:129-135).
**Evidence:** `01-product/WHY_BEST_IN_CATEGORY_2026.md` (whole file, started 2026-07-17);
memory: why-best-in-category-2026.md; savings-line mechanics also in `README.md`:3.
**Article angle:** The positioning claims a structural rather than feature moat — the cost
counterfactual is only computable by a router that lives inside the harness it meters.
**Review later:** yes — the posts themselves are triggered by "M5 done + FEEL-approved"; each claim
must pass the evidence matrix at write time.

---

## standing since 2026-07-16 (research) / 2026-07-17 (ratified in CONTRACTS_M5): product identity — "a harness with a meter", explicitly not a chat UI

**Decision:** The product identity is fixed as: "nosis is the agent harness with a meter: it routes
every task to the cheapest CAPABLE model — by clock, cache, modality, and thinking budget — and
hands you the receipt." The README states the negative half publicly: "It is a harness with a
meter, not a chat UI." Every M5 seam was required to be congruent to this identity.
**Alternatives considered:**
- "Best AI coding agent" positioning — rejected: "that's the benchmark-ceiling incumbents' game"
  (`01-product/WHY_BEST_IN_CATEGORY_2026.md`:15-17).
- A pre-run forecast / `cost_estimate` feature — rejected as scope spend "flirting with M6's verb";
  "One verb — meter. No second verb." (`CONTRACTS_M5.md`:17, 22-23;
  `WHY_BEST_IN_CATEGORY_2026.md`:43-44).
- Keeping subscription delegates (Claude/Codex/Gemini headless) as a marquee product pillar —
  demoted to an escalation-gate footnote; "full delegate adapter class cut" from v1 because the
  economics broke (Anthropic moved programmatic use to API pricing 2026-06-15; Gemini CLI died as
  an open delegate 2026-06-18) and open-weight parity made it unnecessary
  (`00-start-here/RESEARCH_2026-07_harness.md`:37, 61, 314; commit `d3cac39`;
  `00-start-here/CURRENT_TASK.md`:13 "the delegate class is CUT from v1").
**Why:** Two independent research engines (Fable 5 with July-2026 web citations; GPT-5.6 Sol xhigh
over the code) converged on the same identity and the same priority — "make the meter true +
visible + safe before adding autonomy or providers." That convergence is called "the spine" in the
contract (`CONTRACTS_M5.md`:8-10; `RESEARCH_2026-07_harness.md`:45).
**Immediate effect on the harness:** M5 was scoped as "The Honest Meter" with five slices all
serving one verb; forecast/cost_estimate held out; new providers and autonomy held out. The
identity line was "held firm" through the release slice (`CURRENT_TASK.md`:3).
**Long-term consequence:** Forecloses drifting into a chat-product feature race; makes the receipt
and the counterfactual savings line the product's center of gravity; defers intelligence
(learning router), resilience, and resume to M6 by design.
**Evidence:** `README.md`:3; `CONTRACTS_M5.md`:12-18, 20-29;
`00-start-here/RESEARCH_2026-07_harness.md`:45-46; commit `d3cac39` (2026-07-16, research) and
`e2b2f02` (2026-07-17, "lock CONTRACTS_M5 'The Honest Meter'").
**Article angle:** Two different AI models researching independently converged on the same
one-sentence product identity, and it was then enforced as a contract constraint on every code seam.
**Review later:** yes — the delegate cut keeps a commented catalog schema
(`catalog.toml` `[routes.claude-opus-4-8]` stub, `CURRENT_TASK.md`:13) so the class can return if a
measured workload proves the escalation gate insufficient (`RESEARCH_2026-07_harness.md`:101).

---

## 2026-07-16 (fix `7faf44b`): the identity/honesty constitution applies at EVERY agent surface

**Decision:** The honest-identity system prompt (`identity_constitution` — "You are nosis …
running on '<route>' via <provider> … never claim to be Claude, GPT, or any other assistant") is
applied at every surface that talks to a model: TUI, `nh run`, and `nh chat` (chat also rewrites
`history[0]` on `/model` switch). Standing rule going forward: apply it at every future agent
surface (any new run/chat/tui/mcp entry point).
**Alternatives considered:** none recorded — this was a bug fix to a binding prior UX decision,
not a design fork. The defect: the prompt lived only in `crates/nh-tui/src/lib.rs`, so `nh run`
and `nh chat` sent the raw law constitution with no identity wrapper.
**Why:** Found 2026-07-16 while battery-testing real providers: with receipts confirming correct
routing, `deepseek-v4-flash`/`-pro` and `mimo-v2.5`/`-pro` answered "I am Claude … Claude Sonnet 4"
in `nh run`/`nh chat` (training contamination); only Kimi self-identified honestly. "An agent lying
about who it is fails the honesty bar" — honest identity was already a binding owner decision, and
it was only half-implemented (memory: identity-guard-tui-only.md).
**Immediate effect on the harness:** Commit `7faf44b` (2026-07-16, 3 files: cmd_chat.rs +28,
cmd_run.rs +3, nh-tui lib.rs +5) made `identity_constitution` pub and applied it in both CLI
surfaces; verified live — DeepSeek and MiMo then answered "nosis on <route> … not Claude." Current
tree: `crates/nh-tui/src/lib.rs`:1187 (definition), `crates/nh-cli/src/cmd_run.rs`:162,
`crates/nh-cli/src/cmd_chat.rs`:199 and :415 (re-applied on `/model` switch),
`crates/nh-tui/src/worker.rs`:241, :286.
**Long-term consequence:** Converts a one-off fix into a surface-coverage invariant — the durable
lesson recorded is the rule, not the patch: any control that exists per-surface must be audited at
ALL surfaces. The same failure shape recurred and was caught again in Slice G W6c (H-03: the
credential scrubber refreshed at only some route-change sites; fixed across BOTH agent surfaces —
memory: slice-g-audit-remediation.md:35).
**Evidence:** commit `7faf44b` + docs commit `bd35b4d`; memory: identity-guard-tui-only.md
(contamination map with per-model transcript results); current-code lines above.
**Article angle:** Open-weight models tested through the harness claimed to be Claude until a
per-surface honesty prompt was made universal — and the lasting fix was a rule about surfaces, not
a prompt.
**Review later:** yes — trigger: any new agent-facing entry point (e.g. a future MCP-served chat
surface) must wire `identity_constitution` before shipping.

---

## standing since 2026-07-13: division of labour — Claude orchestrates, Sol implements, Fable writes docs; Claude does NOT hand-write milestone code

**Decision:** The Claude session is the ORCHESTRATOR: "Plans, writes per-milestone briefs + locked
contracts, runs verification gates, adversarially reviews, gates, commits, writes docs. Does NOT
hand-write milestone implementation code." GPT-5.6 Sol (max/xhigh effort, headless via
`codex exec`) writes all milestone implementation code. Fable 5 (high) writes documentation in
parallel background workflows. One human (the owner) directs and ratifies.
**Alternatives considered:**
- GPT-5.6 Terra as default implementer with Sol reserved for the hardest/security work — this WAS
  the earlier decision (`00-start-here/DECISION_LOG.md` 2026-07-11: "Terra by default; Sol for M2
  context engine and anything touching nh-law/security") and was superseded 2026-07-13 by
  "Sol for everything" (memory: m2-m5-codex-sol-directive.md). CONFLICT FLAG:
  `05-ai-collaboration/AGENTS.md`:20 and `CODEX.md`:7 still carry the stale "Terra by default"
  wording; the newer repo file `00-start-here/AUTONOMOUS_HANDOFF.md`:7-8 and all practice since M2
  reflect Sol-only.
- Claude implementing directly — foreclosed by the role definition itself
  (`AUTONOMOUS_HANDOFF.md`:7); the only recorded Claude-authored code changes are the 7faf44b
  identity bugfix ("Carlos-directed, Claude-authored bugfix — NOT a milestone/Sol slice", memory:
  identity-guard-tui-only.md) and small orchestrator glue/mechanical fixes during gating (e.g. the
  `.read(true)` OpenOptions fix in Slice G Wave 3, the 1-line cmd_chat H-03 mirror in W6c —
  memory: slice-g-audit-remediation.md).
**Why:** "Matches the master plan's build loop (Claude plans → Codex implements → Opus-class gates)
and plan §A.7 which designates Sol for the hardest work" (memory: m2-m5-codex-sol-directive.md).
The separation also keeps the reviewer independent of the implementation — the orchestrator
adversarially reviews code it did not write.
**Immediate effect on the harness:** Every feature commit since M2 names Sol as implementer in the
body; the orchestrator's post-Sol cycle (scope-check via `git diff --numstat`/mtimes → gate →
normalize fmt → adversarial review → owner → commit) became the standard loop (memory:
build-loop-resume.md).
**Long-term consequence:** Enables the "how a one-person shop ships an auditable Rust agent" story
(`WHY_BEST_IN_CATEGORY_2026.md`:123-124, an article seed). Locks in per-wave adversarial review as
a structural feature, not a courtesy.
**Evidence:** `00-start-here/AUTONOMOUS_HANDOFF.md`:5-9, 23-31; `05-ai-collaboration/MODEL_ROLES.md`;
memory: m2-m5-codex-sol-directive.md; docs commit `202bdca` ("drafted by a Fable 5 ultracode docs
workflow — 5 writers, each independently accuracy-verified", memory: build-loop-resume.md).
**Article angle:** Three different AI models hold three fixed, non-overlapping roles — planner/gate,
implementer, docs writer — and the planner is contractually barred from writing the product code.
**Review later:** yes — DECISION_LOG 2026-07-11 already marks the implementer choice "review when
quotas or a newer family land"; also the stale "Terra by default" text in
`05-ai-collaboration/AGENTS.md`/`CODEX.md` should be reconciled.

---

## 2026-07-13: after M1, ALL milestones implemented by Sol at max/xhigh via `codex exec`; STOP rather than silently fall back

**Decision:** Standing owner directive: once M1 is done, the entire remainder (M2 context engine,
M3 TUI, M4 fleet/mcp, M5 hardening + launch) is implemented by GPT-5.6 Sol at xhigh (later max)
effort, driven headless via `codex exec` — not by Claude agents and not by Terra. Two hard stop
rules: if `gpt-5.6-sol` stops resolving in the Codex binary, STOP and tell the owner (never
silently fall back to Terra); if Sol fails the same gate twice, STOP and report (never loop).
**Alternatives considered:**
- Terra fallback on Sol unavailability — explicitly prohibited
  (`00-start-here/AUTONOMOUS_HANDOFF.md`:36: "do not silently fall back to Terra").
- Retry-until-green — prohibited by the two-failure stop rule (:37, "plan's escalation rule").
**Why:** Sol is designated for the hardest work (plan §A.7 via memory:
m2-m5-codex-sol-directive.md); the stop rules exist so the owner always knows which model actually
built the code and why a gate is failing — silent substitution would corrupt both the audit trail
and the quality bar.
**Immediate effect on the harness:** All of M2–M5, Slice F, and Slice G were built by Sol runs
(codex run IDs recorded per wave in memory: slice-g-audit-remediation.md). The stop discipline
worked in practice as clean-stops on real scope conflicts: Slice D handoff #1, Slice F W1's m2_exit
fixture, the frozen `e3_korvin.rs` 3-tool assertion, W6a's struct-field ripple, W6b's false-stop on
an in-file test literal — each resolved by owner-authorized amendment, never by Sol improvising
(memory: build-loop-resume.md, slice-g-audit-remediation.md).
**Long-term consequence:** Makes the implementer identity part of the provenance record (commit
bodies name Sol); makes contract amendments — not implementer judgment — the only path around a
frozen boundary.
**Evidence:** memory: m2-m5-codex-sol-directive.md (directive 2026-07-13, SOL-READY smoke test,
2,938 tokens); `00-start-here/AUTONOMOUS_HANDOFF.md`:8-9, 13-21, 36-37.
**Article angle:** The build process treats implementer-model substitution as a stop-the-line
event, on the grounds that an audit trail of who built what is worthless if the "who" can silently
change.
**Review later:** yes — same trigger as above (model family / quota changes).

---

## standing since 2026-07-12: THE LAW as design constitution, with UX-first + drop-if-hard on top

**Decision:** Every change in the project is judged against a ten-word constitution — "small,
simple, secure, safe, lightweight, readable, auditable, modular, congruent, harmonic" — named THE
LAW and declared "top authority" in both agent instruction files. Layered on top: UX/UI is the #1
product priority ("UX IS THE PRODUCT"), with an explicit drop-if-hard rule — a feature that cannot
be made to feel good gets dropped, not shipped rough.
**Alternatives considered:** none recorded in sources. THE LAW arrives fully formed in the earliest
project documents; its word-by-word origin predates the repo (see UNSOURCED).
**Why:** Owner's stated reasoning (2026-07-12, restated forcefully 2026-07-14): a technically
superior harness with bad UX fails its own thesis — the 7 differentiators include "UX that fixes
the documented pain" (approval fatigue, cost opacity, ambiguous status), and "without the best
UX/UI no one uses the product no matter how revolutionary." Passing a mechanical bar is not "done"
if it does not feel good — the owner rejected an artifact-clean M3 TUI on UX grounds alone
(memory: ux-first-and-the-law.md).
**Immediate effect on the harness:** THE LAW is cited in every contract, every Sol brief (e.g.
`Temp/sol_wave7_prompt.txt` constraint 8 quotes it verbatim), and every review. It produced a
FEEL gate distinct from the test gate: milestones with user-facing surfaces are held after all
tests pass until the owner approves how they feel (M5 Slice C/W4 are explicitly "FEEL-gated" —
memory: build-loop-resume.md). Drop-if-hard was exercised concretely: W4-20 (nit-16 zeroize)
was dropped because it needed the W2-frozen `ServeConfig.token` (memory: build-loop-resume.md,
session-4 block).
**Long-term consequence:** Locks in a two-gate release model (mechanical gate + human FEEL gate)
and a permanent bias toward cutting scope over shipping friction. It also became the audit rubric:
the 2026-07-21 pre-release audit scored the workspace against THE LAW clause by clause
(`04-research/AUDIT_2026-07-21_sol-max_pre-release.md`; memory: slice-g-audit-remediation.md).
**Evidence:** `AGENTS.md` (repo root) :5-7; `05-ai-collaboration/AGENTS.md`:13-15;
`00-start-here/AUTONOMOUS_HANDOFF.md`:34-35 ("UX IS THE PRODUCT (Carlos's #1 rule) … drop-if-hard");
`CONTRACTS_M1.md`:22-23; memory: ux-first-and-the-law.md.
**Article angle:** The project's acceptance criterion is a ten-word constitution plus a human
"feel" gate that can veto a fully passing build — and has.
**Review later:** no.

---

## standing since 2026-07-12 (Decision 8): Windows-first as a deliberate wedge

**Decision:** First-class native Windows 11 support (crossterm, tested renderer matrix), with
honest documentation that full syscall sandboxing is Linux-only. Windows containment is stated
honestly rather than faked.
**Alternatives considered:** none recorded — Decision 8 lists no alternatives; the tradeoff row
accepts "weaker containment on Windows in v1, stated honestly rather than faked"
(`02-architecture/ARCHITECTURE_DECISIONS.md`:105-118).
**Why:** "Both dev machines run Windows 11; incumbent sandboxes still don't support native
Windows — a real wedge" (Decision 8 Why). Repeated in the positioning: "Windows-first native
support — a wedge nobody serves" (`12-executive/ONE_PAGE_SUMMARY.md`:27) and in the category
definition itself ("— native on Windows", `WHY_BEST_IN_CATEGORY_2026.md`:18).
**Immediate effect on the harness:** Windows is the dev/test box; Windows-specific correctness
work shipped repeatedly (Slice F W2 `raw_arg` verbatim exec on Windows, `taskkill /T /F` tree
kill — memory: build-loop-resume.md; Slice G Wave 3's Windows `LockFileEx` fix — memory:
slice-g-audit-remediation.md). CI runs windows + ubuntu (commit `d1f9ad0`, `.github/workflows/ci.yml`).
**Long-term consequence:** The wedge is real but the containment story evolved: Decision 8's
promised "Job Objects + restricted tokens" was later contradicted by the ratified
`unsafe_code = "forbid"` constitution — see the dated amendment in
`02-architecture/ARCHITECTURE_DECISIONS.md`. Windows containment in the
shipped tree = approval gate at the exec boundary + env allowlist + timeout + verified tree-kill
with honest failure reporting (`Temp/sol_wave7_prompt.txt` R1).
**Evidence:** `02-architecture/ARCHITECTURE_DECISIONS.md`:105-118;
`12-executive/ONE_PAGE_SUMMARY.md`:27; `08-decisions-and-risk/RISK_REGISTER.md`:11;
`01-product/PRODUCT_BRIEF.md` (customer: "especially on Windows").
**Article angle:** The project targets the platform its own author uses daily and incumbent agent
sandboxes don't natively support, and documents the weaker containment instead of claiming parity.
**Review later:** yes — Decision 8's containment wording needs the dated amendment (now applied in
`02-architecture/ARCHITECTURE_DECISIONS.md`).

---

## standing since pre-M0 (2026-07-12): contracts-first — freeze the public surface in CONTRACTS_M*.md before implementation; amendments numbered and dated

**Decision:** Before each milestone is implemented, the orchestrator writes a LOCKED
`CONTRACTS_<Mx>.md` enumerating the exact public surface: "Builders implement EXACTLY these
surfaces; private helpers are free, public deviations are not." Deviations require a dated,
numbered amendment (M1: §7 additive-only with orchestrator authority; M5: §8, `A-M5-1` …
`A-M5-9`, each dated, attributed, and scoped to a seam table). From M5 on, the contract also
enumerates the mutable surface UP FRONT (§0.1) so the implementer never faces a
break-scope-or-duplicate choice.
**Alternatives considered:**
- Re-convening the original architect for every post-hoc gap — rejected 2026-07-13: "blocking a
  green integration to re-convene the architect adds process without value (THE LAW: simple)";
  instead, additive-only orchestrator amendments land in the contract with a date
  (`00-start-here/DECISION_LOG.md` 2026-07-13, including the acknowledged tradeoff: "two
  ratification authorities for one contract").
- Additive-only contracts for a bug-fix milestone — rejected for M5: "M5 canNOT stay 'additive
  only' like M4 — fixing the meter bugs changes what the wire sends," so behavior-corrections were
  authorized but enumerated seam-by-seam, each pinned by a new test (`CONTRACTS_M5.md`:27-29).
**Why:** The frozen surface is what makes a headless implementer safe: Sol can be told to STOP
CLEAN at any boundary the contract didn't open, and every stop is resolved by a written amendment
(the "A-M4-1 lesson" is named in `CONTRACTS_M5.md`:35). The amendment trail keeps the audit intact.
**Immediate effect on the harness:** Contracts exist for M1–M5 (`CONTRACTS_M1.md` … `CONTRACTS_M5.md`);
locked API contracts predate M0 code (commit `dfbbf26`, 2026-07-12: "workspace scaffold with locked
API contracts"). The amendment mechanism ran hot in M5: A-M5-2 (enum-variant compile ripple),
A-M5-5 (owner-ratified trusted audience source), A-M5-7 addendum (blanket clause so trivial
field-add glue never stops Sol again), A-M5-8 (the ONE authorized reopening of frozen nh-fleet),
A-M5-9 (wire-aware effort) — `CONTRACTS_M5.md` §8.
**Long-term consequence:** Every public-surface change in the project's history is reconstructable
from contract + amendment + commit; frozen crates stay byte-stable across waves (verified per wave
in memory: build-loop-resume.md).
**Evidence:** `CONTRACTS_M1.md`:1-8; `CONTRACTS_M5.md`:1-5, 33-40, 545ff (§8 amendments A-M5-1..);
`00-start-here/DECISION_LOG.md` (2026-07-13 entry); commits `dfbbf26`, `0bd1d7f`, `fe54e48`.
**Article angle:** The public API of each milestone is frozen in a contract before any
implementation model touches the code, and every deviation since is a numbered, dated amendment in
the same file.
**Review later:** no.
