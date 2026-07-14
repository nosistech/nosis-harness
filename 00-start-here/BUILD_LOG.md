# Build Log

Record every meaningful session here.

## 2026-07-14: M3 Slice C — orchestrator review + commit gate (M3 CONTENT-COMPLETE)

Builder:

- Claude (Opus 4.8, Claude Code) — M3 orchestrator: verify, adversarial review, gate, commit

What changed:

- No implementation code. Verified Slice C empirically: frozen nh-core/nh-tools/nh-law/nh-routes
  diffs are EMPTY; no new workspace dep (nh-tui uses the existing `reqwest.workspace`). Scope is
  nh-tui + cmd_tui + docs only.
- Adversarial review of the notify path (highest risk): the Telegram bot token is fetched on the
  side thread and used only inline in the URL; EVERY reqwest/vault error is mapped to a fixed
  "telegram notify failed" string, so a URL-with-token can never reach a rendered error. Redirects
  disabled (Policy::none()), fixed host, 3s/5s timeouts — no token-exfil-via-redirect, no hang. The
  body passes safe_line + a 160-char cap (proven redacted/control-safe). The POST runs on a
  short-lived thread drained via a failure channel — render never blocks; notify fires once per
  Waiting/Blocked transition, not on repeats. Timeline is strictly view-only: `R` shows only the
  deferral note (no restore, no snapshot store), every rail/detail line scrubbed (TestBackend render
  test with an sk- secret confirms), compaction flag is task-local. No confirmed defect → no
  hardening round-trip.

Tests/checks run (orchestrator, independent):

- `cargo test --workspace`: 239 passed, 0 failed, 1 ignored (+12 over Slice B).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.

Next step:

