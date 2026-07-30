# DECISIONS — Release Slice + Slice G (2026-07-20 → 2026-07-26)

Era covered: the owner-directed RELEASE SLICE (private-beta → public-1.0) and SLICE G (remediation of the 2026-07-21 pre-release audit), including the uncommitted Wave 7 R1/R2. 22 entries; entries are newest-first. Index: see `../../00-start-here/DECISION_LOG.md`.
Gate progression (test pass counts, `--release`, all 0 failed / 1 ignored): W4-SURFACES baseline 432 → W1 439 → W2 446 → W3 456 → W4 461 → W5 472 → W6a 483 → W6b 488 → W6c 493; Wave 7 (in flight) starts from 493/0/1.

---

## 2026-07-26: Remove Telegram from public v0.1; retain only a future opt-in extension point

**Decision:** Remove the TUI's Telegram configuration, vault lookup, sender thread, HTTP request,
tests, and direct `reqwest`/`toml` dependencies. Preserve the local terminal bell when approval is
needed. Do not replace Telegram with another remote channel in v0.1. Remote notifications remain
open only as a future, separately reviewed, explicit opt-in integration.

**Alternatives considered:**
- Keep Telegram but move destination configuration to the user-global trust boundary — secure
  provenance fixes one exploit, but the product would still own a bot credential, remote privacy
  semantics, a background sender, and another external failure mode.
- Replace it with desktop toast or another push service — rejected because it substitutes a new
  dependency or network surface rather than removing the non-core surface.
- Delete the idea permanently — rejected; walk-away notification may still be valuable later if
  designed as an isolated opt-in integration with explicit privacy and destination controls.

**Why:** Telegram is not required for the harness's central invariant. Its strongest stated reason
was walk-away/overnight work, but the shipped hook was TUI-only and never integrated with headless
Fleet. The real send also remained verify-live. Removing it reduces credential, destination-config,
privacy, supply-chain, thread-lifecycle, and outbound-network attack surface without weakening the
core agent, Fleet, approval gate, receipts, or local waiting signal.

**Immediate effect on the harness:** `nh-tui` retains its semáforo, taskbar transition, and local
approval bell. It no longer parses `notify.toml`, reads a Telegram vault entry, starts a notification
thread, or sends to Telegram. Current product and architecture docs no longer advertise remote
notification support; historical M3 records are marked superseded rather than erased.

**Long-term consequence:** Any remote notification feature requires a new owner decision and
security review. Prefer an isolated adapter/plugin boundary; require explicit user opt-in,
user-controlled destination provenance, fixed minimal payloads, and no repository ability to add
or redirect a destination.

**Review later:** yes — only when a concrete walk-away workflow justifies reopening it.

---

## 2026-07-24: Wave 7 R1 — verified `taskkill /T /F`, honest failure report; Windows Job Object REJECTED

**Decision:** For timeout kill of a runaway shell command tree on Windows, keep the existing
`taskkill /PID <id> /T /F` (Unix: `kill -KILL -<pid>` against the real process group created at
crates/nh-tools/src/lib.rs:580), but now CAPTURE the tree-kill exit status, poll `child.try_wait()`
within a bounded `KILL_VERIFY_GRACE` (2s), fall back to `child.kill()`, and if the child still is not
reaped, return a `Survived(detail)` outcome and tell the user plainly: "command timed out after 300s —
could NOT be killed: <detail>" instead of the previous unconditional "— killed" claim
(sol_wave7_prompt.txt:121-127, 177-199).

**Alternatives considered:**
- Windows kill-on-close Job Object (the audit's own "minimal fix" for H-06, audit line 155:
  "create ... a kill-on-close Job Object on Windows") — rejected because it needs raw Win32 calls,
  which means a new dependency (windows-sys/winapi) AND `unsafe`, and that breaks the workspace
  `unsafe_code = "forbid"` law shipped in cccb2dc (sol_wave7_prompt.txt:19-23, 121-127).
- Keep the old fire-and-forget kill (discard `taskkill`'s exit status, unbounded `child.wait()`,
  unconditional "killed" message) — this IS the finding being fixed: a failed tree-kill was
  indistinguishable from a successful one, `terminate_child_tree` itself could hang forever, and the
  caller claimed an outcome nobody verified (sol_wave7_prompt.txt:100-106; audit H-06, lines 149-155).

