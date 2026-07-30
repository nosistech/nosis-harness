# Current Task

## Current override — continuation saved 2026-07-27

The authoritative current state and next-action sequence is
[`CONTINUE_HERE.md`](./CONTINUE_HERE.md). The public empty repository now exists at
`nosistech/nosis-harness`; the A+ responsibility refactor remains deliberately uncommitted; the
old release process has exited and its canonical executable must be rebuilt; the current-source
Windows FEEL gate, first push, remote CI, branch protection, and publication are still pending.

Everything below this override is historical provenance. Do not execute an older “NOW” or
“ON continue” block when it conflicts with `CONTINUE_HERE.md`.

## Current state — owner commit approval (2026-07-26)

The whole-project audit, public-v0.1 hardening, Telegram removal, and responsibility-boundary
modularization are committed at `e42a5bc`. The complete local release gate and optimized-binary
smokes pass (512 passed / 0 failed / 1 ignored). The `0.1.0` changelog is rolled and the
evidence-backed release checklist is current. Remaining before `v0.1.0`: the owner's subjective
terminal FEEL pass; a valid intended public GitHub remote; protected `main`, private vulnerability
intake, and repository metadata; green Windows/Ubuntu/macOS/supply-chain CI; then tag and publish.
Historical checkpoints below are retained for provenance; their older "do not commit" and blocker
language is superseded by this status.

## Immediate Goal — **RELEASE SLICE** (owner-directed 2026-07-20): take nosis from private-beta to public-1.0-ready. **W5 "FLEET RELIABILITY" SHIPPED + committed `441727b` (2026-07-20)** — all 11 items W5-1..W5-11, gate 410/0/1 `--release`, clippy `-D` + fmt clean, std `File::try_lock` PRIMARY lock; reviewed ZERO-blocking; docs-closed (BUILD_LOG + CONTRACTS §0.1-F SHIPPED). That closes M5 Slice F HARDEN waves **W1✓→W3✓→W2✓→W5✓**, leaving only **W4** (nh-tui+nh-cli surfaces, FEEL) — now folded into the Release Slice. **NOW = the Release Slice, 4 owner-assigned items:** (1) **LICENSE = MIT © nosistech LLC** (not Carlos personally); (2) **SECURITY.md in ASD-STE100 Simplified Technical English**; (3) **live testing** (orchestrator runs it — DeepSeek/MiMo/Kimi REAL keys, **<$2/provider HARD CAP**; GLM free = `$0.00`) to light up the honest-meter/savings headline + confirm the VERIFY-LIVE wire shapes (DeepSeek `thinking:{type:disabled}`, Kimi K2.6 toggle); (4) **best-long-term MCP server** + finish sections **B** (engineering tail: `#![forbid(unsafe_code)]`+`[workspace.lints]`, cargo-deny green+wired-into-gate, keyless CI, rest of Slice E), **C** (distribution: install path, CHANGELOG, real RELEASE_CHECKLIST, versioning/tags), **D** (user docs: quickstart, privacy statement, telemetry stance, CONTRIBUTING) — every call rooted in long-term harness health, IN-SCOPE, law-abiding. **HARD GATE: MCP server may be hardened now but NOT shipped publicly until the MCP final spec lands 2026-07-28.** Orchestrator brings a scoped MCP recommendation BEFORE building (per [[decisions-explain-rec-then-owner-decides]]). **Execution model:** orchestrator (Opus 5) plans/gates/reviews/live-tests/commits; **Fable 5 high** writes docs (parallel, background); **Sol max** (`gpt-5.6-sol`, `model_reasoning_effort=max`, fallback `xhigh` — report which resolved) writes code (parallel, background) — DISJOINT files, NEVER two nosis codexes at once. **Checkpoint protocol (owner):** at ~250k context orchestrator STOPS + hands a self-contained continuation prompt; owner saves + `/clear` + types `continue` → seamless resume. Product identity held firm (harness with a meter: cheapest CAPABLE route + the receipt — NOT a chat UI, no new providers/autonomy in M5, no M6/M7 scope leak). Prior W1 (`6cefd56`), W3 (`73d278b`), W2 (`2e09513`); Slice A–D + fmt/gate/toolchain done; FULL Fable 5 audit (`c0ceaef`).

**⇒⇒⇒⇒⇒⇒⇒⇒⇒⇒⇒⇒ CHECKPOINT (2026-07-26, session 8) — ORCHESTRATOR HANDOFF: Claude Opus 5 → GPT-5.6 Sol max. NEWEST — read this, then `00-start-here/CONTINUE_HERE.md` IN FULL.**

**The self-contained handoff is `00-start-here/CONTINUE_HERE.md`** (copy at `Temp\sol_continue_prompt.txt`). Root `AGENTS.md` now points at it, so typing `continue` finds it. Sol's FIRST assigned task is **a full project analysis** (owner-directed), then STOP and report — no code, no commit. **Role note:** Sol now holds BOTH orchestrator and implementer roles, so the adversarial review is no longer independent — the owner is the gate.

**Closed this session:** (1) **W7 adversarial review DONE — ZERO BLOCKING**, the last unreviewed wave; H-05 inertness independently re-derived (`exec_verdict` has no `Allow` branch, nh-law:139; all 4 surfaces route through it: cmd_chat:182 / cmd_run:140 / nh-fleet:1487 / nh-tui/worker:221; `ToolCtx::new` defaults Exec→Ask, nh-tools:73) and H-06 confirmed (no unbounded `child.wait()` remains — only `try_wait()` at :596/:717). Two non-blocking nits left deliberately (shared-deadline drain LABEL imprecision; a provably-unreachable `expect`). **NB `git diff crates/nh-tools/src/lib.rs` is CUMULATIVE** — the "+442/−77" attributed to W7 also contains W3's `EditFile` atomic-write work; correct this in the commit message. (2) **All 27 open questions ANSWERED** — owner: "I don't remember any, to be honest." `08-decisions-and-risk/decisions/OPEN_QUESTIONS_RATIONALE.md` rewritten into an answered record where **every reconstruction is explicitly labelled NOT a memory**; the five `DECISIONS_*.md` stay a purely sourced record — **do not merge them.** (3) **Four owner rulings executed:** RISK_REGISTER (struck the false Job-Objects/restricted-tokens containment claim **plus 3 more false rows of the same class**), PRODUCT_BRIEF:9 delegate pitch, `CONTRACTS_M5` §Slice E wip-branch amendment (never adopted → superseded by the gate), BUILD_LOG backfill for M5 A/C/E. Plus a retroactive `CONTRACTS_M3` §9.2 mouse-capture amendment and the stale "Terra by default" wording in AGENTS/CODEX. (4) **Audit nit N-01 was ALREADY FIXED** — verified at `nh-mcp/src/lib.rs:162` (all 6 tools); the record claiming otherwise was stale, **no code change made**. (5) **Transcription audit = FAITHFUL** on both hand-transcribed decision files, zero factual drift — caveat CLOSED.

