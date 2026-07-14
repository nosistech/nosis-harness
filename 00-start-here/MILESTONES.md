# Milestones

Exit criteria are testable, not vibes. Full detail: `../NOSIS_HARNESS_Master_Plan.md` §6 and §4.5.

## Milestone 0: Skeleton (week 1)

The project has:

- Rust workspace: nh-core, nh-routes (stub), nh-tools, nh-vault, nh-cli.
- Turn loop against ONE route (`deepseek-v4-flash`, OpenAI wire).
- Tools: read_file, edit_file, exec_shell (approval prompt before every exec).
- JSONL receipts to `.nosis/receipts.jsonl`; keys in OS-native vault; redaction scrubber.

Exit: fixes a failing test in a sample repo end-to-end. `cargo test` + `cargo clippy -- -D warnings` clean.

Status (2026-07-13): **exit criteria met** — turn loop, approval-gated tools, receipts, and vault all regression-tested; tests + clippy clean; smoke (`nh --help`, keyless `nh chat`) green. Mock-verified: the end-to-end fix runs against loopback mock providers. Live-pending: the same flow on a real DeepSeek key (folded into the M1 live pass).

## Milestone 1: RouteResolver + catalog + MCP client (weeks 2–3)

The project has:

- Catalog TOML, all 5 providers; wire adapters (OpenAI + Anthropic Messages); thinking-mode dialects; modality flags; clock-aware pricing with `valid_until` and `price_confidence`.
- DeepSeek gotcha tests (alias ban, `reasoning_content: ""` replay); banned-string rejection.
- MCP client against one stateless 2026-07-28 server.

Exit: `/model` and `/provider` switch mid-session; peak/off-peak price shown correctly; MCP tool call with handle passthrough and no session header on the wire. MiMo prices verified live (B.3 conflict resolved).

Status (2026-07-13): **exit criteria met** — MiMo prices verified live against first-party pages (B.3 resolved, `confirmed` — see `../04-research/SOURCE_INDEX.md`). Mock-verified: mid-session `/model`/`/provider` switching, peak/off-peak display, and the stateless MCP call (handle passthrough, no session header) all run against loopback mock servers only. Live-pending: real provider calls (DeepSeek keyed + GLM free route), a real 2026-07-28 MCP server, DeepSeek peak windows (re-verify ~2026-07-24), GLM free-tier rate limits.

## Milestone 2: Context engine + law (weeks 3–5)

The project has:

- Byte-stable prefix cache discipline + cache-hit % metric; compaction at 70% with timeline marker; per-route `preserve_reasoning` (Kimi/MiMo).
- Nested constitution loader (bundled law → user law → repo `.nosis/law.toml` → AGENTS.md → memory); mechanical write-holds.

Exit: cache-hit % >60% on a 50-turn session; protected path blocked even in max autonomy.

## Milestone 3: TUI (weeks 5–7)

The project has:

- Semáforo status (WORKING/WAITING/BLOCKED/IDLE), cost HUD (tokens + delegate quota units), timeline scrubber + side-git snapshots, trust dial, `?` palette with live MCP server state, Telegram notify hook.

Exit: full session on the Predator natively (Windows Terminal, VS Code terminal, ConHost), zero renderer artifacts.

## Milestone 4: Fleet + swarm + scheduler + nh-mcp server (weeks 7–9)

The project has:

- Append-only ledger, workers, typed receipts, idempotent resume; off-peak scheduler; Kimi Swarm passthrough; escalation ladder (Flash → K2.7 → V4 Pro High → V4 Pro Max → Opus gate).
- nh-mcp server exposing route-resolver + fleet-runner.

Exit: 10-task fleet run survives kill -9 and resumes idempotently; deferred job executes off-peak; KORVIN connects to nh-mcp and triggers a fleet run; OAuth refresh survives forced expiry.

## Milestone 5: Hardening + launch (weeks 9–10)

The project has:

- Sandbox tiers (Windows: approval-gating + restricted tokens; Linux: Landlock/seccomp; macOS: Seatbelt — honest docs about the difference), headless `nh exec`, docs, launch post.

Exit: public release. Do not ship nh-mcp server publicly until the MCP final spec lands (July 28).

## Milestone 6: Multimodal orchestration (post-launch — added 2026-07-14, Carlos)

Rationale: nosis is the **conductor, not the instruments.** M0–M5 build the best coding conductor.
M6 adds media *instruments* so the SAME routing / fleet / scheduler / governance orchestrates
generation pipelines (manga, anime, audio, game assets) — not just code. Reuses the RouteResolver
(the `modality` flags already exist), the fleet ledger, the off-peak scheduler, and receipts.
Scope stays narrow per THE LAW: nosis calls existing generation models/APIs; it does NOT train them.

The project has:

- **Generation routes** — image / video / audio model endpoints as catalog routes behind the
  existing RouteResolver, using the `modality` flags; a new wire-adapter class for generation APIs
  (submit → poll → fetch-artifact), distinct from the chat-completion wires. Catalog stays data.
- **Generation tools / MCP** — tools the agent loop can call to produce/fetch artifacts
  (text→image, image→video, text→speech, music); artifacts land in a workspace media dir and are
  referenced (by handle, never inlined) in receipts. Every call still passes the approval/law gate.
- **Pipeline orchestration** — a declarative multi-step media pipeline (e.g. script → storyboard →
  panels → voice → music → assemble) run through the fleet with off-peak cost routing and one
  receipt per step; idempotent resume applies.
- **Media preview surface** — media is not previewable in a TUI, so the plan's v2 **Axum web
  companion dashboard** (KORVIN pattern) gains a media/diff preview view. This is also the GUI wedge
  (phone review), with the TUI staying minimal.

Exit (testable, not vibes):

- One end-to-end pipeline turns a text brief into a finished artifact with EVERY step in the ledger
  and reproducible via idempotent resume (reference target: "brief → 4-page manga" → PNG pages +
  per-step receipts; a `kill -9` mid-run resumes without redoing completed steps).
- Cost HUD + budget hard-stop work across generation routes (image/video priced per-artifact, not
  per-token); no fake projected cost.
- No generation tool bypasses the approval/law gate; prompts + artifact metadata pass the scrubber.

Non-goals (honest): training generative models; fully autonomous feature-length anime or AAA games
end-to-end — those are limited by the generation models themselves, not by the harness. M6 makes
nosis the cheapest, most auditable *orchestrator* of whatever generation models exist at the time.
