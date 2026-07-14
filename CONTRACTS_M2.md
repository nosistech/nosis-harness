# CONTRACTS_M2.md — Locked public API for Milestone M2 (context engine + law)

**Status: LOCKED (draft by orchestrator Opus 4.8, 2026-07-13).** Builders (GPT-5.6 Sol
xhigh, via `codex exec`) implement EXACTLY these public surfaces; private helpers are
free, public deviations are not. Spec source: `NOSIS_HARNESS_Master_Plan.md` §3 (cache
discipline, escalation), §5 (UX: trust dial #3, degradation guard #8), §6 (M2),
`02-architecture/SECURITY_MODEL.md` (constitution + write-holds), and `MILESTONES.md`
(M2 exit). Amendments go through the orchestrator only, additive, logged in §7.

---

## 0. Ground rules (bind every builder)

- **M0 + M1 stay green.** All existing public APIs remain source-compatible except the
  explicit amendments in §5.2. `cargo test --workspace` + `cargo clippy --workspace
  --all-targets -- -D warnings` clean before every handoff (currently 180 pass / 1
  ignored). Add tests; never weaken an existing assertion to make room.
- **THE LAW** (top authority): small, simple, secure, safe, lightweight, readable,
  auditable, modular, congruent, harmonic. **UX IS THE PRODUCT**: every user-facing line
  short, concrete, actionable; no stack traces; drop-if-hard.
- **No new external crates** without an orchestrator amendment. nh-law does its own
  minimal glob matching (no `glob`/`globset` dep) and finds the home dir via env
  (`USERPROFILE` on Windows, else `HOME`) — no `dirs` dep. Workspace-version deps only.
- **No plaintext secrets**; every output path still passes `nh_vault::Scrubber`.
- **`exec_shell` always passes the approval gate — no autonomy level overrides it**
  (AGENTS.md hard rule). Max autonomy may auto-approve *file writes*, never exec.
- **Catalog/pricing stays DATA** (`catalog.toml`); law/constitution is DATA
  (`.nosis/law.toml`, `AGENTS.md`, bundled defaults) — never hard-coded policy in Rust
  beyond the shipped bundled default.

**M2 exit criteria (plan §6, MILESTONES.md):**
1. **cache-hit % > 60% on a 50-turn session** — proven by an integration test against a
   prefix-caching mock provider (below). If the harness ever mutates the stable prefix,
   the metric collapses and the test fails.
2. **protected path blocked even in max autonomy** — `edit_file` on a law-protected path
   returns a Block result and does not modify the file, with autonomy = `auto`. Proven at
   three levels: nh-law unit, nh-tools guard unit, nh-cli end-to-end (`nh run --autonomy auto`).

---

## 1. nh-law — NEW leaf crate (constitution loader + trust/write-hold policy)

New workspace member `crates/nh-law`. Depends only on `serde`, `toml`, `anyhow`
(workspace versions) + std. **No dependency on any other nh-* crate** (leaf; keeps the
governance logic isolated and independently testable — THE LAW: modular, auditable).

### 1.1 Types

```rust
/// Autonomy level. `Auto` = "max autonomy" (exit criterion). Comes ONLY from the
/// user-global law or the CLI — never from repo law (security boundary, §1.5).
pub enum Autonomy { Ask, Auto }        // Default: Ask

/// Verdict for one access, autonomy already folded in.
pub enum Verdict { Allow, Ask, Block(String) }   // Block carries a short reason for the UX

/// Compiled, immutable policy. Built once per session.
pub struct Policy { /* private: compiled write/exec globs + autonomy */ }

/// Everything a session needs from the law layer.
pub struct Law {
    pub constitution: String,     // the byte-stable system prefix (§1.3)
    pub policy: Policy,           // §1.4
    pub warnings: Vec<String>,    // friendly one-liners (bad law file, repo tried to raise autonomy…)
}

/// Inputs the caller controls (highest-precedence autonomy).
pub struct LoadOptions { pub cli_autonomy: Option<Autonomy> }   // None = fall back to law files
```

### 1.2 Entry points

```rust
/// The bundled default constitution + policy, embedded at build time (include_str!).
/// Always present; opens with the coding-agent preamble, then THE LAW, then the
/// default protected paths. This is the ONLY policy hard-coded in Rust.
pub const BUNDLED_LAW: &str;                    // a TOML document (schema §1.6)

/// Starter `.nosis/law.toml` written by `nh init` (commented, safe defaults).
pub const STARTER_LAW_TOML: &str;

/// IO convenience: read every source under `repo_root` (+ the user-global law), assemble
/// the constitution, compile the policy. NEVER hard-fails — unreadable/omitted sources
/// and malformed TOML become `warnings` and fall back to defaults (robustness > strictness).
pub fn load(repo_root: &std::path::Path, opts: &LoadOptions) -> Law;

/// Pure assembly (no IO) — the unit-test seam for the constitution.
pub fn assemble_constitution(sources: &ConstitutionSources) -> String;

pub struct ConstitutionSources {
    pub bundled: &'static str,       // always Some content
    pub user_law_text: Option<String>,   // [constitution].text from ~/.nosis/law.toml
    pub repo_law_text: Option<String>,   // [constitution].text from <repo>/.nosis/law.toml
    pub agents_md: Option<String>,       // <repo>/AGENTS.md
    pub memory: Option<String>,          // <repo>/.nosis/memory.md
}
```

### 1.3 Constitution assembly (byte-stable system prefix)

- Sources concatenated in this fixed order, each as a labeled section, **section omitted
  entirely when its source is absent or blank** (order = the plan's precedence:
  bundled law → user law → repo law → AGENTS.md → memory):
  1. `## Operating law` — the bundled preamble + THE LAW (from `BUNDLED_LAW`'s
     `[constitution].text`).
  2. `## User law` — user-global `[constitution].text`.
  3. `## Project law` — repo `[constitution].text`.
  4. `## Project instructions (AGENTS.md)` — verbatim AGENTS.md.
  5. `## Memory` — verbatim `.nosis/memory.md`.
- **Deterministic and byte-stable**: pure function of its inputs; NO timestamps, NO
  environment, NO ordering nondeterminism. Assembled once at session start and reused
  verbatim for every turn. Trailing whitespace normalized (single trailing `\n`).
- Section joiner is a constant (`"\n\n"`); labels are constants. Same inputs ⇒ identical
  bytes, every run. Unit test: byte-equality across two calls; section-omission when a
  source is `None`/blank; order.

### 1.4 Policy compilation + evaluation

```rust
impl Policy {
    /// Effective verdict for writing/creating a file at `rel_path`
    /// (workdir-relative, forward-slashed, lexically normalized — the caller passes the
    /// same normalized path the tool resolved). Autonomy already folded in:
    ///   block-glob  -> Block(reason)        (hard write-hold; autonomy NEVER overrides)
    ///   ask-glob    -> Ask                  (overrides Auto)
    ///   auto-glob   -> Allow                (pre-blessed; no prompt even at Ask autonomy)
    ///   unlisted    -> Allow if Auto, else Ask
    /// Most-restrictive match wins (block > ask > auto). Matching is case-sensitive on
    /// the normalized path.
    pub fn write_verdict(&self, rel_path: &str) -> Verdict;

    /// Effective verdict for a shell command. ONLY `Block` or `Ask` — exec is never
    /// auto-allowed (AGENTS.md hard rule). block-glob -> Block(reason); else Ask.
    pub fn exec_verdict(&self, command: &str) -> Verdict;

    pub fn autonomy(&self) -> Autonomy;
}
```

- **Glob matcher** (in-crate, no dep): matches a normalized relative path against patterns
  built from `/`-separated segments. Supports `*` (any run within one segment, not
  crossing `/`), `?` (one non-`/` char), and `**` (spans zero or more segments, including
  across `/`). A leading `**/` matches at any depth; a trailing `/**` matches everything
  under a dir. Literal segments match exactly. Exhaustive unit tests: `src/**`,
  `migrations/**`, `**/*.pem`, `.git/**`, `a/b.rs`, `*.toml`, `**` (matches all),
  boundary cases (`src` vs `src/x`, dotfiles).
- **exec globs** match against the raw command string (first token AND whole string): a
  pattern with no `/` is matched against both the command's first whitespace token and the
  full command (so `rm` blocks `rm -rf x`, and `git push*` matches the full line). Keep
  the rule in ONE function; unit-test `rm`, `curl *`, `git push`.

### 1.5 Source precedence + the repo-cannot-weaken-you security boundary

- **Policy globs** (`write.auto/ask/block`, `exec.ask/block`) MERGE by union across
  bundled ∪ user ∪ repo. Union + most-restrictive-wins means a repo can only ADD
  protections. Bundled `block` globs therefore always apply and can never be downgraded.
- **`repo` law.toml may set ONLY**: `[constitution].text`, `write.ask`, `write.block`,
  `exec.ask`, `exec.block`. If a repo law.toml sets `[autonomy]` or `write.auto`, those
  keys are **IGNORED** and produce a `warning` ("repo .nosis/law.toml cannot raise
  autonomy or auto-approve paths — ignored"). Rationale (SECURITY_MODEL, lethal-trifecta):
  **a cloned/untrusted repo must never weaken the user's safety posture, only strengthen
  it.** This is a hard, test-covered rule.
- **Autonomy resolution** (first that applies): `opts.cli_autonomy` → user-global
  `[autonomy].default` → bundled default (`Ask`). Repo law never participates.

### 1.6 `.nosis/law.toml` schema (also the shape of `BUNDLED_LAW` / `STARTER_LAW_TOML`)

```toml
[constitution]
text = "…"                       # optional prose appended to the system prefix

[write]
auto  = ["src/**"]               # auto-approve edits (ignored from repo law; user/bundled only)
ask   = ["migrations/**"]        # always ask (any layer)
block = [".git/**", ".nosis/**", "**/*.pem", "**/*.key", "**/id_rsa*", "**/.env*"]  # hard write-hold

[exec]
ask   = []                       # extra commands to always ask (exec already asks by default)
block = []                       # commands hard-blocked

[autonomy]
default = "ask"                  # ask | auto  (ignored from repo law; user/CLI/bundled only)
```

- Unknown keys ignored (forward-compatible). Unknown `autonomy.default` value → warning +
  fall back to `ask`. Bundled default MUST include the coding-agent preamble in
  `[constitution].text`, the `block` list above, `auto = []`, `autonomy.default = "ask"`.

---

## 2. nh-tools — mechanical write-hold enforcement in the tool choke point

The guard lives where mutation happens (auditable, un-bypassable by model text). nh-tools
gains its OWN small enum surface and a `guard` on `ToolCtx`; it does **not** depend on
nh-law (nh-cli bridges the two).

### 2.1 New surface

```rust
/// What a tool is about to do. `Write`=file path (workdir-relative, normalized); `Exec`=command.
pub enum Access<'a> { Write(&'a str), Exec(&'a str) }

/// The guard's answer. `Block` carries a short reason for the user-facing line.
pub enum Guard { Allow, Ask, Block(String) }

/// Consulted before any mutation. Send + Sync.
pub type GuardFn = Box<dyn Fn(&Access) -> Guard + Send + Sync>;
```

`ToolCtx` gains `pub guard: GuardFn` (amendment §5.2). To keep every construction site
compiling and behavior identical by default:

```rust
impl ToolCtx {
    /// Default guard: Write -> Allow, Exec -> Ask (exactly M0/M1 behavior).
    pub fn new(workdir: PathBuf, approve: Box<dyn Fn(&str) -> bool + Send + Sync>) -> Self;
    /// Builder: install a real policy-backed guard.
    pub fn with_guard(self, guard: GuardFn) -> Self;
}
```

- **`EditFile`** (currently ungated) consults the guard on the **normalized
  workdir-relative path** (same normalization used by `resolve_in_workdir`, forward-slashed)
  BEFORE writing:
  - `Block(reason)` → return `Ok("blocked by law: {reason}")` (Ok-shaped, model-readable),
    file untouched.
  - `Ask` → `ctx.approve("edit {path}")`; denial → `Ok("user denied: edit {path}")`.
  - `Allow` → proceed (M0/M1 default path).
  - Path-escape refusal still happens first (unchanged).
- **`ExecShell`** consults the guard on the command:
  - `Block(reason)` → `Ok("blocked by law: {reason}")`, command never runs.
  - `Ask` → existing approval gate (unchanged wording/flow).
  - `Allow` → **not reachable for exec** (policy never returns Allow for exec); if a custom
    guard returns Allow anyway, still run — but the shipped policy guarantees Ask/Block.
- MCP adapters keep their own M1 trust logic; they do **not** consult `guard`. State this.

Unit tests: protected-path Block on `edit_file` leaves the file unchanged and is Ok-shaped;
Ask routes edit through approve; default `ToolCtx::new` preserves exact M0/M1 behavior
(edit proceeds, exec asks).

---

## 3. nh-core — byte-stable prefix, cache-hit metric, compaction at 70%

All within existing modules. No new deps.

### 3.1 AgentLoop amendments (§5.2)

```rust
pub struct AgentLoop {
    // …existing fields unchanged…
    /// Byte-stable system prefix (the assembled constitution). When Some, it becomes the
    /// system message for an empty history, verbatim. When None, the M0/M1 default
    /// system message (coding-agent + tool names) is used — backward compatible.
    pub constitution: Option<String>,
    /// Effective context window in tokens (from route.context). Some => compaction armed.
    pub context_limit: Option<u64>,
}
```

- **Byte-stable prefix discipline**: `history[0]` (the system message) is set once and
  **never mutated** for the life of the session. Dynamic content (tool results, new user
  turns, model output) is only ever *appended* — never spliced into or before the prefix.
  Add a debug-assert / test that `history[0]` bytes are identical before and after each turn.
- The prefix must contain **no per-turn dynamic data** (no timestamps, no changing tool
  list interpolation). The tool schemas travel in the `tools` request field (already true),
  not in the system text.

### 3.2 Cache-hit metric

```rust
/// Session cache-hit percentage (0.0..=100.0) from cumulative usage.
/// None when prompt_tokens == 0 (nothing to divide) — callers omit the chip then.
pub fn cache_hit_pct(prompt_tokens: u64, cached_tokens: u64) -> Option<f64>;
```

- Pure, in `nh_core::wire`. `100.0 * cached / prompt`, clamped to `[0,100]`.
- **No receipt schema change** — `Usage{prompt,completion,cached}` already persists; the
  percentage is derived at display time (keeps receipts stable/auditable).

### 3.3 Compaction (degradation guard, plan §5.8)

Triggered inside `run_with_history`, checked at the top of each turn (before building the
request):

- **Trigger**: `context_limit == Some(limit)` AND the current estimated input size
  ≥ `0.70 * limit` (constant `COMPACT_AT: f64 = 0.70`). "Current input size" = the
  `prompt_tokens` from the most recent response usage if available, else
  `estimate_tokens(&history)`.
- **`estimate_tokens(&[ChatMessage]) -> u64`**: deterministic; `ceil(len/4)` over each
  message's `content` + serialized `tool_calls`, plus a small fixed per-message overhead.
  Also the fallback trigger for providers that omit usage.
- **Compaction algorithm** (mechanical; no summary-model call in M2):
  1. Keep `history[0]` (prefix) byte-identical.
  2. Choose the smallest suffix that (a) **begins at a `user` message** (wire-valid;
     Anthropic requires user-first after system), (b) never splits an assistant
     `tool_calls` message from its following `tool` result(s), and (c) fits under a target
     of `0.50 * limit` estimated tokens — but ALWAYS retain at least the last
     `KEEP_RECENT = 2` user-turns (and everything after the earliest kept user message).
  3. Drop the messages between the prefix and that suffix.
  4. **Prepend a one-line marker to the first retained user message's content**:
     `"[nosis] earlier context compacted: {n} messages, ~{t} tokens elided.\n\n"` + original
     content. (A separate marker message would create two consecutive user messages and
     break the Anthropic wire — so it is folded in. This keeps roles valid and the marker
     auditable.)
  5. Emit one `on_event` line: `"context {pct}% — compacted {n} earlier messages"`.
- **preserve_reasoning**: retained assistant messages keep their `reasoning_content`
  untouched (compaction only drops whole messages; the wire client's replay policy is
  unchanged).
- Compaction mutates the caller-owned `history` in place, so `nh chat` keeps the compacted
  session going forward; the marker persists and stays stable thereafter.
- Never compacts away the just-appended current user task (it is within `KEEP_RECENT`).

Unit/integration tests (inject a small `context_limit`, e.g. 100–400 tokens, to force it):
prefix stays byte-identical; retained suffix starts with `user`; no dangling `tool` result
and no orphan tool-call; marker folded into first kept user message; `on_event` fires;
last user-turn survives; a route with `context_limit == None` never compacts.

### 3.4 Exit-criterion test — cache-hit % > 60% over 50 turns

Integration test in nh-core with a **prefix-caching mock** `ChatClient`:
- The mock records the previous request's serialized `messages`. On each call it reports
  `usage.cached_tokens` = a deterministic token proxy (e.g. `chars/4`) over the longest
  byte-identical *leading run of messages* shared with the previous request, and
  `usage.prompt_tokens` = the same proxy over all messages. (The mock owns this proxy — it
  does not need nh-core's internal estimator, which stays private.) It returns a short
  final answer each turn (no tool calls) so a turn appends exactly `user` + `assistant`.
- Drive 50 sequential `run_with_history` tasks over ONE history, with a non-trivial
  `constitution` (so the stable prefix dominates) and a large `context_limit` (so
  compaction does not fire during the metric run).
- Assert cumulative `cache_hit_pct(Σprompt, Σcached) > 60.0`. (If the prefix were rebuilt
  each turn, cached collapses to ~0 and this fails — that is the point.)

---

## 4. nh-cli — wire the constitution, guard, autonomy, and the cache chip

### 4.1 Assemble + inject (both `nh run` and `nh chat`)

- At startup, after finding the repo root, call `nh_law::load(root, &LoadOptions{ cli_autonomy })`.
  Print each `Law.warnings` line as one `warning: <scrubbed>` to stderr (never fatal).
- Set `AgentLoop.constitution = Some(law.constitution)` and
  `AgentLoop.context_limit = route.context` (updated on every `/model`/`/provider` switch
  in chat, since context windows differ by route).
- Build the `ToolCtx` guard from `law.policy`. Because `nh_law::Verdict` and
  `nh_tools::Guard` live in different crates (nh-law must not depend on nh-tools), the
  `Verdict → Guard` conversion is a **free helper in nh-cli** (`fn guard_from(v: Verdict)
  -> Guard`), not a method on `Verdict`:
  `ToolCtx::new(cwd, approve).with_guard(Box::new(move |access| match access {
     Access::Write(p) => guard_from(policy.write_verdict(p)),
     Access::Exec(c)  => guard_from(policy.exec_verdict(c)), }))`.
  The reason string is user-facing → short and concrete, e.g.
  `"protected path (.git/**) — held even at max autonomy"`.

### 4.2 `nh run --autonomy <ask|auto>`

- New optional flag on `nh run` (clap `ValueEnum`, default absent). Absent → autonomy from
  law files (user-global/bundled). Present → overrides (`opts.cli_autonomy`). This is how
  the "max autonomy" exit demo runs: `nh run --autonomy auto "<edit a protected file>"`
  must print the Block line and leave the file unchanged, exit 0 (Ok-shaped tool result;
  the run still completes). `nh chat` uses the law default (no per-session dial until M3).
- Every existing `nh run` flag/message unchanged; the mapping lives in one function.

### 4.3 Cache-hit chip in the HUD (UX IS THE PRODUCT)

- **`nh run` summary line** gains a cache chip when prompt tokens > 0:
  `turns X | tool calls Y | tokens A in / B out / C cached | cache Z%` (`Z` = `{:.0}`).
- **`nh chat` footer** gains it likewise:
  `<route> | <peak> | session tokens A in / B out / C cached | cache Z%`.
  Omit the `| cache Z%` chip entirely when there is no usage yet (no meaningless `0%`).
- Keep both lines single, aligned, scannable. Update the affected cmd_chat footer tests.

### 4.4 `nh init` writes `.nosis/law.toml`

- `init_at` additionally writes `.nosis/law.toml` from `nh_law::STARTER_LAW_TOML` when
  absent (never overwrites). One confirmation line. law.toml is **committed** project
  policy (not matched by `.nosis/.gitignore`, which stays receipts/logs/auth only).
- Update the init tests: without `.git` the created set is now `.nosis/`, `.gitignore`,
  `law.toml`, `catalog.toml` (4 lines); with `.git`, +pre-commit hook (5). Idempotent
  re-run still `["already set up"]`.

---

## 5. What is frozen

### 5.1 Frozen surfaces
- Everything public in M0 + M1 (CONTRACTS_M1.md §1–§4, §5.1), except the §5.2 amendments.
- Everything specified in §§1–4 of this file. New public items beyond it need an
  orchestrator amendment (§7).

### 5.2 Explicit amendments (the only source-compat changes allowed)
1. `nh_tools::ToolCtx` gains `pub guard: GuardFn` + `ToolCtx::new` / `with_guard`. Every
   construction site updated: `nh-tools` tests, `nh-cli` cmd_run/cmd_chat (+ chat tests),
   `nh-core` `tests/agent_loop.rs`. New public enums `Access`, `Guard` + re-export.
2. `nh_core::agent::AgentLoop` gains `pub constitution: Option<String>` and
   `pub context_limit: Option<u64>`. All `AgentLoop { … }` literals updated (cmd_run,
   cmd_chat, cmd_chat tests, tests/agent_loop.rs) — default both to `None` where a real
   value is not wired, which preserves M0/M1 behavior exactly.
3. `nh_core::wire::cache_hit_pct` added (pure fn).
4. `nh-cli` gains `--autonomy` on `run`; `nh init` writes `law.toml`.
5. New workspace member `crates/nh-law` added to root `Cargo.toml` `members`; `nh-law`
   added to `nh-cli` dependencies (path). No new entries in `[workspace.dependencies]`.

### 5.3 Dependency additions allowed
- New crate `nh-law`: `serde`, `toml`, `anyhow` (all `.workspace = true`). Nothing else.
- `nh-cli`: `nh-law` (path). No other additions.

---

## 6. Assumptions / verify-live ledger (M2)

| item | where | note |
|---|---|---|
| DeepSeek/OpenAI-wire auto-caches a byte-stable prefix without an explicit marker | §3 | assumed true (standard prefix caching); metric surfaces the provider's own `cached_tokens`. Verify on the live DeepSeek run. |
| Anthropic-wire `cache_control` breakpoints | §3 (deferred) | NOT in M2. DeepSeek `-anthropic` variants rely on prefix stability only; explicit `cache_control` (system-block form) is a LATER hardening pass if the live metric needs it. Keeps M1 anthropic wire/tests unchanged. |
| GLM/MiMo free routes may omit `usage` | §3.3 | `estimate_tokens` fallback covers the compaction trigger; cache chip omitted when usage absent. |
| `KEEP_RECENT = 2`, `COMPACT_AT = 0.70`, target `0.50` | §3.3 | pinned defaults; revisit if live sessions thrash. |

---

## 7. Integration amendments (append here, dated, orchestrator authority)

_(none yet)_
