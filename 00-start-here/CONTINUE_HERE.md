=====================================================================
STATUS UPDATE — 2026-07-26
=====================================================================
The incoming orchestrator completed the requested whole-project analysis, public-v0.1 hardening,
Telegram removal, and responsibility-boundary modularization, committed at `9856ebf`. The complete
local release gate and optimized-binary smokes pass: 512 passed / 0 failed / 1 ignored. The `0.1.0`
changelog is rolled. Remaining before `v0.1.0`: the owner's subjective terminal FEEL pass, the
intended public GitHub remote and repository controls, green Windows/Ubuntu/macOS/supply-chain CI,
then tag and publication. Read the newest BUILD_LOG entry for exact evidence. The remainder of
this file is preserved as pre-gate historical context; its old "do not commit" and blocker
statements are no longer current instructions.

You are taking over as ORCHESTRATOR of the nosis Harness project (repo: C:\Users\capv2\Desktop\nosis-Harness).
The previous orchestrator (Claude Opus 5) handed off on 2026-07-26. Read this whole brief before acting.

=====================================================================
YOUR FIRST TASK: ANALYZE THE WHOLE PROJECT (owner-directed)
=====================================================================
Before touching anything, produce a full analysis of the project. Do NOT write code, do NOT commit,
do NOT run cargo fmt. Read widely first. The owner wants your independent read of where this stands.

Cover, with evidence (file:line, commit SHA, or test output for every claim):
  1. ARCHITECTURE - the 9 crates (nh-cli, nh-core, nh-routes, nh-law, nh-tools, nh-mcp, nh-tui,
     nh-fleet, nh-vault), their dependency edges, and whether the layering is still clean.
  2. THE LAW conformance - the ten words are: small, simple, secure, safe, lightweight, readable,
     auditable, modular, congruent, harmonic. Where does the code violate them? Be specific.
     Known offender: crates/nh-tui/src/lib.rs is ~5K lines (this is the deferred "Slice H" work).
  3. RELEASE READINESS for v0.1.0 - what actually blocks the tag, ranked.
  4. SECURITY POSTURE - two audits were run (a Fable 5 full audit, and a Sol-max pre-release audit
     that found 2 CRITICAL / 14 HIGH and said "not releasable"). Slice G remediated all of it across
     7 waves. Your job: verify the criticals C-01 and C-02 are ACTUALLY closed in the current tree,
     not just claimed closed. Do not trust the record - re-derive it.
  5. WHAT THE PREVIOUS ORCHESTRATOR MAY HAVE GOTTEN WRONG. Be adversarial. It made at least one
     confirmed error this session (claimed CI verified Linux; there is no git remote so CI has never
     run even once). Assume there are others.
  6. RISKS the project is not currently tracking.

Deliver the analysis as a written report to the owner. Then STOP and await direction.

=====================================================================
STATE OF THE TREE - READ THIS CAREFULLY
=====================================================================
HEAD = 5496b44 on branch main. There is NO git remote (local only - this matters, see below).

The working tree holds ONE BIG UNCOMMITTED LUMP that must be committed as a single coherent commit:
  - M5 Slice F Wave 4 "SURFACES" (7 source files, implemented 2026-07-21, never committed)
  - Slice G audit remediation, ALL 7 waves W1-W7 (implemented 2026-07-22 .. 2026-07-25)
  - This session's documentation corrections (2026-07-25/26, listed below)

NOTHING MAY BE COMMITTED until: FEEL gate passes + Linux verified + owner approves. The owner was
explicit: "we will not commit until we do all of these."

GATE STATUS: PASS - 497 passed / 0 failed / 1 ignored, --release.
gate.ps1 is 4 steps: cargo fmt --check, clippy -D warnings, cargo deny check, cargo test --release.
NO CODE WAS CHANGED after that gate run (2026-07-25), so the result still stands. If you change any
code you MUST re-run it: Set-Location "C:\Users\capv2\Desktop\nosis-Harness"; .\gate.ps1 *> Temp\<name>.log
The gate log is UTF-16 - Bash grep CANNOT read it. Use PowerShell Select-String or a file-read tool.
The real verdict is the "GATE: PASS/FAIL" line, NOT the wrapper exit code.

