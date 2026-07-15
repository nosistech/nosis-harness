# CONTRACTS_M3.md — Locked public API for Milestone M3 (TUI)

**Status: LOCKED (draft by orchestrator Opus 4.8, 2026-07-13).** Builders (GPT-5.6 Sol
xhigh, via `codex exec`) implement EXACTLY these public surfaces; private helpers are free,
public deviations are not. Spec source: `NOSIS_HARNESS_Master_Plan.md` §2 (nh-tui crate), §5
(UX answers 1–8), §6 (M3), `02-architecture/SECURITY_MODEL.md`, `MILESTONES.md` (M3 exit).
Amendments go through the orchestrator only, additive, logged in §7.

Carlos's M3 scoping decisions (2026-07-13), binding:
- **Timeline = view-first.** M3 ships the timeline as VIEW + diff-inspect. Side-git snapshots
  and `R` restore are DEFERRED to a later slice — do NOT build a snapshot store in M3.
- **Telegram = build now, live-pending token.** Implement the notify hook + config, mock-tested;
  live verification waits for a real bot token (verify-live ledger §6).
- **Delivery = core-first, then layer.** Slice A de-risks the renderer; B and C layer on top.

---

## 0. Ground rules (bind every builder)

- **M0 + M1 + M2 stay green.** All existing public APIs stay source-compatible. `cargo test
  --workspace` + `cargo clippy --workspace --all-targets -- -D warnings` clean before every
  handoff (currently 206 pass / 1 ignored). Add tests; never weaken an existing assertion.
- **nh-core and nh-tools are NOT modified by M3.** The TUI is a new consumer crate. It drives
  the EXISTING `AgentLoop` + `ToolCtx` by wiring channel-backed closures into the already-public
  `ToolCtx.approve` and `AgentLoop.on_event`. If you believe a core change is unavoidable, STOP
  and request an orchestrator amendment — do not edit nh-core/nh-tools unilaterally.
- **THE LAW** (top authority): small, simple, secure, safe, lightweight, readable, auditable,
  modular, congruent, harmonic. **UX IS THE PRODUCT** — this is THE milestone for it: every
  visible surface is one scannable line/element, no stack traces, no ambiguous spinners,
  drop-if-hard. When a fancy render is hard or flickers, ship the simple robust one.
- **Windows-first rendering.** The exit criterion is zero renderer artifacts on Windows
  Terminal, VS Code integrated terminal, and ConHost (legacy console). Enter the alternate
  screen, enable raw mode, and ALWAYS restore the terminal on every exit path (normal quit,
  error, panic) — no lost cursor, no stuck raw mode, no mouse-tracking residue. A panic hook
  must restore the terminal before printing.
- **Secrets.** Every string rendered anywhere in the TUI (transcript, approval prompt, palette,
  footer, timeline, error toast) passes `nh_vault::Scrubber` first, exactly like `nh chat`.
  The Telegram hook body is scrubbed before send.
- **exec_shell always passes the approval gate** — the TUI renders the approval in-pane; it
  does not bypass it. Max autonomy may auto-approve file writes (guard returns Allow), never exec.
- **New external crates allowed for this milestone (orchestrator-authorized, §5.3):** `ratatui`
  and `crossterm` (current stable, added to workspace deps). No others without an amendment —
  Telegram uses the existing `reqwest` blocking client; OS alert is a terminal bell (no dep).

**M3 exit criteria (plan §6, MILESTONES.md):**
1. A full interactive session runs natively on the three Windows terminals (Windows Terminal,
   VS Code terminal, ConHost) with ZERO renderer artifacts. Unit/integration tests cover the
   pure state logic; the render-artifact check is a real-terminal smoke (verify-live §6, needs
   Carlos on the Predator).
2. Every M3 surface present and each a short/scannable element: semáforo (one state, always),
   cost HUD footer, trust-dial view, timeline view + diff-inspect, `?` palette with live MCP
   state, and the notify hook (bell + Telegram).

---

## 1. nh-tui — NEW crate. Slice A: render loop + agent thread + semáforo + cost HUD

