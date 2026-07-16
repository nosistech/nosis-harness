# Current Task

## Immediate Goal — M4 Slice C (nh-mcp server, E3). Slices A + B DONE + committed.

**M4 IN PROGRESS. HEAD `ecadc0a` = M4 Slice B committed** (Slice A `347bce6`, docs `5889fb7`).
`CONTRACTS_M4.md` is **LOCKED** (owner scope-approved 2026-07-15). Tree clean, **284 pass / 1 ignored**
`--release`, clippy `--release -D warnings` clean, frozen crates + catalog.toml untouched. Slice B FEEL
(escalation ladder climb + off-peak parking) owner-approved through the real `nh` binary.

### Owner scope rulings (baked into CONTRACTS_M4.md — do not relitigate):
1. **OAuth2 in FROZEN nh-tools authorized** — amendment **A-M4-1** (+ nh-vault keyring setter
   **A-M4-2**). The ONLY frozen-crate writes in M4; every other frozen need STOPS for an amendment.
2. **Opus 4.8 gate = review-pause** (no live delegate / no `claude -p`).
3. **nh-mcp HTTP server = `tiny_http`** (blocking, no tokio).
4. **Kimi swarm = MINIMAL seam + verify-live** (Carlos: "don't overdo it, budget").

### M4 slice status (spec = `CONTRACTS_M4.md`):
- **Slice A ✅ DONE + committed `347bce6`** — crate `crates/nh-fleet`: fsync-durable append-only
  ledger + std-thread workers + idempotent resume + budget stop; `nh fleet run/resume`. **E1 (kill-9
  idempotent resume) GATED** via a real-binary `Child::kill` integration test. Frozen crates untouched.
- **Slice B ✅ DONE + committed `ecadc0a`** — off-peak scheduler (injected `Clock` + pure
  `ready_to_dispatch` reusing frozen `price_at`; peak tasks park, 100ms coordinator tick; **E2 gated**
  via injected `MockClock`) + escalation ladder (pure `next_step`; live-wired Flash→K2.7→V4-Pro/High→
  V4-Pro/Max→**Opus review-pause GATE**, ≤2 tries/tier, typed `TaskEscalated` + preceding `TaskReceipt`;
  **resume self-derives the ladder from a `RunStarted.escalate` flag + `ladder_position` and continues
  the climb**) + **MINIMAL** Kimi swarm seam (`Backend{Native,KimiSwarm}` + `SwarmClient`; Native done,
  KimiSwarm mock test + honest `PendingSwarmClient` "arrives live in M6" stub). Frozen crates untouched.
- **Slice C — NEXT** — nh-mcp server (`tiny_http`, stateless 2026-07-28 wire mirroring `nh_tools::mcp`; tools
  `route_resolve`/`fleet_run`/`fleet_status`; **E3** test drives it with `nh_tools::mcp::McpClient` as
  the KORVIN stand-in; `run_id` = stateless passthrough handle). Do NOT ship publicly pre-2026-07-28.
- **Slice D** — OAuth2 MCP client (amendment **A-M4-1/2**; refresh on expiry/401 + retry once; keyring
  token store; **E4** — force expiry, assert refresh succeeds). Replaces the `oauth2_is_deferred_to_m4` test.

### ON RESUME ("continue"):
1. **Sanity:** `git log --oneline -1` = `ecadc0a`; clean tree; kill any `nh.exe` (locks the debug exe);
   `cargo test --workspace --release` (**284 pass / 1 ignored**) + `cargo clippy --workspace --all-targets
   --release -- -D warnings` clean. Use `--release` — Kaspersky blocks the debug test exe (os error 5).
2. **Brief Sol on Slice C** per `CONTRACTS_M4.md §"Slice C"` (executor invocation below). nh-mcp =
   `tiny_http` (blocking, no tokio); stateless wire mirroring `nh_tools::mcp` (NEVER an `Mcp-Session-Id`
   header); tools `route_resolve`/`fleet_run`/`fleet_status`; bind `127.0.0.1` + preview banner; **do NOT
   ship publicly pre-2026-07-28**. E3 test uses the EXISTING `nh_tools::mcp::McpClient` as the KORVIN
   stand-in. `fleet_run` reuses the Slice-A/B `nh_fleet::run` on a background thread; `run_id` = stateless
   ledger handle; `fleet_status` reads the ledger. Gate empirically (numstat = truth; EOL/CRLF = noise),
   adversarial review, show Carlos the FEEL of the nh-mcp responses, then commit on `main` **after Carlos
   approves**. After Slice C: Slice D (OAuth2, A-M4-1/2, E4).

### UX is THE priority — still governs M4/M5/M6 (see [[ux-first-and-the-law]])
"Pretty but frustrating" = failure. Judge by FEEL first, tests second. Reference bar = CodeWhale.
Self-teaching, no handholding, delightful for small tasks.

