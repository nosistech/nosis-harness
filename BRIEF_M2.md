# BRIEF_M2 — Sol executor brief for Milestone M2

Orchestrator = Opus 4.8 (plans, gates, reviews). Executor = **GPT-5.6 Sol xhigh** via
Codex CLI, writes all M2 implementation code. Contract of record: **`CONTRACTS_M2.md`**
(locked). This brief slices M2 into three `codex exec` calls so each stays small and ends
green. Verify Sol's output yourself after each slice — do NOT trust the self-report.

## Executor invocation (run from repo root)

```
codex exec --skip-git-repo-check -s workspace-write \
  -m gpt-5.6-sol -c model_reasoning_effort=xhigh \
  "<slice prompt below>"
```

Long slices: `run_in_background: true`, then poll the output file. After each slice, the
ORCHESTRATOR (not Sol) runs `cargo test --workspace` + `cargo clippy --workspace
--all-targets -- -D warnings` and checks the slice's acceptance points before continuing.

## Guardrails to repeat in every slice prompt
- Read `AGENTS.md` and `CONTRACTS_M2.md` first; implement EXACTLY the locked surfaces.
- THE LAW + UX IS THE PRODUCT. No new external crates. No plaintext secrets.
- `exec_shell` always passes the approval gate — no autonomy overrides it.
- Run `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`
  before finishing; both must be clean. Keep M0+M1's 180 tests green.

---

## Slice A — nh-law crate (leaf, pure; CONTRACTS_M2.md §1)

> Read AGENTS.md and CONTRACTS_M2.md. Implement Milestone M2 **Slice A only**: the new
> leaf crate `crates/nh-law` per §1 — `Autonomy`, `Verdict`, `Policy`, `Law`,
> `LoadOptions`, `ConstitutionSources`; `BUNDLED_LAW`, `STARTER_LAW_TOML`; `load`,
> `assemble_constitution`; `Policy::write_verdict` / `exec_verdict` / `autonomy`; the
> in-crate glob matcher (no external glob crate); the repo-cannot-raise-autonomy security
> boundary (§1.5); the law.toml schema (§1.6). Add `crates/nh-law` to the root Cargo.toml
> `members`. Deps: only serde, toml, anyhow (`.workspace = true`). Do NOT touch other
> crates in this slice. Exhaustive unit tests: constitution byte-stability + section
> order + omission; glob matcher cases; write/exec verdict tiers incl. most-restrictive-
> wins; repo law.toml ignoring autonomy/auto with a warning; malformed law.toml → warning
> not panic. Run cargo test + clippy -D warnings before finishing.

Acceptance (orchestrator verifies): new crate compiles + tests/clippy clean; workspace
still 180+ green; `assemble_constitution` is byte-identical across two calls; a repo
law.toml setting `[autonomy] default = "auto"` is ignored + warns; bundled `block` globs
(`.git/**`, `.nosis/**`, `**/*.pem` …) yield `Verdict::Block`.

## Slice B — nh-core context engine (CONTRACTS_M2.md §3)

> Read AGENTS.md and CONTRACTS_M2.md. Implement Milestone M2 **Slice B only**: nh-core §3
> — add `AgentLoop.constitution: Option<String>` and `AgentLoop.context_limit:
> Option<u64>` (§5.2 amend 2; update every AgentLoop literal in nh-core incl.
> tests/agent_loop.rs to default both to None); byte-stable prefix discipline (history[0]
> set once, never mutated, dynamic content only appended); `nh_core::wire::cache_hit_pct`;
> compaction at 70% per §3.3 (keep prefix byte-identical; retained suffix starts at a
> `user` message; never split tool_call/tool_result; KEEP_RECENT=2; COMPACT_AT=0.70;
> target 0.50; fold the `[nosis] … compacted …` marker into the first retained user
> message; on_event line; preserve_reasoning untouched). Add the §3.4 exit test: a
> prefix-caching mock client, 50 `run_with_history` turns over one history with a
> non-trivial constitution + large context_limit, assert cumulative cache_hit_pct > 60%.
> Add compaction tests with a small injected context_limit. Do NOT change nh-tools or
> nh-cli in this slice (leave their AgentLoop literals compiling by keeping the new fields
> defaulted where those crates build AgentLoop — you MAY update those literals minimally
> to add `constitution: None, context_limit: None`). Run cargo test + clippy -D warnings.

Acceptance: cache-hit exit test passes (>60%); compaction test shows prefix byte-identical,
user-first suffix, no dangling tool result, marker folded in, last user-turn kept; a None
context_limit never compacts; workspace green.

## Slice C — nh-tools guard + nh-cli wiring (CONTRACTS_M2.md §2, §4)

> Read AGENTS.md and CONTRACTS_M2.md. Implement Milestone M2 **Slice C only**: nh-tools §2
> — `Access`, `Guard`, `GuardFn`, `ToolCtx.guard` + `ToolCtx::new`/`with_guard` (default
> guard = Write→Allow, Exec→Ask, preserving M0/M1 behavior); EditFile consults the guard
> on the normalized workdir-relative path (Block→Ok "blocked by law: …" file untouched;
> Ask→approve; Allow→proceed); ExecShell consults it (Block→Ok blocked; else existing
> approval gate); MCP adapters unchanged. Update every ToolCtx construction site (nh-tools
> tests, nh-cli cmd_run/cmd_chat + chat tests, nh-core tests/agent_loop.rs). nh-cli §4 —
> add `nh-law` path dep; assemble constitution + policy via `nh_law::load`; set
> `AgentLoop.constitution`/`context_limit` (update on chat route switches); build the guard
> with an nh-cli `guard_from(Verdict)->Guard` helper; `nh run --autonomy <ask|auto>`; cache
> chip in the `nh run` summary and `nh chat` footer (omit when no usage); `nh init` writes
> `.nosis/law.toml` from STARTER_LAW_TOML (update init tests to 4/5 created lines). Add the
> protected-path-blocked-in-max-autonomy end-to-end test (`nh run --autonomy auto` on a
> law-protected path → Block line, file unchanged). Run cargo test + clippy -D warnings.

Acceptance: both M2 exit criteria pass (cache >60% from Slice B; protected path blocked at
`--autonomy auto` end-to-end); footer/summary show the cache chip; `nh init` scaffolds
law.toml idempotently; workspace test count up, clippy clean; keyless `echo /quit | nh chat`
still exits 0.

---

## After all three slices (orchestrator)
1. Full verify: `cargo test --workspace` + `cargo clippy --workspace --all-targets -- -D
   warnings`; confirm both M2 exit criteria by reading the test names/output (don't assume).
2. **Adversarial review** vs THE LAW + SECURITY_MODEL.md: focus on write-hold bypasses
   (path normalization / `..` / symlink / case-fold on Windows), constitution byte-stability
   under route switch, compaction breaking tool-pairing on the Anthropic wire, and the
   repo-cannot-raise-autonomy boundary. Send confirmed findings back to Sol (hardening pass).
3. Update `BUILD_LOG.md` + `CURRENT_TASK.md`; `git commit` (trailer `Co-Authored-By:
   Claude Opus 4.8 <noreply@anthropic.com>`; body notes Sol as implementer). Then M3.