- Commit Slice C — **M3 is content-complete** (all §1/§2/§3 surfaces present + green). Remaining M3
  exit item is the MANUAL three-terminal render smoke on the Predator (Windows Terminal + VS Code
  terminal + ConHost) and the live Telegram send (needs Carlos's KORVIN bot token). Then M4 (fleet).

## 2026-07-14: M3 Slice C — timeline view + Telegram notifications

Builder:

- Codex (GPT-5.6 Sol) — M3 Slice C implementer

What changed:

- Added the view-only `l` timeline overlay over in-memory task receipts and answers. Rows show the
  sequential turn, outcome, input/output/cached tokens, and the M2 compaction marker; Up/Down scrub,
  Enter inspects the selected receipt and answer, and `R` only shows the locked restore deferral.
  Added the timeline to the `?` palette. No snapshot store or restore path was added.
- Forwarded successful receipts through an additive nh-tui worker event while preserving the
  existing Usage/Answer flow. Failed tasks also receive one friendly fail projection without
  changing frozen nh-core. Compaction is detected solely from the existing `context NN% —
  compacted ...` progress line for the active task.
- Added optional `.nosis/notify.toml` loading once in `cmd_tui`, before terminal takeover. Missing or
  broken config becomes bell-only with one scrubbed stderr warning. Telegram tokens are fetched as
  vault entry `telegram` on a short-lived side thread; the fixed-host POST has short timeouts and
  redirects disabled, and URL/token details never enter UI errors.
- Added pure short notification bodies, transition-driven sends for Waiting/Blocked, an injectable
  sender seam, and one dim `telegram notify failed` transcript line per failed attempt. Every
  timeline and notification body string passes the shared nh-vault scrub/display-safety path.
- Added headless coverage for receipt projection, task-local compaction, timeline selection/inspect,
  disabled restore, timeline rendering safety, missing/broken/valid config, disabled zero-send,
  injected successful sends, short scrubbed messages, and one-line failure degradation. nh-core and
  nh-tools remain unchanged.

Tests/checks run:

- `cargo test --workspace`: 239 passed, 0 failed, 1 ignored (pre-existing keyring round-trip).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.

Next step:

- Orchestrator review/commit gate. Slice C's only verify-live item is the real Telegram send with
  Carlos's bot token/chat id. The previously-open M3 three-terminal renderer smoke remains manual.

## 2026-07-14: M3 Slice B — orchestrator review + commit gate

Builder:

- Claude (Opus 4.8, Claude Code) — M3 orchestrator: verify, adversarial review, gate, commit

What changed:

- No implementation code. Verified Slice B empirically: scope is exactly nh-tui + cmd_tui + the
  additive `nh_law::PolicyView` (+47, owned clones, fields still private, verdict/autonomy behavior
  unchanged) — `git diff HEAD` on nh-core/nh-tools is EMPTY (frozen surfaces honored).
- Adversarial review: `reduce_key -> UiAction` is a pure reducer; overlays only open from
  Idle/Blocked (guarded behind the Working/Waiting checks) so they can't open mid-task; dispatch is
  suppressed while an overlay is open (proven by test); palette windowing keeps the highlighted
  index correct; inset/scroll math is saturating. Security: MCP tools are describe-only (no
  invocation from the palette, §2.2); `mcp_state` derivation matches nh-tools' real warning format;
  every overlay string passes `nh_vault::safe_line`, with a `TestBackend` render test proving a
  secret + control chars in a tool description come back `[REDACTED]` with no raw `\r`/`\x1b` in the
  drawn buffer. Conservative styling + Clear-before-draw keep it ConHost-safe.
- No confirmed defect → no hardening round-trip (THE LAW: small/simple — don't spend a cycle on a
  non-finding). One drop-if-hard nicety noted for later: the trust dial doesn't scroll a very long
  rule list (law lists are small; correctness/security unaffected).

Tests/checks run (orchestrator, independent):

- `cargo test --workspace`: 227 passed, 0 failed, 1 ignored (+10 over Slice A).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `nh tui --help`: launches, exposes `--model` / `--budget`. The three-terminal render-artifact
  smoke on the Predator remains the manual M3 exit check.

Next step:

- Commit Slice B. Then Slice C (timeline view + notifications: bell + Telegram hook).

## 2026-07-14: M3 Slice B — trust dial + discoverability palette

Builder:

- Codex (GPT-5.6 Sol) — M3 Slice B implementer

What changed:

- Added the additive owned `nh_law::PolicyView` projection and `Policy::view()` accessor for the
  read-only trust dial; policy fields remain private and verdict/autonomy behavior is unchanged.
  Logged the pre-authorized §2.1 amendment in `CONTRACTS_M3.md` §7.
- Added pure nh-tui overlay state and reducers for the `t` trust-dial view and `?` palette. The
  palette has case-insensitive in-memory filtering, command activation, built-in tool descriptions,
  the visible deferred `R` note, and MCP server/tool rows with enabled/auth-ok/stale/discover-only
  startup state. Every overlay line passes the shared `nh_vault::safe_line` path.
- Made `nh tui` read and discover `.nosis/mcp.toml` once before terminal takeover through the
  existing nh-tools loader/toolset. Missing/empty config stays empty; malformed or unavailable MCP
  becomes warning-backed stale/discover-only palette data without crashing. The worker, approval
  gate, semáforo, HUD, nh-core, and nh-tools are unchanged.
- Added headless coverage for the owned policy projection, pure palette filtering and MCP state,
  empty/broken MCP config, trust-dial none rows, overlay dispatch suppression/Esc, palette command
  activation/tool description, and rendered overlay scrubbing/control safety.

Tests/checks run:

- `cargo test --workspace`: 227 passed, 0 failed, 1 ignored (pre-existing keyring round-trip).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.

Next step:

- Orchestrator review/gate for Slice B, then Slice C (timeline view + notifications). The three-
  terminal render-artifact smoke remains the manual M3 exit check.

## 2026-07-14: M3 Slice A — orchestrator review + commit gate

Builder:

- Claude (Opus 4.8, Claude Code) — M3 orchestrator: resume, verify, adversarial review, gate, commit

What changed:

- No implementation code (orchestrator does not hand-write milestone code). Resumed after a `/clear`
  with Slice A's disk state unknown; determined it empirically. A transient CRLF/format pass made
  `git status` show nh-core/nh-tools/nh-vault as modified — `git diff --numstat HEAD` proved ZERO
  content change (EOL noise only), so the frozen crates were never touched. Let the in-flight Sol
  codex finish rather than kill a valid build.
- Read the full nh-tui surface + wiring. Confirmed: RAII terminal guard + panic hook restore on
  every exit path; the `approve` closure is Mutex-backed (Send+Sync, no new dep) and default-deny on
  every failure path; exec still routed through the policy guard (never auto-approved); every
  rendered string passes `nh_vault::safe_line` (scrub + control-char escape); semáforo is a
  single-state pure reducer; budget is a hard stop; peak_status lifted (not copy-pasted).
- One finding fed back to Sol as a bounded hardening pass: the safe_line/sanitize_line display-safety
  primitive was duplicated in nh-tui — lifted into `nh_vault` (additive), reused by nh-cli + nh-tui
  (§5.2 congruence). Re-verified green after.

Tests/checks run (orchestrator, independent):

- `cargo test --workspace`: 217 passed, 0 failed, 1 ignored.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- Env note: Kaspersky/McAfee on-access scanning blocked execution of the freshly-linked 8 MB
  `wire_clients` test exe (frozen nh-core, green at M2 and in Sol's run); confirmed green after AV was
  paused. The three-terminal render-artifact smoke on the Predator remains the manual M3 exit check.

Next step:

- Commit Slice A. Then Slice B (trust-dial view + `?` palette).

## 2026-07-14: M3 Slice A display-safety hardening

Builder:

- Codex (GPT-5.6 Sol) - M3 Slice A hardening implementer

What changed:

- Lifted the canonical scrub-then-control-character-escape-then-truncate display helper into
  additive `nh_vault::safe_line` and `nh_vault::sanitize_line`, with the 500-character cap kept
  private to nh-vault.
- Made nh-cli's existing crate-private helper delegate to nh-vault and made nh-tui use the same
  implementation through its shared scrubber lock. Rendered output and cmd_chat/cmd_run behavior
  are unchanged.
- Moved the control-character and truncation tests to nh-vault while retaining the existing
  nh-cli redaction and nh-tui rendered-line safety regressions. Recorded the §5.2 congruence
  amendment in `CONTRACTS_M3.md` §7.

Tests/checks run:

- `cargo test --workspace`: 217 passed, 0 failed, 1 ignored (pre-existing keyring round-trip).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.

Next step:

- Orchestrator commit gate for M3 Slice A.

## 2026-07-13: M3 Slice A - TUI core

Builder:

- Codex (GPT-5.6 Sol) - M3 Slice A implementer

What changed:

- Added the `nh-tui` library crate: a channel-backed single-worker agent boundary, pure semaforo reducer, conservative full-screen layout, inline approval flow, scrubbed/sanitized rendering, cumulative usage and budget HUD, hard budget stop, keyless task errors, and panic-safe RAII terminal restoration.
- Added `nh tui [--model <id>] [--budget <tokens>]`, mirroring chat's catalog/law setup and warning order while keeping the terminal launch keyless.
- Lifted peak/off-peak status into `nh_routes::ResolvedRoute::peak_status` and shared it with chat and TUI; recorded the additive decision in `CONTRACTS_M3.md` section 7.
- Added headless reducer, approval, HUD, budget, redaction/control-character, terminal teardown, worker-history, and keyless-start coverage. Slice B/C keys remain reserved no-ops.

Tests/checks run:

- `cargo test --workspace`: 217 passed, 0 failed, 1 ignored (pre-existing keyring round-trip).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `nh tui --help`: compiled command exposes the locked model and budget options.
- Live visual verification on Windows Terminal, VS Code terminal, and ConHost remains the human M3 Slice A exit check.

Next step:

- Run the three-terminal verify-live matrix, then orchestrator review and commit gate before Slice B.

## 2026-07-13: M2 orchestrator review + commit gate

Builder:

- Claude (Opus 4.8, Claude Code) — M2 orchestrator: verification, adversarial review, gate, commit

What changed:

- No implementation code (orchestrator does not hand-write milestone code). Read every M2 slice (nh-law, nh-core context engine, nh-tools guard, nh-cli wiring, m2_exit e2e) and independently re-ran the gate — confirmed green, not just Sol's self-report.
- Adversarial review vs THE LAW + SECURITY_MODEL. The write-hold holds: guard receives the workdir-relative `/`-joined path; `Verdict::Block` wins before the `is_file` check; symlinks resolve to their canonical target (escapes caught by `starts_with`); and structurally `exec_verdict` can only return `Block`/`Ask` — never `Allow` — so exec is never auto-approved even at `--autonomy auto`. Confirmed the Windows case-fold new-file bypass is NOT reachable through `EditFile` (it only mutates existing files, whose case `canonicalize` normalizes before the guard sees them; missing paths bail before any write).
- Fed three confirmed findings back to Sol as ONE bounded hardening pass (dead `exec_ask`, non-hermetic protected-path test, undocumented case-fold invariant); re-verified after.

Tests/checks run (orchestrator, independent):

- `cargo test --workspace`: 206 passed, 0 failed, 1 ignored (pre-existing keyring round-trip).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- M2 exit #1 PROVEN by name: `stable_constitution_exceeds_sixty_percent_cache_hits_over_fifty_turns` = 97.70% (> 60%).
- M2 exit #2 PROVEN by name: `protected_path_is_blocked_at_auto_end_to_end` — real `nh run --autonomy auto`, model-readable law block, `.nosis/law.toml` byte-unchanged, exit 0.

Next step:

- Commit all of M2 together, then M3 (TUI).

## 2026-07-13: M2 bounded hardening pass

Builder:

- Codex (GPT-5.6 Sol) — M2 hardening executor

What changed:

- Removed the behaviorless compiled `exec_ask` state and matching work while preserving `[exec] ask` TOML compatibility; shell execution still blocks configured commands and asks for every other command.
- Made the bundled protected-path autonomy test independent of the developer's real home law.
- Documented why existing-file canonicalization makes the case-sensitive write-hold safe on case-insensitive filesystems, plus the required guard hardening for any future file-creation tool.

Tests/checks run:

- `cargo test --workspace`: 206 passed, 0 failed, 1 ignored (pre-existing keyring round-trip).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.

Next step:

- M2 reviewer and commit gate.

## 2026-07-13: M2 Slice C — nh-tools guard + nh-cli law wiring

Builder:

- Codex (GPT-5.6 Sol) — M2 Slice C executor

What changed:

- Added the locked nh-tools `Access` / `Guard` / `GuardFn` surface and `ToolCtx::new` / `with_guard`. `edit_file` now evaluates normalized workdir-relative forward-slashed paths before any file check or write; Block and Ask denials stay Ok-shaped. `exec_shell` evaluates the command before execution, while shipped policy still routes every non-blocked command through the existing approval gate. MCP adapters retain their independent M1 trust logic.
- Wired nh-law into both `nh run` and `nh chat`: scrubbed non-fatal warnings, byte-stable constitution, route context windows, policy-backed tool guards, and route-switch context refresh. Added optional `nh run --autonomy ask|auto` with one translation function.
- Added conditional cache-hit chips to the run summary and chat footer. `nh init` now creates `.nosis/law.toml` from the starter policy without overwriting it.
- Added the process-level M2 exit test `protected_path_is_blocked_at_auto_end_to_end`; it runs `nh run --autonomy auto`, observes the model-readable law block, and proves `.nosis/law.toml` is unchanged.

Tests/checks run:

- `cargo test --workspace`: 206 passed, 0 failed, 1 ignored (pre-existing keyring round-trip).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- Keyless smoke: `echo /quit | nh chat` exited 0 with one actionable missing-key warning.

Next step:

- Orchestrator adversarial review and M2 commit gate.

## 2026-07-13: M2 Slice B — nh-core context engine

Builder:

- Codex (GPT-5.6 Sol) — M2 Slice B executor

What changed:

- Added the locked `AgentLoop.constitution` and `AgentLoop.context_limit` surfaces. A supplied constitution is installed verbatim only when session history is empty; the existing coding-agent/tool-list system message remains the `None` fallback.
- Added the pure `wire::cache_hit_pct` metric and mechanical 70% context compaction. Compaction preserves the byte-identical system prefix, retains at least two recent user turns from a user boundary, keeps complete tool-call/result groups and reasoning bytes, folds the audit marker into the first retained user message, and emits one concise event.
- Added context-engine integration coverage for prefix stability, usage-omitted token estimation, compaction invariants, disabled compaction, and the 50-turn prefix-cache exit criterion. The deterministic mock observed a 97.70% cumulative cache-hit rate.
- Updated the existing `AgentLoop` literals in nh-core tests and nh-cli with `constitution: None` and `context_limit: None`; nh-cli behavior is otherwise unchanged in this slice.

Tests/checks run:

- `cargo test --workspace`: 195 passed, 0 failed, 1 ignored (pre-existing keyring round-trip).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.

Next step:

- M2 Slice C: nh-tools guard enforcement and nh-cli law/autonomy/cache-HUD wiring.

## 2026-07-13: M2 Slice A — nh-law leaf crate

Builder:

- Codex (GPT-5.6 Sol) — M2 Slice A executor

What changed:

- Added the `nh-law` leaf crate with the locked constitution, policy, loader, and public types from `CONTRACTS_M2.md` section 1. Bundled and starter policy remain TOML data; the crate depends only on workspace `serde`, `toml`, and `anyhow` plus std.
- Implemented byte-stable constitution assembly, non-fatal layered loading, CLI/user/bundled autonomy precedence, unioned protections, the repo-cannot-weaken boundary, and the in-crate segment glob matcher. Repository autonomy and auto-approval attempts are ignored with the required warning.
- Added 11 unit tests covering assembly stability/order/omission, glob and exec matching, verdict precedence, bundled protected paths, source precedence, malformed input, unknown autonomy, and repo-law restrictions.

Tests/checks run:

- `cargo test --workspace`: 191 passed, 0 failed, 1 ignored (pre-existing keyring round-trip).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.

Next step:

- M2 Slice B: nh-core stable-prefix cache discipline, cache metric, and compaction.

## 2026-07-13: M1 build finalized (Fable 5 multi-agent workflow)

Builder:

- Claude (Fable 5, Claude Code) — multi-agent workflow: architect + 4 parallel crate builders, integrator, 4 adversarial reviewers, hardening pass

What changed:

- M1 implemented end-to-end per CONTRACTS_M1.md: full 5-provider catalog with clock-aware pricing, Anthropic Messages wire adapter, thinking-mode dialects + `nh run --think`, stateless MCP client (2026-07-28 draft), and `nh chat` with mid-session `/model`/`/provider` switching and cost HUD. Integration green after merging the crate builders' outputs.
- 4 adversarial review findings addressed (fixes detailed in the M1 hardening entry below).
- Price verification pass (full detail: `../04-research/SOURCE_INDEX.md`, 2026-07-13 section): all four API providers checked against their OWN pricing/docs pages — DeepSeek, Kimi, MiMo, and GLM base prices and base URLs are all first-party CONFIRMED. MiMo plan-B.3 conflict RESOLVED first-party (current `mimo.mi.com/docs/pricing` matches the marketplace figures, superseding the May 27 cut notice; `verify_live` → `confirmed`). Two earlier Kimi "reported" figures were wrong (kimi-k2.6 is $0.16 hit / $0.95 miss / $4.00 out, not ~$0.55–0.60 in / $2.50–2.65 out; k2.7 highspeed bills 2x standard input) and the catalog's old MiMo host was wrong (fixed to `api.xiaomimimo.com`). Still open, honestly: DeepSeek's peak 2x windows are announcement-only (secondary press, not yet on the first-party pricing page — re-verify on/around 2026-07-24), and GLM free-tier rate limits remain unpublished.

Tests/checks run:

- cargo build --workspace: green. cargo test --workspace: 176 passed, 0 failed, 1 ignored (keyring round-trip; nh-cli 49, nh-core lib 21 + integration 5+4+49, nh-routes 38, nh-vault 10). cargo clippy --workspace --all-targets -- -D warnings: clean. M0 smoke: `nh --help` exit 0; `echo /quit | cargo run -p nh-cli -q -- chat` exit 0 with no key configured (friendly warning to stderr, stdout empty). Committed on main as 0ed3d6d 'M1: full catalog, clock pricing, Anthropic wire, thinking dialects, MCP client, chat session'.

Next step:

- Live provider verification (DeepSeek keyed run + GLM free-route run), then M2: context engine + law.

## 2026-07-13: M1 hardening — adversarial-review fixes

Builder:

- Claude (Fable 5, Claude Code) — M1 hardening agent

What changed:

- Wire-client HTTP config (nh-core): both clients now build via one `http_client()` — explicit 600 s request timeout + 10 s connect timeout (reqwest's blocking default silently aborted every request at 30 s, killing long thinking turns) and `redirect::Policy::none()` (reqwest forwards custom headers like `x-api-key` across cross-host redirects — a redirecting endpoint is now a friendly HTTP error, never a key leak). Timeout failures get their own actionable line ("provider at <url> did not answer within 600s — retry, or switch to another route") instead of the misleading "could not reach provider". `McpClient` got the same explicit timeouts.
- `nh chat` keyless recovery (nh-cli): a keyless start now retries the real connection at the next task, so `nh key add <provider>` in another terminal works without restarting the session; reconnect registers the new key on every scrub path (shared `install_client` helper with `/model`/`/provider`).
- CONTRACTS_M1.md §7 amendment 4 (orchestrator authority): ratified `nh run --think none|low|high|max` + per-dialect defaults (always-thinking/glm-hm → High, deepseek-nhm/none → None) — closes the frozen-surface gap flagged in review.
- Deleted the reviewer's truncated `adv_redirect_probe.rs` (marked DELETE AFTER REVIEW, did not compile); its concern lives on as the `cross_host_redirects_are_refused_never_followed` regression test in `wire_clients.rs`.

Tests/checks run:

- `cargo test --workspace` (180 passed, 1 ignored keyring round-trip, 0 failed; 4 new tests: timeout error line, timeout floor guard, redirect refusal on both wires, keyless reconnect), `cargo clippy --workspace --all-targets -- -D warnings` clean. Smoke: `echo /quit | nh chat` exit 0 keyless; `nh run --help` surface unchanged.

Next step:

- M1 exit demo against a live key (unchanged from integration entry).

## 2026-07-13: M1 integration — workspace green, contracts reconciled

Builder:

- Claude (Fable 5, Claude Code) — M1 integrator

What changed:

- Reconciled the 3 nh-routes tests asserting pre-verification catalog values with the 2026-07-13 live-verified first-party prices (kimi-k2.6 output 4.00 confirmed, mimo B.3 conflict resolved confirmed, glm free tier confirmed); `mimo_prices_are_verify_live` renamed to `mimo_prices_are_confirmed_first_party`. Catalog is data-of-record — tests follow data.
- `nh chat` keyless startup: connect failure at start is now one `warning:` line plus a stand-in client that re-surfaces the vault error only when a task runs; commands work keyless and `echo /quit | nh chat` exits 0 on a fresh machine (new test).
- CONTRACTS_M1.md: §5.2 amendment 3 (`AgentLoop.thinking`), §5.3 nh-cli `serde_json` dev-dep, §6 ledger rows marked RESOLVED (base URLs, MiMo prices, Kimi K2.6 cache-hit $0.16) + DeepSeek 2026-07-24 re-verify row, new §7 integration amendments (keyless startup, `peak <mult>x until HH:MM` format blessed, nh-tools crate-root re-exports).

Tests/checks run:

- `cargo build --workspace`, `cargo test --workspace` (176 passed, 1 ignored keyring round-trip, 0 failed), `cargo clippy --workspace --all-targets -- -D warnings` — all green. M0 smoke: `nh --help` exit 0; `echo /quit | nh chat` exit 0 keyless; `/price` keyless shows live peak HUD.

Next step:

- M1 exit demo against a live key: mid-session `/model`+`/provider` switch, one stateless MCP call with handle passback; verify-live ledger items (`reasoning_effort`, `ttlMs`, GLM rate limits; DeepSeek prices on/around 2026-07-24).

## 2026-07-13: M1 nh-cli (`nh chat` REPL + `nh run --think`)

Builder:

- Claude (Fable 5, Claude Code) — nh-cli builder (CONTRACTS_M1.md §4)

What changed:

- New `nh chat [--model <id>]` (cmd_chat.rs): line REPL over an injected line-source/Write abstraction (piped-stdin friendly, BOM-tolerant for Windows pipes); prompt `nh> ` on stderr, stdout carries only answers and command output. Plain lines run through `AgentLoop::run_with_history` on ONE persistent history; `/model` and `/provider` switch routes mid-session preserving history and cumulative usage (M1 exit criterion); `/price` and the per-answer footer are the first cost HUD lines — peak indicator renders "peak 2x until HH:MM" with the window boundary converted to the user's local time; `/tools` lists builtin then MCP tools; unknown commands get the one-line help. MCP tools load at chat start from `.nosis/mcp.toml` via `nh_tools::mcp` (broken file = one warning, chat continues).
- Scrubbing: one shared `Arc<RwLock<Scrubber>>` across stdout/stderr/approval/receipt paths, rebuilt on every switch so switched-away keys stay redacted for the whole session.
- `nh run --think none|low|high|max` mapped to `ThinkingEffort` (`effort_for`): absent flag defaults per dialect — High for always-thinking/glm-hm, None for deepseek-nhm/none. `nh run` now uses `make_client` (both wires) and refuses delegate routes with the M4 message.
- Deps per §5.3: `chrono` (workspace) added to nh-cli; `serde_json` (workspace) added as dev-dependency so tests build `ChatMessage` via serde instead of struct literals (§5.2 grep rule).

Tests/checks run:

- `cargo test -p nh-cli` 48 passed (23 new chat tests: switching, price/footer formats, scrubbing, loop basics, MCP load); `cargo clippy -p nh-cli --all-targets -- -D warnings` clean; workspace clippy clean. Manual piped smoke: `/price` (live peak boundary in local time), friendly missing-key error keeps route, `/quit` exits 0. Workspace tests: all crates green EXCEPT 3 nh-routes tests asserting pre-verification catalog values (kimi-k2.6 output 2.65→4.00, mimo 0.87, glm confirmed) — catalog owner's reconciliation, not nh-cli.

Next step:

- nh-routes tests vs live-verified catalog.toml reconciliation; M1 live MCP call + `reasoning_effort` verify-live ledger.

## 2026-07-13: M1 nh-tools (MCP client, stateless 2026-07-28)

Builder:

- Claude (Fable 5, Claude Code) — nh-tools builder (CONTRACTS_M1.md §3)

What changed:

- New `nh_tools::mcp` module (re-exported at crate root; existing tools untouched): `.nosis/mcp.toml` parsing (`load_mcp_config`, unknown keys ignored, friendly errors naming valid auth/trust/spec values), `McpClient` (blocking JSON-RPC 2.0 over Streamable HTTP POST, `tools/list` with `ttlMs` cache — absent→60s, 0→no cache — `tools/call` text/non-text block rendering, discovery via GET `.well-known/mcp.json` with `server/discover` POST fallback).
- Statelessness invariant: no `initialize`, no `Mcp-Session-Id` ever; every request's params carry `_meta` (protocolVersion echoing config spec, clientInfo, capabilities). State handles are ordinary tool arguments (handle-passthrough test).
- Auth §3.4 (none / apikey via nh-vault per call / oauth2 deferred to M4 with one message) + §3.5 outbound header lint choke point (`Mcp-*`/`x-mcp-*` with Scrubber-shaped values refused; `Authorization` exempt).
- §3.6 adapters: `mcp_tools` builds `mcp__<server>__<tool>` adapters (`[MCP <server>] ` description prefix); trust=ask gates via `ctx.approve("mcp <server> <tool> <args>")` with Ok-shaped denial; trust=auto skips the gate only for server-annotated `readOnlyHint` tools; trust=block servers are never contacted and offer no tools; failing servers contribute one warning line, never a hard failure.
- Deps added to nh-tools per §5.3: `reqwest`, `toml` (workspace), `nh-vault` (path).

Tests/checks run:

- `cargo test -p nh-tools` 49 passed (37 new MCP tests against a hand-rolled `std::net::TcpListener` mock — no live calls, no heavy dev-deps); `cargo clippy -p nh-tools --all-targets -- -D warnings` clean. Workspace: nh-vault/nh-routes/nh-core lib+agent green; nh-cli has a pre-existing mid-flight compile error (`cmd_run::run` arity, cli owner's area); nh-core `wire_clients` test binary transiently file-locked by a concurrent build (os error 32), retried.

Next step:

- nh-cli `/tools` wires `mcp_tools` + warnings through the Scrubber; M1 live verify of `ttlMs` location against a real 2026-07-28 server.

## 2026-07-13: M1 nh-core (wire clients, thinking dialects, session history)

Builder:

- Claude (Fable 5, Claude Code) — nh-core builder (CONTRACTS_M1.md §2)

What changed:

- nh-core wire: `AnthropicMessagesClient` (POST `{base_url}/v1/messages`, `x-api-key` + `anthropic-version: 2023-06-01`, required `max_tokens`, full tool_use/tool_result mapping with consecutive-tool-result merge, usage from `input_tokens`/`output_tokens`/`cache_read_input_tokens`).
- nh-core wire: `make_client` factory — dispatches on `route.wire`, captures per-route policy (thinking dialect, `preserve_reasoning`, deepseek tool-replay quirk); `max_tokens = min(max_out, 8192)` on the anthropic wire.
- nh-core wire: `ThinkingEffort` enum + one-function `(dialect, effort)` mapping (deepseek-nhm → `reasoning_effort`, flagged verify-live; always-thinking/glm-hm/none → no toggle); `ChatRequest.thinking` and `ChatMessage.reasoning_content` amendments per §5.2; reasoning replay rules (preserve / strip / quirk empty-string, stored value wins) in one function.
- nh-core agent: `run_with_history` for `nh chat` sessions (`run` is now a thin wrapper); additive `AgentLoop.thinking` field — nh-cli's one literal updated in step (`thinking: None`).

Tests/checks run:

- `cargo test -p nh-core` 30 passed (incl. loopback mock-server tests for both wires — no live calls); `cargo clippy -p nh-core --all-targets -- -D warnings` clean; workspace clippy clean. Workspace tests: nh-core/nh-cli/nh-tools/nh-vault green; 3 pre-existing nh-routes failures from catalog.toml (2026-07-13 confirmed prices) vs nh-routes test expectations (verify_live/reported/2.65) — routes owner's area, untouched here.

Next step:

- Routes owner: reconcile nh-routes tests with the updated catalog.toml. nh-cli builder: `nh chat` on top of `run_with_history` + `make_client`.

## 2026-07-13: M0 hardening (adversarial review fixes)

Builder:

- Claude (Fable 5, Claude Code) — hardening agent

What changed:

- nh-cli: every stderr path now passes the Scrubber — progress lines, the approval prompt, and the final `nh:` error line; model-supplied text is also control-char-escaped (`sanitize_line`) so \r/ANSI cannot spoof the approval gate, with a visible truncation marker past 500 chars.
- nh-tools: `exec_shell` strips `NH_*_KEY` env vars from the child, closing the key-exfiltration-to-disk path via the env fallback.
- nh-routes: `from_toml` rejects banned route keys AND banned `model_id` values (clean alias can no longer smuggle a dead id onto the wire).
- nh-cli: `nh init` writes a starter catalog.toml (embedded repo-root catalog — still data), so `nh run` works in a fresh repo; missing-catalog error now says "run `nh init` to create one".

Tests/checks run:

- `cargo test --workspace` (62 passed, 1 ignored keyring round-trip), `cargo clippy --workspace --all-targets -- -D warnings` — green. Manual: `nh init` + `nh run` flow in a fresh temp dir reaches the key prompt.

Next step:

- M1: live route/pricing verification against providers.

## 2026-07-12: M0 build finalized (Fable 5 multi-agent workflow)

Builder:

- Claude (Fable 5, Claude Code) — multi-agent workflow: 5 parallel crate builders, integrator, 3 adversarial reviewers, hardening pass

What changed:

- M0 implemented end-to-end across all five crates via the multi-agent workflow; integration green after merging the crate builders' outputs.
- 6 adversarial review findings addressed (fixes detailed in the M0 hardening entry above).

Tests/checks run:

- 53 passed; 0 failed; 1 ignored (keyring_round_trip) across 6 test binaries: nh-core unit 12, nh-cli 8, nh-core integration 3, nh-routes 10, nh-tools 10, nh-vault 10 (+1 ignored); doc-tests 0; clippy -D warnings clean.

Next step:

- Verify live against DeepSeek (`nh key add deepseek`, then `nh run` on a sample repo), then M1.

## 2026-07-12: M0 implemented (turn loop, tools, vault, routes, CLI)

Builder:

- Claude (Fable 5, Claude Code) — 5 parallel crate builders + integrator

What changed:

- Implemented all locked `todo!()` contracts across `nh-core` (AgentLoop, OpenAiCompatClient, receipts), `nh-routes` (RouteResolver, catalog parsing, banned-string rejection), `nh-tools` (read_file / edit_file / exec_shell behind approval gate; denial is an Ok-shaped "user denied: <command>" tool result), `nh-vault` (OS keyring + env fallback + Scrubber), `nh-cli` (init / key / run).
- Sanctioned contract addition: `AgentLoop.on_event: Option<Box<dyn Fn(&str) + Send>>` for progress lines; nh-cli wires it to stderr. Field set is now frozen.

Tests/checks run:

- `cargo build --workspace`, `cargo test --workspace` (53 passed, 1 ignored keyring round-trip), `cargo clippy --workspace --all-targets -- -D warnings` — all green.

Next step:

- M1: live route/pricing verification against providers.

## 2026-07-12: Project OS created

Builder:

- Carlos + Claude (Fable 5, Claude Code)

What changed:

- Adapted ProjectStarterTemplate into this folder as the project operating system.
- Filled core docs from Master Plan v0.1: master context, current task, roadmap, milestones, decision log, product brief, architecture overview/decisions, security model, AI-collaboration set (AGENTS/CLAUDE/CODEX/MODEL_ROLES/CONTEXT_HANDOFF/PROMPT_LIBRARY), risk register, one-page summary.
- Re-pointed the M0 implementer prompt from "Codex 5.5" to GPT-5.6 (Terra default / Sol for hardest), added nh-vault to M0 scope per plan §A.10.7.

Files changed:

- All of `00-start-here/`, `05-ai-collaboration/`, plus README, PRODUCT_BRIEF, ARCHITECTURE_OVERVIEW, ARCHITECTURE_DECISIONS, SECURITY_MODEL, RISK_REGISTER, ONE_PAGE_SUMMARY.

Decisions made:

- Adopted the template as project OS (see DECISION_LOG 2026-07-12).

Tests/checks run:

- None (no code yet).

Next step:

- `git init`, root AGENTS.md, first commit, hand M0 to Codex (prompt in `../05-ai-collaboration/CODEX.md`).

Risks:

- All Appendix B prices are `reported`, not confirmed — verify live at M1. MiMo first-party pricing sources conflict.

## 2026-07-09 → 2026-07-11: Master Plan v0.1 + Appendices A/B

Builder:

- Carlos + Claude (research/planning)

What changed:

- Master Plan v0.1 written (verdict, capability matrix, architecture, routing brain, fleet, MCP 2026-07-28 strategy, UX, build plan M0–M5, risks, Codex first prompt).
- Appendix A: two-backend access architecture (API routes vs subscription delegates), per-provider deep dives, nh-vault spec.
- Appendix B: complete verified model catalog (July 11) with delta logs.

Next step:

- Organize project folder, then pre-M0 setup.