New workspace member `crates/nh-tui`. Depends on: `ratatui`, `crossterm` (new, §5.3),
`nh-core`, `nh-routes`, `nh-law`, `nh-tools`, `nh-vault` (path), `anyhow`, `chrono` (workspace).
The binary entry stays in nh-cli (§4) — nh-tui is a library crate so its state logic is unit-testable
headlessly (no terminal required in tests).

### 1.1 The agent/UI boundary (the crux — get this right first)

The agent loop is synchronous and blocking. The TUI runs it on ONE worker thread and renders on
the main thread; they communicate over channels. **No nh-core change is needed** — the worker
wires closures into the existing `ToolCtx.approve` (`Fn(&str)->bool + Send + Sync`) and
`AgentLoop.on_event` (`Fn(&str) + Send`).

```rust
/// Everything the render loop learns from the running agent. Sent worker -> UI.
pub enum AgentEvent {
    Progress(String),          // from on_event: "turn 2: edit_file src/lib.rs"
    Approval(ApprovalRequest), // agent is blocked awaiting a y/N decision (-> WAITING)
    Usage(nh_core::wire::Usage), // cumulative usage snapshot for the cost HUD (see 1.4)
    Answer(String),            // final assistant text for this task (-> IDLE)
    Failed(String),            // one friendly, scrubbed line (-> BLOCKED)
}

/// A pending approval. The UI renders `prompt`, collects y/N, and answers via `reply`.
/// Dropping `reply` (e.g. user quits mid-approval) is read as deny by the worker.
pub struct ApprovalRequest {
    pub prompt: String,                 // already scrubbed + control-char-escaped
    pub reply: std::sync::mpsc::Sender<bool>,
}
```

Wiring rules (locked behavior; internal channel types are the builder's choice as long as the
`Send + Sync` bound on `approve` is satisfied — wrapping the sender/receiver pair in a `Mutex`
is the sanctioned no-new-dep way):
- `on_event` closure sends `AgentEvent::Progress`.
- `approve` closure sends `AgentEvent::Approval { prompt, reply_tx }` to the UI, then BLOCKS on
  `reply_rx.recv()`; returns the received bool (or `false` if the channel closed). The prompt is
  built with the existing `safe_line` discipline (scrubbed + control-char-escaped).
- When `AgentLoop::run_with_history` returns `Ok((answer, receipt))`, the worker sends
  `AgentEvent::Usage(receipt.usage)` then `AgentEvent::Answer(answer)`; on `Err(e)`, it sends
  `AgentEvent::Failed(scrubbed(e))`.
- History is owned across tasks (one `Vec<ChatMessage>`), exactly like `nh chat`, so the session
  persists turn to turn.

### 1.2 Semáforo (plan §5.1) — exactly one state at all times

```rust
/// The single status. Rendered as color + WORD + icon; never two at once, never a spinner.
#[derive(Clone, PartialEq)]
pub enum Status {
    Idle,               // grey/dim,  "IDLE"          — no task running, awaiting input
    Working,            // green,     "WORKING"       — agent thread active
    Waiting,            // amber,     "WAITING ON YOU" — blocked on an approval (bell rings once)
    Blocked(String),    // red,       "BLOCKED"        — last task failed; carries a short reason
}
```

Transitions (derived purely from the boundary in 1.1; unit-testable via a pure reducer):
- dispatch task -> `Working`
- `AgentEvent::Approval` -> `Waiting` (ring the bell once; see 1.5)
- approval answered -> back to `Working`
- `AgentEvent::Answer` -> `Idle`
- `AgentEvent::Failed(reason)` -> `Blocked(reason)`
- new task dispatched from `Idle`/`Blocked` -> `Working`

Provide a pure function so the state machine is tested without a terminal:
```rust
/// Fold one event into app state; returns the new Status (and updates HUD, transcript).
/// Pure over (‑&mut App, event) — the render layer calls this, tests call it directly.
pub fn apply_event(app: &mut App, event: AgentEvent) -> &Status;
```

