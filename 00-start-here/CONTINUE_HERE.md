# Continue Here — authoritative checkpoint

**Updated:** 2026-07-31 America/Guatemala
**Resume phrase:** `continue`
**Works for:** Codex or Claude in `C:\Users\capv2\Desktop\nosis-Harness`

This file supersedes every older checkpoint and "next task" in `CURRENT_TASK.md`. Historical records
remain useful for provenance, but do not execute their stale instructions.

## THE HEADLINE: three waves shipped. The price treadmill is gone for good. Product work is next.

Waves **M1 (images in)**, **M2 (tool floor)** and **M3 (prices don't expire)** are committed and
gated. The owner's standing instruction as of this session: **all product work from here.**

## First action on `continue`

Read this file in full. Then verify with read-only commands:

```powershell
git status --short                  # expect CLEAN
git log -3 --oneline                # expect the M3, docs, and M2 commits
git log origin/main..HEAD --oneline # expect nothing pending (or push if there is)
git config --local user.email       # expect 98294098+arparvar@users.noreply.github.com
Get-Process -Name nh,codex -ErrorAction SilentlyContinue | Select-Object Id,ProcessName,StartTime
```

Any `codex.exe` you find is probably the OWNER'S on another project — on 2026-07-30/31 two of his
ran the whole session on `super-triplets` and `Ashveil-horror-story`. Check
`~/.codex/sessions` rollout files for `"cwd"` before assuming a codex is yours.

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

- **`2e0fea0` — wave M2 "TOOL FLOOR"**, 9 files, +1317/−14. `write_file` (create-only, refuses an
  existing path and a missing parent), `grep_files` (literal substring, NOT regex — no regex crate
  and the description says so), `glob_files` (sorted output, because `read_dir` order is
  filesystem-dependent and non-determinism poisons the prefix cache).
  **The `.GIT/x` / `.ENV` case-fold bypass is CLOSED** — the WARNING left at `lib.rs:277` for
  exactly this wave is discharged. `creation_guard_verdict` checks the typed path, its folded form,
  and the resolved actual path plus its folded form; `merge_guard_verdict` takes Block > Ask > Allow.
  Two hardenings beyond the brief: `symlink_metadata` makes a destination symlink count as existing,
  and the parent is canonicalized before creation.
  Search consults the law per file and **excludes non-Allow files silently while counting them** —
  no approval storm. Eight honesty counters in the footer. Iterative stack, no symlink following,
  20k-file / 500-match caps, `target`/`node_modules`/`.venv`/`dist`/`build` pruned and **disclosed**.
- **`9939db3` — docs**: M1 + M2 close, six decision entries, `SECURITY.md` rewritten to lead with
  GitHub private vulnerability reporting (verified `enabled` before advertising it) and demote the
  unmonitored email to explicitly best-effort.
- **`223217a` — wave M3 "PRICES DON'T EXPIRE"**, 17 files, +82/−158, a net deletion. See below.

## THE PRICE TOPIC IS CLOSED. DO NOT REOPEN IT.

The owner said three times, with rising frustration, that maintaining prices would keep dragging him
back to the product. **Do not propose a price-freshness feature, a recheck cadence, a CI price
watcher, or a `verified_on`-style date. Do not add `valid_until` back to any catalog block.**

What was deleted: all 12 route-price `valid_until` lines, `PriceQuote.stale`, the staleness
computation, the catalog parsing, `usd_compare_key`'s `stale` parameter, six `*price stale` display
markers, and the `stale` member of the MCP `route_cost` payload. A test **pins the absence** of both,
so an expiry cannot return by accident.

What was NOT deleted: **prices themselves.** Metering, receipts, `nh why` and cheapest-capable
selection are unchanged. `price_confidence` and the per-route first-party citation comments stay.

Also swept — the obligation lived in DOCUMENTS as much as in code, and removing the field alone
would have removed the enforcement while keeping the chore: `RELEASE_CHECKLIST.md` (was a release
blocker, now says in bold it can never block again) and `PROMPT_LIBRARY.md` (told future research
agents to record a `valid_until` — would have rebuilt the machinery), plus `PRODUCT_BRIEF`, `COSTS`,
`ENVIRONMENT`, `VENDOR_MAP`. Historical records left untouched.

**fx staleness is DIFFERENT and stays.** An old price is a number a reader can judge; an old
exchange rate silently mis-converts CNY to USD into a confidently wrong number they cannot judge.
It costs nothing: there is no `[fx]` block in `catalog.toml`, so the path is dormant.

Accepted tradeoff, recorded honestly: receipts carry no freshness signal, so a silent provider price
change is metered wrong until a human notices.

## Remaining work, newest priority first — ALL PRODUCT WORK

1. **NEXT WAVE — "RESILIENCE": retry with exponential backoff.** There is **no HTTP-level retry
   anywhere** on the interactive path. `nh-core/src/wire/http.rs:71` answers a 429 with
   `"rate limited; retry later"` — it tells the HUMAN to retry. The only `Retry` in the tree is
   nh-fleet's task ladder (`model.rs:147`) and a file-lock wait. Live evidence: free
   `glm-4.6v-flash` needed **four manual retries at 6/12/24/48s** during the 2026-07-30 image
   probes. GLM-free is the on-ramp for anyone trying nosis without a credit card, and it currently
   fails and blames the user.
   **Honesty requirement:** a retried call's cost is the SUM of attempts that returned usage;
   attempts with no usage block cost nothing and must be reported as zero, never estimated. The
   receipt should say something like "3 attempts, 2 rate-limited". Research Tier 4 is unbuilt.
2. **`nh resume` + session ledger.** A crashed session loses everything today. Table stakes, and the
   substrate the compaction-receipt work needs. Research Tier 3 is partial: `effective_context`
   exists; five-stage compaction, compaction receipts and the session ledger do not.
3. **Compaction receipts** — show what was dropped and what it saved. The honesty thesis applied to
   context instead of money. Differentiator, not a chore.
4. **Ubuntu test-suite hang — THE blocker for `v0.1.0`.** Linux compiles and lints clean; `cargo
   test` never finishes and hits the 30-minute CI ceiling. Confirmed again 2026-07-31: Windows,
   macOS and Supply-chain all green, **ubuntu-latest cancelled at ~35m for the fifth time**. The
   hanging test is unnamed (GitHub archives no logs for a timed-out job). **Ruled out, do not
   re-investigate:** keyring/D-Bus (the only keyring test is the `#[ignore]`d one) and Unix-generic
   causes (macOS passes). Prime suspects: `nh-fleet`'s `File::try_lock`/flock semantics, or Linux
   process-group timing. **The owner runs the Ubuntu VM himself** (VirtualBox Ubuntu 26.04 Desktop,
   `libdbus-1-dev` required).
5. **Two M1 nits, non-blocking:** image part ordering is emitted text-first but was only measured
   image-first (one sub-cent call settles it); and MiMo's documented 8192-pixel minimum is
   unenforced because the wave does not decode image dimensions.
6. **Cruft, non-blocking:** three `nh-fleet` test fixtures still carry route-price `valid_until`
   lines. Dead keys — serde ignores them. `nh-fleet` is frozen; left for a future sweep.
7. Then: FEEL pass on the current binary, required status checks, final gate, **tag `v0.1.0`**.

## OWNER ACTIONS STILL OWED (neither is blocking)

1. **Branch protection.** Ratified 2026-07-30, still NOT applied — `main` is unprotected. The
   harness classifier blocks the orchestrator from writing GitHub settings, so the owner runs it:
   `gh api -X PUT repos/nosistech/nosis-harness/branches/main/protection --input <json>`
   with force-push and deletion protection, `enforce_admins: true`, and **no required status
   checks** (those wait for tag time, once Ubuntu is green, because required checks and direct
   pushes are the same lever).
2. **Commit-author email — LEAVE IT.** Decided. No action. A second rewrite would invalidate the
   published SHA map.

## Settled — do not reopen

- **The price topic** (see above). This is the strongest one on the list.
- Provider scope is **DeepSeek + Kimi + GLM + MiMo + local**. No Anthropic/OpenAI/Gemini API routes.
- **All 12 catalog routes are `wire = "openai"`.**
- Never add a frontier price row as a `top_tier` anchor.
- Zero-price routes are a selectable tier, not a routing winner.
- **The harness does not auto-route.** The "router inside the harness" moat claim is retired, which
  makes the research's Tier-2 "learning router" **stale as written**.
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

Tier status: Tier 0/1/9 largely shipped; **Tier 2 stale** (assumes auto-routing); Tier 3 partial;
**Tier 4 unbuilt** (this is item 1 above); **Tier 5 and 8 unbuilt**.

## Do not redo

- The five-lane audit, the responsibility refactor, the GitHub bootstrap, Slice G, the history
  rewrite, waves 1/2/3/3b/A/4, or waves M1, M2, M3.
- The image-input probes, the image-generation research, the MiMo off-peak verification, or the
  Kimi batch verification.
- **A passing test proves consistency, not truth.** Wave M1 found a green test protecting a false
  catalog claim; wave M2 found two more obsolete assertions passing. Only a live call breaks the tie.
- Do not claim Linux support. macOS **is** verified green; no support claim ships without owner
  sign-off.

Newest-first detail is in `00-start-here/BUILD_LOG.md`; decisions in `00-start-here/DECISION_LOG.md`;
the release truth table is `03-execution/RELEASE_CHECKLIST.md`; the SHA map is
`08-decisions-and-risk/HISTORY_REWRITE_2026-07-30.md`.
