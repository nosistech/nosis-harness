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

## Slice C — DONE (committed 2026-07-14) — M3 CONTENT-COMPLETE

Timeline VIEW (`l`): left-rail turn list from in-memory receipts+answers (turn/outcome/tokens +
compaction marker), Up/Down scrub, Enter inspects the receipt+answer, `R` shows the deferral note
only (no restore, no snapshot store). Added to the `?` palette. Notifications: `.nosis/notify.toml`
([telegram] enabled + chat_id; token via vault entry `telegram`), loaded once in cmd_tui; on
entering Waiting/Blocked a short scrubbed body POSTs to Telegram on a short-lived side thread
(redirects off, 3s/5s timeouts, every error → fixed "telegram notify failed" so the token never
leaks), fires once per transition, failure = one dim line. Additive `AgentEvent::TaskReceipt`;
nh-core/nh-tools untouched; no new dep (existing reqwest). Gate: 239 pass / 1 ignored, clippy clean.

Remaining M3 exit items (both need Carlos, not code): (1) three-terminal render smoke on the
Predator; (2) live Telegram send with the KORVIN bot token.

## Slice B — DONE (committed 2026-07-14)

Trust-dial VIEW (`t`) + `?` discoverability palette. Additive `nh_law::PolicyView` + `Policy::view()`
(owned, read-only; fields still private; §7 amendment). nh-tui overlay reducer (`reduce_key ->
UiAction`), case-insensitive in-memory palette filter, MCP server/tool rows with
enabled/auth-ok/stale/discover-only startup state (derived from `McpToolset.warnings` +
trust/auth), built-in commands + tools, visible deferred `R` note. MCP loaded ONCE in cmd_tui before
alt-screen (no render-thread network). Every overlay line via `nh_vault::safe_line` (TestBackend
render test proves scrub + control-char safety). nh-core/nh-tools untouched. Gate: 227 pass / 1
ignored, clippy clean.

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

## Next Action — Slice C (CONTRACTS_M3 §3), the last M3 slice

1. Write `%TEMP%\brief_m3_sliceC.txt` from CONTRACTS_M3 §3: timeline VIEW ONLY (left-rail turn list
   from in-memory session history/receipts + compaction markers; arrow-scrub; Enter inspects a
   turn's receipt/answer; `R` stays a disabled seam with the "restore arrives later" note — NO
   snapshot store) + notifications (terminal bell baseline already in Slice A; add the Telegram hook
   + `.nosis/notify.toml`, token via nh-vault, scrubbed body, POST on entering Waiting/Blocked on a
   side thread — never block the render loop; failure = one dim line). No new deps (reqwest blocking
   exists). Telegram send is verify-live (Carlos's KORVIN bot token) — build + mock-test now.
2. Drive Sol (background codex exec), let it finish, verify empirically (numstat: nh-core/nh-tools
   still frozen; timeline is a projection over existing history/receipts, not a new persistence layer).
3. Verify: message builder produces a short scrubbed body per state; disabled/absent notify.toml =
   no HTTP; a failing POST (mock) degrades to one warning, session continues; timeline scrub/inspect
   is pure + headless-tested; `R` shows the deferral note, never restores.
4. Gate: `cargo test --workspace` + `cargo clippy --workspace --all-targets -- -D warnings`.
   (If Kaspersky is ON it blocks the 8 MB `wire_clients` test exe — pause AV or exclude
   `...\nosis-Harness\target`; wire_clients is frozen nh-core, green.)
5. Adversarial + UX review vs THE LAW + SECURITY_MODEL. Bounded hardening pass if warranted; re-verify.
6. Update BUILD_LOG + this file; commit Slice C. **Then M3 is content-complete** — hand Carlos a
   runnable `nh tui` for the three-terminal render smoke (the M3 exit criterion). Report at the M3
   boundary; only stop for guardrail conditions.

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