### 1.3 Layout (ratatui) — minimal, robust, artifact-free

Three regions, top to bottom:
1. **Header bar (1 line):** semáforo chip (icon + WORD, colored) on the left; route id on the right.
2. **Transcript (fills):** scrollable task/answer/progress/approval log, newest at the bottom.
   Wrap long lines; never emit raw control chars (reuse the sanitize discipline). A pending
   approval renders as a highlighted `approve? <cmd>  [y/N]` line inside this pane.
3. **Cost HUD footer (1–2 lines, §1.4).** Plus a one-line input field for the next task.

Keep styling conservative: named colors only, no 24-bit truecolor assumptions, no custom
box-drawing that ConHost can't render. This is the artifact budget.

### 1.4 Cost HUD (plan §5.2)

Footer chips, each short and aligned, built from session-cumulative usage + the resolved route
(reuse `nh_core::wire::cache_hit_pct` and the M1 `price_at`/peak logic — do NOT reimplement clock
pricing; call the existing nh-routes surface):
- session tokens `in / out / cached`
- `cache NN%` (omit before any usage, exactly like the chat footer)
- route id + `peak Nx until HH:MM` / `off-peak` / `no price data` (same helper semantics as
  `cmd_chat::peak_status`; if that helper must be shared, lift it via an orchestrator amendment
  rather than copy-pasting)
- a **budget bar** with a hard stop: an optional `--budget <tokens>` on `nh tui`; render a bar of
  used/limit; when exceeded, the semáforo goes `Blocked("budget reached")` and no new task
  dispatches until the user raises it. Projected cost-to-goal is NOT in M3 (needs goal-based
  loops) — omit it; do not fake it.

### 1.5 Input + keys (Slice A baseline)

- Type a task, `Enter` dispatches it (ignored while `Working`/`Waiting`).
- During `Waiting`: `y`/`Y` approves, any other key (default) denies — same default-deny as the CLI.
- `Ctrl-C` / `q` (when idle) quits cleanly (restore terminal). `?` is reserved for the palette (Slice B).
- Bell: on entering `Waiting`, emit one terminal bell (`\x07`). This is the M3 "OS notification"
  baseline; the Telegram push (Slice C) is the remote channel. A real desktop toast is out of scope
  (lightweight; drop-if-hard).

### 1.6 Public surface of nh-tui (Slice A)

```rust
pub struct App { /* private: status, history, transcript, hud, input, budget… */ }
pub enum Status { … }         // 1.2
pub enum AgentEvent { … }     // 1.1
pub struct ApprovalRequest { … }

/// Build the initial app state from a resolved session (route, law, client factory).
/// Mirrors nh chat's construction so law/scrubber/receipts behave identically.
pub struct TuiConfig { /* route resolver, model id, law, budget: Option<u64> */ }

/// Run the full-screen TUI to completion. Owns terminal setup/teardown + the worker thread.
/// Returns Ok(()) on clean quit; never leaves the terminal in raw/alt-screen state.
pub fn run(config: TuiConfig) -> anyhow::Result<()>;

/// Pure reducer seam (1.2) for headless tests.
pub fn apply_event(app: &mut App, event: AgentEvent) -> &Status;
```

### 1.7 Slice A tests (headless — no real terminal)

- `apply_event` drives every semáforo transition (idle→working→waiting→working→idle;
  working→blocked; blocked→working).
- Approval reducer: an `Approval` event sets `Waiting`; answering `true`/`false` returns to
  `Working` and forwards the bool.
- Cost HUD: chip omits `cache` before usage; shows `NN%` after; budget-exceeded flips to `Blocked`.
- Scrubber: a rendered transcript line containing a fake key literal comes back `[REDACTED]` and
  has no control chars.
- Terminal restore: a unit around the guard type proving Drop restores cooked mode + leaves the
  alternate screen (assert the teardown runs; a RAII terminal-guard type is the clean shape).

---

## 2. nh-tui — Slice B: trust-dial view + `?` discoverability palette

Locked surfaces (implemented after Slice A is gated). No new deps.

