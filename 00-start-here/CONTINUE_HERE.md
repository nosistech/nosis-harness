# Continue Here — authoritative checkpoint

**Updated:** 2026-07-30 (late session) America/Guatemala
**Resume phrase:** `continue`
**Works for:** Codex or Claude in `C:\Users\capv2\Desktop\nosis-Harness`

This file supersedes every older checkpoint and "next task" in `CURRENT_TASK.md`. Historical records
remain useful for provenance, but do not execute their stale instructions.

## THE HEADLINE: the product moved again. Wave M1 "IMAGES IN" is committed and gated, NOT pushed.

`nh` now accepts image input. HEAD is **`05c53cc`**, working tree **CLEAN**, gate
**`GATE: PASS` — 599 passed / 0 failed / 1 ignored `--release`**, all five steps green.

**⚠ `05c53cc` HAS NOT BEEN PUSHED.** The push was blocked by the harness permission classifier, not
by git. `origin/main` is still at `52314a3`. **First action: ask the owner to run
`git push origin main`** (he can type `! git push origin main` in the prompt), or confirm he wants
it held locally.

## First action on `continue`

Read this file in full. Then verify with read-only commands:

```powershell
git status --short                  # expect CLEAN
git log -1 --oneline                # expect 05c53cc feat(images), or later
git log origin/main..HEAD --oneline # expect ONE commit pending push (05c53cc)
git config --local user.email       # expect 98294098+arparvar@users.noreply.github.com
Get-Process -Name nh,codex -ErrorAction SilentlyContinue | Select-Object Id,ProcessName,StartTime
```

Any `codex.exe` you find is probably the OWNER'S on another project. On 2026-07-30 two of his ran
the whole session on `super-triplets` and `Ashveil-horror-story`. Check
`~/.codex/sessions` rollout files for `"cwd"` before assuming a codex is yours.

## ⚠ EVERY COMMIT SHA CHANGED ON 2026-07-30 (earlier that day)

History was rewritten to remove the owner's personal email from all 74 commits. **No file content
changed.** To resolve ANY old SHA, use the 74-entry map in
`08-decisions-and-risk/HISTORY_REWRITE_2026-07-30.md`. **Do not rewrite history again** without an
explicit owner decision. `git config --local user.email` is now the noreply address — do not reset it.

## Hard constraints

- **Do not commit, push, or tag without owner authorization.**
- **Do not run write-mode `cargo fmt`** as a habit — but note the orchestrator DOES run the scoped
  normalize (`cargo fmt -p <crates>`) after a Sol wave, because Sol is forbidden from formatting and
  `fmt --check` is gate step 1. That is the standing protocol, not a violation.
- **Sol must never run `cargo fmt`.**
- Do not expose MCP publicly. Loopback-only, bearer-guarded, preview.
- Do not delete or truncate `.nosis/` receipts or Fleet state.
- **Do not add dependencies.**
- Never print, persist, or upload credentials.
- **Never run two nosis codexes.**
- Roles: **Sol max = executor** (all code, one wave at a time, via `codex exec`), **Opus 5 =
  orchestrator** (briefs, gates, review, commits; writes no feature code), **Fable 5 = research**.
- **Sol launch shape** — PS 5.1 mangles big multiline args, so the prompt goes via STDIN. Note that
  `Start-Process` and PowerShell background mode are BLOCKED in this harness (EPERM on uv_spawn);
  launch through the **Bash tool with `run_in_background: true`** instead:
  `codex exec -m gpt-5.6-sol -c 'model_reasoning_effort=max' -s workspace-write --color never -C <repo> -o <out>.md < <brief>.txt > <run>.log 2>&1`
- **Write commit messages to a file and use `git commit -F`.** Verify no BOM (first bytes must not
  be `EF BB BF`). Use the Write tool or `[System.IO.File]::WriteAllText` with `UTF8Encoding($false)`.
- Follow root `AGENTS.md`, `05-ai-collaboration/AGENTS.md`, and THE LAW.

## SCOPE BRIEFS FOR SOL: define scope BY CRATE, never by file list