=====================================================================
WHAT THE PREVIOUS SESSION COMPLETED (2026-07-25/26)
=====================================================================
1. W7 "EXEC BOUNDARY" ADVERSARIAL REVIEW - DONE, ZERO BLOCKING. This was the last unreviewed wave.
   Verified independently (not taken on trust):
     - nh_law::exec_verdict (crates/nh-law/src/lib.rs:139) has NO Allow branch - only Block or Ask.
     - All 4 surfaces route Access::Exec through it: cmd_chat.rs:182, cmd_run.rs:140,
       nh-fleet/src/lib.rs:1487, nh-tui/src/worker.rs:221.
     - ToolCtx::new default guard returns Guard::Ask for Exec (nh-tools/src/lib.rs:73).
     - Therefore H-05 (exec requires approval on EVERY non-Block verdict) is behaviorally INERT for
       every shipped surface, and closes the boundary against a future permissive guard.
     - H-06: no unbounded child.wait() remains in nh-tools (only try_wait() at :596 and :717).
     - The drain thread never holds the mutex across a blocking read (guard is a loop-body local).
   TWO NON-BLOCKING NITS, deliberately left unfixed - your call whether they matter:
     a) DrainHandle::finish takes a shared deadline but a separate `grace` used only for the LABEL.
        If stdout consumes the full 5s, stderr's marker still says "capture incomplete after 5s"
        even though it waited ~0s. Fail-safe (can only over-warn) but slightly imprecise.
     b) status.expect("normal command completion includes an exit status") is a new panic path.
        Provably unreachable (termination.is_none() <=> status.is_some() in the same loop).
   NOTE: `git diff crates/nh-tools/src/lib.rs` is CUMULATIVE from HEAD. The "+442/-77" recorded in
   older notes as "W7's diff" actually also contains Slice G W3's EditFile atomic-write work. Fix
   this in the commit message.

2. THE 27 OPEN QUESTIONS - ANSWERED. The owner said "I don't remember any, to be honest" and asked
   for reasoned answers so the project could move forward. Handled in
   08-decisions-and-risk/decisions/OPEN_QUESTIONS_RATIONALE.md, which was rewritten from a question
   list into an answered record.
   *** CRITICAL CONVENTION YOU MUST PRESERVE ***
   Every reconstructed answer is explicitly labelled "Rationale (reconstructed 2026-07-25)" and
   states it is NOT a memory and NOT a transcript. This is deliberate: nosis's entire product thesis
   is that it never fabricates (the meter prints "unpriced" not 0.00; nh why refuses to compare CNY
   to USD; W7 exec reports "could NOT be killed" rather than claiming a kill). A decision log with
   invented deliberation would fail the exact test the product passes. The five DECISIONS_*.md files
   remain a PURELY SOURCED record; reconstructions are quarantined in the one file so the boundary
   between evidence and inference stays legible. DO NOT merge them.

3. FOUR OWNER RULINGS, all executed:
   - RISK_REGISTER.md: struck the false "Job Objects + restricted tokens" Windows containment claim.
     Job Objects were REJECTED 2026-07-24 (they need `unsafe`, which would break the workspace-wide
     unsafe_code="forbid" from d1f9ad0); restricted tokens were never implemented. Replaced with the
     containment that actually exists. THREE MORE false rows in the same file were fixed as the same
     class of defect: "no direct-to-main" (never adopted), "Pin SDK to frozen RC + conformance check
     in CI" (there is no SDK - the client is hand-rolled in nh-tools/src/mcp.rs and the server is
     tiny_http - and no such CI job exists), and two delegate-quota rows for a feature cut from v1.
   - PRODUCT_BRIEF.md:9 - replaced the superseded "subscription delegates" pitch with the shipped
     positioning, with a dated amendment note.
   - CONTRACTS_M5.md Slice E - dated amendment: the wip/<slice> branch rule was NEVER ADOPTED
     (git log shows direct-to-main throughout); superseded by the gate + per-wave adversarial review.
   - BUILD_LOG.md - backfilled the missing M5 Slice A, C, E close-out entries from commit records
     (9c96259, a0a4036, bc2a1b1, a71eb23, 70a2f9d, 3a5df91, 0c14743). Three details are explicitly
     marked "not recorded at the time" rather than guessed.
   Also: CONTRACTS_M3.md 9.2 got a dated retroactive amendment recording that Slice F removed mouse
   capture (commit 3fcd00e) so native click-drag copy works; wheel-scroll loss is covered by the
   keyboard scroll from Slice E. And AGENTS.md:20 + CODEX.md:7 were corrected from the stale
   "Terra by default" wording to gpt-5.6-sol at max (superseded 2026-07-13).