### 2.1 Trust dial view (plan §5.3) — a VIEW over the M2 policy, not a new policy

The autonomy/write-hold logic already lives in `nh-law` (`Policy`, `Verdict`). Slice B renders it:
- A panel (toggled by a key, e.g. `t`) showing the session autonomy (`ask`/`auto`) and the compiled
  rule classes in plain words: auto-approve paths, always-ask paths, hard-block paths, blocked
  commands. This is READ-ONLY in M3 (editing law is out of scope; law is data in `.nosis/law.toml`).
- If exposing the compiled rules requires a read-only accessor on `nh_law::Policy`, add it as an
  orchestrator amendment (additive getter returning owned strings) — logged in §7 — rather than
  making fields public.

### 2.2 `?` palette (plan §5.5, §4.5) — commands + MCP tools with LIVE state

- `?` opens a fuzzy-filterable overlay listing: built-in TUI commands/keys, built-in tools, and
  every MCP tool/server from `.nosis/mcp.toml` WITH current state — `enabled / auth-ok / stale /
  discover-only`. Reuse the existing `nh_tools::mcp` loader + `McpToolset.warnings`; do not add a
  second MCP config path.
- `Esc` closes; typing filters; `Enter` on a command runs it. Selecting a tool just shows its
  one-line description (no invocation from the palette in M3).
- Tests: palette filter is pure over a tool list; MCP state string derives correctly from a
  toolset with/without warnings; a broken `mcp.toml` shows the servers as `stale`/`discover-only`,
  never crashes the palette.

---

## 3. nh-tui — Slice C: timeline VIEW + notifications (Telegram hook)

Locked surfaces (implemented after Slice B is gated). No new deps (Telegram = existing `reqwest`).

### 3.1 Timeline view (plan §5.4) — VIEW ONLY in M3 (Carlos's decision)

- A left-rail vertical list of the session's turns; each entry = turn number + outcome +
  cost/tokens + the compaction marker when that turn compacted (the `[nosis] earlier context
  compacted…` fold from M2 is the data source). Arrow keys scrub the list; `Enter` inspects that
  turn's detail (the receipt + the answer/diff text already in history).
- **No side-git snapshot store and no `R` restore in M3.** Leave a clearly-labelled seam
  (e.g. a disabled `R` key that shows "restore arrives in a later slice") so the deferral is
  visible, not silent.
- Data source is the in-memory session history + receipts already produced by `AgentLoop`; the
  timeline is a projection, not a new persistence layer.

### 3.2 Notifications (plan §5.1) — bell (baseline) + Telegram push (build now, live-pending)

```toml
# .nosis/notify.toml  (all optional; absent file = bell only, no remote push)
[telegram]
# bot_token and chat_id are read from the vault/env, NOT stored in this file in plaintext.
enabled = true
# token entry name resolved via nh-vault (e.g. NH_TELEGRAM_KEY), chat id may live here (non-secret)
chat_id = "123456789"
```
- On entering `Waiting` or `Blocked`, if telegram is enabled, POST a short scrubbed message to the
  Telegram Bot API via the existing `reqwest` blocking client (on the worker/a side thread — never
  block the render loop). Failure to notify is one dim status line, never a crash and never fatal.
- The token is fetched through `nh-vault` (same discipline as provider keys); it is scrubbed from
  every rendered/logged surface.
- Tests: message builder produces a short scrubbed body for each state; disabled/absent config =
  no HTTP attempt; a failing POST (mock) degrades to one warning, session continues. The real
  end-to-end send is verify-live (§6) — needs Carlos's bot token.

---

## 4. nh-cli — the `nh tui` subcommand

Additive subcommand; does not change `run`/`chat`/`init`/`key`.

```
nh tui [--model <id>] [--budget <tokens>]
```
- Resolves the catalog + law exactly like `nh chat` (find catalog, `nh_law::load`, warnings to
  stderr BEFORE entering the alternate screen), builds `TuiConfig`, calls `nh_tui::run`.
