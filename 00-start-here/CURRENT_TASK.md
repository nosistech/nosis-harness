# Current Task

## Immediate Goal — M5 scope RATIFIED + `CONTRACTS_M5.md` LOCKED (2026-07-17). NEXT = brief Sol for Slice A.

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

**M4 is CLOSED.** HEAD `6de331a` (docs: close M4) on `d3cac39` (research) on `aa751f4` (Slice D feat).
All four M4 slices committed: A `347bce6` (fleet), B `ecadc0a` (scheduler/ladder/swarm-seam),
C `26c6a22` (nh-mcp), D `aa751f4` (OAuth2, E4). `CONTRACTS_M4.md` LOCKED; §8 has the as-implemented
A-M4-1 clarification. Working tree clean after this session's 3 commits.

The **deep improvement research** is committed (`d3cac39`) — read it before planning M5:
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

### ON RESUME ("continue") — the task is to PLAN M5, not implement:
1. **Read the research report** `00-start-here/RESEARCH_2026-07_harness.md` (esp. §1 top-15, §3 live
   issues, §7 master backlog, §10 sequencing). Confirm M4 clean: `git log --oneline -3` = `d3cac39`
   / `aa751f4` / `bd35b4d`; `git status` clean.
2. **Get owner ratification** of the M5 "Honest Meter" scope above (slice set + order + the CUT). The
   owner wants a *cohesive, logical, congruent, harmonic* project — pressure-test each slice against
   the meter identity and drop anything that doesn't serve it. UX/FEEL is still THE gate.
3. **Write `CONTRACTS_M5.md`** (mirror the M4 contract shape): ground rules, the per-milestone
   mutable-crate surface + amendment list, per-slice specs with exit criteria mapped to real tests,
   a verify-live ledger (carry the report §8 items — DeepSeek peak windows @ `valid_until 2026-07-24`,
   thinking defaults, kimi reasoning replay, cache fields). LOCK it.
4. **Brief Sol for Slice A** (see Executor invocation). Then the loop: Sol implements → Claude re-gates
   (`cargo test --workspace --release` + clippy) + adversarial review → owner FEEL-approve → commit.

## Roles (fixed) — [[m2-m5-codex-sol-directive]]
- **Orchestrator = Opus 4.8** (this session): plans, writes contracts/briefs, runs gates, adversarially
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