**⇒ MAJOR FINDING — THE PROJECT HAS NEVER BEEN BUILT OR TESTED OFF WINDOWS.** `git remote -v` is EMPTY, so `.github/workflows/ci.yml` **has never run, not once.** The earlier claim "CI verifies Linux every push" was FALSE and was corrected to the owner. TRUE status: **Windows** = 497/0/1 + live-verified on 4 providers; **Linux** = `#[cfg(unix)]` paths written but never built/tested/run; **macOS** = never built. **No Linux or macOS support claim may ship until actually tested.** Agreed plan: Linux via **VirtualBox Ubuntu 26.04 LTS Desktop** (6 GB / 4 vCPU / 60 GB / 3D OFF; NAT port-forward 127.0.0.1:2222→22; **`libdbus-1-dev` required** or the keyring backend won't link; Desktop-not-Server so ONE VM covers both the gnome-keyring path and the SSH/no-D-Bus `NH_<ENTRY>_KEY` path; cap `cargo build -j 4`). macOS via **GitHub Actions `macos-latest`** (VirtualBox macOS rejected: Apple EULA + no supported guest) — which also finally makes CI execute. Host: 24 cores / 31.4 GB (only ~9.7 GB free); **VBS/Memory Integrity is RUNNING** so VirtualBox falls back to the slow Hyper-V backend — owner advised to LEAVE IT ON (this box holds the API keys); do not quietly reverse that.

**⇒ STILL BLOCKING the ONE commit + v0.1.0 tag:** (1) **FEEL GATE — NOT DONE** (script ready at `Temp\feel-gate.ps1`, 15 steps, A1–A11 free via `glm-4.7-flash`; **actually open a window with `Start-Process wt.exe` — the owner justifiably complained that past sessions asked him to test without ever putting a window in front of him**); (2) **Linux verification** on the VM; (3) **live `finish_reason` strings** (~1 cent, owner approved — the classifier's Normal set was inferred, never observed); (4) **SECURITY.md:57 rewrite** ("The audit found no critical problems", written 2026-07-20, was outdated the next day by the 2 CRIT / 14 HIGH pre-release audit — verify C-01/C-02 genuinely closed, then name BOTH audits honestly). Non-blocking: DECISION_LOG index (mechanical; recipe in CONTINUE_HERE), CHANGELOG roll, README platform wording (**under-claim**). **Ratified, do not reopen:** tag v0.1.0 FIRST (Slice H → 0.1.1); MCP stays loopback-preview, not public before 2026-07-28; source-install tag; MIT © nosistech LLC. **Gate 497/0/1 `--release` still valid — no code changed this session.**

**⇒⇒⇒⇒⇒⇒⇒⇒⇒⇒⇒ CHECKPOINT (2026-07-25, session 7d) — SLICE G COMPLETE (W7 = LAST WAVE, IMPLEMENTED); GATE IN FLIGHT; OWNER'S NEXT ACT = ANSWER THE 27 OPEN QUESTIONS.**

HEAD = `c9863d1` on `main`; working tree = ONE uncommitted lump (W4 SURFACES + Slice G W1–**W7** + this session's docs). **NO commit until the gate is green + adversarial review + owner FEEL.** Then ONE coherent commit → Slice H (modularize) → v0.1.0.

**⇒ ON `continue`, DO THESE IN ORDER:**
1. ~~Read the gate log~~ **DONE — gate3 = `GATE: PASS`, 497 passed / 0 failed / 1 ignored `--release`** (`fmt --check` 0, `clippy -D` 0, `deny check` 0, `test --release` 0). 493 → 497 = Sol's 4 new W7 tests. Log = `Temp\w7-gate3.log` (UTF-16 — `Select-String`/Read tool, never Bash grep). **Slice G is now fully implemented and green.**
2. **Adversarially review W7's diff** — `git diff crates/nh-tools/src/lib.rs`. NOT YET DONE and REQUIRED before FEEL. Review targets below.
3. **ANSWER THE 27 OPEN QUESTIONS with the owner** — this is what the owner asked for on `continue`. File = `08-decisions-and-risk/decisions/OPEN_QUESTIONS_RATIONALE.md` (in-repo, 27 numbered items grouped by era + 10 flagged doc conflicts). Each item states the sourced facts and asks ONE question. Work through them with the owner, then fold the answers into the matching entries in `08-decisions-and-risk/decisions/DECISIONS_*.md`.
4. Build the **`00-start-here/DECISION_LOG.md` index** (deliberately deferred — it is mechanically derivable, so it cannot be lost). Recipe: grep the five era files for `^## ` headings and `**Article angle:**` lines, pair them, emit a newest-first table `Date | Decision | Era file | Article angle` (~76 rows). Then add: a "Review later" section, the STALE CLAIMS section, a link to OPEN_QUESTIONS_RATIONALE.md, and — CRITICAL — preserve the existing 85-line DECISION_LOG.md content verbatim under "## Pre-M2 entries (original format, preserved)".
5. FEEL gate → ONE commit (W4 + all Slice G W1–W7 + docs) → Slice H → v0.1.0.

**W7 "EXEC BOUNDARY" (H-05 + H-06) IMPLEMENTED**, codex Sol max, brief `Temp\sol_wave7_prompt.txt`, self-report `Temp\sol_wave7_out.md`. **ONE file: `crates/nh-tools/src/lib.rs`** (+442/−77, mtime-verified sole crate file Sol touched). Items W7-1..W7-9 all done, no deferrals. **H-05** = `ExecShell` now refuses on `Guard::Block` and requires `ctx.approve` for EVERY other verdict (`Allow` is no longer a bypass) + regression test. Grounding that made this safe: `nh_law::exec_verdict` returns only Block/Ask (test `..._never_allows`), all 4 surfaces map Verdict→Guard 1:1, and `ToolCtx::new`'s default guard returns Ask for Exec → **behaviorally inert for every shipped surface and the whole existing suite**. **H-06** = two hangs closed, not one: (a) the unbounded drain `join()` the audit named, (b) an unbounded `child.wait()` **inside `terminate_child_tree` itself**. New constants `DRAIN_GRACE = 5s`, `KILL_VERIFY_GRACE = 2s`. +4 tests; `nh-tools` = 83 passed / 0 failed. **Ratified owner calls: R1** = keep verified `taskkill /T /F` + report failure honestly; kill-on-close **Job Object REJECTED** (needs a new dep + `unsafe`, would break `unsafe_code="forbid"` from `cccb2dc`); accepted residual = a re-parenting grandchild may survive and is reported honestly, never claimed killed. **R2** = bounded drain via shared buffer + `recv_timeout`, **partial output preserved** + honest "capture incomplete" suffix, on BOTH timeout and normal-exit paths; accepted residual = the blocked reader thread is abandoned until the pipe closes (bounded ≤2 MiB by `MAX_TOOL_READ_BYTES`).

**W7 REVIEW TARGETS (step 2):** the four `recv_timeout` outcomes are all handled honestly (Ok(Ok)/Ok(Err io)/Timeout/Disconnected) and an io error DEGRADES to partial output instead of `?`-ing out the turn; the drain thread never holds the lock across a blocking `read`; `Termination::Survived` detail is honest and no unbounded `child.wait()` remains; the `Block` refusal string and the `"user denied: {cmd}"` string are byte-identical to before; cap semantics unchanged (still reads past the cap so the child never blocks on a full pipe).

**GATE HISTORY THIS SESSION (all `--release`, 4 steps):** gate1 **FAIL** — `fmt --check exit=1` (expected Sol drift) + 1 test. → orch ran the scoped normalize `cargo fmt -p nh-tools` (exit 0). gate2 **FAIL** — fmt/clippy/deny ALL GREEN, one test still red (the fixture's SECOND date, see below). gate3 **PASS = 497/0/1** after both fixture dates were fixed.

**⇒ THE DATE-BOMB FIXTURE (owner-authorized out-of-scope fold, W6c-style).** Failing test = `tests::why_command_uses_live_resolver_trace` (`crates/nh-tui/src/lib.rs`, assertion `app.transcript … starts_with("route: meter-route")`). Cause: that test deliberately uses the **live** clock (its siblings inject `fixed_at()`), while `METER_CATALOG` hardcoded `valid_until = "2026-07-24"`. When UTC rolled to 2026-07-25 the price went stale and the resolver **correctly** fail-closed — **the product was right, the fixture was a time bomb**. Fix = bump to the in-tree sentinel `2099-01-01` (convention already used in `crates/nh-mcp/src/lib.rs:1188/1206/1223`) + an explanatory comment. **NB: there are TWO dates in that fixture** — the `[fx]` block AND `[routes.meter-route.price]`. Session 7d fixed the `[fx]` one first, gate2 still failed, then fixed the route-price one; gate3 tests that. If gate3 still fails on this test, the remaining cause is NOT a date. **Deliberately NOT swept:** `cmd_chat.rs:660/680`, `cmd_why.rs:94`, `cmd_run.rs:593` still hold dated fixtures — audit them deliberately, because `cmd_chat.rs:1051` (`price_after_valid_until_adds_stale_warning`) **depends** on its fixture expiring (it injects a 2026-08-01 clock); a blind sweep would delete that test's purpose while leaving it green. Logged as a v0.1.0 blocker in `03-execution/RELEASE_CHECKLIST.md`.

**⇒ RELEASE_CHECKLIST additions (2 new pre-release-gate boxes, session 7d):** (1) **SECURITY.md audit statement must be true for the release** — `SECURITY.md:57` says "The audit found no critical problems", committed `7f4add6` 2026-07-20 describing the Fable 5 audit; the Sol-max pre-release audit ran the NEXT day and found **2 critical / 14 high**, "not releasable". Before tagging: verify C-1/C-2 are actually closed by Slice G, then rewrite that sentence to name BOTH audits, their real findings, and the remediation commit. (2) **No test fixture may age out** (the date-bomb rule above).

**⇒ DECISION RECORD BUILT (owner-directed, session 7d).** Owner: "i need all those decisions to write articles too, why we chose to do this over x and what that means for long term and immediate harness." `DECISION_LOG.md` + `ARCHITECTURE_DECISIONS.md` had been stale since 2026-07-12/13, so 5 Fable 5 writers reconstructed every decision M2→today from primary sources. **NEW: `08-decisions-and-risk/decisions/`** = `DECISIONS_M2-M3.md` (14) + `DECISIONS_M4-M5-A-E.md` (13) + `DECISIONS_M5-SLICE-F.md` (15) + `DECISIONS_RELEASE-SLICE-G.md` (21) + `DECISIONS_STANDING.md` (13) = **76 entries**, each with Decision / Alternatives-considered-and-why-rejected / Why / Immediate effect / Long-term consequence / Accepted residual risk (security only) / Evidence / **Article angle** / Review later. **`02-architecture/ARCHITECTURE_DECISIONS.md` amended in place** (originals untouched, dated amendment blocks appended): Decision 8 (Job Object rejected — the R1 story), Decision 4 (delegate class cut from v1), Decision 5 (a "pin SDK + CI conformance" mitigation that never existed), Decision 7 (`preserve_when_thinking` naming drift). **CAVEAT TO VERIFY:** the assembler had Bash/PowerShell denied and **hand-transcribed** `DECISIONS_RELEASE-SLICE-G.md` + `DECISIONS_STANDING.md` from Read output rather than copying mechanically — diff them against `Temp\decisions_D_release_sliceG.md` and `Temp\decisions_E_crosscutting.md` before treating them as a verbatim record. **NOT a leak:** "Opus 5 will land" in `ARCHITECTURE_DECISIONS.md:49` and `RISK_REGISTER.md:8` is ORIGINAL 2026-07-12 forecast text (git-diff verified), not from the Opus-5 sweep.

**⇒ OPUS 5 SWEEP DONE (session 7d).** Owner: "everything that said opus 4.8 should defer now to opus 5". The harness reports the orchestrator's running model as `claude-opus-5`, superseding session 7c's "no Opus 5 exists" note (which was wrong and told future sessions not to change anything). Swept 10 identity/role docs incl. the commit-trailer template (`Co-Authored-By: Claude Opus 5`). **Left as-is on purpose:** BUILD_LOG entries + audit header + past commit trailers (immutable history of what actually ran), `04-research/**` + RESEARCH pricing/benchmark rows citing Opus 4.8 $5/$25 (verified research data, unverified for Opus 5), and the commented `[routes.claude-opus-4-8]` catalog stub (a real API model ID; do NOT write `claude-opus-5` into API-facing config until verified against a live catalog).

**⇒⇒⇒⇒⇒⇒⇒⇒⇒⇒ CHECKPOINT (2026-07-24, session 7c) — SLICE G: W1–W6c DONE + GATED; WAVE 7 (LAST) IS NEXT.**

HEAD = `c9863d1` on `main`; working tree = ONE uncommitted lump (W4 SURFACES + Slice G W1–W6c). **NO commit until W7 passes + owner FEEL**, then ONE coherent commit → Slice H (modularize) → v0.1.0. Full per-wave detail in memory [[slice-g-audit-remediation]]; audit `04-research/AUDIT_2026-07-21_sol-max_pre-release.md`.

**W6c "REDACTION + APPROVAL" (H-11 + H-14 + H-03) DONE + GATED PASS 493/0/1** `--release` (fmt/clippy-D/deny clean; ONE orch `cargo fmt -p nh-tui` normalize), codex `bey3o2vbk` (Sol max), **5 files**: nh-vault (H-14 literals sorted longest-first via `sort_by_key(Reverse(len))` + new canonical `escape_untrusted` no-truncation escaper; `sanitize_untrusted_text` refactored through it; `sanitize_line`/`safe_line` untouched), nh-tools/mcp.rs (H-11a `ARGS_SUMMARY_MAX` 120→**500** + honest `… (+N more chars)`), nh-tui/lib.rs (H-11b `scrub_full_line` → `nh_vault::escape_untrusted`, escapes bidi + strips zero-width, no truncation), nh-tui/worker.rs (H-03 `apply_new_credential` refreshes shared+ctx+receipts scrubbers together at all 3 route-change branches), **+ owner-ratified FOLD nh-cli/cmd_chat.rs** (Sol flagged the same H-03 in `nh chat` `install_client`; orch applied the 1-line `s.agent.ctx.scrubber = registry.clone();` mirror → H-03 closed on BOTH agent surfaces). +6 tests (493 = 488+5). 2 ratified calls (H-14 longest-first-sort; H-11 cap 500). Sol's sound deviation: `sort_by_key(Reverse)` (clippy-clean, identical behavior). Adversarial review ZERO-blocking. NO commit.

**ON `continue` → GROUND WAVE 7 "EXEC BOUNDARY" (H-05 + H-06) — the LAST Slice-G wave.** Pull EXACT H-05/H-06 audit text; re-ground vs CURRENT tree (nh-tools/src/lib.rs `ExecShell` + nh-law; re-grep — audit line #s predate all waves). H-05 = the hard "exec always requires approval" invariant not enforced at the `ExecShell` op boundary (meta-finding #1: enforce controls AT the op boundary). H-06 = process-tree kill (W2 already added a dep-free 300s timeout + whole-tree taskkill — re-verify what H-06 still wants). **STEPS (same cycle as W6a/b/c):** ground seams → bring owner design calls (recommend+why, [[decisions-explain-rec-then-owner-decides]]) → write `Temp/sol_wave7_prompt.txt` → **ASK owner before launching Sol** → post-Sol (codex clear [leave owner's own] → kill nh.exe → mtime scope-check → `Set-Location` repo; `.\gate.ps1 *> Temp\w7-gate.log` [UTF-16; verdict = `GATE: PASS/FAIL` line] → fmt drift → scoped `cargo fmt -p <crates>` → re-gate → adversarial review → memory, NO commit). **After W7: owner FEEL gate → ONE commit (W4 SURFACES + ALL Slice G W1–W7) → Slice H (modularize) → v0.1.0.** LIVE-VERIFY the 4 providers' finish_reason strings (W5) before the final commit. Operational gotchas unchanged (Temp\ = `C:\Users\capv2\AppData\Local\Temp`; PS not repo-rooted → `Set-Location` before gate; gate log UTF-16; Sol never runs fmt; NEVER two nosis codexes; in-file test literals in an authorized file ARE in scope — don't over-restrict the brief's stop instruction; Sol may FLAG a same-class bug in an out-of-scope file — fold via a small orch mirror with owner OK, as W6c did for cmd_chat).

**⇒ "Opus 5" RESOLVED (2026-07-24, session 7d) — SUPERSEDES the earlier NON-EVENT note.** On 2026-07-23 the claude-api catalog reference showed no Opus 5 and the latest Opus was 4.8; on 2026-07-24 the **harness itself reports the orchestrator's running model as `claude-opus-5`** (session environment line). Owner re-directed: "everything that said opus 4.8 should defer now to opus 5". **DONE — identity/role docs only** (AUTONOMOUS_HANDOFF, AGENTS, CLAUDE.md, CODEX, MODEL_ROLES, PROMPT_LIBRARY, RISK_REGISTER, ONE_PAGE_SUMMARY, PRODUCT_BRIEF, this file; commit-trailer template now `Claude Opus 5`). **DELIBERATELY LEFT AS-IS:** (a) BUILD_LOG entries + the audit header + past commit trailers = immutable history of what actually ran; (b) `04-research/**` + RESEARCH_2026-07_harness market rows citing Opus 4.8 **pricing/benchmarks** ($5/$25, Terminal-Bench) = verified 2026-07 research data, unverified for Opus 5 — rewriting them would fabricate numbers; (c) the commented `[routes.claude-opus-4-8]` delegate stub in `catalog.toml` = a real API model ID, and the delegate class is CUT from v1 — do NOT write `claude-opus-5` into any API-facing config until that ID is verified against a live catalog. **Claude Code CLI:** owner asked to update it; it's **winget-managed** (`winget upgrade Anthropic.ClaudeCode`, 2.1.178 → 2.1.219) — a CLI update does NOT and cannot add an Opus 5 (model availability is server-side, not CLI-version-gated).

**⇒⇒⇒⇒⇒⇒⇒⇒⇒ CHECKPOINT (2026-07-23, session 7b) — SLICE G: W1–W5 + W6a + W6b DONE + GATED; WAVE 6c IS NEXT.**

HEAD = `c9863d1` on `main`; working tree = ONE uncommitted lump (W4 SURFACES + Slice G W1–W6b). **NO commit until ALL Slice-G waves pass + owner FEEL**, then ONE coherent commit → Slice H (modularize) → v0.1.0. Full per-wave detail + ratified decisions in memory [[slice-g-audit-remediation]]; audit `04-research/AUDIT_2026-07-21_sol-max_pre-release.md`.

**W6b "MCP LIMITS + LIFECYCLE" (H-12 + M-06) DONE + GATED PASS 488/0/1** `--release` (fmt/clippy-D/deny clean, NO fmt drift — no orch normalize needed), codex `bilkrf1u7` (Sol max, v2 brief), 3 files (nh-core/lib.rs, nh-tools/mcp.rs, nh-mcp/lib.rs — mtime-verified sole fresh files, zero new deps, forbid-unsafe). **H-12** = per-crate `read_body_capped` (Content-Length precheck → `take(MAX+1)` stream → post-read len reject-not-truncate; lossy UTF-8) at nh-core 2 providers (**8 MiB**) + nh-tools get_well_known/rpc_with (**4 MiB**) + OAuth `.json()`→capped `from_str` (**256 KiB**); `ttlMs` clamp **≤24h** + `checked_add` (raw `Instant+from_millis` panic path REMOVED); tools/list truncate-and-warn **≤512**. **M-06** = McpServer `handle: Option<JoinHandle>` + `impl Drop`(signal+join, no double-join with `shutdown(self)` take); caller token **≥32 B** else `bail!` at bind(); fleet_run clamp `max_workers` to config ceiling (`.min(ceiling)`) + global active-run cap **4** (`Runtime.active_runs: Arc<AtomicUsize>` + `ActiveRunGuard` RAII, decrements on ALL 3 exits incl spawn-fail — std drops the moved guard, no double-decrement). +5 tests (488 = 483+5). **v1 brief STOPPED CLEAN (false stop)** on the in-file `test_runtime()` Runtime literal (mis-worded C2 guardrail "stop if outside bind()") → orch confirmed IN scope (same authorized file's #[cfg(test)]; grep = exactly 2 Runtime literals, both in-file) → corrected C2 (authorize both literals) → relaunched v2 = clean full implementation. Adversarial review ZERO-blocking (byte caps reject+lossy; ttl clamp kills panic; Drop take no-double-join; guard no-double-decrement on spawn-fail; token floor; max_workers clamp; active-run invariant ≤4). Brief `Temp/sol_wave6b_prompt.txt` (v2), self-report `Temp/sol_wave6b_out.md`, gate log `Temp/w6b-gate.log`. NO commit.

**ON `continue` → GROUND WAVE 6c "REDACTION + APPROVAL" (H-11 + H-14 + H-03).** Pull EXACT audit text for H-11/H-14/H-03; re-ground each seam vs the CURRENT tree (audit line #s predate ALL waves; W2/W6a/W6b moved code — always re-grep). Known from prior notes: **H-14** = scrubber replacement order can reveal a longer secret's suffix (nh-vault:~119-135 → dedupe + sort literals longest-first, or a multi-pattern matcher; test both insertion orders + overlapping secrets). **H-11** = approval display truncates the dangerous tail (nh-tools/mcp.rs `ARGS_SUMMARY_MAX`=120 + cmd_run sanitizer + nh-tui); ratified Q4 = minimal-honest approval (escape + "+N more chars", defer digest renderer). **H-03** = pull exact audit text (approval/execution display divergence — likely nh-tools). **STEPS (same cycle as W6a/W6b):** ground seams → bring owner design calls (recommend+why per [[decisions-explain-rec-then-owner-decides]]) → write `Temp/sol_wave6c_prompt.txt` → **ASK owner before launching Sol** → post-Sol (`Get-Process codex` clear [leave owner's own] → kill `nh.exe` → mtime scope-check → `Set-Location` repo; `.\gate.ps1 *> Temp\w6c-gate.log` [UTF-16; verdict = `GATE: PASS/FAIL` line] → fmt drift → scoped `cargo fmt -p <crates>` → re-gate → adversarial review → memory, NO commit). **After W6c: W7 exec boundary (H-05+H-06)** → FEEL → ONE commit (W4 + all Slice G) → Slice H (modularize) → v0.1.0. Operational gotchas unchanged (Temp\ = `C:\Users\capv2\AppData\Local\Temp`; foreground PS not repo-rooted → `Set-Location` before gate; gate log UTF-16; Sol never runs fmt; NEVER two nosis codexes; Sol STOPS CLEAN on relocated/out-of-scope seams — re-grep before briefing; NB an in-file test literal in an authorized file IS in scope — don't over-restrict the brief's stop instruction).

**⇒⇒⇒⇒⇒⇒⇒⇒ CHECKPOINT (2026-07-23, session 7) — SLICE G: W1–W5 + W6a DONE + GATED; WAVE 6b IS NEXT.**

HEAD = `c9863d1` on `main`; working tree = ONE uncommitted lump (W4 SURFACES + Slice G W1–W5 + W6a). **NO commit until ALL Slice-G waves pass + owner FEEL**, then ONE coherent commit → Slice H (modularize) → v0.1.0. Full per-wave detail + ratified decisions in memory [[slice-g-audit-remediation]]; audit `04-research/AUDIT_2026-07-21_sol-max_pre-release.md`.

**WAVE 6 was SPLIT finest this session** (owner ratified, for auditable diffs): **W6a MCP egress/SSRF/trust (H-04)✓ → W6b limits+lifecycle (H-12+M-06) NEXT → W6c redaction/approval (H-11+H-14+H-03) → W7 exec boundary (H-05+H-06)** → FEEL → ONE commit. Four decisions ratified: sizing=finest-split; Q2 MCP-trust=add user-global `~/.nosis/mcp.toml` as trust source (repo tighten-only); SSRF depth=link-local+metadata literal IPs only; H-11=minimal-honest approval (defer digest renderer). **NEW HARD PROJECT RULE (owner 2026-07-23):** every options-menu you present MUST carry your recommendation + why — see [[decisions-explain-rec-then-owner-decides]].

**W6a "MCP EGRESS + SSRF + TRUST" (H-04) DONE + GATED PASS 483/0/1** `--release` (fmt/clippy-D/deny clean), codex `b3520tbf8` (Sol max), 6 files: nh-vault (`host_of` + `is_link_local_or_metadata`), nh-tools/mcp.rs (`mcp_tools(configs, send_allowed)` discovery gate BEFORE network + bare-host `Access::Send` in `execute`), nh-cli/cmd_run (`load_and_vet_mcp_configs`+`merge_and_vet`: user-global trust source, repo tighten-only `more_restrictive_mcp_trust`, repo-only link-local drop + auto→ask clamp, then credential-audience drop), cmd_chat + cmd_tui (shared helper + send_allowed closure + `home` via `nh_law::user_home_dir()`), nh-law (`pub fn user_home_dir`). +11 tests. **First clean-stop→Option-A pivot** (v1 added a `source` field → out-of-scope ripple into frozen e3_korvin.rs + nh-tui/lib.rs; Sol stopped clean; owner chose Option A = provenance in nh-cli's separate-file merge, no struct change). Adversarial review ZERO-blocking. ONE orchestrator fmt normalize. NO commit.

**ON `continue` → GROUND WAVE 6b "MCP LIMITS + LIFECYCLE" (H-12 + M-06).** H-12 seams (current tree): unbounded `.text()` at nh-core/src/lib.rs:264 (OpenAiCompat) + :554 (Anthropic), nh-tools/src/mcp.rs:322 (get_well_known) + :382 (rpc_with); OAuth :517 `.json()` also unbounded; `ttlMs`→Instant overflow at nh-tools/mcp.rs:267 (no checked_add; OAuth expires_at:523 already safe from W2). M-06 seams: nh-mcp McpServer no `Drop` (only `shutdown(self)`; accept-loop error at :142 only warns); caller `ServeConfig.token` no min-entropy; `fleet_run` (nh-mcp:862) any max_workers + no global active-run cap. **Orchestrator's recommended caps to confirm with owner (recommend+why each):** provider body ≤8 MiB / MCP ≤4 MiB / OAuth ≤256 KiB via `Response::take(MAX+1)`+Content-Length precheck (per-crate helper, no new dep); ttlMs clamp ≤24h; tools/list ≤512 tools; McpServer `Drop`(signal+join); caller token ≥32 chars else reject; fleet_run clamp max_workers to config ceiling + global active-run cap 4. **STEPS (same cycle as W6a):** pull H-12/M-06 exact audit text; re-ground seams vs CURRENT tree (line #s above are current but RE-VERIFY); bring owner the cap confirmations; write `Temp/sol_wave6b_prompt.txt`; **ASK owner before launching Sol**; post-Sol (Get-Process codex clear → kill nh.exe → mtime scope-check → `.\gate.ps1 *> Temp\w6b-gate.log` UTF-16, verdict = `GATE: PASS/FAIL` line → fmt drift → scoped `cargo fmt -p <crates>` → re-gate → adversarial review → memory, NO commit). **Operational gotchas unchanged** (Temp\ = AppData\Local\Temp; PS not repo-rooted → `Set-Location` before gate; gate log UTF-16; Sol never runs fmt; NEVER two nosis codexes; Sol STOPS CLEAN on relocated/out-of-scope seams — re-grep before briefing, enumerate ripples).

**⇒⇒⇒⇒⇒⇒⇒ CHECKPOINT (2026-07-23, session 6) — SLICE G: W1–W5 DONE + GATED; WAVE 6 IS NEXT (to ground).**

HEAD = `c9863d1` on `main`; working tree = ONE uncommitted lump (W4 SURFACES + Slice G Waves 1–5). **NO commit until all 7 Slice-G waves pass + owner FEEL**, then ONE coherent commit → Slice H (modularize, deferred) → v0.1.0. Full per-wave detail + ratified Q1/Q2/Q3 security decisions live in memory [[slice-g-audit-remediation]]; audit = `04-research/AUDIT_2026-07-21_sol-max_pre-release.md`. Exec model: orchestrator (Opus 5) plans/gates/reviews; **Sol max** (`gpt-5.6-sol`, `model_reasoning_effort=max`) writes code, ONE wave at a time; NEVER two *nosis* codexes (the owner runs his own codex/TUI sessions as separate PIDs — e.g. `47112` was Carlos's, leave it).

**Slice G status: W1✓ W2✓ W3✓ W4✓ W5✓ (5 of 7).** All land uncommitted in the lump. The two newest:
- **W4 "mandatory receipts + fleet-worker cleanup" DONE + GATED 461/0/1** (codex `b5p3yarqa`, 3 files): H-07 RAII `WorkerPool` guard (drop job_tx before joining on EVERY exit path + a drop-order fix so a worker stuck on an event/ack send can't deadlock the join); H-08 `FailureClass::Unreceipted` (a failed required receipt downgrades Pass→Fail but KEEPS the answer); M-05 (`append_run_failed`→Result composed into the returned error; `run_with_id` refuses a non-empty ledger → "use resume").
- **W5 "fail-closed meter + finish_reason" DONE + GATED 472/0/1** (codex `b9m92avi7`, Sol max, 9 files, +11 tests; gate log `Temp/w5g-gate2.log`): H-09 `cost_of`→`Option<f64>` (rejects cached>prompt / non-finite; `money()` renders "unpriced" not 0.00; atomic checked usage accumulation → `MeterIncomplete` on overflow); H-10 `classify_finish_reason` (normal/absent→Pass, length/max_tokens→Partial, content_filter→Fail+`FailureClass::Filtered`, unknown→Partial+"unrecognized finish reason" emit; answer ALWAYS returned); M-02 `to_usd_approx` requires present-AND-unexpired FX (native billed cost untouched). Sol STOPPED CLEAN twice on prior-wave-relocated code → owner-approved adding `cmd_why.rs` + `nh-tui/src/worker.rs`. ONE orchestrator clippy fix (`!is_some_and` → `is_none_or`, semantics-preserving). **LIVE-VERIFY before the final commit:** the exact `finish_reason` strings the 4 live providers emit on a normal completion (classifier Normal set = {"",stop,end_turn,stop_sequence}, easy to extend).

**ON `continue` → GROUND WAVE 6 "MCP EGRESS POLICY + LIMITS" (H-04, H-12, M-06; fold in H-11 approval-display-truncation + H-14 scrubber-longest-first where they fit).** Crates likely nh-tools/src/mcp.rs (+ maybe nh-mcp, nh-core, nh-vault). Findings (pull EXACT text from the audit; audit line #s PREDATE W1–5 and **W2 refactored nh-tools/nh-mcp so code has MOVED** → always re-grep the current tree): **H-12** = unbounded `.text()` buffering of provider/MCP/OAuth responses + a remote `ttlMs`→`Instant` overflow/panic (audit cited nh-core:250/539, nh-tools/mcp.rs:322/503/260); **H-04+M-06** = MCP egress policy/limits; **H-14** = scrubber replacement order can reveal a longer secret's suffix (nh-vault:119-135 → dedupe+sort literals longest-first, or a multi-pattern matcher); **H-11** = approval display truncates the dangerous tail (nh-tools/mcp.rs 120-char cap, cmd_run 500-char sanitizer, nh-tui bidi-escape). **STEPS (identical cycle to W3/W4/W5):** (a) pull H-04/H-12/M-06/H-11/H-14 exact text from the audit; (b) re-ground each seam file:line vs the CURRENT tree (re-grep — do NOT trust audit line #s); (c) bring the owner design calls (recommendation + why on each, [[decisions-explain-rec-then-owner-decides]]); (d) write `Temp/sol_wave6_prompt.txt`; (e) **ASK the owner before launching Sol**; (f) post-Sol cycle: `Get-Process codex` clear (LEAVE the owner's own codex PIDs) → kill any `nh.exe` → mtime scope-check (only the authorized files fresh) → `Set-Location "C:\Users\capv2\Desktop\nosis-Harness"; .\gate.ps1 *> Temp\w6g-gate.log` (UTF-16; the real verdict is the `GATE: PASS/FAIL` line, NOT the wrapper exit; if fmt drift → orch runs the normalizing scoped `cargo fmt -p <touched crates>` under 1.96.0, then re-gate) → adversarial review → update memory + report (NO commit). **After W6 only W7 remains** (exec approval-at-boundary + process-tree kill, H-05/H-06), then FEEL → ONE commit (W4 + all Slice G) → Slice H (modularize) → v0.1.0.

**Operational gotchas (verified across W3–W5):** `Temp\` = `C:\Users\capv2\AppData\Local\Temp\` (NOT a repo dir); prior briefs/reports are `Temp\sol_wave{3,4,5}_prompt.txt` + `_out.md`. Foreground PowerShell is NOT rooted in the repo → cargo needs `--manifest-path C:\Users\capv2\Desktop\nosis-Harness\Cargo.toml`, or `Set-Location` first. Gate log is UTF-16 (read via PowerShell `Select-String` or the Read tool, not Bash grep). Sol launch (single nosis codex, background, prompt via STDIN — PS 5.1 mangles a big multiline arg): `Get-Content -Raw "C:\Users\capv2\AppData\Local\Temp\sol_wave6_prompt.txt" | codex exec -m gpt-5.6-sol -c 'model_reasoning_effort=max' -s workspace-write --color never -C "C:\Users\capv2\Desktop\nosis-Harness" -o "C:\Users\capv2\AppData\Local\Temp\sol_wave6_out.md" *> "C:\Users\capv2\AppData\Local\Temp\sol_wave6_run.log"`. Sol never runs `cargo fmt` (formatting is the gate's job — orch normalizes drift post-Sol). Recurring pattern to expect: Sol STOPS CLEAN if the brief lists a stale/relocated seam or an unauthorized file — re-grep the current tree BEFORE briefing and enumerate every ripple (W5 needed 2 amendments because W2's refactors had moved code).

**⇒⇒⇒⇒⇒⇒ CHECKPOINT (2026-07-22, session 5b) — SLICE G WAVE 3 DONE + GATED; H-02 RATIFIED; WAVE 4 WAS NEXT.**

HEAD = `c9863d1` on `main`; working tree = ONE uncommitted lump (W4 SURFACES + Slice G Waves 1–3). **NO commit until all 7 Slice-G waves pass + owner FEEL**, then ONE coherent commit → Slice H (modularize, deferred) → v0.1.0. Full per-wave detail + ratified Q1/Q2/Q3 security decisions + operational gotchas live in memory [[slice-g-audit-remediation]]; audit is `04-research/AUDIT_2026-07-21_sol-max_pre-release.md`.

**Done since the session-5 checkpoint below:** (1) **H-02 shutdown surface RATIFIED "Accept"** by owner (Wave 2's 250ms bounded-join + detach-on-deadline for an uninterruptible in-flight provider HTTP call). (2) **Wave 3 "FS TRUST BOUNDARIES" DONE + GATED PASS 456/0/1 `--release`** (fmt/clippy-D/deny clean), codex `b7nxo8ged` (Sol max). Fixed **C-02** (constitution symlink exfil → nh-law `read_guarded_text`: symlink-reject + canonical containment + **64 KiB skip-and-warn**), **H-08 fs-part** (nh-core `ReceiptWriter::append`: no-follow path guard + `File::lock` + `sync_all`), **H-13** (nh-cli `cmd_init`: **trust-git-query** + git-registered-worktree check that defeats a forged `gitdir:` + custom-`core.hooksPath` refuse + no-follow install), **M-04** (nh-tools `EditFile`: single-handle capped read, over-cap refuse before mutation, atomic temp→fsync→drop→rename). 4 files only (nh-law/nh-core/nh-cli/nh-tools), zero new deps, mtime-verified scope, adversarial review **ZERO-blocking**. **Orchestrator applied ONE mechanical fix** (gated green): added `.read(true)` to `ReceiptWriter::append`'s `OpenOptions` — Windows `File::lock()`/`LockFileEx` needs read/write DATA access, so a pure `.append(true)` handle failed `ACCESS_DENIED (os error 5)` in Sol's own concurrent-append test; append semantics preserved. 2 owner design calls delivered: C-02 cap = 64 KiB skip-and-warn; H-13 = trust git query.

**ON `continue` → WAVE 4 "mandatory receipts + fleet-worker cleanup"** (Sol's BLOCKER order): **H-07** (fleet coordinator `?`-returns drop live worker `JoinHandle`s → detached workers keep doing provider/tool/receipt work after the run lock is released; fix = an RAII worker-pool guard that closes work channels + signals cancellation + joins/drains EVERY worker on EVERY exit path before releasing the lock; nh-fleet ~892–1153), **H-08 mandatory part** (make a failed REQUIRED receipt an explicit unreceipted/FAILED outcome per Q3 — the piece deliberately left out of Wave 3; nh-core `append_receipt` ~1811 currently swallows the append error, + parallel fleet receipt writes ~1413), **M-05** (fleet `RunFailed` append result discarded ~861 + call sites return only the initiating failure ~402/550; `run_with_id` repairs then reuses a NON-EMPTY ledger as a new run ~385/442 — fix = compose bookkeeping failure into the returned error + reject a non-empty run-id from the new-run path, require the explicit resume path). Reopens **nh-fleet** (already unfrozen under A-M5-8 since W5) + **nh-core**. **STEPS:** (a) ground each seam file:line vs the CURRENT tree — audit line numbers predate Waves 1–3, so re-grep; (b) **design call to bring the owner** (recommend + why, [[decisions-explain-rec-then-owner-decides]]): does a failed required receipt fail the WHOLE run, or yield a distinct 'unreceipted' outcome? + confirm the RAII-guard shape; (c) write the seam-by-seam brief `Temp/sol_wave4_prompt.txt`; (d) **ASK the owner before launching Sol**; (e) post-Sol: `Get-Process codex` clear → kill any `nh.exe` → mtime scope-check → gate → adversarial review → update memory + report (NO commit).

**Operational gotchas (verified this session):** the foreground PowerShell shell is NOT rooted in the repo → cargo needs `--manifest-path C:\Users\capv2\Desktop\nosis-Harness\Cargo.toml`, and run the gate as `Set-Location "C:\Users\capv2\Desktop\nosis-Harness"; & ".\gate.ps1" *> Temp\wN-gate.log`. The gate log is **UTF-16** (Bash `grep` can't read it — use PowerShell `Select-String` or the Read tool); the gate's real verdict is its `GATE: PASS/FAIL` line, not the wrapper exit. Sol launch = `Get-Content -Raw Temp\brief.txt | codex exec -m gpt-5.6-sol -c 'model_reasoning_effort=max' -s workspace-write --color never -C "C:\Users\capv2\Desktop\nosis-Harness" -o Temp\out.md *> Temp\run.log` (prompt via STDIN; PS 5.1 mangles a big multiline arg). NEVER two nosis codexes (`Get-Process codex` first). Sol never runs `cargo fmt` (orchestrator runs the normalizing scoped fmt post-Sol).

**⇒⇒⇒⇒⇒ CHECKPOINT (2026-07-22, session 5) — M5 SLICE G: AUDIT REMEDIATION, WAVE 2 IN FLIGHT (owner `/clear`'d mid-run).**

A pre-release **Sol-max audit** (read-only `codex exec`) of the whole workspace at the W4 FEEL hold produced `04-research/AUDIT_2026-07-21_sol-max_pre-release.md` (Claude gate-note on top; 5 load-bearing findings re-verified vs real code = all CONFIRMED). Result **2 CRIT / 14 HIGH / 10 MED / 1 LOW / 1 NIT; verdict NOT releasable as v0.1.0.** Owner chose **FULL REMEDIATION FIRST** → **Slice G**: 7 gated BLOCKER waves in the audit's prioritized order; **NO commit until all 7 pass + owner FEEL**, then ONE coherent commit (W4 SURFACES + all of Slice G — they are one uncommitted lump); then **Slice H** (cosmetic god-module splits, deferred) → v0.1.0. Memory: [[slice-g-audit-remediation]].

**Ratified security decisions (apply across ALL waves):** **Q1** credential attaches only on `https` + exact origin (scheme+host+port); plain `http` only for literal loopback (`localhost`/`127.0.0.0/8`/`::1`, decided WITHOUT DNS). **Q2** repo `.nosis/*.toml` (catalog/mcp/notify) is RESTRICT-ONLY — may tighten, never introduce a credential audience / MCP auto-trust / notify destination; new trust only from user-global `~/.nosis`. **Q3** fail-closed everywhere (invalid/overflow usage → UNAVAILABLE; non-finite cost → "unpriced" not 0.00; non-normal/unknown `finish_reason` → Partial/Refused not Pass; failed required receipt → run FAILED).

**Wave 1 (credential centralization C-01 + M-10) DONE + GATED PASS 439/0/1** `--release`, clippy-D + deny + fmt clean (codex `b4lc0pu25`). New `nh-core::credential::connect` = the ONE credentialed-client boundary (non-forgeable `ResolvedRoute` + `get_scoped` at materialization + min_cap); `nh-vault::exact_origin` (scheme+host+port, loopback-http no-DNS, typed `AudienceRefused{Unapproved|InsecureTransport}`); `nh-routes::validate_route_url`; `ResolvedRoute` fields private + accessors + compile-fail doctest. **4 surfaces RATIFIED by owner:** (1) credential module home = nh-core (adds nh-mcp→nh-core edge); (2) host-only law audiences now mean `https://host:443`, non-default ports need explicit origin; (3) refusals show effective ports + audience-refusal before missing-key; (4) catalogs with empty/remote-http base_url now fail resolution (shipped catalog all-https = safe).

**⇒ WAVE 2 DONE + GATED PASS 446/0/1** `--release` (clippy-D + deny + fmt clean), codex **`bg5kehifp`** (Sol max). New modules `crates/nh-tui/src/terminal.rs` + `worker.rs`; Claude independently reviewed = correct. Fixed **nh-tui lifecycle**: **H-01** terminal restore = independent best-effort (a failing step no longer skips the rest; undo only what setup enabled — NO blind mouse/kitty/CPR disables) + failure-injection tests; **H-02** bounded deadlock-free worker shutdown (drop approval sender first → Stop → BOUNDED join, detach-on-timeout not hang; cancellation-aware approval waits; never unconditional `join()` in Drop); **L-01** panic-hook restored under `catch_unwind`; + targeted `terminal`/`worker` module extraction from the ~5K-line lib.rs. Brief = `Temp/sol_wave2_prompt.txt`; self-report → `Temp/sol_wave2_out.md`; log → `Temp/sol_wave2_run.log`.

**ON `continue`:** Wave 2 DONE + gated (446/0/1). **H-02 surface AWAITING owner ratification:** 250ms shutdown timeout + detach-on-deadline for an uninterruptible in-flight provider HTTP call (left to nh-core's request timeout) — orchestrator recommends ACCEPT (normal/idle/parked-approval shutdown finishes <10ms; detach only when a synchronous provider call is mid-flight; alternative = the H-02 hang). Once ratified → launch **Wave 3**. **Waves 3-7 remain** (Sol's BLOCKER order): 3 = fs trust boundaries (C-02, H-08 symlink/lock, H-13, M-04) / 4 = mandatory receipts + fleet-worker cleanup (H-07, H-08, M-05) / 5 = fail-closed metering + finish_reason (H-09, H-10, M-02) / 6 = MCP egress policy + limits (H-04, H-12, M-06) / 7 = exec approval-at-boundary + process-tree kill (H-05, H-06); plus redaction/approval-display (H-03, H-11, H-14) folded where they fit. No commit until all 7 + FEEL.

**Process rules:** Sol implements via `Get-Content -Raw brief.txt | codex exec -m gpt-5.6-sol -c 'model_reasoning_effort=max' -s workspace-write --color never -C <repo> -o out.md *> run.log` (prompt via **STDIN** — PS 5.1 mangles a big multiline arg). Claude gates. **NEVER two codexes at once** (`Get-Process codex` before launching). Sol never runs `cargo fmt`. Tree at checkpoint: **21 files changed/new, uncommitted, on `main` @ `c9863d1`** (W4 + Slice G Wave 1 + in-flight Wave 2). See [[decisions-explain-rec-then-owner-decides]] (bring surfaces, recommend, owner decides).

**⇒⇒⇒⇒ CHECKPOINT (2026-07-21, Release-Slice session 4) — W4 IMPLEMENTED + GATED + REVIEWED, HELD AT THE FEEL GATE (owner restarting PC for an update, will type `continue`).** HEAD = `c9863d1` (CONTRACTS W4 pre-auth). **W4 "SURFACES" (the LAST Slice-F wave) is fully implemented by Sol xhigh and lives in the WORKING TREE, UNCOMMITTED** (7 source files, +817/−147: `crates/nh-tui/src/lib.rs`, `crates/nh-cli/src/{cmd_chat,cmd_init,cmd_run,main}.rs`, `crates/nh-mcp/src/lib.rs`, `crates/nh-vault/src/lib.rs`). **DO NOT re-brief or re-launch Sol for W4** — verify with `git status` that the 7 files are modified; if present, W4 is DONE-pending-FEEL (only if the tree is empty were the changes lost → recover from `Temp/w4-last-message.txt` + re-run the brief `Temp/slice-f-w4-brief-v1.txt`).
- **Result:** 19/20 items DONE; **W4-20 (nit-16) DROPPED** — correct drop-if-hard (end-to-end zeroize needs the W2-frozen nh-mcp `ServeConfig.token` boundary). **Gate PASS** (`gate.ps1`, log `Temp/w4-gate.log` [UTF-16]): fmt/clippy/deny/test all exit 0; **test --release 432/0/1** (up from 416, +16 tests). Orchestrator ran the normalizing scoped `cargo fmt -p nh-tui -p nh-cli -p nh-vault -p nh-mcp` post-Sol (Sol ran no fmt); fmt --check clean. **Adversarial review: ZERO blocking** — verified `from_vault` returns entry NAMES not values; `Scrubber::add_literals` union keeps switched-away keys (medium-20); meter markers never fabricate numbers (medium-18 "not reported" / medium-19 peak-boundary via `price_at().peak` / low-27 "(incomplete — N unpriced turns)" / medium-14 MeterIncomplete "? incomplete"); `safe_text` per-line control-escape (medium-21) + route/error `safe_line` (low-26); honest Esc legend `[y] yes [a] always [n]/[Esc] no` (nit-12); approval modifier-gate `difference(SHIFT).is_empty()` (medium-12); scroll-while-Working (low-24); `working_since.get_or_insert` survives approvals (nit-13); panic-abort flag exits `ui_loop` cleanly (low-23); worker Drop always joins (low-22); word-wrap counter cross-checked vs a real ratatui TestBackend render (medium-13); **nit-11 render de-scrub is SAFE** (both insertion sites `push_line`/`push_approval_line` scrub at insertion — confirmed). **`nh.exe` REBUILT with W4** at `target/release/nh.exe`.
- **HELD at the FEEL gate:** owner chose "I'll drive the TUI first." I gave a **19-step TUI/CLI test script** (launch `.\target\release\nh.exe tui --model deepseek-v4-flash`; default autonomy is **ask** so tool tasks trigger approvals; steps cover nit-12 legend, medium-12 Ctrl+A/Y/N no-op, nit-13 heartbeat, low-24 scroll+input-block, medium-13 no-clip, /why, /profile, /model switch, /timeline, clean /quit; bonus CLI `nh chat --model glm-4.7-flash` → "(incomplete — 1 unpriced turn)" [low-27] + `nh run "hi" --max-turns 0` reject [nit-15]).
- **ON `continue`:** (1) re-verify HEAD `c9863d1` + the 7 W4 files still modified (`git status`). (2) Ask the owner the FEEL result. (3) **If APPROVED:** commit W4 feat — `git add` the **7 source files ONLY** → commit (feat msg ending with the `Co-Authored-By: Claude` trailer + a body line "Implemented by GPT-5.6 Sol xhigh; gated + adversarially reviewed by the orchestrator (Opus 5)"); then **docs-close**: mark CONTRACTS §0.1-F W4 **SHIPPED** (+ the nit-16 drop), add W4's user-visible lines to `CHANGELOG.md [Unreleased]`, add a BUILD_LOG entry, refresh this file + memory. That **closes M5 Slice F HARDEN (W1✓W3✓W2✓W5✓W4✓) and the Release-Slice engineering tail.** (4) **If CHANGES:** route the fix (orchestrator tweak or a Sol refinement handoff, like Slice C/D). Gate already PASSED → re-running `gate.ps1` on continue is optional confirmation, not required.
- **THEN → release-prep** (per `03-execution/RELEASE_CHECKLIST.md`): roll `CHANGELOG [Unreleased] → [0.1.0] - 2026-07-2X`, keep version `0.1.0`, final `gate.ps1` green on the release commit + secret-pattern scan of the release diff + release-diff security review, confirm MCP stays loopback-preview, then `git tag v0.1.0` + `cargo build --release` + smoke (`nh --version`, `nh why`). **Timeline told to owner:** once W4 FEEL-approved, ONE release-prep session → a **source-install v0.1.0 tag** (build-from-source; packaged installers = later, optional M7). Only hard external gate = **MCP public NOT before 2026-07-28** — does NOT block a CLI/TUI release as long as `nh mcp serve` stays the loopback preview + no docs promote it.

**⇒⇒⇒ CHECKPOINT (2026-07-20, Release-Slice session 3).** HEAD `dc227f2` on main, tree clean. **LIVE PROVIDER TESTS DONE** (owner picked live-first this session) — all four passed against the shipped MCP-tools `nh.exe`, total real spend ≈ **$0.0014** (Kimi $0.0009 top, ~2200× under the $2/provider cap): GLM `glm-4.7-flash` **$0.00** free; DeepSeek `deepseek-v4-flash` **¥0.0025 (≈$0.0003)**; Kimi `kimi-k2.6` **$0.0009**; MiMo `mimo-v2.5` **$0.0002**. Verified LIVE: cross-currency REFUSED in `nh why` (¥-vs-$ "not directly comparable"), `usd_approx` only on fresh fx, DeepSeek 2× **peak** applied honestly (11:36 Beijing ∈ 09:00–12:00), no fabricated savings, and **both VERIFY-LIVE §7 wire shapes CONFIRMED** (DeepSeek `thinking:{type:disabled}` via `--think none`; Kimi K2.6 toggle) — clearing the last two `[VERIFY-LIVE]` guesses carried since Slice A. Typed receipts appended to gitignored `.nosis/receipts.jsonl`. Recorded in BUILD_LOG (docs-only, no code commit). **SOLE REMAINING Release-Slice item = W4** (nh-tui+nh-cli surfaces, FEEL — the last Sol code wave: W4-held audit low-16 + medium-20 + any tui/cli findings) + fold in the cosmetic `print_banner` fix (banner still hints only the OLD 3 MCP tools; tools/list authoritative returns 6). ON `continue`: ground the nh-tui/nh-cli seams → enumerate W4 audit items → write the seam-by-seam brief → bring FEEL design calls for owner ratification → **ask before launching Sol** → post-Sol cycle → owner FEEL-approve → commit. MCP hardened but NOT public before 2026-07-28.

**⇒⇒ CHECKPOINT (2026-07-20, Release-Slice session 2).** HEAD advanced `7f4add6` → `7c2b2c4` (MCP feat) → `b708b8c` (docs C+D); docs-close (BUILD_LOG + this file) committing next. **MCP metered-service expansion SHIPPED** (`7c2b2c4`): new loopback tools `why`/`route_cost`/`receipts` + `structuredContent` on all + enhanced `route_resolve`/`fleet_status`; nh-mcp ONLY (`src/lib.rs` +826/−19, `tests/e3_korvin.rs` +3/−0 = the owner-authorized tool-set widening), no Cargo.toml, no new deps. Sol (`gpt-5.6-sol`, **xhigh** this run — owner switched from `max` after the Fable window closed) STOPPED CLEAN on the first run (frozen `e3_korvin.rs` asserted the exact old 3-tool set) and reverted; owner authorized widening ONLY that assertion (v2 brief `Temp/release-mcp-brief-v2.txt`, self-report `Temp/mcp-last-message-v2.txt`). Gate **416/0/1** `--release`, clippy -D + fmt clean (orch ran the normalizing `cargo fmt -p nh-mcp`). Adversarial review ZERO-blocking (every checkpoint invariant verified vs code + a dedicated test: structuredContent always `scrub_json`'d; `receipts` read-only + never mutates + redacts in both surfaces + missing→count 0; `fleet_status` finished line byte-identical; loopback + banner unchanged; `why` == `resolve_capable`; `usd_approx` omitted when fx stale). **LIVE-VERIFIED over 127.0.0.1** (owner "don't assume"): `nh.exe` rebuilt with the tools; `tools/list`=6; `why`→mimo USD honest one-liner + structuredContent (NO fabricated savings, cross-currency REFUSED "¥ vs $ not directly comparable"); `route_cost`→deepseek CNY quote + `usd_approx` via fresh fx; `receipts`→10 real receipts (typed via `nh_fleet::LedgerEvent::TaskReceipt`), bounded scrubbed text; no-bearer→**HTTP 401**. **Docs C+D SHIPPED** (`b708b8c`): CHANGELOG + README quickstart + PRIVACY + CONTRIBUTING + real 1.0 RELEASE_CHECKLIST — drafted by a **Fable 5 ultracode docs workflow** (5 writers, each independently accuracy-verified vs the real CLI), orchestrator spot-checked (gate.ps1 / rust-toolchain.toml / catalog.toml). **Open finding:** `print_banner` still hints only the OLD 3 tools (`route_resolve/fleet_run/fleet_status`) — cosmetic (tools/list is authoritative + returns all 6); fold a 1-line fix into Section B or W4. **Section B SHIPPED** (`cccb2dc`): forbid-unsafe via `[workspace.lints.rust] unsafe_code = "forbid"` (+ `[lints] workspace = true` in all 9 crates; zero in-tree unsafe), `license = "MIT"` workspace-wide, cargo-deny 0.20.2 GREEN + wired into gate.ps1 as a 4th step (advisories/bans/licenses/sources ok; **nothing suppressed** — `ignore` empty; only added `CDLA-Permissive-2.0` for webpki-roots), 27 path deps versioned (`wildcards=deny`; lock unchanged), keyless CI (`.github/workflows/ci.yml`, win+ubuntu, pinned 1.96.0 + separate deny job). Gate now 4 steps → **416/0/1**. Deferred to backlog: nextest, AV canary, frozen-surface sensor. **STILL NEXT (this slice):** (a) live provider tests (<$2/provider HARD CAP; GLM free + deepseek/kimi/mimo real keys — capture honest cost/savings/receipt); (b) **W4** (nh-tui+nh-cli surfaces, FEEL — the last Sol code wave). Plus the cosmetic `print_banner` 3-tool-hint fix, foldable into W4. MCP hardened but **NOT public before the MCP final spec 2026-07-28**. Full detail: BUILD_LOG 2026-07-20 top entry + [[build-loop-resume]].

**⇒ CHECKPOINT (2026-07-20, Release-Slice session 1).** HEAD = `7f4add6` on main; tree clean at checkpoint.
Committed this session: `441727b` (W5 feat) + `30f6760` (W5 docs) + `7f4add6` (LICENSE MIT © nosistech LLC +
SECURITY.md ASD-STE100). **IN FLIGHT: Sol max (`gpt-5.6-sol`, `model_reasoning_effort=max`, background id
`bse8z4d0w`)** implementing the **MCP metered-service expansion** per `Temp/release-mcp-brief-v1.txt`
(self-report → `Temp/mcp-last-message.txt`, log → `Temp/mcp-run.log`). New tools `why` (flagship, mirrors
`nh why`) + `route_cost` + `receipts`, `structuredContent` on all, enhances route_resolve/fleet_status;
nh-mcp ONLY, no new deps, loopback + preview banner unchanged. Owner ratified: MCP = FULL expansion ("do it
well, test + verify, don't assume, best-for-the-project as of 2026-07-20"); security contact
`info@nosistech.com`; 5-business-day SLA. `nh.exe` built at `target/release/nh.exe` (pre-MCP; fine for
provider live tests). **ON `continue`:** (1) if `bse8z4d0w` done → kill nh.exe → read self-report →
`git diff --stat` (scope MUST be nh-mcp only) → authoritative `gate.ps1` (fmt drift → normalizing `cargo fmt`
under 1.96.0 → re-gate; read the `GATE: PASS/FAIL` line, NOT the wrapper exit) → adversarial review
(structuredContent ALWAYS `scrub_json`'d; `receipts` read-only + never mutates the file + redacts secrets in
task text; fleet_status finished-line byte-identical; loopback + banner unchanged; `why` semantics ==
`resolve_capable`; usd_approx omitted when Fx stale) → owner approve → commit MCP feat + docs-close. (2)
**Live-VERIFY the MCP** (owner: "don't assume"): start `nh mcp`, call why/route_cost/receipts over 127.0.0.1
with the minted bearer, confirm real structuredContent + scrubbing. (3) **Live provider tests** (orchestrator
runs; **<$2/provider HARD CAP**, tiny prompts): GLM free `glm-4.7-flash` → `$0.00`; `deepseek-v4-flash`
(verify VERIFY-LIVE `thinking:{type:disabled}`); `kimi-k2.6` (verify K2.6 toggle); `mimo-v2.5` — capture the
honest cost/savings/receipt as launch evidence (keys in OS vault, env fallback `NH_<ENTRY>_KEY`; use
`nh run "<prompt>" --model <route>`, confirm flags via `nh run --help`). (4) **Section B** (SEPARATE Sol wave,
after the MCP wave commits — only ONE nosis codex at a time): `#![forbid(unsafe_code)]` + `[workspace.lints]`,
`cargo install cargo-deny --locked` + wire `cargo deny check` into gate.ps1, minimal keyless CI (.github
windows+ubuntu mock suite), rest of Slice E (nextest, AV canary, gate frozen-surface sensor), `license="MIT"`
in workspace Cargo.toml. (5) **Section C** (Fable 5 docs): CHANGELOG (Keep-a-Changelog) + fill
RELEASE_CHECKLIST into a real 1.0 gate + versioning/tags + install path. (6) **Section D** (Fable 5 docs):
README quickstart (`nh key add`/`nh run`/`nh why`) + privacy statement (prompts go to the selected provider) +
telemetry stance (nosis does not phone home) + CONTRIBUTING. (7) **W4** (nh-tui + nh-cli surfaces, FEEL-gated)
— the last M5 audit wave, folded in. **Exec model:** orchestrator (Opus 5) plans/gates/reviews/live-tests/
commits; **Fable 5 high** = docs (∥ background); **Sol max** = code (ONE wave at a time, ∥ background); NEVER
two nosis codexes (PIDs 19820 + 50340 are Carlos's own TUIs — leave them). **Refresh THIS memory
[[build-loop-resume]] + MEMORY.md early next session** (deferred here to conserve context).

**⇒⇒ W1 DONE + committed `6cefd56` (2026-07-19).** Sol (codex exec, xhigh) implemented all 13 items
W1-1..W1-13 (14 audit findings: 1 high / 4 med / 7 low / 2 nit); orchestrator gated + adversarially reviewed
+ committed to main. **Gate: 363/0/1 `--release`, clippy `-D` clean, `cargo fmt --all --check` clean.**
8 files: nh-vault+nh-law src, nh-cli cmd_run/cmd_chat, root Cargo.toml + nh-vault/Cargo.toml, Cargo.lock
(only the nh-vault→url edge), + 1 owner-approved scope amendment (`nh-cli/tests/m2_exit.rs` — isolated temp
user-law declares the synthetic entry's audience; W1-6 fail-closed required it; security-neutral, also fixed
a real-home leak). Frozen signatures byte-stable (`Scrubber::new(Vec<String>)`, `send_verdict(&self,&str)`);
**nh-fleet untouched → NO A-M5-8** (reserved for W5). `url` = zero build weight (transitive via reqwest
2.5.8). Key fixes: url-crate host parity closes the `\@` exfil differential + `pub normalized_host`;
fail-closed audience; typed `AudienceRefused` + downcast; `\b`-anchored key shapes; bidi escape both
sanitizers; `Zeroizing` literals; keyring error surfaced; ITERATIVE glob/segment matchers (O(1) stack, no
60k-segment/200k-char DoS, semantics byte-identical); send_verdict + exec normalize. **Sol correctly STOPPED
CLEAN once** on a real scope conflict (M2 exit test's undeclared synthetic entry → W1-6 rightly refused);
the amendment resolved it. Full detail in BUILD_LOG 2026-07-19; brief archived
`Temp/slice-f-w1-brief-v1.txt`, amendment prompt `Temp/w1-amend-prompt.txt`.

**⇒⇒ W3 DONE + committed `73d278b` (2026-07-19).** Sol (codex exec, xhigh) implemented W3-1..W3-14 across
nh-core (turn loop + wire math) + nh-routes (honest routing/cost); orchestrator gated + adversarially reviewed
+ committed to main. **Gate: 377/0/1 `--release`, clippy `-D` clean, `cargo fmt --all --check` clean** (orch
ran the normalizing `cargo fmt` post-Sol to clear 3 hand-reflow drifts). 7 files: nh-core/lib.rs,
nh-routes/lib.rs+profiles.rs, nh-cli/cmd_run+cmd_chat+cmd_profile, nh-tui/lib.rs. Key fixes: compaction
cost-guard DROPPED (high-1 — now fires on a normal uniform history, counts `max(provider,estimate)`);
cross-currency normalized only via FRESH fx, stale = REFUSE not raw ¥-vs-$ (high-2, trace prints native);
`resolve_effort` wire-aware (A-M5-9, AnthropicMessages→None honest provider-default); receipt-append non-fatal
(paid answer + real provider error both survive); `cache_hit_pct` honest None on inconsistent usage; both HTTP
clients propagate body-read errors; anthropic `tool_use` requires nonempty id+name; +`push_user_block`/`min_cap`
reuse. **Owner ratified 3 design calls** (high-1 drop compaction guard; med-2 A-M5-9 wire-aware effort; high-2
USD-normalize via fresh fx, fail-safe REFUSE on stale). **+ owner-approved W3b addendum** (extend A-M5-9 glue
to `nh chat` + `nh profile` — display-only, Anthropic wire never serializes effort; folded `effort_for` to take
`Wire`, dropped `effort_for_wire`). **nh-fleet untouched → still NO A-M5-8** (reserved for W5). W3 + W3b both
PASS (one self-corrected compile slip in W3b). Briefs archived `Temp/slice-f-w3-brief-v1.txt` +
`slice-f-w3b-brief-v1.txt`; self-reports `Temp/w3-last-message.txt` + `w3b-last-message.txt`. Full detail in
BUILD_LOG 2026-07-19 (W3). §8 amendment **A-M5-9** (+ glue-extension note) in CONTRACTS_M5.

**⇒⇒ W2 DONE + committed `2e09513` (2026-07-19).** Sol (codex exec, xhigh) implemented all 18 items W2-1..W2-18
across nh-tools + nh-mcp in ONE run (2 high / 3 med / 8 low / 5 nit), no deferrals; orchestrator gated +
adversarially reviewed + committed to main. **Gate: 395/0/1 `--release`, clippy `-D` clean, `cargo fmt --all
--check` clean** (Sol ran no fmt; no drift this wave). +18 tests over 377. 8 files, +981/−175: nh-tools/{lib.rs,
mcp.rs}, nh-mcp/{lib.rs,Cargo.toml}, cmd_run+cmd_chat (1-line `with_scrubber` glue), nh-tui/lib.rs, Cargo.lock.
Key fixes: high-3 ALL tool egress through `ToolResultEnvelope`(+ctx scrubber) at the single MCP-adapter choke
point; high-4 Windows exec `raw_arg` verbatim; medium-4 dep-free exec 300s timeout + null stdin + whole-tree
kill; medium-11 synchronous `fleet_run` preflight rejected to caller before spawn; low-6 MCP egress gated on
`Access::Send` (**Block stops egress before trust; default Allow byte-identical**); getrandom CSPRNG token +
subtle constant-time bearer; nh-mcp `State`→`Runtime` refactor; exact-route-only (404 else) + 1 MiB body cap
(413); OAuth refresh persist-warn + coalescing mutex. **Owner ratified 4 design calls** (low-6 `[send]` gate
Block-stops / getrandom+subtle vetted primitives / nit-8/9 State→Runtime refactor / medium-4 dep-free timeout).
**Two sound deviations** (both improvements, in the commit): (a) W2-17 `ServeConfig.token` stays
`Option<String>` (caller input = "mint one if absent"; the guaranteed-`Some` bound token lives on the new
`Runtime` struct → public config type unchanged for scoped callers); (b) W2-12/W2-10 use nh_fleet's REAL
`.nosis/fleet/{run_id}` ledger path, not the brief's approximate `run_root.join(run_id)`. **nh-fleet untouched
→ still NO A-M5-8** (built via `ToolCtx::new(...).with_guard(...)`, takes default scrubber). New deps getrandom
0.2.17 + subtle 2.6.1 (nh-mcp direct edges; already transitive → zero build weight, §0.4 W2 exception).
Adversarial review: **zero blocking issues**. Brief archived `Temp/slice-f-w2-brief-v1.txt`; self-report
`Temp/w2-last-message.txt`. Full detail in BUILD_LOG 2026-07-19 (W2).
- **Deferred to W5/A-M5-8** (owner-authorized frozen-boundary stop): a `fleet_run` that fails AFTER the
  synchronous preflight now emits a scrubbed warning, but cannot be returned to the original caller without an
  nh-fleet **RunFailed ledger event** — nh-fleet is frozen, so that surfacing is now formally W5 work.

**⇒⇒ NEXT: W5 "FLEET RELIABILITY" (nh-fleet)** — ratified order W1✓→W3✓→W2✓→**W5**→W4. NOT yet briefed. This is
the ONE wave that requires an amendment first: nh-fleet is M4-FROZEN, so W5 needs **A-M5-8** to define its
mutable surface before any Sol code. Sequence:
1. **Draft + ratify A-M5-8** (CONTRACTS_M5 §8): define nh-fleet's exact mutable surface for W5, INCLUDING the
   **RunFailed ledger event** contract W2 deferred (so a post-preflight fleet-run failure can reach the caller).
2. **Ground the seams + write the W5 brief** seam-by-seam (file:line) with adversarial tests — same cycle W1/W3/W2
   got. W5 targets (§0.1 Slice F plan): #6 budget-halt hang; #7 ledger torn-read; single-writer lock; resume
   hardcodes Native/drops offpeak; run_id unvalidated; receipts-without-usage never trip budget; run() dup
   validation. Pull exact finding IDs from `04-research/AUDIT_2026-07_fable5-full.md`.
3. **Owner GO** on the design calls (recommendation + why on each, per [[decisions-explain-rec-then-owner-decides]]),
   then **ask before launching** the Sol run.
- **Held for W4 (NOT W5):** low-16 (from_vault skip-signal), medium-20 (install_client key-literal union).
- **LAUNCH INVOCATION** (background, xhigh, single codex — never two nosis codexes at once):
  `codex exec --skip-git-repo-check --dangerously-bypass-approvals-and-sandbox -m gpt-5.6-sol -c model_reasoning_effort=xhigh -o <last-msg-file> - < <brief-file>`
  (W1/W2 used this form — brief piped via stdin `-`, `-o` captures Sol's machine-readable self-report; the older
  `-s workspace-write` sandbox form also works — pick per environment. Verify `19820`-style stray codex
  sessions are yours before launching a second codex; NEVER two *nosis* codexes at once.)
- **After Sol returns:** kill nh.exe → `git diff --stat` (scope) → authoritative gate via `gate.ps1`
  (fmt --check + clippy + test --release; redirect + `echo $?`, NEVER `| tail`; **note the gate's real
  verdict is its `GATE: PASS/FAIL` summary line, NOT the background wrapper's exit code**; if `fmt --check`
  flags Sol's hand-written reflow drift, orch runs the normalizing `cargo fmt` under pinned 1.96.0 then
  re-gates) → adversarial security review → commit → next wave (W4, the last).

**PRIOR IN FLIGHT (Slice D — DONE, historical):**

**IN FLIGHT (2026-07-18):** Slice D "LEVER" briefed to Sol (gpt-5.6 xhigh, `codex exec` background). Brief
= `slice-d-brief-v1.txt` (+ `slice-d-preamble-v2.txt` for handoff #2). **Handoff #1 (`b123jcp3f`) STOPPED
clean** at a real frozen boundary (the `Receipt.effective_profile` field forces `effective_profile: None`
glue in FROZEN nh-fleet test literals slice_b.rs:405/490 that A-M5-7 hadn't authorized — Sol changed no
files, good catch). Fixed via the **A-M5-7 addendum** (§8: authorizes the full `Receipt`-literal ripple —
nh-fleet:405/490 + nh-tui:1373/3067 = `None`; make_receipt:1622 sets the real value; + a blanket clause so
trivial glue for the two field-adds never stops Sol again). **Handoff #2 = `bfan4b9wi` DONE + GATED GREEN** (357 pass / 0 fail / 1 ignored `--release`, clippy `-D
warnings` clean, verified independently; +18 tests over 339; scope exactly the surface — nh-fleet got its
3 authorized glue lines; EOL-noise on cmd_init/cmd_key/m2_exit restored). Adversarial review passed:
profiles.rs tighten-only correct, resolve_effort matrix honest (AlwaysThinking can't disable / None can't
enable), clamp_route only touches max_out (audience gate preserved), balanced==today default path,
all surfaces scrubbed. **FEEL gate:** owner ran `nh profile` demo + approved **resolve-thinking-to-route**
(the honest display: show effective effort none/low/high/max per route, not the abstract posture — same
"meter must not lie" principle as A-M5-6). **Handoff #3 = `b1dwlm8r2` DONE** (display-only FEEL refinement:
resolved-effort display none/low/high/max, shared `effort_label`). Re-gated independently GREEN
(**357/0/1** `--release`, clippy clean) + owner FEEL-approved the honest `nh profile` output. **SLICE D
COMMITTED `2564476` on `213ed0a`** (feat; CURRENT_TASK.md deliberately held out for the docs-close).
Amendment **A-M5-7** logged (§8: `Receipt.effective_profile` + `AgentLoop.profile` + frozen nh-fleet:1227
`profile: None` ripple, + the Receipt-literal-ripple addendum). **Owner ratified 3 design calls:** (1) two levers only — thinking tier + output cap; NO route
selection (M6), NO currency hard-stop; (2) frugal = route thinking floor + cap ≤16384 + off-peak pref;
balanced = today byte-for-byte; max-quality = route thinking ceiling (High) + cap = route.max_out; (3)
route capability immutable (AlwaysThinking always thinks, no-toggle never thinks; law never weakened).
**Design:** apply at the caller (clone route w/ clamped `max_out` → existing `make_client`, NO sig change
[5 callers incl frozen nh-fleet:1203]; set live `AgentLoop.thinking`); nh-routes new `profiles` module
(Profiles + EffectiveExecutionPolicy + ThinkingPosture, layered bundled→user→repo tighten-only mirroring
nh-law:200-325); nh-core `resolve_effort` posture×dialect helper + receipt field; nh-cli `--profile` +
`nh profile`; nh-tui `/profile` + HUD chip + live re-apply (SetProfile worker cmd).

**✓ DONE (2026-07-18, this session — the Slice E fmt piece pulled forward + more):**
- **Fmt normalized + gated** (commit `68f71cd`): one-time `cargo fmt --all` cleared the 37-hunk / 7-file
  backlog (behavior-preserving); added `gate.ps1` mechanizing fmt --check + clippy -D warnings + test
  --release with per-step exit-code aggregation (never `| tail`, whose 0 masks a failure). Re-gate GREEN:
  **357 pass / 0 fail / 1 ignored** `--release`, clippy clean.
- **Build hygiene** (commit `059a00e`): `rust-toolchain.toml` pins 1.96.0 + rustfmt/clippy (stops fmt
  drift recurrence at the root — a newer rustfmt can no longer silently reflow); `.gitattributes` EOL
  policy; `deny.toml` DORMANT cargo-deny policy (cargo-deny NOT installed — wire `cargo deny check` into
  gate.ps1 after `cargo install cargo-deny --locked`).
- **FULL Fable 5 high audit DONE** (background workflow run `wf_72da5ecf-6f6`, 95 agents, ~3M tokens; report
  committed `c0ceaef` = `04-research/AUDIT_2026-07_fable5-full.md`). **86 raw → 75 confirmed / 11 refuted:
  0 critical, 7 high, 21 med, 30 low, 17 nit.** 3 highs orchestrator-confirmed vs source (#2 nh-routes
  cross-currency compare, #3 nh-tools MCP egress unbounded/unscrubbed, #5 nh-vault backslash audience
  bypass). Pre-scan: `unsafe` = **0**; `#[allow]` = 10 (benign).

**⇒ M5 "Slice F: HARDEN" — full audit remediation (owner chose EVERYTHING ACTIONABLE). Sol implements each
wave; owner FEEL-approves every human-facing surface BEFORE commit; nh-fleet is M4-FROZEN → its fixes need
a logged CONTRACTS amendment (A-M5-8). Ground each brief seam-by-seam w/ file:line + ASK owner before each
Sol launch. Full finding detail in the audit report.**
- **W1 SECURITY FLOOR — nh-vault + nh-law:** #5 backslash bypass; IPv6-audience-always-refused, `sk-`
  no-left-boundary, bidi spoof; empty-audience fail-open, Scrubber/Zeroize contract, make `normalized_host`
  pub (kill nh-cli `host_of` dup); law glob-recursion stack-overflow, `send_verdict` fail-open + host-norm,
  `exec_block` first-token sidestep; BUNDLED_LAW parsed-twice, repo_tries_to_weaken false-warn.
- **W2 TOOL EGRESS + EXEC — nh-tools + nh-mcp:** #3 MCP results through ToolResultEnvelope; #4 Windows exec
  `raw_arg`; ExecShell timeout+stdin, tools/list timeout, envelope literal-scrub, Send-verdict unenforced,
  OAuth refresh persist/race; nh-mcp CSPRNG token + constant-time compare + body cap + accept_loop signal,
  fleet_status 'starting', scrubber-recompile / State-dup / loose-routing.
- **W3 METER TRUTH — nh-core + nh-routes:** #1 compaction-dead (fold cost-check into candidate selection +
  realistic test); #2 cross-currency (partition-by-currency / fx-normalize + honest trace); compaction
  stale count, Anthropic wire drops resolved effort, deepseek think-low display; cache_hit_pct clamp,
  receipt-append destroys outcome, HTTP-body→empty, tool_use missing id; read_optional_profiles error + nits.
- **W4 SURFACES — nh-tui + nh-cli [FEEL-GATED]:** approval modifier-keys, wrapped_rows clip, failed-turn
  metering; worker-abandoned-on-quit, panic-hook, input-dead-while-Working; Esc-interrupt legend lie,
  heartbeat reset; usage-missing-as-fact `$0.00`, whole-run single-instant pricing, install_client scrub
  drop, ANSI passthrough, catalog injection, session-total omits unpriced, host_of dup, max-turns 0.
- **W5 FLEET RELIABILITY — nh-fleet [FROZEN → A-M5-8]:** #6 budget-halt hang, #7 ledger torn-read;
  single-writer lock, resume hardcodes Native/drops offpeak, run_id unvalidated, receipts-without-usage
  never trip budget, run() dup validation.
- **Order — OWNER-RATIFIED 2026-07-18:** W1 (security) → W3 (the meter thesis) → W2 (egress) → W5 (fleet)
  → W4 (surfaces, FEEL, last — consumes the fixed lower layers). A bare **"continue"** = ground + draft the
  **W1** Sol brief (nh-vault + nh-law) seam-by-seam, then **ask the owner before launching the Sol run**.

**Still queued (mechanical, no Sol — do opportunistically, e.g. while a Sol wave runs):**
`[workspace.lints]` + `#![forbid(unsafe_code)]` (audit done reading → unblocked; codify `unsafe`=0);
rest of Slice E "LOOP" (keyless CI, `codex exec --output-schema`, nextest+AV canary, gate.ps1
frozen-surface sensor). Then M5 DONE → write the ≥5 launch posts ([[why-best-in-category-2026]]).

**Slice C "VISIBLE" SHIPPED (E3 met — THE FEEL GATE PASSED).** Committed `a0f77be` on `1fb0861`. Gate
green: **339 pass / 0 fail / 1 ignored** `--release`, clippy `-D warnings` clean (319 → +20: Slice C +17,
sub-cent fix +3). Built by Sol (two handoffs: the full slice, then a sub-cent honesty fix live-testing
exposed); gated + adversarially reviewed + **live-verified with the owner's real GLM key** (free
glm-4.7-flash end-to-end → honest `$0.00`; paid glm-5.2 → clean "insufficient balance" error; `nh why`
live) + **owner FEEL-approved before commit**. Delivered: money HUD (currency cached/miss/output split +
per-currency session total, replacing token-only; token budget hard-stop kept); THE counterfactual
savings line (`cost … — saved N% vs no-cache` + peak/no-cache/top-tier breakdown; cold turn makes no
false claim); approximate USD gloss (native = billed truth, `≈$` omitted when stale/absent, NEVER
FX-summed across CNY/USD); adaptive money precision (a real sub-cent spend never renders `$0.00` — only a
genuinely-free route does); `/why` (TUI + `nh why` CLI) off live `resolve_capable` + `RejectionTrace`;
approval cluster L6 fix (y/a/n/Esc only, any other key = no-op never a silent deny, always-this-session
rule, visible legend, Esc-to-interrupt); working heartbeat; OSC 9;4 Windows taskbar; errors-that-teach.
**drop-if-hard call:** "Esc to stop" while working was dropped (no truthful cooperative-cancel path — the
heartbeat shows `● WORKING · Ns` without claiming an interrupt). Amendment **A-M5-6** (USD gloss + `[fx]`
catalog data + no-cache-headline FEEL ruling) logged in `CONTRACTS_M5.md` §8. **FEEL finding worth
remembering:** at 2 dp a real ~$0.003 turn rounded to `$0.00` — fixed with adaptive precision (congruent
with "the meter must not lie"). Live money/savings % demo still pending a key with paid balance (free GLM
caps at `$0.00`; DeepSeek/Kimi or a funded GLM key would light up "saved 93%").

**Slice A "TRUTH" SHIPPED (E1 met).** Committed `68f91e6` on `a126ee2` on `88b84e8`. Full workspace gate
green: **306 pass / 0 fail / 1 ignored** `--release`, clippy `-D warnings` clean (baseline 292 → +14
Slice A tests). No FEEL gate needed (no human-facing surface). Built by Sol (two handoffs: truth-math +
resolver, then an Anthropic-wire fix); gated + adversarially reviewed by the orchestrator.
- **Delivered:** L1 explicit thinking-disable + kimi-toggle dialect; L2 state-aware reasoning replay
  (K2.6 thinking+tools no longer errors); L7 cache-safe compaction (elision note = new msg, retained
  msgs byte-identical); L8 reasoning+tool-spec token counting; L9 output cap on both wires; L12
  all-builds PrefixSeal + cache-break signal; effective_context clamp; native cache-field fallback;
  nh-routes `resolve_capable` + `RejectionTrace` (cheapest context-fitting priced route + audit trace).
- **Amendments logged (all in `CONTRACTS_M5.md` §8):** A-M5-1 (KimiToggle variant + preserve_when_thinking
  field, nh-routes); A-M5-2 (KimiToggle compile-compat arm in nh-fleet/nh-tui/nh-cli `effort_for` — the
  enum-variant ripple; orchestrator glue); A-M5-3 (build_anthropic_body consecutive-user merge — fixes an
  L7 regression the review caught: two consecutive user messages post-compaction 400 the Anthropic wire).
- **Two `[VERIFY-LIVE §7]` guesses to confirm with a live key later:** DeepSeek explicit non-thinking
  wire shape (`thinking:{type:disabled}`) + Kimi K2.6 toggle shape (`thinking:{type:enabled|disabled}`).
- **PROCESS LESSON (bit us this slice):** the workspace was clippy-clean but NEVER `cargo fmt`-clean;
  running `cargo fmt --all` mid-gate reformatted the ENTIRE workspace and polluted the diff across frozen
  crates. Recovered by reverting fmt-only churn to HEAD + re-applying the glue. **RULE: never `cargo fmt
  --all` mid-slice — use scoped `cargo fmt -p <crate>`; and always `git diff --stat` after a Sol run to
  confirm scope.** Slice E must add a scoped `cargo fmt --check` gate + a one-time workspace normalization.

**Slice B "FLOOR" SHIPPED (E2 met).** Committed `edfcd62` on `7404878`. Gate green: **319 pass / 0 fail /
1 ignored** (`--release`), clippy `-D warnings` clean (306 → +13). Built by Sol (single handoff), gated +
adversarially reviewed + live-demoed (audience redirect refused 3 ways incl. userinfo `@`-trick + suffix
spoof; nh-mcp unauth `fleet_run`→401, cross-Origin/Host→403, loopback→200). Delivered: L3 read guard
(`Access::Read`/`Send` + `[read]`/`[send]`/`[credential]` law classes mirroring `write_verdict`) +
`ToolResultEnvelope` (bounds + shape-scrubs tool output); L4 `get_scoped` credential-audience broker
(**trusted-law** source — bundled/user, repo can't grant; host-only compare; refuses BEFORE materialize)
wired on CLI routes + MCP config-load; L5 nh-mcp fail-closed OS-seeded token + Host/Origin (DNS-rebind);
L10 min-env exec allowlist; L11 MCP text sanitize; F8 scrubber widen (ghp_/AKIA/AIza/xox…) + `from_vault`;
F7 OAuth `resource` (RFC 8707). Amendments **A-M5-4** (Access-variant ripple; frozen nh-fleet + nh-tui got
exactly +2 guard arms each) + **A-M5-5** (audience broker call-sites; owner-ratified trusted-law source)
logged in `CONTRACTS_M5.md` §8.

**NEXT = Slice C "VISIBLE" (E3) — THE FEEL GATE (the milestone is won or lost here).** Crates: `nh-tui` +
`nh-cli` + the `nh-routes` cost helpers. **Graded by FEEL first, tests second; `drop-if-hard` per sub-item;
owner FEEL-approves every human-facing surface BEFORE commit.** Items (see `CONTRACTS_M5.md` Slice C): money
cost HUD (currency split over cached/miss/output + running session total + budget hard-stop, replacing the
token-only HUD); **THE counterfactual savings line** (`cost ¥0.11 — saved 93% vs naive (peak ¥0.44 ·
cache-miss ¥1.62 · pro-tier ¥3.90)` — the launch screenshot) via a new nh-routes `naive_cost`; `/why`
route-explain (CLI + TUI chip + receipt) using Slice A's `RejectionTrace`; approval cluster (explicit
y/n/Esc + visible legend — fixes L6 — prefix-rule approvals, Esc-to-interrupt, working heartbeat); OSC 9;4
Windows taskbar semáforo; "errors that teach" as a tested invariant. Slice C consumes Slice A's
`RejectionTrace` (shipped) + Slice B's floor (shipped). Brief Sol the SAME way that worked for A and B:
read the real nh-tui/nh-cli/nh-routes seams first, ground the brief seam-by-seam with file:line, define +
pre-authorize the mutable surface UP FRONT, enumerate cross-crate ripples BEFORE briefing, then ASK the
owner before launching the Sol run. Because C is the FEEL gate, plan to run the built `nh.exe` / TUI and
have the owner approve the FEEL before committing.

**M5 "The Honest Meter" — scope ratified & contract locked (2026-07-17, this session):**
- **Five slices A–E**, re-slotted by *seam* (not theme) for congruence: A TRUTH (nh-core meter-math
  + nh-routes thin honest-routing) / B FLOOR (nh-tools/law/vault/mcp) / C VISIBLE (nh-tui/cli — THE
  FEEL gate) / D LEVER (profiles) / E LOOP (build hardening). See `CONTRACTS_M5.md` (LOCKED).
- **Ratified calls:** thin honest-routing IN (Slice A; context-fit + expected-cost + rejection trace;
  NO jurisdiction/learning); forecast/`cost_estimate` OUT (one addition, not two); TWO defers held out
  (MCP TOFU-pin → M7 keep only sanitize; jurisdiction/governance/privacy-router → M6 keep only the
  `[read]`/`[send]` law class); behavior-corrections authorized enumerated (§0.1 mutable surface).
- **Positioning captured:** `01-product/WHY_BEST_IN_CATEGORY_2026.md` — the best-in-category thesis +
  seeds for ≥5 launch posts; append new article ideas there as found (write posts when M5 is "done").
- **NEXT:** write the Slice A Sol brief → `codex exec` (background) → gate (`cargo test --workspace
  --release` + clippy) + adversarial review → owner FEEL-approve → commit (`wip/slice-a` → main).

**M4 is CLOSED.** HEAD `0039cc4` (docs: close M4) on `a2c2b83` (research) on `9344251` (Slice D feat).
All four M4 slices committed: A `96db4f7` (fleet), B `25bd5b3` (scheduler/ladder/swarm-seam),
C `ece6bb0` (nh-mcp), D `9344251` (OAuth2, E4). `CONTRACTS_M4.md` LOCKED; §8 has the as-implemented
A-M4-1 clarification. Working tree clean after this session's 3 commits.

The **deep improvement research** is committed (`a2c2b83`) — read it before planning M5:
`00-start-here/RESEARCH_2026-07_harness.md` (report) + `04-research/_harness-research-2026-07/`
(14 raw files: Sol xhigh 60-item backlog + Fable 5 high lenses A-M, 265 sources). Two models
independently converged on the identity and the priority; that convergence is the plan's backbone.

### The product identity both models converged on (this is the spine — keep everything congruent to it)
> **nosis is the agent harness with a meter: it routes every task to the cheapest CAPABLE model —
> by clock, cache, modality, thinking budget (and, new, jurisdiction) — and hands you the receipt.**

Design test for any feature: *can it produce a receipt / does it serve "priced, routed, receipted,
and you can see why"?* If not, it isn't nosis. The 60-second "aha" = the **counterfactual savings
line** (actual cost next to naive cost — a line no incumbent can print because their router can't
see their cache). This is the thing to build the whole milestone around.

## M5 = "The Honest Meter" (owner-picked direction 2026-07-17; RATIFY, then contract + brief)
Thesis: **make the meter TRUE, SAFE, and VISIBLE before adding autonomy or providers.** Both models
proposed this exact order (fix security + measurement first). Five congruent slices, each a Sol brief:

- **Slice A — TRUTH (the meter must not lie).** Fixes the live cost/correctness bugs found in the
  research (report §3): L1 thinking-defaults (governor None/Low silently buys full high thinking;
  DeepSeek normalizes low→high, auto-escalates harnesses to max — send `thinking:{type:disabled}`,
  new `kimi-toggle` dialect); L2 `reasoning_content` conditional replay (kimi-k2.6 thinking+tools
  ERRORS today); L8 `estimate_tokens` counts reasoning + tool-spec bytes; L9 Anthropic-wire output
  cap + `output_config` effort; parse DeepSeek native cache fields as fallback. Touches nh-core +
  catalog. **These are the highest per-dollar fixes in the whole report.**
- **Slice B — FLOOR (the meter must be safe/auditable).** L3 `read_file` guard + `[read]`/`[send]`
  law verdict class (closes the Lethal-Trifecta read leg); L4 credential-audience binding (repo
  config can currently redirect a vault secret to any URL); L5 nh-mcp inbound token-default +
  Host/Origin; L6 fix any-key-silently-denies approval bug; L11 MCP tool TOFU pinning + ANSI/
  invisible-char sanitize; min-env exec allowlist; widen Scrubber shapes + registry from all vault
  entries; supply-chain `deny.toml` + cargo-audit/deny; OAuth `resource` param (RFC 8707).
  Touches nh-tools/nh-law/nh-vault/nh-mcp.
- **Slice C — VISIBLE (the meter felt).** Money cost HUD (`turn_cost` over cached/miss/output) +
  session total + budget hard-stop; the **counterfactual savings line** (the aha); `/why` route-
  explain (CLI + TUI chip + receipt); honest activity suffix + Esc-to-interrupt + working heartbeat;
  OSC 9;4 Windows taskbar semáforo; "errors that teach" as a tested invariant. Mostly nh-tui + a
  tiny nh-routes helper.
- **Slice D — LEVER (savings selectable — the owner's toggle-per-provider-by-profile ask).**
  `profiles.toml` (frugal / balanced / max-quality) layered like law.toml → one
  `EffectiveExecutionPolicy` clamping wishes to route caps; `/profile` + HUD chip + receipt field;
  output caps threaded to both wires; tool-result envelope (bounded, handle+digest); cache-aware +
  **append-only** compaction (L7 — never mutate the prefix); `effective_context` clamp (context-rot
  guard). nh-routes/nh-core/nh-tui.
- **Slice E — LOOP HARDENING (continuous; congruent with the build process, no frozen crates).**
  `wip/<slice>` commit rule (durability — Slice D nearly lived only in Temp); `gate.ps1` frozen-
  crate/allowed-files sensor; minimal keyless CI (windows-latest + ubuntu, 292 mock tests);
  `codex exec --output-schema` structured Sol handoff; cargo-nextest + AV canary preflight;
  `[workspace.lints]` + pinned `rust-toolchain.toml`.

**FROZEN-CRATE NOTE (important):** the M4 freeze was per-milestone. Many Slice A/B items live in
nh-core/nh-tools/nh-law/nh-routes/nh-vault. **CONTRACTS_M5.md must define its own mutable surface +
the amendment list UP FRONT** (learn from A-M4-1: pre-authorize the seams so Sol never faces a
break-scope-or-duplicate choice). Additive/behavior-preserving where possible; adversarial security
review on every Slice-B item.

**The arc beyond M5 (directional, re-plan after M5 — do NOT let it leak into M5 scope):**
- **M6 = the Learning + Private + Resilient meter** — Route Scorecard off receipts → outcome-weighted
  ladder + failure-class-aware next_step + pre-run forecasts + `nh bench` (the moat); privacy-aware
  routing (governance metadata + privacy profile + custody receipts + one-question `nh init` — the
  new differentiator); five-stage compaction + session ledger/`nh resume` + file-memory; reliability
  (typed RouteError + backoff, availability re-resolve, cooldown, $0 local Ollama floor).
- **M7 = ecosystem + launch** — `nh exec --output json` + GitHub Action ($0 GLM CI), MCP Tasks,
  `nh gateway`, SKILL.md + `extensions.lock`, `nh acp`, cargo-dist→winget; **GLM/Z.ai key** (free,
  best ratio — $0 CI + vision + FlashX + 3rd Anthropic wire + SG privacy lane); multimodal input;
  hash-chained receipts + `nh verify`; Windows sandbox tier; nosistech.com launch post.
- **Scope CUT ratified by the research + July-2026 news:** demote subscription delegates from marquee
  pillar to escalation-gate footnote (Anthropic moved programmatic use to API pricing 2026-06-15;
  Gemini CLI died as open delegate). Reposition: **"open-weight-first harness with a frontier review
  gate."** Keep the commented catalog delegate schema; don't build the full adapter class in v1.

### ON RESUME ("continue") — Slices A–D + fmt/gate/toolchain DONE; audit DONE; NOW = Slice F HARDEN remediation.
**State (2026-07-18):** M5 Slices A "TRUTH" (`68f91e6`) + B "FLOOR" (`edfcd62`) + C "VISIBLE" (`a0f77be`) +
**D "LEVER" (`2564476`)** shipped. This session (owner "continue" + expanded scope): fmt-normalize +
`gate.ps1` (`68f71cd`), toolchain pin + `.gitattributes` + `deny.toml` (`059a00e`), docs-close (`6a11f32`),
+ a **FULL Fable 5 high audit** (report `c0ceaef`: 75 confirmed = 0 crit / 7 high / 21 med / 30 low / 17
nit). Gate GREEN **357/0/1**, clippy clean. §8 amendments A-M5-1..**7** (A-M5-8 pending for W5 fleet fixes).
**Owner chose "EVERYTHING ACTIONABLE" → M5 "Slice F: HARDEN"** = full remediation in 5 Sol-implemented waves
(W1 security floor / W2 tool egress+exec / W3 meter truth / W4 surfaces [FEEL] / W5 fleet [frozen]). **W1
(`6cefd56`), W3 (`73d278b`), W2 (`2e09513`) are DONE** — order now W1✓→W3✓→W2✓→**W5**→W4. If the owner typed
**"continue"**: do the **⇒⇒ NEXT: W5** block at the TOP — ground the nh-fleet seams + draft the **A-M5-8**
amendment (nh-fleet is frozen; W5 needs it FIRST) incl. the RunFailed ledger event W2 deferred, then write the
W5 brief seam-by-seam, **ask the owner before launching the Sol run**, gate + adversarially review, then W4
(the last, FEEL-gated). Also queued (no Sol): `[workspace.lints]`+`forbid(unsafe_code)` + rest of Slice E.
Do NOT re-do Slices A–D, W1/W3/W2, or the fmt/hygiene commits. Read the top blocks first.

1. **Confirm clean:** `git log --oneline -1` = `a0f77be`; `git status` clean. Kill any `nh.exe` before builds.
2. **Read the real seams FIRST** for Slice D: `nh-routes` (a NEW `Profiles` module + `EffectiveExecutionPolicy`
   that clamps profile wishes to route caps — layered bundled→user→repo like law, repo may only *tighten*);
   `nh-core` (apply the clamped policy at request-build: output cap [Slice A's mechanism], thinking tier);
   `nh-tui`/`nh-cli` (`/profile` toggle + HUD **profile chip** [deferred from C] + receipt field). Ground the
   brief seam-by-seam with file:line, the way A/B/C worked.
3. **Pre-authorize the mutable surface + enumerate cross-crate ripples** BEFORE briefing (the A-M5-2/-4/-6
   lesson). `profiles.toml` is new DATA (frugal/balanced/max-quality). Log any needed §8 amendment first.
   Then **ask the owner** the genuine decisions (e.g. the three profiles' exact caps; whether a *currency*
   budget hard-stop lands here — it was held out of C as a D lever) and **ask before launching the Sol run**.
4. **Brief Sol** (gpt-5.6 xhigh, `codex exec`, background — invocation below; NEVER two nosis codexes at
   once) for Slice D per `CONTRACTS_M5.md` Slice D. `drop-if-hard` per sub-item.
5. **Gate:** `git diff --stat` for scope → kill nh.exe → `cargo test --workspace --release` (≥339 + new,
   0 fail) + `cargo clippy --workspace --all-targets --release -- -D warnings`. **GATE RULE:** never pipe
   `cargo test`/`clippy` through `| tail` (a pipeline's exit code is the LAST command's — `tail`'s 0 masks a
   real failure); redirect to a file + `echo $?`. Never `cargo fmt --all` (scoped `cargo fmt -p` only).
   Adversarial review. If Sol STOPS at a frozen boundary, apply the compile-glue as orchestrator + log §8.
6. **FEEL gate + commit:** switching frugal↔max-quality must visibly change the built body (output cap /
   route ceiling) + HUD profile chip + receipt; a **repo** profile may only *tighten*. Owner FEEL-approves
   the profile chip/`/profile` BEFORE commit, then commit per-slice to `main` + update this file + memory.
   Slice E (LOOP/gate.ps1+CI — can land anytime, ideally soon to mechanize the gate) is the last M5 slice.

**When M5 is "done" (shipped + FEEL-approved):** write the ≥5 launch posts from
`01-product/WHY_BEST_IN_CATEGORY_2026.md` (append new article seeds there as they surface). [[why-best-in-category-2026]]

## Roles (fixed) — [[m2-m5-codex-sol-directive]]
- **Orchestrator = Opus 5** (this session): plans, writes contracts/briefs, runs gates, adversarially
  reviews, commits, docs. Does NOT hand-write milestone code.
- **Executor = GPT-5.6 Sol xhigh** via `codex exec` — writes all milestone implementation.
- STOP (don't fall back to Terra) if gpt-5.6-sol stops resolving or Sol fails the same gate twice.

## Executor invocation (proven — used for both M4 Slice D and the July-2026 research pass)
```
codex exec --skip-git-repo-check -s workspace-write -m gpt-5.6-sol \
  -c model_reasoning_effort=xhigh "$(cat /c/Users/capv2/AppData/Local/Temp/<brief>.txt)" < /dev/null
```
Run in background (harness-tracked); verify empirically after (numstat = truth; EOL/CRLF flags = noise).
Do NOT start a second codex on nosis while one writes nosis. Consider adding `--output-schema` for a
machine-readable self-report (report Lens G finding 5).
**STANDING RULE (2026-07-18): every Sol brief must say "do NOT run `cargo fmt`."** The workspace is now
fmt-clean, toolchain-pinned (1.96.0), and gated (`gate.ps1` runs `cargo fmt --all --check`). Formatting is
the orchestrator/gate's job; a Sol `fmt` run only risks re-reflowing whatever file it touches (this is what
polluted the Slice A and Slice D diffs). Sol writes code; the gate formats + checks.

## UX is THE priority (see [[ux-first-and-the-law]])
"Pretty but frustrating" = failure. Judge by FEEL first, tests second. Self-teaching, no handholding,
delightful for small tasks. Do NOT commit milestone work until Carlos approves the FEEL.

## Environment gotchas (bit us before)
- A running `nh.exe` LOCKS `target\debug\nh.exe` → build/test link fails. Kill it first.
- Bash tool `cd` PERSISTS → use absolute paths or `cd /c/Users/capv2/Desktop/nosis-Harness`.
- PowerShell reads UTF-8 as OEM codepage → box-drawing/`—` look like mojibake; check raw BYTES.
- Kaspersky AV blocks freshly built `nh.exe` (os error 5) → `fleet_kill_resume` + `m2_exit` spawn
  tests fail on env, not code. cargo-nextest + an AV canary preflight (Slice E) will classify this.
- Do NOT change Carlos's Claude Code settings or Kaspersky settings without asking.

## Do Not Do
- Do NOT hand-write milestone code — Sol implements; Claude plans + gates.
- Do NOT commit milestone work until Carlos approves the FEEL (UX is the gate, not "tests pass").
- Do NOT touch a crate M5's contract hasn't opened, without a logged CONTRACTS_M5 amendment.
- Do NOT let the M6/M7 arc leak into M5 scope (scope creep is the plan's named #1 killer).
- Do NOT ship the nh-mcp server publicly before the MCP final spec lands (2026-07-28).