- Keyless start behaves like chat: the TUI opens; a task surfaces the friendly "run `nh key add`"
  line in the transcript rather than failing to launch.
- `--budget` feeds the HUD budget bar (§1.4).

---

## 5. What is frozen / amendments / deps

### 5.1 Frozen
- nh-core and nh-tools public surfaces: unchanged this milestone (ground rule §0).
- nh-law: unchanged except a possible additive read-only accessor for the trust-dial view (§2.1),
  logged in §7 if used.
- Catalog/pricing and law stay DATA.

### 5.2 Amendments
- Any shared helper lifted out of nh-cli (e.g. `peak_status`/`safe_line` reuse) is an additive
  amendment logged in §7; prefer reuse over copy-paste (THE LAW: congruent, no duplication).

### 5.3 Dependency additions allowed (orchestrator-authorized)
- `ratatui` + `crossterm` (current stable) added to workspace `Cargo.toml` and used only by
  nh-tui. No `notify-rust`, no `glob`, no async runtime — the agent stays on a plain thread.

---

## 6. Assumptions / verify-live ledger (M3)

- **Windows renderer matrix** — the exit criterion. Automated tests cover state logic only;
  artifact-free rendering on Windows Terminal + VS Code terminal + ConHost is a manual smoke on
  the Predator (Carlos). Flag anything that looks off there as a hardening finding.
- **Telegram push** — built + mock-tested in Slice C; real send needs Carlos's KORVIN bot token
  (`nh key add telegram` or `NH_TELEGRAM_KEY`) + chat id. Live-pending.
- **Terminal teardown on panic** — assert via a RAII guard + a panic hook; verify by eye that a
  forced panic leaves a usable terminal.

## 7. Integration amendments (append here, dated, orchestrator authority)

- **2026-07-13 — shared peak chip:** lifted the existing chat peak/off-peak display
  logic into additive nh_routes::ResolvedRoute::peak_status; both nh chat and
  nh-tui call the same method, preserving the existing chat strings and tests.
- **2026-07-14 — shared display safety:** safe_line/sanitize_line display-safety helper lifted
  into nh_vault (additive pub `safe_line`/`sanitize_line`), reused by nh-cli + nh-tui, removing
  the nh-tui duplicate — congruence per §5.2. cmd_chat/cmd_run behavior unchanged.
- **2026-07-14 — trust-dial policy view:** added additive `nh_law::PolicyView` and
  `Policy::view()`, returning owned copies of autonomy and the four compiled rule classes for the
  read-only Slice B trust-dial. Policy fields and verdict behavior remain unchanged, per §2.1.
- **2026-07-14 — Slice D (UX overhaul) authorized:** the artifact-free M3 TUI was rejected by the
  owner on UX grounds (flat/unfinished look; overlays bled the transcript around a margin). Slice D
  (§8) re-skins nh-tui ONLY into framed panels + chat-style transcript + framed centered modals +
  self-teaching affordances. No behavior/keys/deps change; nh-tui public API stays source-compatible.
  The plan's deliberately-minimal renderer (§1.3 "artifact budget") is relaxed now that the renderer
  is proven artifact-free — plain single-line borders + 16-color only stay the safety envelope.

---

## 8. nh-tui — Slice D: UX overhaul (framed panels + chat transcript + framed modals)

**Carlos's binding UX directive (2026-07-14):** without best-in-class UX/UI the product goes unused
no matter how good the engine — so this slice is graded by FEEL, not just "renders." It must be
self-teaching (zero handholding), delightful enough to use for small things, and match the
CodeWhale bar the owner referenced. Direction chosen: **framed panels + chat-style transcript.**

### 8.0 Scope + frozen
- Touch **crates/nh-tui ONLY.** All other crates unchanged (source-compatible). nh-tui public API
  (`run`, `App`, `Status`, `AgentEvent`, `ApprovalRequest`, `TuiConfig`, `apply_event`) keeps its
  signatures — internal rendering refactor. No new deps (ratatui + crossterm only). No async.
