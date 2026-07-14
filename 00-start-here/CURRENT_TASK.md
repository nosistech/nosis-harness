# Current Task

## Immediate Goal

Drive the **M3 (TUI) build loop**. M0/M1/M2 DONE + committed (HEAD `3155949`). **M3 Slice A
(nh-tui render core + `nh tui`) is DONE**: implemented by Sol, orchestrator-verified (217 pass / 1
ignored / 0 fail, clippy clean), one hardening pass (display-safety `safe_line` lifted into
nh-vault), **committed**. Next = **Slice B** (trust-dial view + `?` palette), then **Slice C**
(timeline view + notify).

## Roles (fixed)

- **Orchestrator = Opus 4.8, high effort** (this session): plans, writes contracts/briefs, runs
  gates, adversarially reviews, commits, docs. Does NOT hand-write milestone code.
- **Executor = GPT-5.6 Sol xhigh** via `codex exec` — writes all M3 implementation. Memory
  [[m2-m5-codex-sol-directive]], [[ux-first-and-the-law]], [[build-loop-resume]].

## Executor invocation (proven working)

```
codex exec --skip-git-repo-check -s workspace-write -m gpt-5.6-sol \
  -c model_reasoning_effort=xhigh "$(cat /c/Users/capv2/AppData/Local/Temp/<brief>.txt)" < /dev/null
```
Run in background (harness-tracked); verify empirically after (git diff --numstat HEAD is truth,
git status EOL flags are noise). Do NOT start a second nosis codex while one writes nosis.

## Slice A — DONE (committed 2026-07-14)

`crates/nh-tui` (ratatui + crossterm) + `nh tui [--model][--budget]` in nh-cli. Channel-backed
single worker; Mutex-backed default-deny approval (Send+Sync, no new dep); exec still policy-gated;
RAII terminal guard + panic hook restore on every exit path; every rendered string via
`nh_vault::safe_line` (scrub + control-char escape); single-state semáforo pure reducer; cost HUD +
hard `--budget` stop; bell on entering Waiting. `peak_status` and the `safe_line`/`sanitize_line`
display-safety primitive both lifted to shared crates (nh-routes, nh-vault) — §7 amendments logged.
Reserved no-ops for Slice B/C: `?`, `t`, `R`. nh-core/nh-tools untouched.

Verify-live still open: three-terminal render-artifact smoke (Windows Terminal + VS Code terminal +
ConHost) on the Predator — the M3 exit criterion, human-checked (not unit-testable).

## Next Action — Slice B (CONTRACTS_M3 §2)

1. Write `%TEMP%\brief_m3_sliceB.txt` from CONTRACTS_M3 §2 (trust-dial VIEW over `nh_law::Policy`
   toggled by `t`; `?` fuzzy palette listing TUI commands + built-in tools + MCP tools/servers with
   LIVE state from `nh_tools::mcp`). No new deps. Read-only law view. If exposing compiled rules
   needs a read-only accessor on `nh_law::Policy`, that is an additive §7 amendment (owned strings).
2. Drive Sol (background codex exec), let it finish, verify empirically.
3. Verify against the checklist: no nh-core/nh-tools edits; palette filter is a PURE function over a
   tool list (headless-testable); MCP state string derives from `McpToolset.warnings` (broken
   mcp.toml → servers shown `stale`/`discover-only`, never a crash); every rendered string scrubbed;
   `Esc` closes, typing filters, `Enter` runs a command (tools show one-line desc only).
4. Gate: `cargo test --workspace` + `cargo clippy --workspace --all-targets -- -D warnings`.
   (If Kaspersky is on, it blocks the 8 MB `wire_clients` test exe — pause AV or exclude
   `...\nosis-Harness\target`; wire_clients is frozen nh-core, green.)
5. Adversarial + UX review vs THE LAW + SECURITY_MODEL. One bounded hardening pass to Sol; re-verify.
6. Update BUILD_LOG + this file; commit Slice B. Then Slice C (§3). Report at the M3 boundary.

## Do Not Do

- Do NOT hand-write milestone implementation code — Sol implements; Claude plans + gates only.
- Do NOT start a second codex ON NOSIS while one is writing nosis (Carlos's other codex sessions are
  fine — verify a process is actually writing nosis via `git status`, not by name).
- Do NOT commit a slice until it is verified AND its hardening pass is done.
- If `gpt-5.6-sol` stops resolving, or Sol fails the same gate twice → STOP and tell Carlos
  (don't silently fall back to Terra).

## Definition Of Done (M3)

- Full session renders artifact-free on Windows Terminal + VS Code terminal + ConHost (manual smoke).
- Semáforo, cost HUD, trust-dial view, timeline view + diff-inspect, `?` palette w/ live MCP state,
  notify hook (bell + Telegram) — all present, each a short/scannable surface.
- `cargo test --workspace` + `cargo clippy … -D warnings` green; BUILD_LOG updated; committed.
