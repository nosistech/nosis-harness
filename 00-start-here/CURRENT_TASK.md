# Current Task

## Immediate Goal

Begin **M3 — the TUI**. M2 (context engine + law) is DONE, orchestrator-verified, and
committed. M3 is the UX-critical milestone: it is where "UX IS THE PRODUCT" matters most
(memory [[ux-first-and-the-law]]). Same build loop: Claude plans + writes the contract/brief +
gates; **GPT-5.6 Sol xhigh implements** via `codex exec` (memory [[m2-m5-codex-sol-directive]]).

## Why This Matters

M3 is the face of the harness — the semáforo, the cost HUD, the timeline scrubber, the trust
dial, the `?` palette. If it renders cleanly and every line is short/concrete/actionable, the
product feels trustworthy. If it has renderer artifacts on the Predator's terminals, it does not.

## Status — M2 COMPLETE (committed)

Both exit criteria met and proven by test name; `cargo test --workspace` = 206 passed / 0
failed / 1 ignored; `cargo clippy --workspace --all-targets -- -D warnings` clean.

- **Slice A — `crates/nh-law`**: constitution loader + trust/write-hold policy. Byte-stable
  assembly; repo law cannot raise autonomy or auto-approve (security boundary); bundled block
  globs → `Verdict::Block`; in-crate glob matcher (case-sensitive by design, §1.4).
- **Slice B — nh-core context engine**: byte-stable `history[0]` prefix (debug-asserted every
  turn), `cache_hit_pct`, compaction at 70% (KEEP_RECENT=2, target 0.50, cuts only at user
  boundaries so tool-pairs never split, marker folded into first retained user msg).
  **Exit #1: 50-turn cache-hit = 97.70% (>60%)** — `stable_constitution_exceeds_sixty_percent_cache_hits_over_fifty_turns`.
- **Slice C — nh-tools guard (§2) + nh-cli wiring (§4)**: `Access`/`Guard`/`GuardFn`,
  `ToolCtx::new`/`with_guard`; edit/exec consult the guard with the workdir-RELATIVE
  forward-slashed path; Block/Ask denials stay Ok-shaped; `nh run --autonomy ask|auto`; law
  wired into run + chat (warnings, constitution, context limit, route-switch refresh); cache
  chips; `nh init` writes `.nosis/law.toml`. **Exit #2: `protected_path_is_blocked_at_auto_end_to_end`**
  runs the real binary at `--autonomy auto`, the model's edit of `.nosis/law.toml` is blocked
  (model-readable line), the file is byte-unchanged, exit 0.
- **Hardening pass (Sol)**: removed dead `exec_ask` plumbing; made the protected-path autonomy
  test hermetic; documented the case-fold write-hold safety invariant in nh-tools.

Orchestrator adversarial-review conclusions (see BUILD_LOG M2 entries): write-hold is sound —
`exec_verdict` can only return Block/Ask (never Allow), Block wins before `is_file`, symlinks
resolve to canonical target, and the Windows case-fold new-file bypass is NOT reachable via
`EditFile` (existing-file-only + `canonicalize` normalizes case). Documented for any future
file-creation tool.

## Next Action (orchestrator = Opus 4.8 high; executor = GPT-5.6 Sol xhigh)

1. **Scope M3.** Read the master plan §6/§4.5 (TUI) + `MILESTONES.md` M3 + `02-architecture/`
   + memory [[ux-first-and-the-law]]. Decide the crate shape (likely a new `nh-tui` crate or a
   `nh-cli` TUI module) and the terminal backend (ratatui/crossterm is the obvious fit; confirm
   against THE LAW: lightweight, no heavy deps). Exit criterion is renderer-artifact-free on
   Windows Terminal, VS Code terminal, and ConHost — a Windows-native rendering concern.
2. **Write `CONTRACTS_M3.md`** (locked public APIs, same pattern as CONTRACTS_M1/M2) + a brief,
   split into 2–3 slices (large milestone). Candidate slices: (A) status/semáforo + cost HUD
   render core; (B) timeline scrubber + side-git snapshots + trust dial; (C) `?` palette w/ live
   MCP state + Telegram notify hook. Keep each slice small.
3. **Drive Sol** per slice: `codex exec --skip-git-repo-check -s workspace-write -m gpt-5.6-sol
   -c model_reasoning_effort=xhigh "<brief>" < /dev/null` (background; stdin from /dev/null to
   avoid the stdin-wait hang; watch for the ~0-CPU stall = kill + retry). Do NOT run two codexes
   on nosis at once.
4. **Gate each slice**: `cargo test --workspace` + `cargo clippy … -D warnings` + adversarial
   review vs THE LAW + the UX rule (every line short/concrete/actionable; no stack traces;
   drop-if-hard). Rendering can't be proven by unit tests alone — plan a real-terminal smoke on
   the three Windows terminals for the exit criterion (may need Carlos to eyeball, or a scripted
   capture). Send findings back to Sol as one hardening pass.
5. Update `BUILD_LOG.md` + this file; `git commit` M3 together (trailer
   `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`; body credits Sol as implementer).

## Do Not Do

- Do NOT hand-write milestone implementation code — Sol implements; Claude plans + gates only.
- Do NOT start a second codex on nosis while one is running (concurrent workspace-write conflicts).
- Do NOT commit a slice until it is verified AND its adversarial review + hardening pass is done.
- If `gpt-5.6-sol` stops resolving, or Sol fails the same gate twice → STOP and tell Carlos
  (don't silently fall back to Terra).

## Definition Of Done (M3)

- Full session renders on Windows Terminal, VS Code terminal, and ConHost with zero renderer
  artifacts (the milestone exit criterion).
- Semáforo, cost HUD, timeline scrubber + side-git snapshots, trust dial, `?` palette with live
  MCP state, Telegram notify hook — all present, each surface a short/scannable line.
- `cargo test --workspace` + `cargo clippy … -D warnings` green; BUILD_LOG updated; committed.
