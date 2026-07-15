# Current Task

## Immediate Goal — M4: Fleet + swarm + scheduler + nh-mcp server (M3 CLOSED)

**HEAD `3fcd00e` — M3 TUI UX overhaul (Slices D+E+F) is COMMITTED and M3 is CLOSED (UX-approved).**
Carlos ran the interactive re-smoke in Windows Terminal and said "it feels right, commit it." The
overhaul (framed chat transcript + roles, type-freely slash commands, live `/model`/`/provider`/
`/effort` with history preserved, keyboard scroll + overflow hints, honest identity, native mouse-copy
restored, bracketed-paste fixed) + BUILD_LOG + MILESTONES landed in `3fcd00e`. Verified 261 pass /
1 ignored, clippy `-D warnings` clean; orchestrator stress-tested the reducers/renderer (tiny
terminals, 200k/unicode paste, boundary nav, 20k-event fuzz) with zero panics.
**One optional follow-up (NOT blocking, Carlos's call):** `/effort HIGH` uppercase is rejected
(`parse_effort` is lowercase-only) — one-line Sol fix to case-fold the arg if he wants `/effort High`.

### ON RESUME ("continue") — start M4:
1. **Quick sanity:** `git log --oneline -1` = `3fcd00e`; clean tree; optional `cargo test --workspace`
   (261 pass / 1 ignored) + `cargo clippy --workspace --all-targets -- -D warnings` clean.
2. **Draft `CONTRACTS_M4.md`** (Claude plans + gates; Sol implements — [[m2-m5-codex-sol-directive]]).
   M4 scope + exit criteria are in `MILESTONES.md §Milestone 4`: append-only ledger, workers, typed
   receipts, **idempotent resume that survives `kill -9`**, off-peak scheduler, Kimi Swarm passthrough,
   escalation ladder (Flash → K2.7 → V4 Pro High → V4 Pro Max → Opus 4.8 gate), and an **nh-mcp
   server** exposing route-resolver + fleet-runner. Exit: 10-task fleet run survives `kill -9` and
   resumes idempotently; a deferred job runs off-peak; KORVIN connects to nh-mcp and triggers a fleet
   run; OAuth refresh survives forced expiry. **Do NOT ship nh-mcp publicly** until the MCP final spec
   lands (2026-07-28) — see M5 note.
3. **Brief Sol, run the loop, gate empirically** (numstat = truth; EOL/CRLF flags = noise). Slice the
   work; verify each slice green + adversarially review before the next. UX-first still governs any
   surface M4 adds (see below). Commit per-slice on `main` (repo convention).

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
