# Decisions — M2 + M3 (2026-07-13 → 2026-07-15)

Era file: M2 (context engine + law, commit `3155949`) and M3 (TUI, commits `f45fb02`..`d5143c5` + follow-up `9b0a8ad`). 16 entries, newest-first, bodies verbatim from the source draft.
Index: [`DECISION_LOG.md`](../../00-start-here/DECISION_LOG.md) (`00-start-here/`). Large technical decisions also carry numbered entries in [`ARCHITECTURE_DECISIONS.md`](../../02-architecture/ARCHITECTURE_DECISIONS.md).
"THE LAW" = the project's ten-word quality constitution (small, simple, secure, safe, lightweight, readable, auditable, modular, congruent, harmonic).

---

## 2026-07-15: M3 closes only on owner FEEL approval; a known input nit ships and is fixed one commit later

**Decision:** M3's binding close gate was Carlos's interactive "FEEL re-smoke" in Windows
Terminal (native mouse copy, real clipboard paste, glyph/frame render, clean `/quit`
restore) — not the green test suite. One FEEL nit surfaced at the gate (`/effort HIGH`
rejected because `parse_effort` was lowercase-only); Carlos said commit anyway, and the
one-line case-fold fix landed as the immediately following commit.

**Alternatives considered:**
- Hold the commit until the nit was fixed — rejected in the moment: BUILD_LOG records "not
  a bug, fails safe … left as-is (Carlos said commit; optional one-line follow-up)"
  (00-start-here/BUILD_LOG.md:648-650).
- Close on tests alone — foreclosed by the standing "graded by FEEL, not just renders"
  directive (CONTRACTS_M3.md §8, line 330-332).

**Why:** UX IS THE PRODUCT is a stated ground rule (CONTRACTS_M3.md:28-30); a milestone
whose whole point is feel cannot be closed by an automated gate. Shipping a fail-safe nit
rather than blocking is THE LAW: small/simple.

**Immediate effect on the harness:** M3 committed as `3fcd00e` (261 tests / 0 fail / 1
ignored, clippy clean) with the nit documented; `9b0a8ad` then made `parse_effort` trim +
ASCII-lowercase so `/effort High|MAX|None` work.

**Long-term consequence:** Establishes the FEEL gate as a repeatable, owner-held release
gate for every UX-bearing slice (it reappears in M5 Slice C and Slice F W4).

**Evidence:** commits `3fcd00e`, `9b0a8ad`; 00-start-here/BUILD_LOG.md:615-663 (gate),
648-650 (nit ruling).

**Article angle:** The milestone's exit criterion was a human sitting in Windows Terminal
saying "this feels right," with the test suite demoted to a precondition.

**Review later:** no.

---

## 2026-07-15: Slice F — native click-drag copy beats mouse capture; paste goes through bracketed paste

**Decision:** Remove terminal mouse capture entirely (no `EnableMouseCapture`) so native
click-drag copy works with no Shift key, and handle paste via bracketed paste
(`Event::Paste` → `reduce_paste`: multi-line collapses to one line, never auto-dispatches;
`DisableBracketedPaste` added to the panic-safe restore sequence). Scrolling stays
keyboard-only (`↑↓`/PageUp/PageDown/End with `↑/↓ more` hints).

**Alternatives considered:**
- Keep mouse capture (as CONTRACTS_M3.md §9.2, lines 418-422, had actually specified:
  "MOUSE WHEEL. Enable mouse capture on startup") — reversed in Slice F because capture
  broke native copy; BUILD_LOG:637-640 records "removed mouse capture … so native
  click-drag copy works again with NO Shift". The sources record the reversal but not an
  explicit weighing of losing wheel-scroll (see UNSOURCED).

**Why:** The owner's smoke showed native mouse-copy broken and paste eaten
(BUILD_LOG:624-626) — copy/paste is table-stakes terminal UX (UX IS THE PRODUCT). Making
multi-line paste never auto-dispatch is THE LAW: safe (a paste can never fire a task).

**Immediate effect on the harness:** crates/nh-tui input layer reworked; the 20,000-event
orchestrator fuzz proved input never held a raw `\n`/`\r` (BUILD_LOG:641-648).

**Long-term consequence:** The TUI permanently forgoes in-app mouse interaction (wheel
scroll, click targets) in exchange for the terminal's native selection/clipboard; any
future mouse feature must re-litigate this.

**Evidence:** commit `3fcd00e` (Slice F paragraph); 00-start-here/BUILD_LOG.md:637-640;
CONTRACTS_M3.md:418-422 (the reversed §9.2 spec). Note: Slice F has no contract section of
its own — §8/§9 cover only D/E.