### Carlos's binding UX decisions (2026-07-14/15)
- **Framed + chat transcript** (Slice D): bordered outer frame, `❯ you` / `◆ nosis` roles, turn
  separation, framed centered modals (no bleed), welcome, key-hint strip.
- **Slash commands** (Slice E): type freely; `/` opens a live command menu. `/help /trust /timeline
  /model /provider /effort /quit`. NO bare-letter shortcuts (they collided with typing).
- **Copy over scroll** (Slice F): remove mouse capture so native select/copy works; scroll is
  keyboard-only (`↑↓`/PageUp/PageDown/End) + `↑ more`/`↓ more` hints. **Fix paste** (bracketed paste).
- **Live controls surfaced**: `/model`/`/provider` switch preserves history (only cache warmth
  resets); `/effort none|low|high|max` sets DeepSeek thinking; header shows route + effort.
- **Honest identity**: system prompt says "You are nosis … running on <route> … never claim to be
  Claude" — because DeepSeek V4 Flash self-IDs as Claude (training contamination; routing verified
  = deepseek-v4-flash via receipts, NOT misrouting).

### Environment gotchas (bit us this session)
- A running `nh.exe` LOCKS `target\debug\nh.exe` → `cargo build/test` link fails. Kill it first.
- Bash tool `cd` PERSISTS across calls → always `cd /c/Users/capv2/Desktop/nosis-Harness` or use
  absolute paths (a stray `cd crates/...` caused a false "nh.exe missing").
- PowerShell reads UTF-8 files as the OEM codepage → box-drawing/`—` look like mojibake in probes;
  check raw BYTES (UTF-8 seq) not the decoded string before believing a glyph is "missing."
- TUI window launch that WORKS: `Start-Process -FilePath <root>\target\debug\nh.exe -ArgumentList
  tui -WorkingDirectory <root>` (the `wt.exe … cmd /k` wrapper silently failed to run nh).
- Kaspersky can block the `wire_clients` test exe (os error 5 / LNK1104) — AV/env, not code.

### Claude Code side (NOT nosis — why Carlos restarted)
Carlos ran Claude Code in a **classic PowerShell console** in fullscreen (`"tui": "fullscreen"`):
Backspace deleted whole words (legacy-console key encoding) and mouse select needed Shift (fullscreen
TUI captures the mouse). Neither is live-fixable in a running process. I set **Windows Terminal as
default terminal** (`HKCU:\Console\%%Startup` DelegationConsole `{2EACA947-7F5F-4CFA-BA87-8F7FBEEFBE69}`
/ DelegationTerminal `{E12CFF52-A866-4C77-9A90-F570A7AA2C6B}`) so relaunching (`claude --continue`)
opens in WT with correct Backspace + copy. Carlos was annoyed it didn't fix the LIVE window — **he
may ask to revert that reg change** (reversible in Settings → Default terminal). He restarted to get
into WT.

## Roles (fixed)
- **Orchestrator = Opus 4.8** (this session): plans, writes contracts/briefs, runs gates,
  adversarially reviews, commits, docs. Does NOT hand-write milestone code. [[m2-m5-codex-sol-directive]]
- **Executor = GPT-5.6 Sol xhigh** via `codex exec` — writes all milestone implementation.

## Executor invocation (proven)
```
codex exec --skip-git-repo-check -s workspace-write -m gpt-5.6-sol \
  -c model_reasoning_effort=xhigh "$(cat /c/Users/capv2/AppData/Local/Temp/<brief>.txt)" < /dev/null
```
Run in background (harness-tracked); verify empirically after (numstat = truth; EOL flags = noise).
Do NOT start a second codex on nosis while one writes nosis. STOP (don't fall back to Terra) if
gpt-5.6-sol stops resolving or Sol fails the same gate twice.

## M3 slices (COMMITTED in `3fcd00e`, 2026-07-15 — full detail in BUILD_LOG)
- **Slice D** — framed panels + chat transcript + anti-bleed modals + welcome + key-hint strip (CONTRACTS_M3 §8).
- **Slice E** — slash commands + live `/model`/`/provider` (history preserved) + `/effort` + keyboard
  scroll + overflow hints + honest identity + mojibake fix (§9).
- **Slice F** — mouse capture removed (native click-drag copy, no Shift) + bracketed-paste fix
  (multi-line → one line, never auto-dispatches; `DisableBracketedPaste` in the panic-safe restore).
- All verified 261 pass / 1 ignored, clippy clean; orchestrator-stress-tested; FEEL-approved by Carlos.

## Do Not Do
- Do NOT commit milestone work until Carlos approves the FEEL (UX is the gate, not "tests pass").
- Do NOT hand-write milestone code — Sol implements; Claude plans + gates.
- Do NOT touch frozen crates (nh-core/nh-tools/nh-law/nh-routes/nh-vault) without a logged,
  pre-authorized CONTRACTS amendment.
- Do NOT change Carlos's Claude Code settings without asking (he was burned once already).
- Do NOT ship the nh-mcp server publicly before the MCP final spec lands (2026-07-28).
