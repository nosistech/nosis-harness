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

Exit: `/model` and `/provider` switch mid-session; clock-aware catalog prices render correctly; MCP tool call with handle passthrough and no session header on the wire. Provider prices are verified against first-party sources.

Status (updated 2026-07-26): **exit criteria met for the implemented surface.** Provider calls were live-smoked on 2026-07-20. Prices and limits were reverified against first-party pages on 2026-07-26; the current catalog has no provider peak windows because DeepSeek's current pricing page no longer publishes the earlier schedule. Generic peak-window behavior remains mock-tested. A real external 2026-07-28 MCP implementation is still pending.

## Milestone 2: Context engine + law (weeks 3–5)

The project has:

- Byte-stable prefix cache discipline + cache-hit % metric; compaction at 70% with timeline marker; per-route `preserve_reasoning` (Kimi/MiMo).
- Nested constitution loader (bundled law → user law → repo `.nosis/law.toml` → AGENTS.md → memory); mechanical write-holds.

Exit: cache-hit % >60% on a 50-turn session; protected path blocked even in max autonomy.

## Milestone 3: TUI (weeks 5–7)

The project has:

- Semáforo status (WORKING/WAITING/BLOCKED/IDLE) with a local approval bell, token/cost HUD, view-and-inspect timeline, trust dial, and `?` palette with live MCP server state. Snapshot/restore, delegate quota units, and remote notifications are not part of public v0.1.

Exit: full session on the Predator natively (Windows Terminal, VS Code terminal, ConHost), zero renderer artifacts.

Status (2026-07-15): **exit criteria met (UX-approved).** Slices A–C initially shipped the surfaces
(semáforo, cost HUD, timeline, trust dial, `?` palette, and a Telegram hook). On 2026-07-26 the
owner removed the remote-notification implementation from public v0.1 to eliminate its credential,
destination-config, privacy, and outbound-network attack surface; the local bell remains and a
future explicit opt-in integration is still open. Carlos then rejected the
content-complete-but-flat TUI on UX grounds, so M3 was reopened and re-skinned + interaction-fixed
across **Slices D+E+F**: framed chat transcript with `❯ you`/`◆ nosis` roles + turn separation;
type-freely **slash-command** input (`/` live menu; removed the bare-letter shortcuts that collided
with typing); live `/model`/`/provider` switch preserving history + `/effort none|low|high|max`;
keyboard scroll + `↑/↓ more` overflow hints; honest identity system prompt (`nosis on <route>`, never
Claude — fixes DeepSeek V4 Flash training contamination); **native mouse click-drag copy restored**
(mouse capture removed) + **bracketed paste fixed** (multi-line → one line, never auto-dispatches).
Carlos ran the interactive re-smoke in Windows Terminal (his standardized default) and approved the
FEEL — the binding gate. Orchestrator adversarially stress-tested the reducers/renderer (tiny terminals
1×1, 200k/emoji/CJK/control paste, boundary nav, 20k-event fuzz) — zero panics. 261 pass / 1 ignored,
clippy `-D warnings` clean. Committed on main. Optional follow-ups (not blocking): separate re-smoke in
VS Code terminal + ConHost; case-insensitive `/effort`.

## Milestone 4: Fleet + swarm + scheduler + nh-mcp server (weeks 7–9)

The project has:

- Append-only ledger, bounded workers, typed receipts, idempotent resume, a generic off-peak scheduler, and an escalation ladder ending at a human gate. The Kimi Swarm backend interface exists, but the public Swarm client remains an explicit pending stub.
- nh-mcp server exposing route-resolver + fleet-runner.

Exit: 10-task fleet run survives kill -9 and resumes idempotently; deferred job executes off-peak; KORVIN connects to nh-mcp and triggers a fleet run; OAuth refresh survives forced expiry.

## Milestone 5: Hardening + launch (weeks 9–10)

The project has:

- Policy-level containment, exact-origin credential scoping, bounded provider/tool/MCP inputs and outputs, denial-of-wallet budgets, supply-chain policy, pinned multi-OS CI, current security/privacy documentation, and the headless `nh run` surface.

Exit: public source release after the full debug/release gate, owner FEEL check, remote CI on Windows/Linux/macOS, a configured public remote, and release smoke tests. `nh-mcp` stays loopback-only even after the MCP final spec lands.

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