**Why:** The workspace-wide `unsafe_code = "forbid"` (cccb2dc) is treated as law: the project refuses
to add the first `unsafe` block in the tree even when a security audit literally recommends the Win32
API that requires it. The compensation is honesty rather than stronger force: verify the kill, and
when verification fails, say so ("Honest reporting beats optimistic reporting — never claim an outcome
you did not verify", sol_wave7_prompt.txt:33-34). Serves THE LAW's safe + auditable + congruent
clauses (congruent = the exec subsystem obeys the same no-unsafe rule the rest of the tree ships).

**Immediate effect on the harness:** `terminate_child_tree` returns a typed
`Termination { Reaped(ExitStatus) | Survived(String) }`; the unbounded `child.wait()` tail is removed
(it was a second infinite hang inside the function meant to end one); the timeout message stays
byte-identical on a verified kill and becomes an explicit could-NOT-be-killed report on failure
(sol_wave7_prompt.txt:177-199).

**Long-term consequence:** Locks in "no unsafe, ever" as a real constraint with teeth — the harness
will accept a weaker-but-verified containment primitive over a stronger one that costs the guarantee.
Forecloses Job-Object-grade containment unless the forbid law itself is ever revisited. Establishes
the pattern that Windows process containment is best-effort-plus-verification, not absolute.

**Accepted residual risk (stated by the owner):** "a grandchild that re-parents while taskkill walks
the tree may survive — we REPORT that honestly instead of pretending we killed it"
(sol_wave7_prompt.txt:125-127, verbatim).

**Evidence:** C:\Users\capv2\AppData\Local\Temp\sol_wave7_prompt.txt:121-127 (R1 verbatim), :100-106
(the two holes), :177-199 (W7-7/W7-8); audit finding H-06
(04-research/AUDIT_2026-07-21_sol-max_pre-release.md:149-155); cccb2dc (the forbid law); no commit
hash yet — Wave 7 is uncommitted and in flight. Gate baseline 493/0/1.

**Article angle:** When its own security audit recommended the Windows API that requires `unsafe`, the
project kept the zero-unsafe guarantee and shipped verified-kill-plus-honest-failure-report instead.

**Review later:** yes — if a packaged/installer-era release ever relaxes the no-new-deps posture, the
Job Object (behind a safe wrapper crate) is the known stronger fix for the surviving-grandchild
residual.

---

## 2026-07-24: Wave 7 R2 — bounded output drain with a shared buffer; partial output preserved

**Decision:** Replace the unbounded `join()` on the stdout/stderr drain threads with a shared-buffer
design: each drain thread appends into an `Arc<Mutex<BoundedOutput>>` the main thread can also read;
the main thread waits with a grace deadline (`DRAIN_GRACE` = 5s) instead of joining forever. On
expiry it renders the partial output actually captured so far plus an honest note that capture did
not finish (e.g. "…[stdout capture incomplete after 5s — a surviving child process may still hold the
pipe]") and RETURNS. Applies on BOTH the timeout path and the normal-exit path
(sol_wave7_prompt.txt:129-136, 138-176).

**Alternatives considered:**
- Discard partial output on expiry — rejected: "the user loses the evidence of what the runaway
  command was doing" (sol_wave7_prompt.txt:133-135).
- Keep the unbounded `join_output` — this is audit H-06's "the caller can hang indefinitely": a
  surviving descendant that inherited the pipe write end keeps the read end open, the drain thread
  never sees EOF, and the join never returns — it can even bite on a NORMAL exit (a detached
  grandchild like `cmd /c start /b longtask` holds the pipe though the direct child exited 0)
  (sol_wave7_prompt.txt:108-117; audit lines 149-155).
- Related call folded into the same design: a mid-stream capture io-error used to abort the whole
  tool result via `?`; it now degrades to honest partial output plus the io error text
  (sol_wave7_prompt.txt:148-159).

**Why:** Same honesty-over-optimism rule as R1: the caller must never hang, and what WAS captured is
evidence the user is entitled to see, labeled as incomplete. Four drain outcomes (complete / io-error /
timeout / thread-panic) are distinguished explicitly rather than collapsed (sol_wave7_prompt.txt:146-152).
Serves safe (no infinite hang), auditable, and readable (distinct scannable markers in house style).

**Immediate effect on the harness:** New `DRAIN_GRACE` (5s) constant next to `EXEC_TIMEOUT` (300s);
`render_bounded_output` gains incomplete/failed-capture markers that can compose with the existing
byte-identical truncation marker; existing cap semantics preserved exactly (2 MiB retained max, keep
reading past the cap so the child never blocks on a full pipe) (sol_wave7_prompt.txt:152-176).

**Long-term consequence:** The exec tool can no longer hang its caller after a timeout — the last
known infinite-hang path in the tool boundary (per the audit's H-06) is bounded. Locks in the
"partial evidence beats no evidence" rendering convention for every future capture failure mode.

**Accepted residual risk (stated in the ratified decision):** "the blocked reader thread is abandoned
until the pipe finally closes; its memory is already bounded by `MAX_TOOL_READ_BYTES` (2 MiB) per
stream" (sol_wave7_prompt.txt:135-136, verbatim).

**Evidence:** C:\Users\capv2\AppData\Local\Temp\sol_wave7_prompt.txt:129-136 (R2 verbatim), :108-117
(HOLE 2), :138-176 (W7-4/5/6); audit H-06 (AUDIT_2026-07-21:149-155). Uncommitted, in flight; gate
baseline 493/0/1.

**Article angle:** A timeout handler that could itself hang forever was replaced by a bounded drain
that returns on a deadline and shows the user the partial output as labeled evidence.

**Review later:** no.

---

## 2026-07-24: Wave 7 (H-05) — exec approval enforced AT the operation boundary, structurally

**Decision:** `ExecShell` itself now enforces "exec always requires approval": `Guard::Block` refuses
(byte-identical string); EVERY other verdict — `Ask` AND `Allow` — must call the approval callback,
and the match is written so a future `Guard` variant cannot silently become a bypass (no catch-all
`_ => {}` arm; non-Block arms fall into the approval path) (sol_wave7_prompt.txt:73-79).

**Alternatives considered:**
- Keep relying on policy goodwill (`nh_law::Policy::exec_verdict` never returns `Allow`, so no
  shipped surface can bypass today) — rejected by the audit's reasoning: `Guard` and
  `ToolCtx::with_guard` are `pub`, so "an external caller or future policy regression can violate the
  repository's non-negotiable approval rule" (audit H-05, lines 141-147; sol_wave7_prompt.txt:55-57).

**Why:** This is the audit's meta-finding #1 made concrete: "Controls are enforced above the dangerous
operation rather than at the operation boundary" (audit line 330). The fix is verified behaviorally
INERT for every shipped surface and the whole existing test suite — all four surfaces map
Verdict→Guard 1:1 and none can produce `Guard::Allow` for exec (sol_wave7_prompt.txt:59-71). Serves
secure + congruent (the boundary now enforces the invariant the docs claim).

**Immediate effect on the harness:** No shipped behavior change; a regression test proves
`Guard::Allow` + a rejecting approval callback yields exactly `"user denied: <command>"`, calls
approval exactly once, and never runs the command (sol_wave7_prompt.txt:85-91).

**Long-term consequence:** The library can be embedded by external callers without them being able to
route around exec approval; adding a `Guard` variant can never silently reopen the hole.

**Evidence:** audit H-05 (AUDIT_2026-07-21:141-147, confirmed in the gate note at line 14);
sol_wave7_prompt.txt:38-91. Uncommitted, in flight.

**Article angle:** The one non-negotiable rule ("exec always asks") moved from policy convention into
the type-shaped structure of the executing function itself.

**Review later:** no.

---

## 2026-07-24: W6c (H-14) — scrubber literals sorted longest-first

**Decision:** The secret scrubber's replacement literals are deduplicated and sorted longest-first
(`sort_by_key(Reverse(len))` in nh-vault), so a shorter secret that is a prefix of a longer one can no
longer be replaced first and leave the longer secret's suffix visible
(CURRENT_TASK.md:9; audit H-14, AUDIT_2026-07-21:213-219).

**Alternatives considered:**
- A simultaneous multi-pattern matcher (the audit's other suggested fix, line 219) — the ratified
  call was the longest-first sort (CURRENT_TASK.md:9 records "2 ratified calls (H-14
  longest-first-sort; H-11 cap 500)"); the recorded sources do not state the reason the matcher was
  passed over (see UNSOURCED).
- Sol's sound deviation inside the ratified call: `sort_by_key(Reverse)` instead of a comparator —
  clippy-clean, identical behavior (CURRENT_TASK.md:9).

**Why:** Audit H-14 (CONFIRMED, including in Claude's independent gate note, audit line 15): replace
in insertion order and "the longer value becomes `[REDACTED]` plus its remaining suffix" — partial
secret disclosure despite the scrubber reporting success. Serves secure + auditable. Also added: a
canonical `escape_untrusted` no-truncation escaper that `sanitize_untrusted_text` now routes through
(CURRENT_TASK.md:9).

**Immediate effect on the harness:** Overlapping/prefix-related secrets redact fully in every output
path; +tests covering both insertion orders (gate 488 → 493/0/1).

**Long-term consequence:** Redaction correctness no longer depends on the order keys were added to the
vault — a property that would otherwise silently break as users add keys.

**Evidence:** audit H-14 (AUDIT_2026-07-21:213-219, gate-note confirmation line 15);
CURRENT_TASK.md:9 (session-7c checkpoint, codex `bey3o2vbk`, 5 files); gate 493/0/1 (2026-07-24).
Uncommitted (part of the one Slice-G lump).

**Article angle:** A one-line ordering bug meant a shorter vault secret could expose the tail of a
longer one; the fix makes redaction order-independent and tests both orders.

**Review later:** no.

---

## 2026-07-24: W6c (H-11) — approval display cap 120 → 500 chars with an honest "(+N more chars)"; digest renderer deferred

**Decision:** The MCP tool-approval argument summary cap (`ARGS_SUMMARY_MAX`) was raised from 120 to
500 characters, and any remainder is now declared with an honest `… (+N more chars)` suffix instead of
being silently cut; the TUI's `scrub_full_line` now escapes bidirectional-control characters and
strips zero-width characters via the canonical `nh_vault::escape_untrusted`, with no truncation
(CURRENT_TASK.md:9). The fuller fix — a single shared approval renderer binding a digest to the exact
executed bytes — was explicitly deferred: the owner ratified "H-11 = minimal-honest approval (defer
digest renderer)" (CURRENT_TASK.md:27).

**Alternatives considered:**
- The audit's full "minimal fix": one shared approval renderer showing the full canonical
  command/JSON with scrolling and a digest bound to the executed bytes (audit line 195) — deferred by
  the owner's ratified minimal-honest call (CURRENT_TASK.md:27); the recorded sources do not state the
  deferral reasoning beyond the ratification itself (see UNSOURCED).
- Keep the silent 120-char cap — this is the finding: "the approver can authorize bytes they were not
  shown or whose visual order is misleading" (audit H-11, lines 189-195).

**Why:** Audit H-11: approval displays omitted the dangerous tail of the operation being approved, and
bidi characters could visually reorder what the approver read. The minimal-honest form closes the
deception vectors (silent omission, bidi reordering) without building new UI. Serves secure +
auditable + small (drop-if-hard on the renderer).

**Immediate effect on the harness:** Approvers see up to 500 chars plus a truthful count of what they
are NOT seeing; TUI approval lines are bidi/zero-width-safe (gate 493/0/1).

**Long-term consequence:** A known, owner-acknowledged gap remains — very long arguments are still
approved partially sight-unseen (now with disclosure); the digest-bound renderer stays on the table.

**Evidence:** audit H-11 (AUDIT_2026-07-21:189-195); CURRENT_TASK.md:27 (session-7 ratification) and
:9 (session-7c implementation). Uncommitted lump.

**Article angle:** Instead of pretending a 120-character preview was the whole command, the approval
prompt now shows 500 characters and states exactly how many it is not showing.

**Review later:** yes — the deferred digest-bound shared approval renderer is the completion of H-11.

---

## 2026-07-24: W6c (H-03) — one scrubber refresh on route change, mirrored into `nh chat` via an owner-approved fold

**Decision:** `apply_new_credential` in the TUI worker now refreshes the shared, tool-context, and
receipts scrubbers TOGETHER at all three route-change branches (closing audit H-03: a route switch
left the agent tool scrubber holding the old secret set). Sol, while in-scope, FLAGGED the identical
defect in `nh chat`'s `install_client` (out-of-scope file); rather than launching another wave, the
orchestrator applied a 1-line mirror (`s.agent.ctx.scrubber = registry.clone();` in
crates/nh-cli/src/cmd_chat.rs) with explicit owner ratification — closing H-03 on BOTH agent surfaces
(CURRENT_TASK.md:9).

**Alternatives considered:**
- Leave the `nh chat` twin for a later wave — rejected in favor of the owner-ratified fold; the
  process rule is recorded: "Sol may FLAG a same-class bug in an out-of-scope file — fold via a small
  orch mirror with owner OK, as W6c did for cmd_chat" (CURRENT_TASK.md:11).

**Why:** Audit H-03: "after changing routes, tool results containing the newly active secret can
leave the tool boundary without that secret being redacted" (audit lines 125-129). Serves secure +
congruent (both agent surfaces behave identically).

**Immediate effect on the harness:** Route/profile/reconnect changes atomically refresh every scrubber
snapshot in TUI and `nh chat` (gate 493/0/1, +6 W6c tests).

**Long-term consequence:** Establishes the flag-and-fold protocol as the sanctioned way scoped waves
handle same-class defects outside their file scope, without scope creep and without leaving known twins
unfixed.

**Evidence:** audit H-03 (AUDIT_2026-07-21:123-129); CURRENT_TASK.md:9 and :11. Uncommitted lump.

**Article angle:** A strict one-file wave scope still caught and fixed the same bug on a second
surface — by flagging instead of editing, with the owner approving a one-line orchestrator mirror.

**Review later:** no.

---

## 2026-07-23: W6b — response-body caps that REJECT rather than truncate; global active-run cap of 4; MCP lifecycle closed

**Decision:** All remote response ingestion is now byte-capped per crate via `read_body_capped`
(Content-Length precheck → `take(MAX+1)` stream → post-read length REJECT-not-truncate): provider
bodies ≤ 8 MiB (nh-core), MCP well-known/RPC ≤ 4 MiB (nh-tools), OAuth JSON ≤ 256 KiB. Remote `ttlMs`
is clamped ≤ 24h with `checked_add` (removing an `Instant` overflow panic path); `tools/list` is
truncated-and-warned at ≤ 512 tools. Lifecycle/quota: `McpServer` gains a real `Drop`
(Option-take handle, signal+join, no double-join with `shutdown(self)`); caller-supplied bearer tokens
must be ≥ 32 bytes or bind fails; `fleet_run` clamps `max_workers` to the config ceiling; a global
active-run cap of 4 is enforced by an `ActiveRunGuard` RAII that decrements on all three exit paths
including spawn-fail (CURRENT_TASK.md:19; caps recommended-then-confirmed per CURRENT_TASK.md:31).

**Alternatives considered:**
- Truncate oversized bodies instead of rejecting — the shipped choice is explicitly
  "reject-not-truncate" (CURRENT_TASK.md:19); a truncated provider/MCP body would be parsed as if
  complete. (The reject-over-truncate reasoning is stated only as the shipped property, not argued in
  the recorded sources — see UNSOURCED for the cap-number rationale.)
- No caps (status quo) — audit H-12: "a provider or MCP server can cause memory exhaustion or a
  process panic" via unbounded `.text()` buffering and an unchecked `ttlMs` add (audit lines 197-203).
- No lifecycle/quota work (status quo) — audit M-06: no `Drop` on the accept thread, no caller-token
  entropy floor, unbounded background fleet runs (audit lines 255-259).

**Why:** Fail-closed (ratified Q3) applied to resource limits: an over-limit response is an error, not
silently degraded data. Serves secure + safe + lightweight. Process note: the v1 Sol run false-stopped
on an in-file `test_runtime()` literal due to a mis-worded scope guardrail; the orchestrator corrected
the brief (C2) and v2 ran clean — recorded as a briefing lesson, not a code decision
(CURRENT_TASK.md:19).

**Immediate effect on the harness:** A hostile or broken server can no longer exhaust memory, panic
the process via `ttlMs`, or pile up unbounded background fleet runs; +5 tests (gate 483 → 488/0/1).

**Long-term consequence:** Every future remote read has a house pattern to copy (per-crate capped
reader, no new dependency); the active-run cap fixes MCP's concurrency story at 4 until deliberately
revisited.

**Evidence:** audit H-12 (AUDIT_2026-07-21:197-203) + M-06 (:255-259); CURRENT_TASK.md:19 (session-7b,
codex `bilkrf1u7`, 3 files) and :31 (recommended caps). Gate 488/0/1. Uncommitted lump.

**Article angle:** Oversized responses are treated as errors, not trimmed input — the harness would
rather refuse a 9 MiB reply than pretend it read it.

**Review later:** yes — the specific numbers (8 MiB / 4 MiB / 256 KiB / 24h / 512 / 4) are
first-release constants; revisit when real workloads exercise them.

---

## 2026-07-23: W6a — a repository can NEVER grant MCP auto-trust; only user-global `~/.nosis/mcp.toml` can

**Decision:** MCP trust gets a user-global source (`~/.nosis/mcp.toml`) and the repository config
becomes tighten-only: the merge (`merge_and_vet` with `more_restrictive_mcp_trust`) lets a repo
`.nosis/mcp.toml` restrict but never introduce auto-trust; repo-declared servers with `trust = auto`
are clamped to ask, and repo-only entries pointing at link-local/metadata literal IPs are dropped.
Discovery (`tools/list`) is gated as network egress BEFORE any connection, and tool calls apply a
bare-host `Access::Send` check (CURRENT_TASK.md:29; ratified as Q2-extension "MCP-trust = add
user-global `~/.nosis/mcp.toml` as trust source (repo tighten-only)" and "SSRF depth = link-local +
metadata literal IPs only", CURRENT_TASK.md:27).

**Alternatives considered:**
- Repo-config trust as-is — audit H-04: "opening a repository can initiate POST requests to
  loopback/private/link-local services; hostname blocks may not match; a hostile server can mislabel
  mutations as read-only" (audit lines 131-139).
- Deeper SSRF filtering (e.g. full private-range / DNS-resolution checks) — the ratified depth was
  "link-local + metadata literal IPs only" (CURRENT_TASK.md:27); reasoning beyond the ratification is
  not recorded (see UNSOURCED).
- Sol's v1 approach (adding a `source` field to the MCP config struct) — stopped clean because it
  rippled into the frozen `e3_korvin.rs` test and nh-tui; the owner chose Option A: provenance kept in
  nh-cli's separate-file merge, no struct change (CURRENT_TASK.md:29).

**Why:** This is ratified Q2 made mechanical: "repo `.nosis/*.toml` ... is RESTRICT-ONLY — may
tighten, never introduce a credential audience / MCP auto-trust / notify destination; new trust only
from user-global `~/.nosis`" (CURRENT_TASK.md:59). A checked-out repository is untrusted input; only
the user's own machine-global config can widen what the harness will talk to. Serves secure + safe +
congruent.

**Immediate effect on the harness:** Opening a hostile repo can no longer make the harness silently
contact attacker-chosen or cloud-metadata endpoints or self-approve tool calls; +11 tests
(gate 472 → 483/0/1).

**Long-term consequence:** A durable trust hierarchy — user-global grants, repo restricts — that every
future config surface (catalog, notify, MCP) must obey; forecloses "convenient" repo-side trust
features permanently.

**Evidence:** audit H-04 (AUDIT_2026-07-21:131-139); CURRENT_TASK.md:27, :29 (session-7, codex
`b3520tbf8`, 6 files), :59 (Q2). Gate 483/0/1. Uncommitted lump.

**Article angle:** The harness draws a hard line between "the repo you just cloned" and "your own
machine config": only the latter can ever widen network trust.

**Review later:** no.

---

## 2026-07-23: Wave 6 split three ways for auditable diffs; "explain rec, then owner decides" made a hard rule

**Decision:** Wave 6 (MCP egress + limits + redaction) was split at the finest grain the owner
ratified — W6a (egress/SSRF/trust, H-04) → W6b (limits+lifecycle, H-12+M-06) → W6c
(redaction/approval, H-11+H-14+H-03) — "for auditable diffs" (CURRENT_TASK.md:27). The same session
established a hard project-wide rule: every options-presentation must carry the orchestrator's
recommendation + why + the tradeoff — never a bare menu, never a silent decision (CURRENT_TASK.md:27;
memory [[decisions-explain-rec-then-owner-decides]], owner 2026-07-23).

**Alternatives considered:**
- One combined Wave 6 (the audit's prioritized list groups H-04/H-12/M-06 as one BLOCKER, audit line
  342) — rejected by the owner's finest-split ratification for auditability (CURRENT_TASK.md:27).

**Why:** Smaller waves keep each Sol diff independently reviewable by the adversarial-review step —
serving auditable + small. The decision rule keeps the owner as decider on every fork while requiring
the orchestrator to commit to a position first.

**Immediate effect on the harness:** Three separately gated, separately reviewed diffs (483 → 488 →
493) instead of one large one; process docs updated.

**Long-term consequence:** Sets the granularity norm for future remediation work and makes
recommendation-first decision-making a standing project rule beyond Slice G.

**Evidence:** CURRENT_TASK.md:27 (session-7 checkpoint); audit prioritized item 6
(AUDIT_2026-07-21:342).

**Article angle:** A single audit blocker was deliberately cut into three separately gated waves so
each diff stayed small enough to adversarially review.

**Review later:** no.

---

## 2026-07-23: W5 — the meter fails closed: unpriceable is "unpriced", truncated is not "Pass"

**Decision:** Metering and outcome classification fail closed: `cost_of` returns `Option<f64>` and
rejects impossible usage (cached > prompt) and non-finite values, with `money()` rendering "unpriced"
instead of "0.00"; usage accumulation is checked and overflow yields an explicit `MeterIncomplete`
state; `classify_finish_reason` types the provider's finish reason (normal/absent → Pass,
length/max_tokens → Partial, content_filter → Fail + `FailureClass::Filtered`, unknown → Partial with
an "unrecognized finish reason" emit — the answer is ALWAYS still returned); `to_usd_approx` requires
present-AND-unexpired FX (CURRENT_TASK.md:39).

**Alternatives considered:**
- Saturating/zero-rendering status quo — audit H-09: "costs and savings can be presented as
  authoritative despite invalid or overflowed inputs ... Never display invalid cost as zero" (audit
  lines 173-179); audit H-10: "length, token-limit, or content-filter termination can be recorded as
  completed work" (lines 181-187); audit M-02: absent freshness metadata treated as valid forever
  (lines 229-235).

**Why:** Direct application of ratified Q3 ("fail-closed everywhere: invalid/overflow usage →
UNAVAILABLE; non-finite cost → 'unpriced' not 0.00; non-normal/unknown finish_reason →
Partial/Refused not Pass", CURRENT_TASK.md:59). For a product whose identity is the honest meter,
a fabricated $0.00 is worse than no number. Serves secure + auditable + congruent + harmonic.

**Immediate effect on the harness:** 9 files, +11 tests (gate 461 → 472/0/1); one orchestrator clippy
fix (`!is_some_and` → `is_none_or`, semantics-preserving). A LIVE-VERIFY of the four providers' actual
`finish_reason` strings is required before the final commit (CURRENT_TASK.md:11, :39).

**Long-term consequence:** The meter's honesty claims become structural: no code path can render an
invalid cost as a number, and a truncated answer can never again count as a completed task (which
fleet retry/escalation depends on).

**Evidence:** audit H-09/H-10/M-02 (AUDIT_2026-07-21:173-187, 229-235); CURRENT_TASK.md:39 (session-6,
codex `b9m92avi7`). Gate 472/0/1. Uncommitted lump.

**Article angle:** The cost meter now refuses to print a number it cannot stand behind — "unpriced"
replaced every path that used to show $0.00 for invalid arithmetic.

**Review later:** yes — the finish_reason Normal set ({"", stop, end_turn, stop_sequence}) must be
live-verified against all four providers before the Slice-G commit (CURRENT_TASK.md:11).

---

## 2026-07-23: W4 — a failed required receipt downgrades Pass → Fail (`Unreceipted`) but KEEPS the answer

**Decision:** If a required audit receipt cannot be persisted, the run outcome is downgraded from Pass
to Fail with a distinct `FailureClass::Unreceipted` — but the model's answer is kept and shown.
Alongside: an RAII `WorkerPool` guard closes work channels and joins every fleet worker on EVERY exit
path (with a drop-order fix so a worker stuck on an event/ack send cannot deadlock the join), and
`run_with_id` refuses a non-empty ledger as a new run ("use resume"); `append_run_failed` failures
compose into the returned error (CURRENT_TASK.md:38).

**Alternatives considered:**
- Fail the WHOLE run and discard the answer vs. a distinct unreceipted outcome — this exact fork was
  the design call brought to the owner ("does a failed required receipt fail the WHOLE run, or yield a
  distinct 'unreceipted' outcome?", CURRENT_TASK.md:51); the shipped shape is
  downgrade-but-keep-the-answer (CURRENT_TASK.md:38).
- Status quo — audit H-08: "a run can report success without its required audit record" (lines
  165-171); audit H-07: coordinator `?`-returns detach live workers past the run lock (lines 157-163);
  audit M-05: `RunFailed` bookkeeping silently disappears (lines 249-253).

**Why:** Q3 fail-closed applied to auditability: success without the audit record is not success. But
destroying user value (the answer) would punish the user for a bookkeeping failure — so the outcome is
honest (Fail/Unreceipted) while the work product survives. Serves auditable + safe + harmonic.

**Immediate effect on the harness:** 3 files (gate 456 → 461/0/1); fleet workers can no longer outlive
the coordinator's run lock; run IDs cannot be silently reused.

**Long-term consequence:** "Receipted" becomes a hard postcondition of Pass — the property the whole
launch narrative (the receipt) rests on is now enforced, not aspirational.

**Evidence:** audit H-07/H-08/M-05 (AUDIT_2026-07-21:157-171, 249-253); CURRENT_TASK.md:38
(session-6, codex `b5p3yarqa`) and :51 (the design fork). Gate 461/0/1. Uncommitted lump.

**Article angle:** A run that cannot write its receipt is now recorded as failed even when the answer
was fine — and the answer is still handed to the user.

**Review later:** no.

---

## 2026-07-22: W3 design calls — constitution files capped at 64 KiB skip-and-warn; `nh init` trusts a git query, not path heuristics

**Decision:** Two owner-ratified calls inside the filesystem-trust wave: (1) C-02's repo instruction
files (`AGENTS.md`, `.nosis/memory.md`, `law.toml`) load through a new `read_guarded_text` —
symlink-reject + canonical containment + a 64 KiB cap with skip-and-warn semantics (an over-cap file
is skipped with a warning, not partially ingested); (2) H-13's git-directory discovery is replaced by
a trust-git-query approach with a git-registered-worktree check that defeats a forged `gitdir:`
pointer, refuses a custom `core.hooksPath`, and installs hooks no-follow (CURRENT_TASK.md:49). Also in
the wave: `ReceiptWriter::append` gains no-follow + `File::lock` + `sync_all` (H-08 fs-part), and
`EditFile` gets capped single-handle reads with atomic temp→fsync→rename replacement (M-04).

**Alternatives considered:**
- Unbounded, symlink-following `fs::read_to_string` (status quo) — audit C-02 (CRITICAL): "a hostile
  checkout can point either file outside the repository and cause arbitrary readable local content to
  be uploaded" to the provider (audit lines 95-103, independently confirmed at line 13).
- Path-heuristic `.git` resolution (status quo) — audit H-13: "running `nh init` in a crafted
  checkout can create a pre-commit file in an arbitrary existing external directory" (lines 205-211).

**Why:** Both close hostile-repo trust-boundary escapes (audit prioritized BLOCKER 3, line 339).
Skip-and-warn on the size cap follows the honesty norm: the file is not silently half-read. Serves
secure + safe + auditable. One orchestrator mechanical fix was needed and gated green: `.read(true)`
added to the receipt `OpenOptions` because Windows `LockFileEx` requires read/write data access
(CURRENT_TASK.md:49).

**Immediate effect on the harness:** 4 files, zero new deps (gate 446 → 456/0/1); a cloned repo can no
longer exfiltrate arbitrary local files via instruction symlinks or plant hooks outside itself.

**Long-term consequence:** Establishes guarded-read as the only sanctioned way repo-provided text
enters a prompt; `nh init` derives trust from git's own answers rather than filesystem shape.

**Evidence:** audit C-02 (AUDIT_2026-07-21:95-103, gate note :13), H-13 (:205-211), H-08 (:165-171),
M-04 (:243-247); CURRENT_TASK.md:49 (session-5b, codex `b7nxo8ged`). Gate 456/0/1. Uncommitted lump.

**Article angle:** A `git clone` could previously read any file on the machine into a provider prompt
via one symlink; now repo instructions load through symlink-rejecting, size-capped guarded reads.

**Review later:** no.

---

## 2026-07-22: H-02 shutdown — 250ms bounded join with detach-on-deadline, ratified "Accept"

**Decision:** TUI worker shutdown (audit H-02: `Worker::drop` could deadlock indefinitely or wait a
full provider HTTP timeout) is fixed as: drop the approval sender first → send Stop → BOUNDED join
with a 250ms deadline → detach on deadline rather than hang; approval waits are cancellation-aware;
never an unconditional `JoinHandle::join` in Drop. The owner ratified this surface "Accept"
(CURRENT_TASK.md:49, :63, :65).

**Alternatives considered:**
- Unconditional join (status quo) — audit H-02: "quit/error/panic handling can hang indefinitely"
  (audit lines 113-121).
- Waiting out the in-flight provider call — rejected in the orchestrator's recorded recommendation:
  a synchronous provider HTTP call is uninterruptible from here, so the alternative to detaching "=
  the H-02 hang"; normal/idle/parked-approval shutdown completes in <10ms and detach occurs only when
  a provider call is mid-flight, left to nh-core's request timeout (CURRENT_TASK.md:65).

**Why:** Bounded shutdown beats a hang; the residual is explicitly scoped and time-limited by the
HTTP timeout. Serves safe + harmonic (quit always feels immediate).

**Immediate effect on the harness:** Part of W2 (new `terminal.rs` + `worker.rs` modules; H-01
independent best-effort terminal restore and L-01 panic-hook repair shipped alongside; gate
439 → 446/0/1).

**Long-term consequence:** Locks in detach-on-deadline as the shutdown contract until provider calls
become cancellable (which would remove the residual entirely).

**Accepted residual risk:** on a deadline expiry the in-flight worker thread is DETACHED, not joined —
it lives until nh-core's own request timeout ends the provider call; owner ratified this trade as
"Accept" (CURRENT_TASK.md:49, :65).

**Evidence:** audit H-02 (AUDIT_2026-07-21:113-121); CURRENT_TASK.md:49 (ratification), :63 (W2
detail, codex `bg5kehifp`), :65 (recommendation + residual). Gate 446/0/1. Uncommitted lump.

**Article angle:** Quit was made unconditionally bounded at 250ms by accepting — and documenting —
that a mid-flight HTTP call briefly outlives the UI instead of holding it hostage.

**Review later:** yes — if provider calls gain cancellation, the detach residual can be eliminated.

---

## 2026-07-22: W1 — one credentialed-client boundary; four owner-ratified surfaces

**Decision:** All credentialed client creation is centralized in a new `nh-core::credential::connect`
— the ONE boundary — taking a non-forgeable `ResolvedRoute` (fields made private + accessors +
compile-fail doctest, closing M-10) and performing `get_scoped` at materialization. `nh-vault` gains
`exact_origin` (scheme+host+port, loopback-http decided without DNS, typed
`AudienceRefused{Unapproved|InsecureTransport}`); `nh-routes` gains `validate_route_url`. Four
surfaces were ratified by the owner: (1) the credential module lives in nh-core (adds an
nh-mcp→nh-core edge); (2) host-only law audiences now mean `https://host:443`, non-default ports need
an explicit origin; (3) refusals show effective ports, and audience-refusal is reported before
missing-key; (4) catalogs with empty or remote-http `base_url` now fail resolution (the shipped
catalog is all-https, so safe) (CURRENT_TASK.md:61).

**Alternatives considered:**
- Per-surface scoping (status quo) — audit C-01 (CRITICAL): host-only audience comparison discarded
  scheme/port (http-downgrade, arbitrary-port), and TUI/fleet/MCP bypassed scoping entirely with raw
  `vault.get` → "direct provider-key disclosure" via a hostile catalog (audit lines 78-93; workspace
  meta-finding 1, line 329: "Credential authorization is not a single congruent abstraction").

**Why:** Q1 ratified: credentials attach only on https + exact origin, with plain http only for
literal loopback decided without DNS (CURRENT_TASK.md:59). One choke point makes the property
auditable instead of re-proven per surface. Serves secure + congruent + modular + auditable.

**Immediate effect on the harness:** New crates/nh-core/src/credential.rs; direct `vault.get` removed
from surfaces; a hostile catalog can no longer redirect a key to an attacker host or downgrade to
http (gate 432 → 439/0/1, codex `b4lc0pu25`).

**Long-term consequence:** Every future surface inherits the origin discipline for free; a breaking
behavioral change is locked in (host-only audiences = port 443; non-default ports must be explicit).

**Evidence:** audit C-01 (AUDIT_2026-07-21:78-93, gate note :12) + M-10 (:279-283);
CURRENT_TASK.md:59 (Q1), :61 (W1 + four ratified surfaces). Gate 439/0/1. Uncommitted lump.

**Article angle:** The critical audit finding was solved by making it structurally impossible — only
one function in the tree can mint a credentialed client, and it demands https plus the exact origin.

**Review later:** no.

---

## 2026-07-22: Slice G ground rules ratified — Q1 (https + exact origin + literal loopback), Q2 (repo restrict-only), Q3 (fail-closed)

**Decision:** Three security decisions ratified to apply across ALL Slice-G waves: **Q1** — a
credential attaches only over `https` with exact origin (scheme+host+port); plain `http` only for
literal loopback (`localhost` / `127.0.0.0/8` / `::1`), decided WITHOUT DNS. **Q2** — repository
`.nosis/*.toml` (catalog/mcp/notify) is RESTRICT-ONLY: it may tighten, never introduce a credential
audience, MCP auto-trust, or a notify destination; new trust comes only from user-global `~/.nosis`.
**Q3** — fail closed everywhere: invalid/overflow usage → UNAVAILABLE; non-finite cost → "unpriced"
not 0.00; non-normal/unknown `finish_reason` → Partial/Refused not Pass; failed required receipt →
run FAILED (CURRENT_TASK.md:59, verbatim structure).

**Alternatives considered:**
- The recorded checkpoint states the ratified outcomes, not the rejected options per question (they
  were presented as recommend+why forks per the project rule); the specific rejected variants are not
  in the written record (see UNSOURCED). The status-quo alternative for each is the corresponding
  audit finding class: C-01 (origin loss), H-04/M-09 (repo-introduced trust), H-09/H-10/H-08
  (fail-open metering/receipts).

**Why:** These are the audit's top LAW violations turned into standing invariants: "Credential
authorization is not a single congruent abstraction"; "Receipt and metering failures can be silently
converted into apparent success" (audit lines 327-333). Deciding them once, up front, gave every wave
a consistent north star. Serves secure + congruent + harmonic.

**Immediate effect on the harness:** Q1 shaped W1, Q2 shaped W6a, Q3 shaped W4/W5/W6b — every wave
cites back to them (CURRENT_TASK.md:61, :29, :38-39, :19).

**Long-term consequence:** Three permanent product invariants that any future feature must satisfy;
in particular Q2 permanently forecloses repo-config convenience features that widen trust.

**Evidence:** CURRENT_TASK.md:59 (session-5 checkpoint, 2026-07-22); audit Part 3 top LAW violations
(AUDIT_2026-07-21:327-333); memory [[slice-g-audit-remediation]].

**Article angle:** Before writing any fix, the project ratified three blanket invariants — exact-origin
credentials, restrict-only repo config, fail-closed everything — and derived seven waves from them.

**Review later:** no.

---

## 2026-07-21/22: FULL-REMEDIATION-FIRST — seven gated blocker waves, ONE commit, no ship until all pass + FEEL

**Decision:** Facing a pre-release audit verdict of "**NO — not releasable as v0.1.0
source-install**" (2 critical / 14 high / 10 medium / 1 low / 1 nit), the owner chose full
remediation BEFORE any release: **Slice G** = 7 gated BLOCKER waves in the audit's own prioritized
order (audit lines 335-346), with **NO commit until all waves pass their gates plus the owner's FEEL
gate**, then ONE coherent commit (the uncommitted W4-SURFACES work + all of Slice G are one lump on
HEAD `c9863d1`), then Slice H (cosmetic god-module splits, deferred), then v0.1.0
(CURRENT_TASK.md:57).

**Alternatives considered:**
- Ship v0.1.0 and patch — the audit's calibration note gave real cover for this: "for the current
  owner-run, loopback-preview posture the *live* exposure is lower than the CRITICAL labels imply —
  **but** THE LAW (secure, safe, congruent, auditable, modular) is genuinely not met, so the 'not
  releasable' verdict stands" (audit line 17). The owner chose remediation-first anyway
  (CURRENT_TASK.md:57: "Owner chose FULL REMEDIATION FIRST").
- Commit wave-by-wave — rejected in favor of one coherent commit after all waves + FEEL; every
  checkpoint repeats "NO commit until ALL Slice-G waves pass + owner FEEL, then ONE coherent commit"
  (CURRENT_TASK.md:7, :17, :25, :47, :57).
- Fold the cosmetic module splits in — deferred to Slice H so remediation stayed pure
  (CURRENT_TASK.md:57; audit item 10, line 346, is the NICE-rated split work).

**Why:** The audit's verdict rested on THE LAW, not on live exploitability — and the project treats
THE LAW as the release bar. The one-commit rule keeps `main` at a known-good HEAD (`c9863d1`)
throughout: at no point does the public history contain a half-remediated state. Waves follow the
audit's own prioritized BLOCKER order 1:1 (audit lines 335-344 → W1 credentials, W2 lifecycle, W3 fs
trust, W4 receipts, W5 meter, W6 MCP, W7 exec). Serves secure + auditable + congruent.

**Immediate effect on the harness:** v0.1.0 was postponed behind ~61 net new tests of security
behavior (432 → 493 and counting); the entire remediation lives as one reviewable uncommitted lump.

**Long-term consequence:** Sets the release precedent: an internal audit verdict is binding even when
the auditor itself notes the live risk is low. Enables the launch claim that v0.1.0 shipped with its
own pre-release audit fully remediated rather than triaged.

**Accepted residual risk:** the whole remediation sits uncommitted in the working tree for days
(protected only by the owner's machine) — implicit in the one-lump rule; every checkpoint carries
recovery instructions (e.g. CURRENT_TASK.md:69). Not framed as a security decision in the sources.

**Evidence:** 04-research/AUDIT_2026-07-21_sol-max_pre-release.md:27 (verdict), :17 (calibration),
:335-346 (prioritized list); CURRENT_TASK.md:57 (the choice), :7/:17/:25/:47 (the standing no-commit
rule); gate counts 439→446→456→461→472→483→488→493.

**Article angle:** The project's own audit said the live exposure was modest — and the release was
still held for seven fully gated remediation waves because the internal law, not exploitability, is
the bar.

**Review later:** no.

---

## 2026-07-20: Live-provider test policy — real keys, a $2-per-provider HARD CAP, ≈$0.0014 actually spent

**Decision:** Launch evidence comes from REAL provider calls, run by the orchestrator (not the owner
by hand), under a hard spending cap of **<$2 per provider**, with keys resolved from the OS vault:
GLM free tier ($0.00) plus real DeepSeek / Kimi / MiMo keys, tiny identical prompts, `--max-turns 2`
(CURRENT_TASK.md:3, :75; BUILD_LOG.md:5-45). Actual total spend ≈ **$0.0014** across all four
providers — the priciest single provider (Kimi, $0.0009) was ~2200× under the cap (BUILD_LOG.md:21-22).

**Alternatives considered:**
- Mocked-only verification — insufficient for the two `[VERIFY-LIVE]` wire-shape guesses carried
  since Slice A (DeepSeek `thinking:{type:disabled}`, Kimi K2.6 toggle), which only a real API could
  confirm; both were CONFIRMED live with no 400 (BUILD_LOG.md:38-39).
- Uncapped live testing — the cap was owner-set as a HARD CAP from the start (CURRENT_TASK.md:3); the
  reasoning for the $2 figure itself is not recorded (see UNSOURCED).

**Why:** The product's headline is the honest meter, so the launch evidence had to be honestly
metered: the live run verified cross-currency refusal ("¥0.0041 vs chosen $0.0003 — different
currency, not directly comparable" — no fake FX), `usd_approx` only on fresh FX, DeepSeek's 2× peak
window applied truthfully ("we ARE at peak", not a fabricated markup), and no invented savings
(BUILD_LOG.md:24-37). Serves auditable + congruent (the meter was proven with the meter).

**Immediate effect on the harness:** No code change — docs-only launch evidence; typed receipts
appended to gitignored `.nosis/receipts.jsonl`; the last two Slice-A wire guesses retired
(BUILD_LOG.md:38-41).

**Long-term consequence:** Every launch-post cost figure has a receipt behind it; the four wire
shapes are now facts, not assumptions. Note for W5: the providers' `finish_reason` strings still need
a live check before the Slice-G commit (CURRENT_TASK.md:11).

**Evidence:** BUILD_LOG.md:5-45 (2026-07-20 entry, per-provider table + invariants);
CURRENT_TASK.md:3 ("<$2/provider HARD CAP"), :75 (session-3 checkpoint); commit 6637b45
("docs(release): record LIVE provider tests — launch evidence (4 providers, ~$0.0014 total)").

**Article angle:** The complete four-provider launch verification cost about a seventh of a cent, and
every number in it came off the product's own receipts.

**Review later:** no.

---

## 2026-07-20: Section B — `unsafe_code = "forbid"` workspace-wide, cargo-deny gated with nothing suppressed, keyless CI (cccb2dc)

**Decision:** The release engineering tail, metadata/config/CI only (no source logic, Cargo.lock
unchanged): (1) unsafe forbidden via root `[workspace.lints.rust] unsafe_code = "forbid"` with
`[lints] workspace = true` in all 9 crates — confirmed zero direct `unsafe` in-tree; (2)
`license = "MIT"` in `[workspace.package]`, inherited per crate; (3) cargo-deny 0.20.2 activated and
wired into gate.ps1 as a fourth step between clippy and test, GREEN with **nothing suppressed** —
`[advisories] ignore = []`, the only policy delta being `CDLA-Permissive-2.0` on the license
allow-list for webpki-roots trust-anchor data; (4) 27 internal path deps pinned to
`version = "0.1.0"` because `wildcards = "deny"` treats a versionless publishable path dep as `*`;
(5) keyless CI (`.github/workflows/ci.yml`): `contents: read`, windows+ubuntu matrix on pinned Rust
1.96.0 running the gate's checks plus a separate cargo-deny job — no secrets, suite offline/mocked
(git show cccb2dc; verified in-tree: gate.ps1:38, deny.toml:6 + :26).

**Alternatives considered:**
- Per-file `#![forbid(unsafe_code)]` — "Chosen over per-file `#![forbid]` for DRY/auditability
  (congruent with THE LAW)" (cccb2dc commit message): one source of truth a new crate cannot forget.
- Suppressing a deny finding to get green — did not arise: "NO RustSec advisory was found or ignored
  (`[advisories] ignore` stays empty; nothing suppressed)" (cccb2dc).
- CI with provider secrets — rejected by construction: keyless, offline/mocked (cccb2dc).

**Why:** Explicitly sourced in the commit: DRY/auditability, THE LAW's congruent clause. The
forbid-unsafe line is what four days later gave Wave 7's R1 its teeth. Deferred to backlog and
recorded as such: cargo-nextest, AV canary, frozen-surface sensor (cccb2dc; BUILD_LOG.md:77-78).

**Immediate effect on the harness:** The gate became four steps (fmt / clippy -D / deny / test) —
416/0/1 at commit; supply-chain policy (crates.io-only sources, wildcard deny, permissive-license
allow-list, yanked deny) is enforced on every commit and in CI (gate.ps1:36-39; deny.toml).

**Long-term consequence:** Zero-unsafe is now a workspace invariant with a compile-time enforcement
point, which constrains future dependency and API choices (see Wave 7 R1). "Nothing suppressed" is a
checkable claim (`ignore = []`) usable in public material.

**Evidence:** commit cccb2dc (13 files, +113/−35); gate.ps1:38 (`deny check` step); deny.toml:6
(`ignore = []`), :26 (CDLA-Permissive-2.0); .github/workflows/ci.yml; BUILD_LOG.md:49-83; gate
416/0/1.

**Article angle:** The supply-chain gate went green without waiving a single advisory, and the
zero-unsafe rule it shipped became a binding constraint on a security fix within the week.

**Review later:** no.

---

## 2026-07-20: FULL MCP metered-service expansion (7c2b2c4) — and the hard gate: loopback-preview only until the MCP final spec lands 2026-07-28

**Decision:** The loopback MCP preview server was expanded to express the product's differentiator
("priced, routed, receipted, and you can see why") as first-class MCP tools: three new read-only
tools — `why` (mirrors `nh why`), `route_cost`, `receipts` — plus `structuredContent` on all six
tools, all egressing through one scrubbing choke (`tool_result` scrubs text via `nh_vault::safe_line`
AND `scrub_json`s the structured value, recursing strings, array elements, object values and keys).
Additive: every existing tool's TEXT response stays byte-compatible; server stays stateless,
loopback-only, preview (git show 7c2b2c4). The owner ratified the FULL expansion: "do it well, test +
verify, don't assume, best-for-the-project as of 2026-07-20" (CURRENT_TASK.md:85-86). Standing HARD
GATE: the MCP server "may be hardened now but NOT shipped publicly until the MCP final spec lands
2026-07-28" (CURRENT_TASK.md:3) — it does not block a CLI/TUI v0.1.0 as long as `nh mcp serve` stays
the loopback preview and no docs promote it (CURRENT_TASK.md:73).

**Alternatives considered:**
- A scoped/minimal MCP addition — the checkpoint records the orchestrator was required to bring "a
  scoped MCP recommendation BEFORE building" and the owner ratified "MCP = FULL expansion"
  (CURRENT_TASK.md:3, :85); the recommendation's content is not in the written record (see UNSOURCED).
- Silently editing the frozen test — Sol's first run STOPPED CLEAN and reverted when the frozen
  `tests/e3_korvin.rs` asserted the exact old 3-tool set; the owner then authorized widening ONLY
  that one assertion (+3/−0, alphabetical; every other assertion byte-identical) — "the only edit
  outside src/lib.rs" (7c2b2c4 commit message; BUILD_LOG.md:91-96).
- Shipping MCP publicly with v0.1.0 — foreclosed by the 2026-07-28 spec gate (CURRENT_TASK.md:3).

**Why:** The MCP surface is where other agents will read the meter, so the meter semantics were made
structural (usd_approx only on FRESH fx; savings OMITTED when naive_cost is None; "never claims a
saving it did not compute") and everything passes the scrubber in BOTH text and structured surfaces
(7c2b2c4). The public-ship gate exists because the MCP final spec had not landed — building against a
moving spec and shipping publicly would have been a compatibility bet. Serves secure + congruent +
auditable.

**Immediate effect on the harness:** nh-mcp only (`src/lib.rs` +826/−19 plus the one authorized test
edit), no new deps, no lockfile change; gate 416/0/1; live-verified over 127.0.0.1: `tools/list` = 6,
cross-currency REFUSED, fresh-fx `usd_approx`, 10 real typed receipts, no-bearer → HTTP 401
(CURRENT_TASK.md:77). Known cosmetic follow-up: `print_banner` still hints only the old 3 tools
(CURRENT_TASK.md:77).

**Long-term consequence:** Locks in the one-scrubbing-choke egress pattern for every future MCP tool,
and the clean-stop-on-frozen-surface protocol (an implementer halts and reverts rather than widening
a frozen test without authorization). Public MCP timing stays coupled to the external spec.

**Evidence:** commit 7c2b2c4; CURRENT_TASK.md:3 (hard gate), :73, :77, :79-110 (sessions 1-2);
BUILD_LOG.md:87-99; audit N-01 later confirmed the banner nit (AUDIT_2026-07-21:295-297). Gate
416/0/1; live verification over 127.0.0.1 recorded 2026-07-20.

**Article angle:** The cost meter became machine-readable over MCP — with the same refuse-to-fabricate
semantics enforced in the structured output — while a spec-driven calendar gate kept the server
loopback-only.

**Review later:** yes — the public-MCP decision reopens when the MCP final spec lands (2026-07-28).

---

## 2026-07-20: LICENSE — MIT, copyright **nosistech LLC**, not the founder personally (7f4add6)

**Decision:** The project is licensed MIT with "Copyright (c) 2026 nosistech LLC" (LICENSE:1-3). The
task was owner-specified as "LICENSE = MIT © nosistech LLC (not Carlos personally)"
(CURRENT_TASK.md:3, verbatim). License metadata followed in Section B (`license = "MIT"`
workspace-wide, cccb2dc), and MIT is on deny.toml's own allow-list (deny.toml:17).

**Alternatives considered:**
- Copyright held by Carlos Paredes Vargas personally — explicitly excluded by the owner's phrasing
  "(not Carlos personally)" (CURRENT_TASK.md:3). The underlying reasoning (liability, transferability,
  branding) is NOT stated in any source examined (see UNSOURCED).
- Other licenses — no alternative license is discussed in any source examined; MIT arrived as a
  settled owner directive.

**Why (as sourced):** Only the fact of the choice and the entity are sourced; note MIT is maximally
compatible with the project's own supply-chain policy (permissive-only allow-list, deny.toml:14-27),
making the harness consumable under the same rules it imposes on its dependencies — an observation
from the artifacts, not a stated rationale.

**Immediate effect on the harness:** The repo became legally distributable; cargo-deny's license gate
and the crate metadata are consistent with the LICENSE file.

**Long-term consequence:** Copyright sits with a company, which affects who can relicense or accept
contributions long-term; MIT permanently permits closed-source forks.

**Evidence:** commit 7f4add6 (LICENSE 21 lines + SECURITY.md 83 lines); LICENSE:1-3;
CURRENT_TASK.md:3, :79-80; cccb2dc (metadata).

**Article angle:** The harness shipped under MIT held by the company entity rather than the founder,
matching the permissive-only policy it enforces on its own dependency tree.

**Review later:** no.

---

## 2026-07-20: SECURITY.md written in ASD-STE100 Simplified Technical English (7f4add6)

**Decision:** The security policy is written in ASD-STE100 Simplified Technical English — short
declarative sentences, controlled vocabulary (visible in the artifact, e.g. "Do not open a public
issue for a security problem... Send a private email to `info@nosistech.com`", SECURITY.md:17). It
defines: latest-release + `main`-only security fixes; private disclosure to `info@nosistech.com` with
a 5-business-day first-response SLA (both owner-ratified, CURRENT_TASK.md:86-87); a security-model
summary (law verdict classes / audience binding / Scrubber / fail-closed / loopback MCP preview); and
a coordinated-disclosure safe harbor. Facts were sourced from 02-architecture/SECURITY_MODEL.md +
CONTRACTS_M5; "no audit numbers published (states 'no critical problems')" (7f4add6 commit message —
note this commit predates the 2026-07-21 audit by one day).

**Alternatives considered:**
- Conventional prose security policy — no alternative style is discussed in the sources; ASD-STE100
  was an owner-assigned requirement ("SECURITY.md in ASD-STE100 Simplified Technical English",
  CURRENT_TASK.md:3). The reason for choosing ASD-STE100 is NOT stated (see UNSOURCED).

**Why (as sourced):** Drafted by Fable 5 high, orchestrator-gated (7f4add6). The style's observable
effect in the artifact is unambiguous reporting instructions readable by non-native speakers — but
that framing is inference from the text, not a recorded rationale.

**Immediate effect on the harness:** A public vulnerability-disclosure channel with a stated SLA
existed before the pre-release audit ran.

**Long-term consequence:** The 5-business-day SLA and the latest-release-only fix policy are public
commitments; the "no critical problems" phrasing became outdated one day later when the 2026-07-21
audit found two criticals — all of which are being remediated pre-release in Slice G, so the statement
is on track to be true again at v0.1.0 (audit AUDIT_2026-07-21:27; CURRENT_TASK.md:57).

**Evidence:** commit 7f4add6; SECURITY.md:1-45 (style + SLA + model summary); CURRENT_TASK.md:3
(assignment), :86-87 (contact + SLA ratified).

**Article angle:** The security policy is written in the controlled language used for aircraft
maintenance manuals, and its disclosure SLA was live before the project audited itself.

**Review later:** yes — re-verify the "no critical problems" claim against the remediated tree before
the v0.1.0 tag (the Slice-G commit should make it accurate; leaving it stale would contradict the
project's own honesty rule).