Wave M1 cost **three wasted launches** because the orchestrator enumerated an exact file whitelist
from partial knowledge. Sol correctly stopped clean each time (zero files changed, nothing
fabricated), but each stop cost a full run. The three misses were:

1. Adding a field to `ChatMessage` forces `parts: None` into every existing struct literal —
   `#[serde(default)]` governs deserialization only, NOT Rust struct construction.
2. `crates/nh-cli/src/main.rs` owns all Clap args and dispatch, so any new CLI flag needs it.
3. `crates/nh-routes/src/tests.rs` asserted the very catalog claim the wave was correcting.

**The fix that worked:** name FULLY-IN-SCOPE crates, MECHANICAL-ONLY crates, and a FORBIDDEN list,
then say explicitly "do not stop for a file inside a fully-in-scope crate." Reuse that shape.

## What shipped this session (2026-07-30 late)

**`05c53cc` — wave M1 "IMAGES IN", 26 files, +948/−41, all 11 items, no deferrals.**
- `nh run --image <path>` (repeatable, max 4) and `/image <path>` in `nh chat`; PNG/JPEG.
- `ContentPart { Text, ImageB64 }` + optional `ChatMessage.parts`. Parts-free requests are
  **byte-identical** to before, asserted literally by `parts_free_request_bytes_remain_identical`,
  so the prefix cache and PrefixSeal are unaffected.
- `load_image` in nh-tools: workdir boundary + law guard (same as read_file), PNG/JPEG allowlist,
  **magic-byte verification**, 3.5 MiB raw cap, dependency-free RFC 4648 base64 (spec vectors tested).
- Capability check fails closed **before any HTTP call**, at three independent layers, naming
  image-capable routes read from the live catalog.
- nh-fleet, nh-tui and `wire/anthropic.rs` received a one-line `parts: None` initializer and
  nothing else.

## The three live-verified findings that drove it (do NOT re-derive from docs)

Probed 2026-07-30, total spend well under $0.01. Full detail in the memory file
`multimodal-verified-facts.md`.

| route | text-only | +image | delta |
|---|---|---|---|
| `kimi-k2.6` | 14 | 43 | +29 |
| `mimo-v2.5` | 254 | 272 | +18 |
| `glm-4.6v-flash` | 13 | 40 | +27 |

1. **All three fold image tokens into `usage.prompt_tokens`**; none exposes a separate image-token
   field. `Usage` already parses that, so **receipts are honest for images with zero code change.**
2. **kimi-k2.6 REJECTS bare base64** ("unsupported image url") — the `data:` prefix is MANDATORY.
   **glm-4.6v-flash accepts both.** So always emit the full data: URI; one code path serves all.
3. **Images coexist with a `tools` array** on kimi-k2.6 (verified, prompt=72).

Also measured: free `glm-4.6v-flash` is **heavily rate-limited** (needed 4 retries at 6/12/24/48s).
`temperature` appears nowhere in `crates/` — the harness never sends it.

## A CATALOG LIE WAS FOUND AND FIXED — and a passing test was protecting it

`mimo-v2.5-pro` declared `modality = ["text","image","video","audio"]`. Xiaomi documents it
text-only, and a live probe returned `404 No endpoints found that support image input`. Corrected to
`["text"]` with a dated citation.

**The reason it survived every prior audit:** `nh-routes/src/tests.rs` had a green test literally
named `mimo_routes_preserve_reasoning_and_are_omni_modal` asserting the false claim for both MiMo
routes. Code and test agreed with each other. Only a live call broke the tie. **Lesson: a passing
test proves consistency, not truth.**

## TWO BACKLOG ITEMS WERE REFUTED — do not build them, do not re-plan them

Both were listed as ready-to-build in the July research. Verification killed both.

1. **MiMo off-peak 0.8× — NOT AVAILABLE TO US.** It exists only on the prepaid **Token Plan** as a
   Credits consumption coefficient; both pay-as-you-go pages (EN and zh-CN, dated 2026-07-15)
   contain zero off-peak language. Worse, Token Plan quota is contractually **coding-tools-only** and
   expressly forbids API use by automation scripts and application backends — which is exactly what
   nosis is. Building it would have written a discount into the meter that we never receive.
   `catalog.toml` has no `off_peak` key, so **it is already correct by omission — change nothing.**