4. AUDIT NIT N-01 (nh mcp serve banner) - ALREADY FIXED, VERIFIED. The record claimed the banner
   still hints only the old 3 MCP tools. FALSE. crates/nh-mcp/src/lib.rs:162 already lists all six
   (route_resolve, fleet_run, fleet_status, why, route_cost, receipts). At HEAD it listed 3; a Slice
   G wave fixed it. NO code change was made. This is why the owner insists: verify, never assume.

5. TRANSCRIPTION AUDIT - the two hand-transcribed decision files (DECISIONS_RELEASE-SLICE-G.md and
   DECISIONS_STANDING.md) were verified against their source drafts in Temp: VERDICT FAITHFUL on
   both, zero factual drift across every SHA, gate count and constant. That caveat is now CLOSED.

=====================================================================
*** MAJOR FINDING - THE PROJECT HAS NEVER BEEN BUILT OR TESTED OFF WINDOWS ***
=====================================================================
`git remote -v` returns NOTHING. This repo is local-only. Therefore .github/workflows/ci.yml
(added in d1f9ad0, matrix windows-latest + ubuntu-latest, plus a cargo-deny job) HAS NEVER RUN. Not
once. The previous orchestrator initially told the owner "CI verifies Linux every push" - that was
FALSE and was corrected.

TRUE platform status:
  Windows - built, 497/0/1 green, live-verified against 4 real providers (GLM/DeepSeek/Kimi/MiMo).
  Linux   - #[cfg(unix)] paths exist and are written, but NEVER built, NEVER tested, NEVER run.
  macOS   - code paths exist (crates/nh-cli/src/cmd_key.rs:8), NEVER built, NEVER tested.
The vault is cross-platform by design (nh-vault/src/lib.rs:19 - Windows Credential Manager / macOS
Keychain / Linux secret-service via the `keyring` crate) with an env fallback NH_<ENTRY>_KEY for
headless boxes. But none of the non-Windows paths have ever executed.

=> NO CLAIM OF LINUX OR MACOS SUPPORT MAY SHIP UNTIL THIS IS ACTUALLY TESTED.

PLAN AGREED WITH THE OWNER:
  - Linux: the owner is downloading Ubuntu 26.04 LTS Desktop (5.9GB) and will run it in VirtualBox
    7.2.14. Agreed VM config: 6 GB RAM / 4 vCPU / 60 GB dynamic disk / 3D acceleration OFF /
    VMSVGA 128MB. Reason for Desktop over Server: one VM covers BOTH credential paths - the GUI
    console session has gnome-keyring/secret-service (desktop path), while an SSH session has no
    D-Bus and exercises the NH_<ENTRY>_KEY env fallback (VPS path).
    Networking: NAT + port forward host 127.0.0.1:2222 -> guest :22 (bridged fails on many Wi-Fi
    adapters). Guest packages needed: openssh-server build-essential pkg-config libdbus-1-dev git
    curl, then rustup. libdbus-1-dev is REQUIRED - the keyring crate's Linux backend won't link
    without it. rust-toolchain.toml pins 1.96.0 and rustup honors it automatically.
    Next step there: owner provides the VM username; orchestrator generates an SSH key on the
    Windows host, owner pastes the pubkey into the VM, then the orchestrator drives it over SSH.
    Build parallelism must be capped (cargo build -j 4) so the OOM killer doesn't kill rustc -
    rule: cores <= RAM/2GB.
  - macOS: do NOT attempt macOS in VirtualBox (Apple's EULA permits macOS virtualization only on
    Apple hardware, and VirtualBox has no supported macOS guest). The agreed path is GitHub Actions
    `macos-latest` runners - real Apple hardware, legal, free for public repos, and a one-line
    change to the matrix at .github/workflows/ci.yml:17. This also finally makes the Windows/Linux
    CI actually execute.
  - NOTE: the owner has a Hostinger VPS but prefers all Linux testing in VirtualBox.

