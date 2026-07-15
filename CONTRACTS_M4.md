# CONTRACTS_M4.md — Locked public API for Milestone M4 (Fleet + swarm + scheduler + nh-mcp)

**Status: LOCKED (orchestrator Opus 4.8; owner scope-approved 2026-07-15).** Builder = GPT-5.6
Sol xhigh via `codex exec`. Claude plans + gates; Sol implements EXACTLY these public surfaces.
Spec source: `NOSIS_HARNESS_Master_Plan.md` §3 (escalation ladder, line 115/339), §4 (Fleet &
swarm, lines 119–123), §4.5 (MCP: nh-mcp server, lines 139–174), `MILESTONES.md §Milestone 4`,
`02-architecture/SECURITY_MODEL.md`. Amendments go through the orchestrator only, additive, logged §8.

**Owner scope rulings (2026-07-15) — the four decisions, resolved:**
1. **A-M4-1 authorized** — OAuth2 in the frozen `nh-tools` is the one sanctioned frozen-crate write
   (+ additive nh-vault keyring setter A-M4-2). Every other frozen need still STOPS for an amendment.
2. **Opus 4.8 gate = review-pause** (Slice B Option A). No live delegate call in M4.
3. **nh-mcp HTTP server = `tiny_http`** (blocking, no tokio).
4. **Kimi Swarm = thin seam + verify-live, kept MINIMAL** (owner: "don't overdo it, budget") —
   Native backend done; KimiSwarm mock-tested + live-pending; no gold-plating.

---

## 0. Ground rules (bind every builder)

### 0.1 Green + frozen
- **M0–M3 stay green.** All existing public APIs stay source-compatible. `cargo test --workspace`
  (261 pass / 1 ignored) + `cargo clippy --workspace --all-targets -- -D warnings` clean before
  every handoff. Add tests; never weaken an existing assertion.
- **FROZEN crates: nh-core, nh-tools, nh-law, nh-routes, nh-vault.** M4 adds two NEW consumer
  crates (`nh-fleet`, `nh-mcp`) and one CLI surface (`nh-cli`). The fleet DRIVES the existing
  `nh_core::agent::AgentLoop` + `nh_routes::RouteResolver` — it does not reimplement them.
  - The **one** sanctioned frozen-crate exception is OAuth2 in `nh-tools` (§5, exit criterion E4),
    which is impossible to satisfy without touching `nh_tools::mcp` (it currently `bail!`s
    "oauth2 arrives in M4"). That edit is pre-authorized as amendment **A-M4-1** (owner-approved
    2026-07-15) and is the only frozen-crate write in M4. Any other frozen need → STOP, amend first.
- **Catalog/pricing/law stay DATA.** Adding routes or flags to `catalog.toml` is data, allowed;
  changing a wire adapter is frozen code, not allowed without an amendment.

### 0.2 THE LAW + UX-first
- THE LAW (top authority): small, simple, secure, safe, lightweight, readable, auditable, modular,
  congruent, harmonic. Reuse over duplication (the ledger reuses `nh_core::receipt`; the scheduler
  reuses `nh_routes` peak logic; nh-mcp mirrors the wire `nh_tools::mcp` already speaks).
- **UX-first STILL governs every surface M4 adds** — graded by FEEL, not just "it runs." Fleet
  progress, `nh fleet status`, and every nh-mcp response are one scannable line each: no walls of
  JSON, no raw stack traces, no ambiguous spinners. `drop-if-hard`. See [[ux-first-and-the-law]].

### 0.3 Security invariants (carry from M0–M3)
- **Every rendered/logged/persisted string passes `nh_vault::Scrubber` first** — ledger events,
  receipts, fleet status, nh-mcp responses, OAuth error lines. Keys never hit disk or the wire in
  the clear. `nh_vault::safe_line` for anything shown to a human.