2. **Kimi Batch API 0.6× — NOT ADOPTABLE NOW.** `completion_window` has a **12-hour MINIMUM**, so
   every call is a ≥12h async job (upload → submit → poll → download → rejoin by `custom_id`). That
   rules out `nh run`/`nh chat` categorically. And the documented batch `usage` block has **no
   `cached_tokens` field** while batch bills cache-hit ($0.10/1M) vs cache-miss ($0.57/1M) **5.7×
   apart** — cost could only be guessed, which is a REFUSE condition for this product. Two traps if
   ever revisited: the 0.6× multiplier does NOT reconcile for k2.6 cached input (published $0.10,
   not $0.096), and the pricing page lists `kimi-k2.7-code` as batch-eligible while the API guide's
   normative warning says the model must be k2.6 or k2.5. Plausible ONLY for fleet mode, and only
   after a live probe.

**IMAGE GENERATION — researched and DECLINED by the owner.** Only Z.ai can generate
(`glm-image` $0.015/img, `cogview-4` $0.01/img) via `POST /api/paas/v4/images/generations` = **a
third wire**, violating the ratified 2-wire rule. It returns **no `usage` object** (cost would be
fabricated), and Z.ai ToU §III.5(d) puts an **affirmative AI-labelling duty on the operator**. If
ever revisited, the least-damaging shape is an `nh-mcp` tool, keeping the router's wire rule intact.

## OWNER DECISIONS RATIFIED 2026-07-30 BUT **NOT YET EXECUTED**

The owner answered these, then redirected to product work before they were applied. They are
authorized and still owed:

1. **Branch protection** — apply **force-push and deletion protection** to `main` now (NOT required
   status checks; those wait for tag time, once Ubuntu is green, because required checks and direct
   pushes are the same lever). `main` is currently **unprotected**. Nothing was applied.
2. **`SECURITY.md`** — the owner said `info@nosistech.com` is **not reliably monitored**. Reword to
   lead with GitHub private vulnerability reporting (already enabled, the real working path) and
   state the email as best-effort. Also still owed from an older note: `SECURITY.md:57` claims "The
   audit found no critical problems", which was outdated the next day by the 2-critical/14-high
   pre-release audit — verify C-01/C-02 are closed, then name BOTH audits honestly.
3. **Commit-author email — LEAVE IT.** Decided. No action. Do not reopen; a second rewrite would
   invalidate the published SHA map.

## Remaining work, newest priority first

1. **Push `05c53cc`** (owner action) and confirm CI.
2. **Docs-close for wave M1** — `BUILD_LOG.md` was deliberately not updated by Sol (out of its edit
   scope). A `DECISION_LOG` entry is owed for: image-generation declined, MiMo off-peak refuted,
   Kimi batch refuted. The owner wants every ratified decision logged the session it happens because
   the log feeds the launch articles.
3. **NEXT PRODUCT WAVE — the tool floor.** The agent has exactly three abilities: `read_file`,
   `edit_file`, `exec_shell` (`nh-tools/src/lib.rs:530`). **It cannot create a new file** —
   `EditFile` only mutates existing ones (see the comment at `lib.rs:267`) — and it cannot search,
   so every grep costs an approval-gated shell call. Add `write_file`, `grep_files`, `glob_files`.
   **SECURITY-CRITICAL, already designed:** `lib.rs:272` carries an explicit WARNING that a
   file-CREATION tool can bypass `.git/**` and `**/.env*` via `.GIT/x` and `.ENV`, because a path
   that does not exist yet cannot be canonicalized to its true on-disk case. Ratified fix: check the
   typed name AND the case-folded name against the law, refuse if either blocks. (This trap does NOT
   affect `load_image`, which only reads existing files.) Ratified too: `write_file` is
   **create-only** — refuse if the file exists, refuse if the parent dir is missing; and the search
   tools consult the guard per file and silently EXCLUDE non-Allow files while reporting an honest
   excluded count, rather than firing an approval storm.