HOST MACHINE FACTS (for VM sizing): Intel Core Ultra 9 275HX, 24 physical cores / 24 logical,
31.4 GB RAM total but only ~9.7 GB FREE, 471 GB free on C:. Memory Integrity / VBS is RUNNING
(VirtualizationBasedSecurityStatus=2, services 2,3,4), so VirtualBox falls back to the Hyper-V
backend and will be slow. The owner was advised to LEAVE Memory Integrity ON - this machine holds
his provider API keys, and trading a real kernel exploit protection for a faster one-off compile is
a bad trade. Do not quietly reverse that.

=====================================================================
WHAT REMAINS BEFORE THE v0.1.0 COMMIT AND TAG
=====================================================================
BLOCKING:
  1. FEEL GATE - NOT DONE. This is the owner's judgement call on how the TUI/CLI feels; tests
     passing is necessary and not sufficient. A ready-made 15-step script exists at
     C:\Users\capv2\AppData\Local\Temp\feel-gate.ps1 and a Windows Terminal window was opened
     running it. Steps A1-A11 (TUI) use the FREE route glm-4.7-flash so they cost $0.00; B1-B4 are
     CLI. It covers: frame/mojibake, honest $0.00 meter, live slash menu, /why, /model switch with
     history preserved, scroll-while-generating, exec approval ALWAYS appearing (W7), Ctrl+A/Y/N
     must NOT approve (medium-12 modifier gate), narrow-resize no-clip, quit-while-generating must
     not hang (W2 H-02 bounded shutdown), clean terminal restore after quit (H-01), and the 6-tool
     MCP banner. RELAUNCH IT for the owner - do not just tell him to test, actually open a window:
       Start-Process wt.exe -ArgumentList 'new-tab','--title','nosis FEEL gate','-d',
         'C:\Users\capv2\Desktop\nosis-Harness','powershell.exe','-NoExit','-ExecutionPolicy',
         'Bypass','-File','C:\Users\capv2\AppData\Local\Temp\feel-gate.ps1'
     The owner complained (justifiably) that previous sessions asked him to test without ever
     putting a window in front of him. Do not repeat that.
  2. LINUX VERIFICATION on the Ubuntu 26.04 VM (above).
  3. LIVE finish_reason VERIFICATION - owner approved, costs ~1 cent total. Slice G W5 added
     classify_finish_reason (normal/absent -> Pass, length/max_tokens -> Partial, content_filter ->
     Fail+Filtered, unknown -> Partial). The "Normal" set {"", stop, end_turn, stop_sequence} was
     INFERRED, never observed. Run one tiny prompt per provider and record the REAL strings before
     committing. Keys are in the OS vault, env fallback NH_<ENTRY>_KEY. HARD CAP <$2/provider.
     Real routes (from catalog.toml): deepseek-v4-flash, deepseek-v4-pro, kimi-k2.6, kimi-k2.7-code,
     kimi-k2.7-code-highspeed, mimo-v2.5, mimo-v2.5-pro, glm-5.2, glm-4.7-flash (FREE),
     glm-4.6v-flash, glm-4.5-flash, plus -anthropic wire variants of the deepseek routes.
  4. SECURITY.md REWRITE - SECURITY.md:57 says "The audit found no critical problems". Written
     2026-07-20 (b43b023) about the Fable 5 audit; the Sol-max pre-release audit ran the NEXT DAY
     and found 2 CRITICAL / 14 HIGH, verdict "not releasable". Before tagging: verify C-01 and C-02
     are genuinely closed by Slice G, then rewrite that sentence to name BOTH audits, their real
     findings, and the remediation commit. Do not tag with that sentence as-is.

NON-BLOCKING but wanted before/with the tag:
  5. Build 00-start-here/DECISION_LOG.md as an INDEX. Mechanically derivable: grep the five
     08-decisions-and-risk/decisions/DECISIONS_*.md files for '^## ' headings and '**Article angle:**'
     lines, pair them, emit a newest-first table (Date | Decision | Era file | Article angle),
     ~76 rows. Then add a "Review later" section, a STALE CLAIMS section, and a link to
     OPEN_QUESTIONS_RATIONALE.md. CRITICAL: preserve the existing 85-line DECISION_LOG.md content
     verbatim under "## Pre-M2 entries (original format, preserved)". Both DECISIONS_RELEASE-SLICE-G.md
     and DECISIONS_STANDING.md currently contain a forward-reference to this index that does not yet
     resolve, plus ~11 dangling "(see UNSOURCED)" cross-refs that should now point at
     OPEN_QUESTIONS_RATIONALE.md.
  6. Roll CHANGELOG [Unreleased] -> [0.1.0].
  7. Decide the README platform-support wording. Owner-agreed principle: under-claim. Say
     Windows-verified; say Linux only once it is actually tested; say macOS untested (or omit).

