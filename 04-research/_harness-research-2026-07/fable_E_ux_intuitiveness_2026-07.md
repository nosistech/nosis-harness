# NOSIS HARNESS — Lens E: UX / Intuitiveness (research findings, 2026-07-17)

**Analyst:** Fable (Claude) · **Reference bar:** CodeWhale — "self-teaching with no handholding, delightful for small tasks"
**Repo grounding:** `crates/nh-tui/src/lib.rs` (4,107 lines, M3 Slices D/E/F shipped + Slice D M4 uncommitted), `catalog.toml`, `NOSIS_HARNESS_Master_Plan.md §5`, `00-start-here/CURRENT_TASK.md`.
**Provider-key reality:** every finding here needs **no new API key** — this lens is pure TUI/CLI work.

---

## What nh-tui already does well (baseline, so we don't re-invent)

- **Semáforo** is real and tested: single `Status` enum (`Idle/Working/Waiting/Blocked(reason)`) rendered as one chip (`status_chip`, lib.rs:2322–2347), reducer-driven (`apply_event`, lib.rs:669), with bell + optional Telegram push on entering `Waiting`/`Blocked` (`notify_message`, lib.rs:726).
- **HUD** exists (lib.rs:401–428): `in/out/cached` tokens, cache-hit %, peak/off-peak chip from `route.peak_status`, token budget bar.
- **Slash menu** (live as-you-type `/` menu), `/help` palette with MCP server/tool state, `/trust` read-only dial, `/timeline` scrubber, `/model` `/provider` `/effort` with history preserved.
- **Windows robustness**: mouse capture removed (native select/copy), bracketed paste sanitized to one line (lib.rs:1392–1417), `TerminalGuard` + panic-hook restore, scrubber on every rendered line (`safe_line`).
- **Errors**: one friendly scrubbed line, no stack trace (test `failed_event_renders_one_friendly_line_without_a_trace`, lib.rs:2911).

That's a strong M3 floor. The gaps below are what separates it from the 2026 best-in-class bar.

---

## F1. Session prefix-rule approvals — "yes / no / **always for this session**" (the approval-fatigue killer)

**Problem.** The approval prompt is binary. `answer_approval` (lib.rs:386–399) sends `bool`; the worker-side approve callback (lib.rs:988–1007) blocks on one `y/n`. Every `cargo test`, every `git status` re-asks. Research shows users approve ~93% of prompts, making each prompt meaningless noise — the exact failure mode the Master Plan §5 names ("approval fatigue") and differentiator 6 promises to fix.

**2026 state of the art.** Codex CLI's *Smart Approvals* (default since ~May 2026) turns each escalation prompt into three options: **accept once / accept + create a prefix rule / decline**. The accepted rule (`prefix_rule()` in `~/.codex/rules/default.rules`) auto-allows future commands with that prefix; conflicting rules resolve most-restrictive-wins (`forbidden > prompt > allow`), and compound commands are split so `git add . && rm -rf /` is never smuggled under a `git add` rule. "Every rule amendment is an explicit human decision, reviewed in the TUI before it takes effect."
Sources: https://codex.danielvaughan.com/2026/05/04/codex-cli-smart-approvals-adaptive-command-policies-prefix-rules/ · https://developers.openai.com/codex/rules · https://smartscope.blog/en/generative-ai/chatgpt/codex-cli-approval-policy-implementation/

**Nosis already has the perfect substrate**: nh-law's `PolicyView` with `auto_paths / ask_paths / block_paths / block_commands` (rendered read-only by `trust_dial_lines`, lib.rs:566–577) and a guard closure wired into `ToolCtx` (lib.rs:1012–1015). What's missing is only the **in-session write path** from the approval prompt into a session-scoped allowlist.

