# Continue Here — authoritative checkpoint

**Updated:** 2026-07-31 America/Guatemala
**Resume phrase:** `continue`
**Works for:** Codex or Claude in `C:\Users\capv2\Desktop\nosis-Harness`

This file supersedes every older checkpoint and "next task" in `CURRENT_TASK.md`. Historical records
remain useful for provenance, but do not execute their stale instructions. **An executor read
`CURRENT_TASK.md` during wave M4 and found its tail still announcing "NEXT = Slice C VISIBLE",
ancient history. Read this file, not that one.**

## THE HEADLINE: retry shipped. The harness no longer tells the user to retry. Next is `nh resume`.

Wave **M4 "RESILIENCE"** is committed as `76cbb54`. Waves **M1 (images in)**, **M2 (tool floor)**
and **M3 (prices don't expire)** shipped earlier. All product work from here.

## First action on `continue`

Read this file in full. Then verify with read-only commands:

```powershell
git status --short                  # expect CLEAN
git log -3 --oneline                # expect the M4 docs and feat commits on top of cb2670b
git log origin/main..HEAD --oneline # expect nothing pending
git config --local user.email       # expect 98294098+arparvar@users.noreply.github.com
Get-Process -Name nh,codex -ErrorAction SilentlyContinue | Select-Object Id,ProcessName,StartTime
```

Any `codex.exe` you find is probably the OWNER'S on another project — on 2026-07-30/31 two of his
ran for days on `super-triplets` and `Ashveil-horror-story`. Check `~/.codex/sessions` rollout files
for `"cwd"` before assuming a codex is yours. Verified procedure: read the first line of the newest
rollout `.jsonl` files and match `StartTime` to the rollout timestamp.

## ⚠ EVERY COMMIT SHA CHANGED ON 2026-07-30

History was rewritten to remove the owner's personal email from all 74 commits. **No file content
changed.** To resolve ANY old SHA, use the 74-entry map in
`08-decisions-and-risk/HISTORY_REWRITE_2026-07-30.md`. **Do not rewrite history again** without an
explicit owner decision. `git config --local user.email` is now the noreply address — do not reset it.

## Hard constraints

- **Do not commit, push, or tag without owner authorization.**
- **Do not run write-mode `cargo fmt`** as a habit — but the orchestrator DOES run the scoped
  normalize (`cargo fmt -p <crates>`) after a Sol wave, because Sol is forbidden from formatting and
  `fmt --check` is gate step 1. That is the standing protocol, not a violation.
- **Sol must never run `cargo fmt`.**
- Do not expose MCP publicly. Loopback-only, bearer-guarded, preview.
- Do not delete or truncate `.nosis/` receipts or Fleet state.
- **Do not add third-party dependencies.** An intra-workspace path edge is NOT a new dependency
  (ratified 2026-07-31, see `DECISION_LOG`).
- Never print, persist, or upload credentials.
- **Never run two nosis codexes.**
- Roles: **Sol max = executor** (all code, one wave at a time, via `codex exec`), **Opus 5 =
  orchestrator** (briefs, gates, review, commits, docs; writes no feature code), **Fable 5 =
  research**.
- **Sol launch shape** — PS 5.1 mangles big multiline args, so the prompt goes via STDIN.
  `Start-Process` and PowerShell background mode are BLOCKED in this harness (EPERM on uv_spawn);
  launch through the **Bash tool with `run_in_background: true`**:
  `codex exec -m gpt-5.6-sol -c 'model_reasoning_effort=max' -s workspace-write --color never -C <repo> -o <out>.md < <brief>.txt > <run>.log 2>&1`
  Verified working on 2026-07-31 for both M4 and M4b.
- **Write commit messages to a file and use `git commit -F`.** Verify no BOM (first bytes must not
  be `EF BB BF`). Use the Write tool or `[System.IO.File]::WriteAllText` with `UTF8Encoding($false)`.
- Follow root `AGENTS.md`, `05-ai-collaboration/AGENTS.md`, and THE LAW.

## SCOPE BRIEFS FOR SOL: define scope BY CRATE, never by file list

This is the single highest-value process lesson in the repo. **Five clean stops** happened across
2026-07-30/31, every one caused by an orchestrator brief that was wrong about the tree, and every one
caught before a bad commit. Sol changed nothing and fabricated nothing on all five.

The shape that works: name **FULLY-IN-SCOPE crates**, **MECHANICAL-ONLY crates**, and a
**FORBIDDEN list**, then say explicitly *"the crate list IS the boundary; any file list is a map
built by reading, and reading can be wrong — trust the compiler over the map, and do not stop for a
file inside a fully-in-scope crate."*

**Wave M4 confirmed this works at scale.** Its brief made `nh-core` fully in scope and *every other
crate* mechanical-only, explicitly pre-authorizing one-line default-field additions even inside
frozen `nh-fleet`. Sol touched 19 files across four crates in one run with **no clean stop**. Before
briefing, size the blast radius yourself: `git grep -n 'StructName {'` counts the struct literals a
new field will break.

The five stops, so nobody repeats them:

1. Brief required `nh_law::glob_matches` while forbidding manifest edits — `nh-tools` had no
   `nh-law` dependency. Unsatisfiable.
2. Brief claimed nh-cli asserts tool names but never counts — `nh-cli/src/cmd_chat/tests.rs:815`
   asserted exactly three lines.
3. Brief said write `verified_on = "2026-07-26"` on all 12 routes — `catalog.toml:153` records Kimi
   K3 re-verified **2026-07-28**. Sol refused to backdate provenance inside the very wave about
   honest provenance. **Guard both directions: never fresher than the evidence, and when uncertain
   take the OLDER date.**
4. Brief pointed at the wrong file for the MCP output schema (it lives in `nh-mcp/src/protocol.rs`,
   not `route_tools.rs`).
5. Wave M1 cost three launches to the same class of error.

Also: **decide by which struct a field feeds, never by a literal string.** `valid_until` exists in
both the route-price and `[fx]` structs, and `cmd_why.rs` uses the fx one.

## What shipped this session (2026-07-31)

**`76cbb54` — wave M4 "RESILIENCE"**, 19 files, +1135/−63, new file
`crates/nh-core/src/wire/retry.rs`. Gate **PASS 636/0/1 `--release`** (fmt, clippy `-D`, rustdoc
`-D`, cargo-deny, tests). 619 → 636 = seventeen new tests, none removed.

There was no HTTP retry anywhere; `http.rs` answered a 429 with "rate limited; retry later" — it
told the HUMAN to retry, while free `glm-4.6v-flash` needed four manual retries at 6/12/24/48s in
the 2026-07-30 image probes. GLM-free is the no-credit-card on-ramp.

- Retry lives in the **wire layer**, not a `ChatClient` decorator: above `complete()` the status and
  `Retry-After` have collapsed into an `anyhow` string, and deciding to spend money again by
  string-matching an error message is what this project exists not to do.
- Policy is pure and injected — sleep, jitter and attempt execution are all parameters, so **no test
  opens a socket or sleeps**.
- `RetryStats { retries, rate_limited }` counts RETRIES not attempts, so a no-retry run serializes
  to zeros, the field is skipped, and `receipts.jsonl` stays byte-identical — **pinned by an exact
  JSON string test**.
- Typed `RetryExhausted` (same downcast pattern as `nh_vault::AudienceRefused`) carries stats and
  salvaged usage into the FAIL receipt, so money spent on failed attempts is not dropped.
- Usage across attempts is SUMMED from blocks actually observed; an absent block contributes ZERO
  and nothing is estimated. Overflow fails closed to `None`.

**Four decisions were ratified and are logged in `DECISION_LOG.md`**: never retry a timeout; the
4-attempt/45s budget with no knob; retry-only scope; and squashing the M4b fix.

### The M4 review lesson — the most reusable thing in this wave

Review caught a real defect in green code. `next_delay` divided its jitter sample by `u32::MAX`
while both production callers supplied `SystemTime` `subsec_nanos`, bounded at 999,999,999 — 23% of
that divisor. The ratified `[0.5, 1.0]` jitter span was really `[0.5, 0.616]`, so backoff ran at
**roughly half its ratified length**, worst for the exact rate-limit case the wave existed to fix.

**The tests were green because they passed `u32::MAX` — a value production can never produce.** A
pure function tested only at values its real caller cannot supply is not tested. The fix named the
domain (`JITTER_SCALE`), collapsed the duplicated closure into one `system_jitter()`, and added the
assertion that the *source* stays in the domain. Squashed into `76cbb54`; `BUILD_LOG` keeps both
wave entries.

## Remaining work, newest priority first — ALL PRODUCT WORK

1. **NEXT WAVE — `nh resume` + session ledger.** A crashed session loses everything today. Table
   stakes, and the substrate the compaction-receipt work needs. Research Tier 3 is partial:
   `effective_context` exists; five-stage compaction, compaction receipts and the session ledger do
   not.
2. **Compaction receipts** — show what was dropped and what it saved. The honesty thesis applied to
   context instead of money. Differentiator, not a chore.
3. **Ubuntu test-suite hang — THE blocker for `v0.1.0`.** Linux compiles and lints clean; `cargo
   test` never finishes and hits the 30-minute CI ceiling. Confirmed again 2026-07-31: Windows,
   macOS and Supply-chain all green, **ubuntu-latest cancelled at ~35m for the fifth time**. The
   hanging test is unnamed (GitHub archives no logs for a timed-out job). **Ruled out, do not
   re-investigate:** keyring/D-Bus (the only keyring test is the `#[ignore]`d one) and Unix-generic
   causes (macOS passes). Prime suspects: `nh-fleet`'s `File::try_lock`/flock semantics, or Linux
   process-group timing. **The owner runs the Ubuntu VM himself** (VirtualBox Ubuntu 26.04 Desktop,
   `libdbus-1-dev` required).
4. **Live-verify the retry path.** Every M4 test is a scripted fake by construction, so the real
   shape of a GLM `Retry-After` header is unverified — the same class of gap VERIFY-LIVE §7 closed
   for thinking dialects. Forcing a 429 on demand is awkward; treat it as opportunistic.
5. **Two M1 nits, non-blocking:** image part ordering is emitted text-first but was only measured
   image-first (one sub-cent call settles it); and MiMo's documented 8192-pixel minimum is
   unenforced because the wave does not decode image dimensions.
6. **Cruft, non-blocking:** three `nh-fleet` test fixtures still carry route-price `valid_until`
   lines. Dead keys — serde ignores them. `nh-fleet` is frozen; left for a future sweep.
7. **Deferred from M4, non-blocking:** `wire/anthropic.rs` duplicates ~118 lines of attempt
   scaffolding from `openai.rs`. Cosmetic today — all 12 catalog routes are `wire = "openai"`.
   Also: there is no in-flight "retrying in 6s" notice, which the live working heartbeat would fix.
8. Then: FEEL pass on the current binary, required status checks, final gate, **tag `v0.1.0`**.

## OWNER ACTIONS STILL OWED (none blocking)

1. **Branch protection.** Ratified 2026-07-30, still NOT applied — `main` is unprotected. The
   harness classifier blocks the orchestrator from writing GitHub settings, so the owner runs it:
   `gh api -X PUT repos/nosistech/nosis-harness/branches/main/protection --input <json>`
   with force-push and deletion protection, `enforce_admins: true`, and **no required status
   checks** (those wait for tag time, once Ubuntu is green, because required checks and direct
   pushes are the same lever).
2. **Commit-author email — LEAVE IT.** Decided. No action. A second rewrite would invalidate the
   published SHA map.

## Settled — do not reopen

- **The price topic.** Route prices deliberately never expire. Do not propose a freshness feature, a
  recheck cadence, a CI price watcher, or a `verified_on` date. **fx staleness is DIFFERENT and
  stays** — an old price is a number a reader can judge; an old exchange rate silently mis-converts
  CNY to USD into a confidently wrong number they cannot judge.
- **Never retry a request timeout** (2026-07-31). A status proves the provider was not billed; a
  timeout may hide a billed response, so retrying double-charges invisibly.
- Provider scope is **DeepSeek + Kimi + GLM + MiMo + local**. No Anthropic/OpenAI/Gemini API routes.
- **All 12 catalog routes are `wire = "openai"`.**
- Never add a frontier price row as a `top_tier` anchor.
- Zero-price routes are a selectable tier, not a routing winner.
- **The harness does not auto-route.** The "router inside the harness" moat claim is retired, which
  makes the research's Tier-2 "learning router" **stale as written**, and constrains what a future
  availability re-resolve may do without asking.
- Do not add a quality axis to routing.
- Do not change the edit format, restructure the prompt, lower temperature, or use
  grammar-constrained decoding.
- **Image generation is DECLINED** (third wire, no `usage` block, Z.ai labelling duty). **MiMo
  off-peak 0.8× is REFUTED** (Token Plan only, and its terms forbid our access pattern). **Kimi
  Batch 0.6× is REFUTED** (12h minimum, no `cached_tokens` against a 5.7× spread). Full reasoning in
  `DECISION_LOG`. Do not re-plan any of the three.
- **ASD-STE100 is scoped, not repo-wide**: user-facing and safety-critical text only. Describe it as
  "STE-informed", never claim conformance.

## TREAT THE ~90-ITEM RESEARCH BACKLOG AS LEADS, NOT SPECIFICATIONS

`00-start-here/RESEARCH_2026-07_harness.md` plus the 14 raw files in
`04-research/_harness-research-2026-07/` are July-2026 leads. On 2026-07-30 three items went to
verification: **two were refuted outright** and the third needed five corrections before it was safe
to build, including one catalog claim protected by a green test. Verify every item against
first-party docs AND a live probe before briefing it.

Tier status: Tier 0/1/9 largely shipped; **Tier 2 stale** (assumes auto-routing); Tier 3 partial
(this is item 1 above); **Tier 4 retry row now SHIPPED**, its re-resolve and cooldown rows unbuilt;
**Tier 5 and 8 unbuilt**.

## Do not redo

- The five-lane audit, the responsibility refactor, the GitHub bootstrap, Slice G, the history
  rewrite, waves 1/2/3/3b/A/4, or waves M1, M2, M3, M4.
- The image-input probes, the image-generation research, the MiMo off-peak verification, or the
  Kimi batch verification.
- **A passing test proves consistency, not truth.** Wave M1 found a green test protecting a false
  catalog claim; wave M2 found two more obsolete assertions passing; **wave M4 found a green test
  suite hiding a halved backoff, because it fed the function a value its real caller could never
  produce.** Only a live call, or an assertion on the real caller, breaks the tie.
- Do not claim Linux support. macOS **is** verified green; no support claim ships without owner
  sign-off.

Newest-first detail is in `00-start-here/BUILD_LOG.md`; decisions in `00-start-here/DECISION_LOG.md`;
the release truth table is `03-execution/RELEASE_CHECKLIST.md`; the SHA map is
`08-decisions-and-risk/HISTORY_REWRITE_2026-07-30.md`.