OWNER DECISIONS ALREADY RATIFIED - DO NOT REOPEN:
  - TAG v0.1.0 FIRST. Slice H (splitting the ~5K-line nh-tui/src/lib.rs and other god-modules) is
    DEFERRED to 0.1.1. Rationale: the tree is gate-green and audited now; a large mechanical
    refactor before a tag adds risk for zero user-visible benefit.
  - MCP MUST NOT GO PUBLIC before the MCP final spec lands 2026-07-28. `nh mcp serve` stays a
    loopback-only (127.0.0.1) preview. This does not block a CLI/TUI release as long as no docs
    promote it.
  - v0.1.0 is a SOURCE-INSTALL tag (build from source). Packaged installers are a later, optional M7.
  - LICENSE is MIT (c) nosistech LLC, not Carlos personally.

=====================================================================
STANDING PROJECT RULES - THESE ARE HARD
=====================================================================
  - THE LAW (ten words): small, simple, secure, safe, lightweight, readable, auditable, modular,
    congruent, harmonic. UX/UI is the top priority, with drop-if-hard as the escape valve.
  - EXPLAIN-THEN-DECIDE: every options-presentation you give the owner MUST carry your
    recommendation and WHY, plus the tradeoff. Never a bare menu. Never decide silently. This is a
    hard project-wide rule from 2026-07-23.
  - EVERY decision gets logged the session it is ratified, with rejected alternatives + immediate
    and long-term consequences + evidence. The decision record is the source material for the
    launch articles.
  - NEVER FABRICATE. No invented numbers, no claimed verification that did not happen, no
    "should work". If you did not run it, say you did not run it.
  - NEVER run `cargo fmt`. Formatting is the gate's job; the orchestrator runs a scoped
    `cargo fmt -p <crate>` only to normalize drift AFTER an implementer wave, then re-gates.
  - NEVER two nosis codex sessions at once. The owner runs his own codex/TUI sessions as separate
    PIDs - check `Get-Process codex` and LEAVE HIS ALONE.
  - Kill any running nh.exe before a build (it locks target\release\nh.exe).
  - Temp\ means C:\Users\capv2\AppData\Local\Temp\ - it is NOT a repo directory.
  - A foreground PowerShell is not necessarily rooted in the repo - Set-Location first, or pass
    --manifest-path C:\Users\capv2\Desktop\nosis-Harness\Cargo.toml.
  - Identity: apply the honest-identity constitution at EVERY agent surface. DeepSeek and MiMo will
    self-identify as "Claude" if not corrected (training contamination); Kimi is honest.
  - The owner uses a ~250k context checkpoint protocol: at that point, STOP and hand back a
    self-contained continuation prompt so he can clear and resume with "continue".

=====================================================================
KEY FILES
=====================================================================
  00-start-here/CURRENT_TASK.md ............ canonical resume file; newest checkpoint at the top
  00-start-here/BUILD_LOG.md ............... newest-first build record
  00-start-here/AUTONOMOUS_HANDOFF.md ...... role directives
  CONTRACTS_M1..M5.md (repo root) .......... milestone contracts + dated amendments
  04-research/AUDIT_2026-07-21_sol-max_pre-release.md ... the audit Slice G remediates
  08-decisions-and-risk/decisions/ ......... 5 sourced decision files + OPEN_QUESTIONS_RATIONALE.md
  08-decisions-and-risk/RISK_REGISTER.md ... corrected 2026-07-25
  gate.ps1 ................................. the 4-step gate
  catalog.toml ............................. routes, prices, fx
  Temp\feel-gate.ps1 ....................... the ready-to-run FEEL script
  Temp\sol_wave7_prompt.txt / _out.md ...... the last implementer wave's brief and self-report

Begin with the ANALYSIS task at the top. Report to the owner, then stop.