**Design sketch (MVP):**
1. `ApprovalRequest` gains the parsed command prefix (first token or nh-law's existing exec-rule shape).
2. Approval keys become `y` (once) / `a` (always this session) / `n`/`Esc` (deny). `a` pushes the prefix into a new `App.session_allow: Vec<String>` **and** into a `Arc<Mutex<Vec<String>>>` shared with the worker's guard closure — checked *before* emitting an `AgentEvent::Approval` (auto-approve path emits a dim transcript line `auto-approved: cargo test (session rule)` so it stays auditable).
3. `/trust` dial shows the session rules under a new `session auto-approve:` group (one `append_rules` call) and offers nothing persistent in MVP — session-only keeps it small and safe. A later slice can offer "press `w` in /trust to write a rule into `.nosis/law.toml`" (explicit, reviewed, exactly the Codex model).
4. Compound-command safety: reuse nh-law's exec verdict on **each** command segment; session rules never override `block_commands` (most-restrictive-wins, same as Codex).

**LAW fit:** small (one Vec + 3 key branches), auditable (every auto-approval is a transcript line + receipt), safe (session-scoped, block rules always win). Tension: none if persistence stays out of MVP.
**Value: high · Effort: M · Key: none.**

---

## F2. Fix the "any key silently denies" approval hazard (quick win, arguably a bug)

**Problem.** In `reduce_key` (lib.rs:1355–1358), while `Status::Waiting` **any** keypress that isn't `y`/`Y` is treated as a denial: `app.answer_approval(matches!(key.code, KeyCode::Char('y'|'Y')))`. A user still typing their next thought — or an arrow-key reflex to scroll — silently denies a tool call the agent then has to route around. Claude Code's own issue tracker shows how corrosive inconsistent interrupt/approve key semantics are (e.g. "esc to interrupt" shown but inert: https://github.com/anthropics/claude-code/issues/16905, https://github.com/anthropics/claude-code/issues/14526). CLIG's principle: the program should guide, not punish (https://clig.dev/#errors).

**Design sketch (MVP):** explicit keys only — `y` approve, `n` or `Esc` deny, (`a` from F1), **everything else ignored**; render the legend directly in the amber approval row: `● WAITING ON YOU — run: cargo test · [y]es [n]o [a]lways this session`. The approval row already exists (`TranscriptKind::Approval`, amber reversed, test at lib.rs:2892); this adds the legend + a stricter match arm. Add a reducer test: pressing `x`/arrow while Waiting changes nothing.

**LAW fit:** safe + simple; strictly less surprising. **Value: high · Effort: S · Key: none.**

---

## F3. Esc-to-interrupt + live "working heartbeat" (elapsed · tokens · esc hint)

**Problem.** While `Status::Working`, all input is discarded (`reduce_key` lib.rs:1359–1361) — there is **no way to stop a runaway turn** short of Ctrl+C quitting the whole app, and the green `● WORKING` chip is static: no elapsed time, no sign of life. Master Plan §5 calls out "ambiguous status" and "tool-call retry loops" as documented pain; 2026 guidance is blunt: "Async UI is non-negotiable — users should always be able to press Esc to get back to a responsive interface" (https://hyperbliss.tech/blog/2026.04.04_terminal-renaissance/). Claude Code's working line is the reference: `Thinking… (12s · ↑ 1.4k tokens · esc to interrupt)` — and its bug history (issues 14526/16905 above) teaches the one rule: **if the hint is shown, it must always work.**

**Design sketch (MVP):**
1. **Heartbeat (S):** UI loop already ticks every 50ms (`EVENT_POLL`, lib.rs:47). Store `working_since: Option<Instant>` set on dispatch; render the chip as `● WORKING (34s · Esc to stop after this step)`. Token count can join once per-turn usage events stream (worker already sends progress events).
2. **Interrupt (M):** blocking `ChatClient::complete` can't be aborted mid-request cheaply, so interrupt at **loop boundaries** (the honest, small version): share an `Arc<AtomicBool>` cancel flag with the worker; the agent loop checks it before each wire call and before each tool execution; on cancel, worker emits a `Failed("stopped by you")`-style event that lands as one friendly line and returns to `Idle` with history intact (so the user can redirect). Esc while Working sets the flag; the chip flips to `● WORKING (stopping…)` so the semantics are never a lie.
3. Keep Ctrl+C = quit unchanged.

**LAW fit:** small seam (one AtomicBool), congruent with the semáforo ("exactly one state" — add no new state, just annotate WORKING), harmonic with F2's key-legend pattern. **Value: high · Effort: M · Key: none.**

---

## F4. Money-denominated cost HUD with the honest-cost rule (kill cost opacity)

**Problem.** `hud_line` (lib.rs:401–428) shows tokens, cache %, peak chip, and a **token** budget bar — but never money. Yet `catalog.toml` carries everything needed per route: `cache_hit / cache_miss / output` per M tokens, `currency` (CNY/USD), peak `multiplier` + windows, `price_confidence`, `valid_until` (catalog.toml:31–120). Cost opacity / "rate-limit shock" is documented pain (Master Plan §5, differentiator 6), and in 2026 live cost display is table stakes: Claude Code's statusline receives `cost.total_cost_usd` updating in real time (https://code.claude.com/docs/en/costs, https://www.voitanos.io/blog/claude-code-cli-statusline/), and ccusage's statusline popularized session cost + **burn rate** + active-block display (https://ccusage.com/guide/statusline). Nobody, however, shows *cache-aware, clock-aware* cost — that's Nosis's differentiator 1+4 made visible.

**Design sketch (MVP):**
1. nh-routes already computes `price_at(clock)`; add a pure `fn turn_cost(usage: &Usage, price: &Price) -> Money` (cached tokens × cache_hit + (prompt−cached) × cache_miss + completion × output). Accumulate per session in `App` next to `usage` (`add_usage`, lib.rs:1163 is the seam).
2. HUD becomes: `¥0.41 session · in 182k (cache 78%) · out 9k · off-peak ✓ · ▰▰▱ 41%`. One currency symbol from the route; no FX conversion in MVP (honest > convenient).
3. **Honest-cost rule enforced in UI:** if `valid_until` is past or `price_confidence = "verify_live"`, render `cost ~¥0.41 (price data stale — /help routes)` — flag, never guess (Master Plan §7's CodeWhale rule, already encoded in catalog comments).
4. Timeline entries already carry per-receipt tokens (`TimelineEntry.tokens`, lib.rs:148); add cost per turn to `timeline_row` so the scrubber doubles as a cost audit.
5. Later (M5+): dual-unit chip for `class = "delegate"` routes (plan-quota units, Master Plan A.0) — out of MVP since no delegate routes are wired yet.

**LAW fit:** lightweight (pure arithmetic on data already loaded), auditable (cost per receipt), congruent (surfaces differentiators 1/4). **Value: high · Effort: M (S if HUD-only) · Key: none.**

---

## F5. Semáforo → Windows taskbar via OSC 9;4 (walk-away visibility, zero deps)

**Problem.** The semáforo is only visible when the terminal is focused. Carlos's real workflow is walk-away runs (Telegram notify exists for that reason, lib.rs:740–817), but the middle ground — nosis minimized on the taskbar — shows nothing.

**2026 fact.** Windows Terminal (≥1.6) implements ConEmu's OSC `9;4` sequence: the app sets a **taskbar progress state** — `0` hidden, `1` normal, `2` error (red), `3` indeterminate (pulsing), `4` warning (yellow) — reflected on the WT taskbar icon exactly like a download bar (https://learn.microsoft.com/en-us/windows/terminal/tutorials/progress-bar-sequences, https://github.com/microsoft/terminal/pull/8055). Cargo itself adopted it in 2025 (https://github.com/rust-lang/cargo/pull/14615), and Ghostty/WezTerm/VTE now honor it too (https://ghostty.org/docs/vt/osc/conemu) — so it degrades gracefully off-Windows.

**Design sketch (MVP):** one function `fn taskbar_state(status: &Status) -> &'static [u8]` emitting `ESC ] 9 ; 4 ; <state> ; <pct> BEL`: Working→`3` (indeterminate), Waiting→`4;100` (solid yellow), Blocked→`2;100` (solid red), Idle→`0`. Emit on every status transition in the UI loop (same place `entered_notify_state` is checked, lib.rs:1241) and **always emit `0` in `TerminalGuard`/panic restore** (join the existing `RestoreCommand` sequence, lib.rs:2450–2475) so no stuck taskbar state survives a crash — the same discipline already applied to bracketed paste. ~20 lines, zero dependencies, Windows-first differentiator made visceral: a yellow taskbar icon *is* "WAITING ON YOU" from across the room.

**LAW fit:** small, lightweight, harmonic (extends the one-status model to the OS shell); safe via restore-sequence. **Value: high · Effort: S · Key: none.**

---

## F6. `/model` and `/provider` pickers that show price · modality · peak state (informed switching = discoverability + cost UX in one)

**Problem.** `/model <id>` and `/provider <name>` are prefill commands (`builtin_palette_entries`, lib.rs:526–540): the user must already *know* route ids. The catalog knows everything interesting — price at the current clock, modality, thinking dialect, context — but none of it surfaces at the decision moment. Best-in-class 2026 TUIs make mid-session model switching a first-class picker (Crush/OpenCode: https://pinggy.io/blog/best_open_source_cli_coding_agents/, OpenCode's 17 discoverable slash commands: https://www.explainx.ai/blog/opencode-slash-commands-complete-reference-guide-2026).

**Design sketch (MVP):** when the live command menu is open on `/model ` (trailing space), populate the menu rows from `RouteResolver` instead of the static palette: one row per route — `deepseek-v4-flash · ¥1.00/M in · off-peak ✓ · text · think:nhm` — reusing the existing `CommandMenu` overlay + `filter_palette` substring filter (lib.rs:474) so typing narrows. Enter switches (existing `resolved_route_action`, lib.rs:1595). Routes whose vault entry is missing render dim with `[no key — nh key add <entry>]` (self-teaching, F8 synergy). Same treatment for `/provider`. This is a *projection*, no new state: a pure `fn route_menu_rows(resolver, now) -> Vec<PaletteEntry>`.

**LAW fit:** readable, congruent (routing brain made visible), modular (pure projection function, unit-testable like `mcp_palette_entries`). **Value: med-high · Effort: S-M · Key: none.**

---

## F7. Codify "errors that teach": every user-facing failure line = what happened + one runnable next step

**Problem/opportunity.** Nosis already has the *pattern* — the Slice D OAuth failure line is exemplary: `mcp server "korvin": oauth refresh failed — re-authorize with 'nh key add korvin-refresh' … (or check token_url in .nosis/mcp.toml)` (CURRENT_TASK.md:24–27). And the TUI guarantees one friendly line, no trace (test lib.rs:2911). But the pattern lives in individual call sites, not as a contract. CLIG's canonical guidance: "catch the error and rewrite the message to be useful… *Can't write to file.txt. You might need to make it writable by running `chmod +w file.txt`*" (https://clig.dev/#errors); irrelevant output delays the user's diagnosis.

**Design sketch (MVP):** a tiny shared helper (nh-core or nh-tui): `fn teach(what: &str, try_next: Option<&str>) -> String` producing `"{what} — try: {try_next}"`, plus a **repo-wide test convention**: every string that can reach `AgentEvent::Failed` or a transcript error line must either contain `" — try: "`/`nh ` (a runnable next step) or be explicitly allowlisted in the test. Sweep the existing error sites (route resolution failure, `NotConnected` (lib.rs:1140–1151), budget `BLOCKED` reason, MCP warnings) to route through it. Examples: `BLOCKED — budget reached — try: /quit or restart with --budget 0 for unlimited`; `could not connect to deepseek — try: nh key add deepseek`. This turns the FEEL rule ("teach, don't trace") into a mechanical invariant — very Nosis.

**LAW fit:** small, readable, auditable (invariant is a test), harmonic with the Slice D precedent. **Value: med-high · Effort: S · Key: none.**

---

## F8. Self-teaching first-run: context-aware welcome + one-line environment doctor (incl. legacy-console detection)

**Problem.** The empty-state welcome (lib.rs:2158–2193) is static: "Welcome to nosis. Type a task…". If the user has no key in the vault, their *first* interaction is a connection error; if they launched in legacy ConHost, Backspace/copy will misbehave — the exact trap that burned Carlos with Claude Code in a classic PowerShell console (CURRENT_TASK.md:127–135). 2026 first-run patterns (Gemini CLI wizard, OpenCode `/help`+Ctrl+P) front-load exactly these checks (https://geminicli.com/docs/cli/cli-reference/, https://opencode.ai/docs/cli/).

**Design sketch (MVP):**
1. **Context-aware welcome lines** (pure function, testable like the existing empty-state test lib.rs:2712): if route resolution or vault lookup failed at startup, the welcome's line 2 becomes the teach-line: `no DeepSeek key found — run: nh key add deepseek (then restart)`. If everything is healthy: current behavior.
2. **Legacy-console hint:** detect `WT_SESSION`/`WT_PROFILE_ID` env absence on Windows + ConHost heuristics; render one dim welcome line: `tip: run nosis inside Windows Terminal for correct Backspace and copy` — the cheapest possible fix for the single worst Windows-terminal failure mode.
3. Optional `nh doctor` CLI subcommand (nh-cli) that prints the same checks headlessly: vault entries present per configured route, catalog `valid_until` staleness, terminal capability (VT enabled), `.nosis/` writability. Reuses the exact same pure check functions — one source of truth.

**LAW fit:** self-teaching without handholding (the hint appears only when the problem exists), small (env probe + string projection). **Value: med · Effort: S-M · Key: none.**

---

## F9. `/copy` — copy the last answer (or last command) to the clipboard via OSC 52, zero dependencies

**Problem.** M3 Slice F deliberately removed mouse capture so native click-drag copy works — the right call. But copying a *whole multi-line answer* through a scrolling TUI viewport is still fiddly (select spans borders/hints). Every modern agent TUI grew a copy affordance.

**2026 fact.** Windows Terminal has supported OSC 52 copy-to-clipboard since 2020 (https://github.com/microsoft/terminal/pull/5823); it's broadly supported across terminals (https://can-i-use-terminal.github.io/features/osc52copy.html). crossterm intentionally declined to wrap it, recommending user-space emission (https://github.com/crossterm-rs/crossterm/pull/697) — i.e., print `ESC ] 52 ; c ; <base64> BEL` yourself. No `arboard`/clipboard dependency, no WinAPI.

**Design sketch (MVP):** `/copy` command → base64-encode the last `TranscriptKind::Answer` text (already stored in `TimelineEntry.answer`) → write the OSC 52 sequence to stdout (bypassing ratatui's buffer via `crossterm::execute!` raw write) → confirm with one dim line `copied last answer (2.1 KB)`. Guard: cap at ~74 KB (OSC 52 practical limit), scrub through `safe_line` first (secrets never reach the clipboard — the scrubber is already the law of every output path). Add `/copy cmd` for the last approved shell command. Base64 is ~10 lines hand-rolled or the `base64` micro-crate already common in workspaces.

**LAW fit:** lightweight (no deps), secure (scrubbed before encode), harmonic with the copy-over-scroll decision. **Value: med · Effort: S · Key: none.**

---

## F10. Input-line ergonomics: cursor movement + input history recall

**Problem.** `App.input` is a bare `String`: Backspace pops the last char (lib.rs:1368–1370); there is no Left/Right/Home/End cursor movement, no word-wise editing, and no way to recall the previous prompt (Up/Down scroll the transcript — a binding worth keeping, per Carlos's keyboard-scroll decision). For a "delightful for small tasks" bar, retyping a 60-char task because of one typo at the front is the anti-delight. Every reference CLI (Claude Code, Codex, OpenCode, Crush) has full line-editing + history (https://opencode.ai/docs/cli/, https://pinggy.io/blog/best_open_source_cli_coding_agents/).

**Design sketch (MVP):**
1. Add `cursor: usize` (char index) to `App`; handle Left/Right/Home/End/Backspace-at-cursor/Delete in `reduce_key`; render the cursor at the true position (the `set_cursor_position` math in `render_input`, lib.rs:1829–1837, already computes from string width — parametrize by cursor index). Ctrl+W delete-word and Ctrl+U clear-line are two more match arms.
2. History: `Vec<String>` of dispatched tasks; recall bound to **Ctrl+Up/Ctrl+Down** (leaves plain Up/Down as transcript scroll, honoring the existing binding contract), or plain Up *only when the input is non-empty*… simpler and less surprising: Ctrl+Up/Down only. Esc restores the draft.
3. All reducer-level, hence unit-testable in the existing style (`type_text`, `code_key` helpers, lib.rs:3389–3398).

**LAW fit:** small per-arm, congruent with keyboard-only scroll decision; no new deps (avoid pulling a readline crate — 6 match arms suffice). **Value: med · Effort: M · Key: none.**

---

## Ranked summary

| # | Finding | Value | Effort | Key |
|---|---|---|---|---|
| F1 | Session prefix-rule approvals (y/a/n) | high | M | none |
| F2 | Fix any-key-denies; explicit approval legend | high | S | none |
| F3 | Esc-to-interrupt + working heartbeat | high | M | none |
| F4 | Money cost HUD + honest-stale-price flag | high | M | none |
| F5 | OSC 9;4 taskbar semáforo (Windows-first) | high | S | none |
| F6 | /model picker with price·modality rows | med-high | S-M | none |
| F7 | "Errors that teach" helper + invariant test | med-high | S | none |
| F8 | Context-aware welcome + legacy-console hint + nh doctor | med | S-M | none |
| F9 | /copy via OSC 52 (scrubbed, dep-free) | med | S | none |
| F10 | Input cursor + history recall | med | M | none |

**Sequencing note for the build loop:** F2+F5+F7 are one small Sol slice (all S, all reducer/render-level, no frozen crates). F1+F3 touch the worker seam and deserve their own contract slice. F4 wants a tiny additive nh-routes helper (`turn_cost`) — check frozen-crate status before briefing; if nh-routes is frozen, the arithmetic can live in nh-tui off the already-public price data.

---

## All sources consulted

- https://codex.danielvaughan.com/2026/05/04/codex-cli-smart-approvals-adaptive-command-policies-prefix-rules/
- https://developers.openai.com/codex/rules
- https://smartscope.blog/en/generative-ai/chatgpt/codex-cli-approval-policy-implementation/
- https://codex.danielvaughan.com/2026/04/20/codex-cli-guardian-approval-configuring-auto-review-policies/
- https://hyperbliss.tech/blog/2026.04.04_terminal-renaissance/
- https://agent-experience.dev/tui-cli
- https://github.com/anthropics/claude-code/issues/14526
- https://github.com/anthropics/claude-code/issues/16905
- https://code.claude.com/docs/en/costs
- https://www.voitanos.io/blog/claude-code-cli-statusline/
- https://ccusage.com/guide/statusline
- https://learn.microsoft.com/en-us/windows/terminal/tutorials/progress-bar-sequences
- https://github.com/microsoft/terminal/pull/8055
- https://github.com/rust-lang/cargo/pull/14615
- https://ghostty.org/docs/vt/osc/conemu
- https://github.com/microsoft/terminal/pull/5823
- https://github.com/crossterm-rs/crossterm/pull/697
- https://can-i-use-terminal.github.io/features/osc52copy.html
- https://clig.dev/#errors
- https://opencode.ai/docs/cli/
- https://www.explainx.ai/blog/opencode-slash-commands-complete-reference-guide-2026
- https://geminicli.com/docs/cli/cli-reference/
- https://pinggy.io/blog/best_open_source_cli_coding_agents/