- All §0 hard invariants still hold: every rendered string via `nh_vault::safe_line`; RAII terminal
  restore + panic hook intact on every exit path; exec still approval-gated; keys unchanged.

### 8.1 Render safety envelope (the artifact budget, tightened for borders)
- Borders: **plain single-line only** (ratatui `BorderType::Plain` / `Borders::ALL`). No rounded,
  double, thick, block, or shade glyphs. Named **16-color ANSI only** — no 24-bit truecolor.
- Safe status glyphs only (`●`/`○`, `◆`, `❯`, `↑`/`↓` render on all three Windows terminals);
  **no emoji** (e.g. no `⚡`) — use a plain-word `cached` label. If unsure a glyph renders on
  ConHost legacy, fall back to ASCII.

### 8.2 Main screen (one outer frame)
- Outer bordered `Block`; top-border titles L→R: ` nosis ` (bold), **status chip** `● WORD`
  colored by state (IDLE dim `○`, WORKING green, WAITING amber, BLOCKED red), route id (cyan,
  right). Inside, top→bottom: transcript (fills) → separator → input line → footer HUD.
- **Chat-style transcript:** each turn shows a role label then indented content —
  user (`TranscriptKind::Task`) → `❯ you` (cyan bold); assistant (`Answer`) → `◆ nosis` (bold);
  `Progress` → dim `· …`; `Approval` → highlighted `approve? <cmd>  [y/N]` (amber); `Error` →
  friendly `! <what> — <do this>` (red, never a trace). Thin dim rule or blank line between turns,
  newest at bottom. Roles derive from the existing `TranscriptKind` — no new public plumbing.
- **Input:** `❯ ` (cyan) + text + visible cursor; empty → dim placeholder (`type a task and press
  Enter…`).
- **Footer HUD:** existing hud data/helpers (`hud_line`/`peak_status`), grouped + readable:
  `in N · out N · cached N · cache NN% · <peak/off-peak chip> · [###----] NN%` (budget bar only
  when `--budget` set; cache omitted before any usage, as today). Do NOT reimplement pricing.

### 8.3 Self-teaching + delight (graded)
- **Always-visible key-hint strip** (one dim line, footer/above-input): `? commands   t trust
  l timeline   q quit`. This is the anti-handholding affordance.
- **Friendly empty state** (fresh launch, empty transcript): warm centered welcome inside the
  frame — greeting + `Type a task and press Enter.` + one example + `Press ? to see everything
  nosis can do.` Replaced by the conversation once a task runs.

### 8.4 Framed modals (fixes the bleed — the #1 defect)
- Each overlay (Trust Dial, Commands+Tools, Timeline) = a **centered** `Block` + `Borders::ALL` +
  title (` Trust Dial · read-only `, ` Commands + Tools `, ` Timeline `) + a one-line
  what-this-is/keys row + 1-col-padded body. `Clear` the ENTIRE block area so nothing from the
  transcript shows inside or around it. Size ≈ min(width−4, 76) × clamped height, centered.
  Replaces the current `inset()`-margin approach that leaks the transcript around the edges.
  Timeline's rail/detail two-column layout lives INSIDE this frame.

### 8.5 Tests (headless TestBackend; keep all 239 green, weaken nothing)
- **Anti-bleed regression (key):** with a non-empty transcript, open each overlay; assert its
  bordered interior holds only overlay content + an intact border ring, no residual transcript char.
- Outer frame + status chip word render for each `Status`; chat role labels (`you`/`nosis`) +
  turn separation; empty-state welcome present on a fresh App, gone after a task; key-hint strip
  present; scrub holds on every new surface (fake key literal → redacted, no control chars).

### 8.6 Gate
- `cargo test --workspace` (≥239 pass, 0 fail) + `cargo clippy --workspace --all-targets -- -D
  warnings` clean before handoff. (Kaspersky blocking the `wire_clients` exe = AV/env, not code.)

---

## 9. nh-tui — Slice E: interaction model (slash commands) + live controls + scroll + identity