- **exec_shell stays approval-gated.** Fleet workers run the SAME `AgentLoop` + `ToolCtx`; a fleet
  run does not silently widen autonomy. Head­less/unattended fleet uses the law's `guard` exactly
  as `nh run` does — max autonomy may auto-approve writes, NEVER exec (SECURITY_MODEL invariant).
- **nh-mcp does NOT ship publicly before the MCP final spec (2026-07-28).** It binds `127.0.0.1`
  by default, carries a "local/preview only" banner, and the outbound-header secret lint + response
  scrubbing from M1 apply. Public exposure is a post-2026-07-28 decision, not M4's.

### 0.4 Dependency additions (orchestrator-authorized here)
- `nh-fleet`: no new external crates — std threads + channels (mirror the nh-tui `Worker` shape),
  `serde`/`serde_json`/`chrono`/`anyhow` (workspace), path-deps on nh-core/nh-routes/nh-law/
  nh-tools/nh-vault. No async runtime.
- `nh-mcp`: **`tiny_http`** (owner-approved 2026-07-15) — blocking, ~zero transitive deps, no
  tokio; congruent with the no-async-runtime stance from M3. Added to workspace deps, used only by
  nh-mcp. (Considered + rejected: hand-rolled `std::net::TcpListener` = more parsing code to harden;
  `axum` = pulls tokio, breaks the no-async posture.)

### 0.5 M4 exit criteria (MILESTONES.md; each maps to a slice + a real test)
- **E1** — a 10-task fleet run survives `kill -9` and resumes idempotently (completed tasks are
  NOT re-run; every task reaches exactly one terminal state). → **Slice A.**
- **E2** — a deferred job executes off-peak. → **Slice B.**
- **E3** — KORVIN connects to nh-mcp and triggers a fleet run. → **Slice C.**
- **E4** — OAuth refresh survives a forced expiry mid-session. → **Slice D.**

---

## Slice A — nh-fleet: append-only ledger + workers + typed receipts + idempotent resume (E1)

New library crate `crates/nh-fleet` (headless-testable; no terminal, no server). The crux of M4.

### A.1 Task input (`nh fleet run tasks.json`)
```jsonc
{
  "tasks": [
    { "id": "fix-auth-test",              // OPTIONAL stable id; if omitted, derived (A.3)
      "task": "fix the failing test in crates/foo",
      "model": "deepseek-v4-flash" },     // OPTIONAL; falls back to the run/default route
    { "task": "update the changelog" }
  ],
  "max_workers": 4,                        // OPTIONAL (default 4; CLI --max-workers overrides)
  "budget_tokens": 1000000,                // OPTIONAL run-level hard stop (A.6)
  "defer_offpeak": false                   // OPTIONAL (Slice B)
}
```

### A.2 The ledger — append-only, fsync-durable (the E1 foundation)
- One run → directory `.nosis/fleet/<run_id>/`; the ledger is `ledger.jsonl`, **append-only**.
- A top-level `.nosis/fleet/index.jsonl` records `{run_id, created_utc, task_count, status}` so
  `nh fleet resume` (no id) finds the latest incomplete run.
- **Durability:** every event is `write` → `flush` → **`File::sync_all()` (fsync)** before the
  writer acknowledges. A `kill -9` cannot lose a committed event. A SINGLE writer (one mutex-guarded
  append point; workers send events over a channel) serializes order + fsync — the ledger is the one
  source of truth. Every event is `Scrubber`-scrubbed before the line is written (same discipline as
  `nh_core::receipt::ReceiptWriter`).
```rust
#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum LedgerEvent {
    RunStarted   { run_id: String, created_utc: String, task_count: usize,
                   max_workers: usize, budget_tokens: Option<u64> },
    TaskQueued   { task_id: String, task: String, route_id: String },
    TaskStarted  { task_id: String, route_id: String, effort: String, attempt: u32 },
    TaskReceipt  { task_id: String, attempt: u32, receipt: nh_core::receipt::Receipt },
    TaskEscalated{ task_id: String, from_route: String, to_route: String, reason: String }, // Slice B
    TaskDone     { task_id: String, outcome: nh_core::receipt::Outcome },   // terminal
    TaskGate     { task_id: String, reason: String },                       // terminal (Slice B gate)
    TaskFailed   { task_id: String, reason: String },                       // terminal
    RunFinished  { run_id: String, done: usize, failed: usize, gated: usize },
}
```