**Article angle:** A spec'd feature (mouse capture) was shipped, felt wrong, and was
deleted two days later because it broke the one mouse gesture users actually rely on.

**Review later:** yes — if users ask for wheel scroll or clickable UI, the capture
tradeoff reopens.

---

## 2026-07-14: Honest-identity system prompt, composed at the config layer — not in nh-core

**Decision:** Add a system-prompt preface — "You are nosis … running on route
'<route_id>' … never claim to be Claude, GPT, or any other assistant" — composed in the
cmd_tui/nh-tui config layer, folded into the constitution string, byte-stable per route,
updated on `/model` switch. Trigger: DeepSeek V4 Flash self-identified as "Claude"
(training contamination); routing was verified correct via receipts first, so the fix
targets the model's claim, not the router.

**Alternatives considered:**
- Suspect and debug misrouting — ruled out empirically: "verified routing via receipts =
  `deepseek-v4-flash`" (CONTRACTS_M3.md:397-398, BUILD_LOG:634-636).
- Put the preface in nh-core — rejected by the contract's own placement rule: "in the
  cmd_tui/nh-tui config layer — NOT nh-core" (CONTRACTS_M3.md:437-438), keeping frozen
  nh-core untouched.

**Why:** A harness that meters and audits third-party models cannot let a model lie about
what it is — "erodes trust" (CONTRACTS_M3.md:398). Byte-stability per route preserves the
M2 cache discipline (THE LAW: congruent).

**Immediate effect on the harness:** Slice E shipped the preface for `nh tui`, with a test
that the composed prompt contains the route id + the "never claim" instruction
(CONTRACTS_M3.md:441-442).

**Long-term consequence:** Created the identity-constitution pattern — and a latent gap:
scoping it to the TUI config layer left `nh run`/`nh chat` uncovered until the M4-era fix
`7faf44b` ("apply the honest-identity prompt in nh run + nh chat, not just the TUI"). The
standing rule since: identity constitution applies at EVERY agent surface.

**Evidence:** CONTRACTS_M3.md §9.5 (lines 436-442) + §9 preamble (393-399); commit
`3fcd00e`; gap-fix commit `7faf44b`.

**Article angle:** The first identity bug wasn't in the router — the receipts proved the
right model was answering; it was the model itself claiming to be Claude.

**Review later:** no (surface-coverage rule already ratified after `7faf44b`).

---

## 2026-07-14: Type-freely + slash commands replace bare-letter shortcuts

**Decision:** Remove every bare-letter-on-empty-input shortcut (`t`/`l`/`q`/`?`/`R`); all
printable keys type into the input; `/` opens a live filtered command menu (`/help`,
`/trust`, `/timeline`, `/model`, `/provider`, `/effort`, `/quit`). Live `/model`/
`/provider` switching preserves history and session usage (with an authorized drop-if-hard
fallback to an honest "restart with --model" line if a clean live rebuild proved too hard
— the real switch shipped).

**Alternatives considered:**
- Keep single-key shortcuts (the Slice A/B design) — rejected after the owner's smoke:
  you "couldn't type tasks starting with t/l — bare-letter shortcuts collided"
  (BUILD_LOG:624-626; CONTRACTS_M3.md:393-396).
- Ship a half-working live switch — pre-forbidden: "never ship a half-broken switch.
  Report which path was taken" (CONTRACTS_M3.md:429-430).

**Why:** Carlos's recorded 2026-07-14 decision: move to the "type-freely + slash-command"
model (CodeWhale/Claude-Code feel) because "UX + security are the product's
differentiators" (CONTRACTS_M3.md:398-399). Congruent with `nh chat`, which already used
slash commands.

**Immediate effect on the harness:** Slice E (committed within `3fcd00e`): slash menu,
keyboard scroll, live route/effort switch via the existing `TuiConfig.resolver`, mojibake
fix; the colliding shortcuts are gone.