**Owner smoke of Slice D (2026-07-14) surfaced two blocking bugs + missing controls.** Bare
single-letter shortcuts (`t`/`l`/`q`/`?`/`R` on empty input) collide with typing any task that
starts with those letters; the transcript won't scroll with arrows/wheel (only PageUp/Down/End);
and the model/effort controls + model switching are hidden. Also: DeepSeek self-identifies as
"Claude" (training contamination — verified routing via receipts = `deepseek-v4-flash`), which
erodes trust. Carlos's decision (2026-07-14): move to a **type-freely + slash-command** model
(CodeWhale/Claude-Code feel). UX + security are the product's differentiators.

### 9.0 Scope + frozen
- Touch **crates/nh-tui + crates/nh-cli/src/cmd_tui.rs (+ helpers) ONLY.** Frozen: nh-core,
  nh-tools, nh-law, nh-routes, nh-vault. nh-tui public API changes are ADDITIVE only (TuiConfig may
  gain fields; `run`/`apply_event` stay source-compatible). If a frozen-crate edit seems required,
  STOP and request an amendment. `TuiConfig.resolver` (already present) enables live route switching.
- All §0 + §8.1 invariants hold: `safe_line` on every rendered string; RAII restore + panic hook on
  every exit path; exec approval-gated; plain single-line borders, 16-color, ConHost-safe glyphs,
  no emoji.

### 9.1 Type-freely + slash commands (the core fix)
- REMOVE every bare-letter-on-empty shortcut. All printable keys type into the input.
- When input starts with `/`, show a live command menu (reuse/extend the palette) filtered by the
  text after `/`; Enter runs it; Esc/clearing closes. Commands: `/help` (=`/?`) opens the full
  palette; `/trust`; `/timeline`; `/model <id>`; `/provider <name>`; `/effort <none|low|high|max>`;
  `/quit`. Unknown → one friendly line. Non-slash Enter dispatches a task (unchanged). Keep Ctrl-C
  quit, Esc close-overlay.

### 9.2 Scroll (fix "stuck")
- Transcript scrolls via Up/Down (line), PageUp/PageDown (page), End (newest), and MOUSE WHEEL.
  Overlays keep Up/Down for their own navigation. Enable mouse capture on startup; DisableMouse
  capture on EVERY teardown (guard + panic hook) — no residue. Show a dim honest overflow hint
  (`↑ more` / `↓ more`); a full Scrollbar is drop-if-hard.

### 9.3 Live model/provider switch (mirror cmd_chat's proven `switch_to`)
- `/model`/`/provider` re-resolve via `TuiConfig.resolver` and rebuild the worker's client for the
  new route while KEEPING history + transcript + session usage; header updates; a transcript line
  notes "context kept · cache resets". Keyless target behaves like launch (friendly key line, no
  crash). HIGHEST RISK — if a clean live rebuild is genuinely hard, DROP-IF-HARD: recognize
  `/model` with an honest "restart with --model <id> (live switch coming)" line; never ship a
  half-broken switch. Report which path was taken.

### 9.4 Live effort switch
- `/effort <none|low|high|max>` sets `AgentLoop.thinking` for subsequent turns; header shows current
  effort. Default stays the route dialect default.

### 9.5 Honest identity (trust/security)
- Compose (in the cmd_tui/nh-tui config layer — NOT nh-core) a system-prompt preface: "You are
  nosis, an autonomous coding harness running on route '<route_id>' via <provider>. If asked what
  model/assistant you are, answer 'nosis on <route_id>'; never claim to be Claude, GPT, or any other
  assistant." Fold into the constitution string passed to the loop; keep it byte-stable per route
  (cache discipline). Updates on `/model` switch. Test: composed prompt contains the route id + the
  "never claim" instruction.

### 9.6 Also
- Header shows route + current effort; key-hint strip → `/ commands   ↑↓ scroll   Enter send
  Ctrl+C quit`. Fix pre-alt-screen stderr mojibake (em-dash → ASCII `-`, or set console UTF-8) in
  cmd_tui/cmd_run warnings.

### 9.7 Gate — same as §8.6; keep all Slice D visuals + tests green, weaken nothing.