### A.3 Stable task ids — the idempotency key
Each task has a **stable** `task_id`: the caller's `id` if given, else deterministic
`format!("t{index:03}-{hash8}")` where `hash8` is a short digest of the trimmed task text. Same
tasks.json → same ids across runs/resumes → resume matches reliably. `id` collisions are rejected
with one friendly line before the run starts (fail fast, don't silently merge).

### A.4 Workers + run loop
- A bounded pool of `max_workers` std threads (mirror `nh_tui::Worker`: channels, no async). Each
  worker pulls a queued task, builds an `AgentLoop` for the task's resolved route — reusing the
  **exact `nh run` construction recipe** (resolve route → build `dyn ChatClient` → law/scrubber/
  receipts/constitution) so behavior is identical to a single-task `nh run` — runs
  `run_with_history` over a FRESH history (fleet tasks are independent), and reports the receipt.
- Progress is one scannable line per task-state change (queued → running → done/failed/escalated),
  surfaced via a `Fn(&str)` callback the CLI prints (core stays print-free, like `on_event`).
- **Heartbeats** (plan §4): a `TaskHeartbeat { task_id, ts }` is appended every ~5 s while a task
  runs, so `nh fleet status` can show liveness and resume can flag a stalled task. `drop-if-hard`
  (the resume model in A.5 does not depend on heartbeats for correctness).

### A.5 Idempotent resume (`nh fleet resume [<run_id>]`) — the E1 crux
Pure, unit-testable fold, then re-enqueue:
```rust
pub struct ResumePlan { pub done: Vec<String>, pub todo: Vec<String> }
/// Fold the on-disk ledger into "already terminal" vs "must (re)run". Pure — no I/O.
pub fn plan_from_ledger(events: &[LedgerEvent]) -> ResumePlan;
```
- `TaskDone` / `TaskFailed` / `TaskGate` = **terminal** → never re-run.
- `TaskStarted` with no terminal event = **interrupted** → re-run from scratch (at-least-once; the
  guarantee is *exactly one terminal record per task*, never a double completion).
- `TaskQueued` only = not yet started → run.
- Resume **appends to the same ledger** and reaches: every task has exactly one terminal event.

### A.6 Budget hard-stop (congruent with the M3 HUD budget)
`budget_tokens` caps cumulative usage across the run. On exceed: stop dispatching NEW tasks, let
in-flight ones finish, write `RunFinished` with a `budget halted` note. No projected/fake cost.

### A.7 Public surface (Slice A)
```rust
pub struct TaskSpec { pub id: Option<String>, pub task: String, pub model: Option<String> }
pub struct FleetConfig { /* resolver, law, default route, tasks, max_workers, budget, run_root */ }
pub struct RunReport { pub run_id: String, pub done: usize, pub failed: usize, pub gated: usize }
pub enum LedgerEvent { /* A.2 */ }
pub struct ResumePlan { /* A.5 */ }

pub fn run(config: FleetConfig) -> anyhow::Result<RunReport>;
pub fn resume(run_root: &std::path::Path, run_id: Option<&str>, config: FleetConfig)
    -> anyhow::Result<RunReport>;
pub fn plan_from_ledger(events: &[LedgerEvent]) -> ResumePlan;
```
CLI (nh-cli, additive): `nh fleet run <tasks.json> [--max-workers N] [--budget T]` and
`nh fleet resume [<run_id>] [--max-workers N]`. Keyless start behaves like `nh chat`: a friendly
"run `nh key add`" line, no crash.

### A.8 Tests (headless)
- `plan_from_ledger`: a hand-built ledger with a mix of done / interrupted / queued yields the exact
  todo set; terminal tasks never reappear; interrupted tasks reappear exactly once.
- Durability: after `run`, the on-disk ledger parses; each committed event round-trips; the last
  line is `RunFinished`; every task has exactly one terminal event.
- **E1 integration (the real check):** a test spawns `nh fleet run` (10 tasks) against a **loopback
  mock provider** (a `ChatClient` test double, echo-style tasks), polls the ledger until ≥N tasks
  are `TaskDone`, **kills the child process** (`kill -9` equivalent — `Child::kill` on Windows), then
  runs `nh fleet resume` and asserts: (a) all 10 reach a terminal state, (b) the pre-kill `TaskDone`
  tasks have NO second `TaskStarted` (not re-run), (c) the mock records each completed task executed
  once. The literal Predator `kill -9` on a keyed run stays a verify-live smoke (§7).
- Scrubber holds: a task/receipt containing a fake key literal is `[REDACTED]` in the ledger.

---

## Slice B — off-peak scheduler + escalation ladder + Kimi Swarm passthrough (E2)

Builds on Slice A. No frozen-crate edits (reuses `nh_routes` peak logic + `Receipt` outcomes).

### B.1 Off-peak scheduler (E2)
- A run or task carries `defer_offpeak = true` (and/or `defer_until = "HH:MM"`). Before dispatching
  a deferred task the scheduler checks the task route's peak state via the **existing**
  `nh_routes::ResolvedRoute::peak_status`/`price_at` (the SAME helper the M3 HUD uses — do NOT
  reimplement clock pricing). In a peak window → the task parks; when off-peak → it dispatches.
- **Clock is injected** (`trait Clock { fn now(&self) -> DateTime<Utc>; }`, default = system) so the
  schedule is testable without waiting. Pure seam:
  `pub fn ready_to_dispatch(route: &ResolvedRoute, now: DateTime<Utc>) -> bool`.
- Routes with no peak data → always off-peak (dispatch now). Honest: never fabricate a window.
- **E2 test:** a deferred task on a peak-windowed route does not dispatch while `now` is inside the
  window; advancing the injected clock to off-peak dispatches it. `RunReport`/ledger show it ran.

### B.2 Escalation ladder (plan §3 line 115/339)
- Default ladder (overridable in config), ordered tiers of `(route_id, effort)`:
  `(deepseek-v4-flash, none)` → `(kimi-k2.7-code, high)` → `(deepseek-v4-pro, high)` →
  `(deepseek-v4-pro, max)` → **GATE(opus-4.8)**.
- Policy (plan: "2 failures per tier, receipt attached; never silently retry the same route more
  than twice"): each tier gets ≤2 attempts; on the 2nd `Outcome::{Fail,Timeout}` (Partial =
  configurable), escalate one tier, writing `TaskEscalated{from,to,reason}` with the **failure
  Receipt attached** (receipts + typed reason, NEVER raw transcripts — plan line 297).
- The terminal **Opus 4.8 gate = review-pause** (owner-approved 2026-07-15): the task stops with
  `TaskGate{reason}`, the accumulated failure receipts are attached, and `RunReport.gated` surfaces
  it for the human/orchestrator to review. nosis has no first-party Opus API route (Opus is the
  reviewer, not a fleet worker); the *live* delegate route (`claude -p` headless) is explicitly OUT
  of M4 — no delegate `ChatClient` adapter, no client-factory (frozen nh-routes) touch this cycle.
- Pure seam: `pub fn next_step(ladder, tier_idx, attempt, outcome) -> Step` where
  `Step = Retry | Escalate(tier) | Gate | Done`. Unit-tested across the whole ladder.

### B.3 Kimi Swarm passthrough (plan §4 line 122) — MINIMAL seam (owner: "don't overdo it, budget")
- A task may set `backend = "kimi-swarm"`: instead of the native worker loop, the fleet writes the
  swarm brief, submits it to a Kimi swarm route, and collects the result as ONE typed receipt.
- **Locked scope (2026-07-15):** implement the `Backend { Native, KimiSwarm }` enum with `Native`
  fully done and `KimiSwarm` as the SMALLEST honest seam — ONE mock-server test proving the
  submit→collect shape produces a receipt, then marked **live-pending** (real Agent-Swarm endpoint +
  Kimi key deferred). Do NOT build polling/retry/streaming machinery, do NOT touch the frozen wire.
  If a real swarm client would need a wire change, STOP — it waits for M6. Budget-minimal by directive.

---

## Slice C — nh-mcp server: route-resolver + fleet-runner over MCP (E3)

New crate `crates/nh-mcp` (library + the `nh mcp serve` bin lives in nh-cli). The server SPEAKS the
same stateless 2026-07-28 JSON-RPC wire that `nh_tools::mcp::McpClient` already speaks — this is the
congruence lever: the E3 test uses the EXISTING client as "KORVIN" against the NEW server.

### C.1 Wire (mirror the client's contract exactly)
- JSON-RPC 2.0 over HTTP POST. **Stateless core:** no `initialize` handshake, **never** an
  `Mcp-Session-Id` header, ever. Echo `params._meta.protocolVersion` handling; `tools/list` returns
  `result._meta.ttlMs`; `tools/call` returns `content` blocks (`text` / `[<type> block]`) + optional
  `isError`; `GET /.well-known/mcp.json` serves the business card; unknown method → JSON-RPC
  `-32601`. Every response string is `Scrubber`-scrubbed. Bind `127.0.0.1` (§0.3).

### C.2 Tools exposed
- **`route_resolve`** (readOnlyHint: true) — `{ model?, prefer_offpeak? }` → resolved route id,
  provider, thinking dialect, peak/off-peak price line. Reuses `nh_routes::RouteResolver`.
- **`fleet_run`** (state-mutating; gated) — `{ tasks:[...], max_workers?, budget?, defer_offpeak? }`
  → `{ run_id }`. The `run_id` is the **stateless passthrough handle** (exactly like `browser_id`
  in M1) — the run state lives in the Slice-A ledger, not a session. Runs the fleet on a background
  thread; returns immediately with the handle.
- **`fleet_status`** (readOnlyHint: true) — `{ run_id }` → `{ done, failed, gated, pending, finished }`
  read from the ledger. This is how KORVIN polls after triggering.
- Plan also lists `receipts_query` / `cost_estimate` (§4.5) — NOT in the exit criteria; add only if
  cheap, else note as deferred (honest, not silent).

### C.3 Auth + safety
- Optional bearer gate: `--token-entry <vault>` requires `Authorization: Bearer <secret>` (fetched
  via nh-vault). Default local + no-token is allowed ONLY on `127.0.0.1`. The M1 outbound-header
  secret lint + response scrubbing apply. Banner on startup: "nh-mcp preview — local only; do not
  expose publicly before the MCP final spec (2026-07-28)."

### C.4 CLI + test
- `nh mcp serve [--addr 127.0.0.1:PORT] [--token-entry <vault>]`.
- **E3 integration:** spawn `nh mcp serve`; drive it with `nh_tools::mcp::McpClient` (KORVIN's
  role): `tools/list` shows the three tools; `route_resolve` returns a route; `fleet_run` with a
  tiny mock-provider task set returns a `run_id`; polling `fleet_status` shows the run reaches
  `finished`. Assert NO session header on any request (stateless invariant holds server-side too).

---

## Slice D — OAuth2 for the MCP client: refresh survives forced expiry (E4) — FROZEN-CRATE amendment

This is the ONE sanctioned frozen-crate edit (amendment **A-M4-1**, owner-approved 2026-07-15, §8).
It touches `nh_tools::mcp` because that is where `McpAuth::OAuth2` currently `bail!`s "oauth2 in M4".

### D.1 Behavior
- `.nosis/mcp.toml` already parses `auth = "oauth2"` (unknown keys ignored). Add ADDITIVE non-secret
  fields: `token_url`, `client_id`, oauth `scopes`. The `client_secret` + `refresh_token` come from
  the vault (never the TOML in the clear).
- A token manager holds the access token + expiry. On a request: if expired (or the server returns
  `401`), it uses the `refresh_token` grant against `token_url` to mint a new access token, stores
  it, and retries the request **once**. Refresh failure → one friendly scrubbed line, no crash.
- **Token storage:** refresh/access tokens in the OS keyring via nh-vault. If the `Vault` trait
  lacks a setter, add an ADDITIVE `store`/`set` (amendment **A-M4-2**, §8) — additive only, existing
  `get` behavior unchanged. (Fallback if the owner prefers zero nh-vault change: an encrypted-at-rest
  token cache under `.nosis/`, still scrubbed — but keyring is the secure default.)

### D.2 Test (mirrors the M1 loopback-mock style)
- A mock token endpoint + a mock MCP server that returns `401` on an expired/absent access token and
  `200` with a fresh one. Force expiry (access-token expiry in the past, or first call `401`) →
  assert the client refreshes via `refresh_token` and the retried call succeeds. The existing
  `oauth2_is_deferred_to_m4_with_one_message` test is REPLACED by real OAuth tests.
- Header lint still holds: the bearer rides `Authorization` only; never an `Mcp-*`/`x-mcp-*` header.

---

## 6. Slice order + gating
1. **Slice A** (E1 — ledger/workers/resume) — foundation; gate on the kill-9 integration test.
2. **Slice D** (E4 — OAuth) — independent of the fleet; can run in parallel conceptually but Sol
   does ONE slice at a time. Sequenced after A so the first Sol run is the pure-new-crate crux.
3. **Slice B** (E2 — scheduler + ladder + swarm seam) — needs A.
4. **Slice C** (E3 — nh-mcp) — needs A (fleet_run) + reuses the M1 client for its test.
- Gate EACH slice: `cargo test --workspace` (≥261 pass, 0 fail) + `cargo clippy --workspace
  --all-targets -- -D warnings` clean; adversarial review; then commit per-slice on `main` after
  the owner approves (UX FEEL is the gate for any human-facing surface). Kill any `nh.exe` before
  builds (it locks `target\debug\nh.exe`).

## 7. Assumptions / verify-live ledger (M4)
- **E1 real `kill -9`** — the in-CI check kills a child process; the literal Predator `kill -9` on a
  keyed 10-task run is a manual smoke (owner).
- **E2 off-peak** — tested with an injected clock; a real off-peak dispatch against DeepSeek is
  live-pending (and note: DeepSeek peak windows re-verify ~2026-07-24, catalog `valid_until`).
- **E3 KORVIN** — the CI test uses `nh_tools::mcp::McpClient` as the KORVIN stand-in; a real KORVIN
  connection is a manual smoke.
- **E4 OAuth** — mock token endpoint in CI; a real OAuth2 MCP server is live-pending.
- **B.3 Kimi Swarm** — mock-tested; the live Agent-Swarm endpoint + a Kimi key are live-pending.
- **Delegate/Opus-gate live** — OUT of M4 (gate is review-pause). Revisit for a later milestone.

## 8. Integration amendments (append here, dated, orchestrator authority)
- **A-M4-1 (AUTHORIZED 2026-07-15) — OAuth2 in nh-tools:** implement `McpAuth::OAuth2` in
  `nh_tools::mcp::request_headers` + a token manager (refresh on expiry/401, retry once). Additive
  `RawServer` fields (`token_url`, `client_id`, oauth `scopes`). Replaces the "oauth2 arrives in M4"
  `bail!` + its test. The ONLY behavioral change to a frozen crate in M4; authorized because E4
  cannot be met otherwise.
- **A-M4-2 (AUTHORIZED 2026-07-15) — nh-vault token setter:** additive `store`/`set` on the vault
  for refresh/access tokens; existing `get` unchanged. (Keyring is the secure default; the `.nosis/`
  cache fallback in D.1 is NOT used unless keyring is unavailable at runtime.)