**Long-term consequence:** Locks the TUI's interaction grammar to
prompt-first-with-slash-commands — the same grammar as chat — so every later surface
(M5's `/why`, profiles) slots into one model instead of a key map.

**Evidence:** CONTRACTS_M3.md §9 (lines 393-399, 410-416, 424-430); commit `3fcd00e`;
00-start-here/BUILD_LOG.md:631-636.

**Article angle:** The first TUI shipped keyboard shortcuts that made it impossible to
type any task starting with "t", and the fix was to adopt chat's slash-command grammar
wholesale.

**Review later:** no.

---

## 2026-07-14: Owner rejects the content-complete TUI on UX grounds — M3 reopened, FEEL becomes the grade

**Decision:** Carlos rejected the artifact-free, all-tests-green, "content-complete" M3
TUI (flat look, overlays bleeding into the transcript, no scroll, hidden model/effort,
broken copy/paste) and reopened the milestone. Slice D was authorized as a
framed-panels + chat-transcript re-skin of nh-tui ONLY, graded "by FEEL, not just
'renders'", to a bar of self-teaching, delightful, matching CodeWhale. The deliberately
minimal renderer rule ("artifact budget", CONTRACTS_M3.md §1.3) was explicitly relaxed —
now that the renderer was proven artifact-free — to plain single-line borders + 16-color
as the new safety envelope.

**Alternatives considered:**
- Close M3 as content-complete and move to M4 — that was the actual state of the tree
  (`28e8cf6` "M3 content-complete", 239 tests green) and the owner overrode it
  (CONTRACTS_M3.md §7 amendment 2026-07-14, lines 318-323; BUILD_LOG:624-627).

**Why:** Carlos's binding UX directive: "without best-in-class UX/UI the product goes
unused no matter how good the engine" (CONTRACTS_M3.md:329-331). This is the project's
UX-first/drop-if-hard doctrine applied with teeth for the first time.

**Immediate effect on the harness:** Three uncommitted slices (D re-skin, E interaction
model, F copy/paste) built on top of `28e8cf6` and committed together as `3fcd00e` only
after FEEL approval; anti-bleed fixed via centered `Clear`-before-draw modals
(CONTRACTS_M3.md §8.4, lines 371-377).

**Long-term consequence:** Set the precedent that "green" is necessary but not sufficient
to close a milestone; the owner's felt experience is a first-class gate. Also permanently
raised the render budget from "minimal" to "framed, within a 16-color/plain-border
envelope."

**Evidence:** CONTRACTS_M3.md §7 amendment 2026-07-14 (lines 318-323) + §8 (327-388);
commits `28e8cf6` (content-complete anchor), `3fcd00e` (the overhaul);
00-start-here/BUILD_LOG.md:615-651.

**Article angle:** A milestone that passed every automated gate was rejected and rebuilt
because the owner sat down, tried to type a task, and couldn't.

**Review later:** no.

---

## 2026-07-14: No hardening round-trip without a confirmed defect

**Decision:** At the Slice B gate the orchestrator found no confirmed defect and
explicitly declined to run a hardening pass anyway, recording the rule: "no confirmed
defect → no hardening round-trip (THE LAW: small/simple — don't spend a cycle on a
non-finding)". A noticed nicety (trust dial doesn't scroll a very long rule list) was
logged as drop-if-hard, not fixed. Slice C repeated the same call.

**Alternatives considered:**
- Run a hardening pass every slice regardless (the M2/Slice-A pattern had a pass each
  time) — rejected as process-without-value when review confirms nothing
  (00-start-here/BUILD_LOG.md:755-757, 684-685).

**Why:** THE LAW: small, simple — cited verbatim in the log. Keeps the Sol round-trip
budget for real findings.

**Immediate effect on the harness:** Slices B and C committed directly after review;
only Slice A (duplication finding) and M2 (three findings) got hardening passes.

**Long-term consequence:** Normalizes "zero-blocking review → commit" as the standard
gate outcome, which the M4/M5 waves then rely on.

**Evidence:** 00-start-here/BUILD_LOG.md:755-757 (Slice B), 684-685 (Slice C: "No
confirmed defect → no hardening round-trip").

**Article angle:** The review process had to learn restraint: a hardening pass with
nothing to harden is itself a defect against a "small, simple" constitution.

**Review later:** no.

---

## 2026-07-13/14: Shared helpers are lifted additively, never copy-pasted (three M3 amendments)

**Decision:** Three cross-crate needs were each resolved by an additive lift logged as a
contract amendment instead of duplication or opening private surfaces:
1. 2026-07-13 — chat's peak/off-peak display logic lifted into
   `nh_routes::ResolvedRoute::peak_status`; chat and TUI call the same method.
2. 2026-07-14 — `safe_line`/`sanitize_line` display-safety lifted into `nh_vault`
   (500-char cap kept private); nh-cli delegates, nh-tui reuses. This one was an
   orchestrator adversarial-review finding — Sol had duplicated the primitive in nh-tui —
   fed back as a bounded hardening pass.
3. 2026-07-14 — read-only trust dial gets additive `nh_law::PolicyView` +
   `Policy::view()` returning owned copies; `Policy` fields stay private,
   verdict/autonomy behavior unchanged.

**Alternatives considered:**
- Copy-paste the helpers into nh-tui — pre-rejected by contract: "prefer reuse over
  copy-paste (THE LAW: congruent, no duplication)" (CONTRACTS_M3.md:288-289); the one
  accidental duplicate was caught in review and removed (BUILD_LOG:821-823).
- Make `nh_law::Policy` fields public for the trust dial — pre-rejected: "additive getter
  returning owned strings … rather than making fields public" (CONTRACTS_M3.md:209-211).

**Why:** THE LAW: congruent (one implementation of a security-relevant primitive) and
auditable (every surface change is a dated §7 amendment under orchestrator authority).

**Immediate effect on the harness:** nh-routes, nh-vault, nh-law each gained one small
additive public item; nh-cli and nh-tui render through identical scrub/escape/truncate
code.

**Long-term consequence:** `nh_vault::safe_line` becomes the single display-safety choke
point every later surface (MCP envelopes, W4 surfaces) routes through; the
amendment-ledger habit carries into every later contract.

**Evidence:** CONTRACTS_M3.md §7 (lines 309-317); commits `f45fb02` (lifts noted in
message), `13c36c9` (PolicyView); 00-start-here/BUILD_LOG.md:821-823, 837-862 (the
hardening pass), 770-791.

**Article angle:** The display-safety function that scrubs secrets exists exactly once in
the codebase because a reviewer refused to let a second copy survive one day.

**Review later:** no.

---

## 2026-07-13: M3 scoped by three binding owner rulings — timeline view-first, Telegram build-now/live-pending, core-first delivery

**Decision:** Carlos's recorded scoping rulings, marked "binding" in the contract header:
(1) the timeline ships as VIEW + inspect only — side-git snapshots and `R` restore are
deferred, and no snapshot store may be built in M3; the deferral must be visible (a
disabled `R` key that says restore arrives later), not silent; (2) the Telegram notify
hook is built and mock-tested now, with the live send waiting on a real bot token
(verify-live ledger); (3) delivery is core-first — Slice A de-risks the renderer, B and C
layer on top.

**Alternatives considered:**
- Build the snapshot/restore store in M3 — explicitly forbidden: "do NOT build a snapshot
  store in M3" (CONTRACTS_M3.md:10-12).
- Wait for the token before building the notify path — rejected in favor of
  build-now/verify-live (CONTRACTS_M3.md:13-14), consistent with the M1 verify-live
  ledger pattern.

**Why:** Scope control on the milestone whose risk is the renderer, not persistence
(THE LAW: small); honest deferral over silent omission (auditable). The Slice C
implementation honored it literally — "`R` shows only the locked restore deferral … No
snapshot store or restore path was added" (BUILD_LOG:706-709).

**Immediate effect on the harness:** Timeline is a pure projection over in-memory
receipts/history (no new persistence layer, CONTRACTS_M3.md:240-242); Telegram POST ships
behind an injectable sender seam with the real send left on the ledger.

**Long-term consequence:** Restore/snapshots remain an open seam (still deferred as of
M3 close); the "build the integration, ledger the live proof" pattern becomes standard.

**Evidence:** CONTRACTS_M3.md:9-15 (binding rulings), §3.1 (233-242), §6 (302-303);
commit `21b92e4`; 00-start-here/BUILD_LOG.md:698-734.

**Article angle:** The undo button shipped as a deliberately disabled key that tells you
it doesn't exist yet, because a visible deferral was ruled better than a silent one.

**Review later:** yes — the snapshot/restore seam is still open and owner-priced.

---

## 2026-07-13: TUI dependency budget — ratatui + crossterm only; no async runtime; bell, not desktop toast

**Decision:** M3 authorized exactly two new external crates, `ratatui` and `crossterm`
(the project's first new deps since M0), used only by nh-tui. Explicitly excluded:
`notify-rust` (OS notification = one terminal bell, `\x07`), `glob`, and any async
runtime — "the agent stays on a plain thread." Telegram uses the existing blocking
`reqwest`. nh-tui is a library crate with the binary entry in nh-cli so state logic tests
run headlessly.

**Alternatives considered:**
- Desktop toast via `notify-rust` — rejected: "A real desktop toast is out of scope
  (lightweight; drop-if-hard)" (CONTRACTS_M3.md:161-164, 42-43).
- Async runtime for the agent/UI split — rejected in favor of one worker thread +
  channels (CONTRACTS_M3.md:292-293, §1.1). No competing TUI stack is named in the
  sources (see UNSOURCED).

**Why:** THE LAW: lightweight/small — each dep is an orchestrator-authorized amendment,
not a default (CONTRACTS_M3.md:41-43, §5.3). Blocking-thread + channels keeps the
synchronous `AgentLoop` unchanged.

**Immediate effect on the harness:** Workspace gained ratatui + crossterm; notifications
= bell locally + Telegram remotely; zero async anywhere in the workspace.

**Long-term consequence:** The no-async, single-worker-thread architecture holds through
M4/M5 (the later MCP server and fleet stay thread-based); the dependency-amendment
discipline later feeds directly into the cargo-deny supply-chain gate.

**Evidence:** CONTRACTS_M3.md:41-43 (§0), 291-293 (§5.3), 160-164 (§1.5), 58-61 (§1);
commit `f45fb02`.

**Article angle:** The interactive terminal UI was built with exactly two new
dependencies and no async runtime, on the theory that a blocking agent loop plus two
channels is all a TUI actually needs.

**Review later:** no.

---

## 2026-07-13: nh-core and nh-tools frozen for M3 — the TUI is a consumer of existing seams

**Decision:** M3 may not modify nh-core or nh-tools. The TUI drives the EXISTING
`AgentLoop` + `ToolCtx` by wiring channel-backed closures into the already-public
`ToolCtx.approve` and `AgentLoop.on_event`; the approve closure blocks on a reply
channel, and dropping the reply sender reads as deny (default-deny preserved). Wrapping
the channel pair in a `Mutex` is the sanctioned no-new-dep way to satisfy `Send + Sync`.
If a core change seems unavoidable: STOP and request an amendment.

**Alternatives considered:**
- Extend nh-core with TUI-aware hooks — pre-forbidden: "If you believe a core change is
  unavoidable, STOP and request an orchestrator amendment — do not edit nh-core/nh-tools
  unilaterally" (CONTRACTS_M3.md:25-26).

**Why:** THE LAW: modular — the M2-hardened agent/guard core stays untouched while a
whole new surface is built on it; the M0 `on_event` seam and M1 approve seam prove out as
the intended extension points. Also auditable: the Slice A/B/C gates verified frozen-crate
diffs were EMPTY (BUILD_LOG:744-746, 673-675).

**Immediate effect on the harness:** crates/nh-tui exists as a pure consumer; approvals,
budget stop, and exec gating all flow through the unchanged M2 guard (exec still never
auto-approved, BUILD_LOG:818-820).

**Long-term consequence:** Established the freeze-and-verify pattern (empirical
empty-diff checks on frozen crates at every gate) that later milestones formalize; proved
the M0/M1 public seams were sufficient for a full interactive frontend.

**Evidence:** CONTRACTS_M3.md:23-26 (§0), 61-99 (§1.1, incl. the Mutex sanction at
88-90); commit `f45fb02`; 00-start-here/BUILD_LOG.md:744-746, 673-675.

**Article angle:** An entire interactive UI was added without changing one line of the
agent core, using two closure hooks that had been public since M0/M1.

**Review later:** no.

---

## 2026-07-13: M2 hardening — delete the dead exec-ask state, keep its TOML key; document the Windows case-fold invariant

**Decision:** The orchestrator's adversarial review fed exactly three confirmed findings
back to Sol as ONE bounded pass: (1) remove the behaviorless compiled `exec_ask` state
(exec already always asks) while keeping `[exec] ask` accepted in TOML for compatibility;
(2) make the bundled protected-path test hermetic (independent of the developer's real
home law); (3) document why existing-file canonicalization makes the case-SENSITIVE
write-hold safe on case-INSENSITIVE filesystems — `EditFile` only mutates existing files,
whose case `canonicalize` normalizes before the guard sees them — plus the guard
hardening any future file-CREATION tool must add.

**Alternatives considered:**
- Fix the case-fold exposure in code now — the review concluded it is NOT reachable
  through `EditFile` ("missing paths bail before any write", BUILD_LOG:897-898), so the
  chosen remedy was a documented invariant + a recorded obligation on future tools, not
  speculative code.
- Keep the dead `exec_ask` machinery — rejected as behaviorless compiled state
  (BUILD_LOG:919).

**Why:** THE LAW: small (no dead state), auditable (the latent hazard is written down
with its trigger condition), safe (hermetic tests can't pass or fail on a developer's
machine state).

**Immediate effect on the harness:** 206 tests still green after the pass; `[exec] ask`
TOML remains valid; the case-fold invariant lives in the code docs.

**Long-term consequence:** Any future file-creating tool inherits a pre-written hardening
requirement; "bounded hardening pass with enumerated findings" becomes the standard
review→fix loop shape.

**Evidence:** 00-start-here/BUILD_LOG.md:911-931 (hardening entry), 896-898 (review
findings, case-fold analysis); commit `3155949` (folded in).

**Article angle:** The security review's most valuable output was a paragraph of
documentation: proof that a Windows case-folding bypass is unreachable today, and the
exact rule that keeps it unreachable tomorrow.

**Review later:** yes — the moment any tool can CREATE files, the documented guard
hardening becomes mandatory work.

---

## 2026-07-13: The repo-cannot-weaken-you boundary — cloned law can only add protections

**Decision:** Law sources merge by union with most-restrictive-wins, and a repository's
`.nosis/law.toml` may set ONLY `[constitution].text`, `write.ask`, `write.block`,
`exec.ask`, `exec.block`. If repo law sets `[autonomy]` or `write.auto`, those keys are
IGNORED with a user-visible warning ("repo .nosis/law.toml cannot raise autonomy or
auto-approve paths — ignored"). Autonomy resolves CLI → user-global → bundled default
(`Ask`); repo law never participates. Bundled block globs (`.git/**`, `.nosis/**`,
`**/*.pem`, `**/*.key`, `**/id_rsa*`, `**/.env*`) can never be downgraded.

**Alternatives considered:**
- Let repo law configure autonomy like any other layer — rejected on the recorded
  lethal-trifecta rationale: "a cloned/untrusted repo must never weaken the user's safety
  posture, only strengthen it. This is a hard, test-covered rule" (CONTRACTS_M2.md:157-163,
  citing SECURITY_MODEL).
- Hard-fail on malformed/hostile law files — rejected: loading "NEVER hard-fails";
  unreadable sources become warnings + defaults ("robustness > strictness",
  CONTRACTS_M2.md:83-85).

**Why:** THE LAW: secure/safe. The threat model is a user running the harness inside a
repo they just cloned; that repo's committed policy must be one-directional.

**Immediate effect on the harness:** nh-law Slice A shipped the boundary with unit tests
(repo autonomy/auto ignored + warned, source precedence, unioned protections); the M2
exit test proved a protected path blocked end-to-end at `--autonomy auto` with
`.nosis/law.toml` byte-unchanged (BUILD_LOG:904-905).

**Long-term consequence:** Locks in one-directional trust for every future config layer;
the same tighten-only merge shape is reused when Slice G W6a later adds the user-global
`~/.nosis/mcp.toml` trust source ("repo tighten-only merge").

**Evidence:** CONTRACTS_M2.md §1.5 (lines 152-164), §1.6 (166-187); commit `3155949`;
00-start-here/BUILD_LOG.md:985-987 (implementation), 904-905 (exit proof).

**Article angle:** A repository you clone can make the harness stricter about itself but
is structurally incapable of making it looser.

**Review later:** no.

---

## 2026-07-13: Exec is never auto-approved — enforced by type shape, not by policy data; denials stay Ok-shaped

**Decision:** `Policy::exec_verdict` returns ONLY `Block` or `Ask` — `Allow` is
structurally unreachable for shell commands, so no autonomy level (including
`--autonomy auto`) can auto-approve exec; max autonomy may auto-approve file WRITES only.
Enforcement lives in the tool choke point (nh-tools guard consulted before any
mutation/execution), and every denial is Ok-shaped ("blocked by law: {reason}" /
"user denied: …") — a model-readable tool result, never a crashed run.

**Alternatives considered:**
- An `exec.auto` allowlist for trusted commands — no such option appears in any M2
  source; the contract states the opposite as a pre-existing hard rule ("AGENTS.md hard
  rule", CONTRACTS_M2.md:25-26, 132-134), i.e. the sources show no alternative was on the
  table in M2.
- Err-shaped blocks that abort the run — rejected by design: Block returns
  `Ok("blocked by law: …")`, the file/command untouched, run completes, exit 0
  (CONTRACTS_M2.md:225-231, 355-361).

**Why:** THE LAW: secure/safe/auditable. The review recorded the structural argument
explicitly: "structurally `exec_verdict` can only return `Block`/`Ask` — never `Allow` —
so exec is never auto-approved even at `--autonomy auto`" (BUILD_LOG:897). Ok-shaped
denials keep the law legible to the model (it can adapt) and the session alive (UX).

**Immediate effect on the harness:** nh-tools gained `Access`/`Guard`/`GuardFn` +
`ToolCtx::new`/`with_guard` (default guard byte-preserves M0/M1 behavior); the M2 exit
demo is literally this rule under max autonomy.

**Long-term consequence:** Every later execution path (M5 exec hardening, fleet runs)
builds on "exec always gates"; making exec auto-approvable would now require changing a
public type's shape, not flipping config.

**Evidence:** CONTRACTS_M2.md:25-26 (§0), 132-137 (§1.4), 199-239 (§2); commit `3155949`
(message: "exec is never auto-allowed"); 00-start-here/BUILD_LOG.md:897, 904-905.

**Article angle:** "Can the agent run shell commands without asking?" is answered by the
return type of one function, which has no variant for yes.

**Review later:** no.

---

## 2026-07-13: Compaction is mechanical at 70% — no summary model, marker folded into the first kept user message

**Decision:** When estimated input reaches 70% of the route's context window, compaction
mechanically drops middle messages down to a 50%-of-limit target: the prefix
(`history[0]`) stays byte-identical, the retained suffix must begin at a `user` message,
tool-call/tool-result pairs are never split, at least the last 2 user-turns survive
(`KEEP_RECENT = 2`), and the audit marker ("[nosis] earlier context compacted: {n}
messages, ~{t} tokens elided.") is PREPENDED to the first retained user message's content
rather than inserted as its own message. One `on_event` line reports it. No
summary-model call in M2. Token estimation falls back to a deterministic `ceil(len/4)`
when providers omit usage.

**Alternatives considered:**
- Model-generated summarization — explicitly out: "mechanical; no summary-model call in
  M2" (CONTRACTS_M2.md:293). The sources scope it out of M2 rather than reject it
  forever.
- A separate marker message — rejected with a wire-correctness reason: it "would create
  two consecutive user messages and break the Anthropic wire — so it is folded in. This
  keeps roles valid and the marker auditable" (CONTRACTS_M2.md:303-305).

**Why:** THE LAW: simple (deterministic, testable, no extra model spend), auditable (the
elision is announced in-band and persists in history), safe (wire-valid message shapes on
both wires; reasoning bytes of retained messages untouched).

**Immediate effect on the harness:** `run_with_history` compacts in place so `nh chat`
sessions carry the compacted history forward; integration tests force it with tiny
injected limits (prefix identity, user-first suffix, no orphan tool result, marker fold,
survivor turns).

**Long-term consequence:** Pinned constants (0.70 trigger / 0.50 target / KEEP_RECENT 2)
are a declared revisit point — "revisit if live sessions thrash" (CONTRACTS_M2.md:417);
the mechanical-first stance held (M5 W3 later ratified DROPPING a compaction guard rather
than adding model summarization).

**Evidence:** CONTRACTS_M2.md §3.3 (lines 281-317), §6 (417); commit `3155949`;
00-start-here/BUILD_LOG.md:963-965.

**Article angle:** Context compaction ships as a deterministic algorithm with an in-band
audit marker, chosen over a summarizer partly because a second user message would
literally be an invalid request on the Anthropic wire.

**Review later:** yes — the 0.70/0.50/2 constants, per the contract's own ledger, if live
sessions thrash.

---

## 2026-07-13: Cache discipline = byte-stable prefix; cache % derived at display time; explicit cache_control deferred

**Decision:** The assembled constitution becomes a byte-stable system prefix: built once
per session as a pure function of its sources (fixed section order bundled law → user law
→ repo law → AGENTS.md → memory; no timestamps, no environment, absent sections omitted),
installed as `history[0]`, and never mutated — debug-asserted each turn. Cache hits are
measured by the pure `cache_hit_pct(prompt, cached)` derived at DISPLAY time — "No
receipt schema change … keeps receipts stable/auditable" (CONTRACTS_M2.md:278-279).
Anthropic-wire explicit `cache_control` breakpoints were deliberately deferred: M2 relies
on prefix stability only, with explicit breakpoints "a LATER hardening pass if the live
metric needs it" (CONTRACTS_M2.md:415). Exit proof: a prefix-caching mock over 50 turns
must exceed 60% cumulative cache-hit — the mock rewards only byte-identical leading runs,
so any prefix mutation collapses the metric and fails the test.

**Alternatives considered:**
- Persist a cache percentage into receipts — rejected to keep the receipt schema stable
  and auditable (CONTRACTS_M2.md:278-279).
- Ship explicit `cache_control` now — rejected: "Keeps M1 anthropic wire/tests
  unchanged" (CONTRACTS_M2.md:415); provider auto-caching of stable prefixes is the
  assumed mechanism, logged on the verify-live ledger (414).
- Dynamic content in the system prompt (timestamps, interpolated tool lists) — forbidden;
  tool schemas travel in the `tools` field, not system text (CONTRACTS_M2.md:265-267).

**Why:** This is the economic core of the product: cache misses are billed money, so the
prefix's byte-stability IS the cost feature (THE LAW: auditable, harmonic — the exit test
encodes the invariant as a falsifiable metric). Result: 97.70% measured
(`stable_constitution_exceeds_sixty_percent_cache_hits_over_fifty_turns`,
BUILD_LOG:903-904).

**Immediate effect on the harness:** `AgentLoop` gained `constitution`/`context_limit`;
cache chips appeared in the `nh run` summary and `nh chat` footer (omitted when no usage
— no fake 0%); `nh init` writes a starter committed `.nosis/law.toml`.

**Long-term consequence:** Every later prompt-touching feature (identity preface,
profiles, W3 meter work) must preserve per-route byte-stability; the honest-metric
pattern (display-derived, never fabricated) becomes the meter's house style.

**Evidence:** CONTRACTS_M2.md §1.3 (99-116), §3.1-3.2 (249-279), §3.4 (319-333), §6
(410-418); commit `3155949` (message: "Byte-stable prefix cache discipline … debug-asserted
every turn"); 00-start-here/BUILD_LOG.md:903-904, 963-965.

**Article angle:** The system prompt is treated as an immutable byte string because every
mutated byte re-bills the whole prefix, and a 50-turn test fails the build if anyone
breaks that.

**Review later:** yes — the deferred `cache_control` breakpoints, if a live Anthropic-wire
cache metric ever underperforms.

---

## 2026-07-13: The law layer is a leaf — nh-law depends on nothing, nh-tools doesn't know it exists, nh-cli bridges

**Decision:** `nh-law` is a new leaf crate depending only on workspace `serde`, `toml`,
`anyhow` + std — no nh-* dependencies, no `glob`/`globset` (in-crate segment glob
matcher), no `dirs` (home dir via `USERPROFILE`/`HOME` env). Enforcement lives in
nh-tools via its OWN small surface (`Access`/`Guard`/`GuardFn`); nh-tools does NOT depend
on nh-law. The `Verdict → Guard` conversion is a free helper in nh-cli
(`guard_from`), which is the only crate that knows both. MCP adapters keep their
independent M1 trust logic and do not consult the guard.

**Alternatives considered:**
- `glob`/`globset` and `dirs` deps — pre-rejected in the ground rules: "No new external
  crates … nh-law does its own minimal glob matching … no `dirs` dep"
  (CONTRACTS_M2.md:21-23).
- nh-tools depending on nh-law directly (no bridge) — rejected: "nh-law must not depend
  on nh-tools" and the conversion is deliberately "a free helper in nh-cli … not a method
  on `Verdict`" (CONTRACTS_M2.md:345-348); the guard sits where mutation happens,
  "auditable, un-bypassable by model text" (191-193).

**Why:** Stated in-contract: "leaf; keeps the governance logic isolated and independently
testable — THE LAW: modular, auditable" (CONTRACTS_M2.md:43-46). Zero-dep policy code is
also the smallest possible security-review surface (small, lightweight).

**Immediate effect on the harness:** crates/nh-law shipped with 11 unit tests and only
three deps; nh-tools' default guard byte-preserves M0/M1 behavior so every existing
construction site kept working.

**Long-term consequence:** The law crate stays independently auditable forever (it was
reopenable in isolation for M5 W1 hardening); the guard seam let M3's TUI and M4's fleet
reuse enforcement without touching policy code. The in-crate glob matcher carried a
latent cost: the M5 audit later found recursion-based matchers needed iterative rewrites
(stack-DoS, fixed in Slice F W1) — the no-dep choice meant owning that hardening.

**Evidence:** CONTRACTS_M2.md:21-23, 43-46 (§1), 140-150 (§1.4 matcher), 191-196 +
234-235 (§2), 344-351 (§4.1); commit `3155949`; 00-start-here/BUILD_LOG.md:977-996.

**Article angle:** The crate that decides what the agent may touch has three
dependencies, all serialization, and cannot see the crates it governs.

**Review later:** no (the glob-matcher hardening debt was paid in M5 Slice F W1).