4. **Two nits from M1, non-blocking:** part ordering is emitted text-first but was only measured
   image-first (one sub-cent call settles it); and MiMo's documented 8192-pixel minimum is
   unenforced because the wave does not decode image dimensions.
5. **Ubuntu test-suite hang** — still THE blocker for `v0.1.0`. Linux compiles and lints clean;
   `cargo test` never finishes and hits the 30-minute CI ceiling. The hanging test is unnamed
   (GitHub archives no logs for a timed-out job). **Ruled out, do not re-investigate:** keyring/D-Bus
   (the only keyring test is the `#[ignore]`d one) and Unix-generic causes (macOS passes). Prime
   suspects: `nh-fleet`'s `File::try_lock`/flock semantics, or Linux process-group timing. **The
   owner runs the Ubuntu VM himself** (VirtualBox Ubuntu 26.04 Desktop, `libdbus-1-dev` required).
6. **All 14 price blocks carry `valid_until = "2026-08-02"`** — two days out. **Two are already
   re-verified**: MiMo pay-as-you-go matched first-party exactly on 2026-07-30
   ($0.0036/$0.435/$0.87 pro, $0.0028/$0.14/$0.28 non-pro, page dated 2026-07-15). Cautions:
   UltraSpeed pricing appears only on marketing pages with no update date — do not catalog it; and
   tax treatment is undocumented, so claim neither inclusive nor exclusive.
7. Then: FEEL re-pass on the current binary, required status checks, final gate, **tag `v0.1.0`**.

## Settled — do not reopen

- Provider scope is **DeepSeek + Kimi + GLM + MiMo + local**. No Anthropic/OpenAI/Gemini API routes.
- **All 12 catalog routes are `wire = "openai"`** — zero Anthropic-wire routes remain after wave 3b.
- Never add a frontier price row as a `top_tier` anchor.
- Zero-price routes are a selectable tier, not a routing winner.
- **The harness does not auto-route.** The "router inside the harness" moat claim is retired. This
  makes the research's Tier-2 "learning router" **stale as written** — it must be re-scoped as
  evidence-you-read before anyone builds it.
- Do not add a quality axis to routing.
- Do not change the edit format, add tools beyond the tool floor above, restructure the prompt,
  lower temperature, or use grammar-constrained decoding.
- **ASD-STE100 is scoped, not repo-wide**: user-facing and safety-critical text only. Describe it as
  "STE-informed", never claim conformance.

## TREAT THE ~90-ITEM RESEARCH BACKLOG AS LEADS, NOT SPECIFICATIONS

`00-start-here/RESEARCH_2026-07_harness.md` (~90 items, 10 tiers) plus the 14 raw files in
`04-research/_harness-research-2026-07/` are excellent but are **July-2026 leads**. On 2026-07-30,
three of its items were taken to verification and **two were refuted outright** (MiMo off-peak, Kimi
batch) while a third (multimodal) needed five corrections before it was safe to build. Verify every
item against first-party docs AND a live probe before briefing it.

Tier status against the current tree: Tier 0/1/9 largely shipped; **Tier 2 stale** (assumes
auto-routing); Tier 3 partial (`effective_context` exists; five-stage compaction, compaction
receipts, session ledger/`nh resume` do not); **Tier 4 unbuilt** (no retry/backoff anywhere — the
GLM 429 storm on 2026-07-30 is live evidence this matters); **Tier 5 unbuilt**; **Tier 8 unbuilt**.

## Do not redo

- The five-lane audit, the responsibility refactor, the GitHub bootstrap, Slice G, the history
  rewrite, waves 1/2/3/3b/A/4, or wave M1.
- The image-input probes, the image-generation research, the MiMo off-peak verification, or the
  Kimi batch verification. All four results are recorded above.
- Do not claim Linux support. macOS **is** verified green; no support claim ships without owner
  sign-off.

Newest-first detail is in `00-start-here/BUILD_LOG.md` (M1 docs-close still owed); the release truth
table is `03-execution/RELEASE_CHECKLIST.md`; the SHA map is
`08-decisions-and-risk/HISTORY_REWRITE_2026-07-30.md`.
