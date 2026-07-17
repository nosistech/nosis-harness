# LENS L — Ecosystem, Interop & Distribution (NOSIS Harness)
Research date: 2026-07-17 · Analyst: Fable (Lens L) · Repo HEAD: bd35b4d (M4 Slice D uncommitted in tree)

## Framing

nosis's ecosystem thesis is already written into the Master Plan: *"MCP server (nh-mcp)… is what stops it being a dead-end CLI and makes it a node in your orchestration layer"* (`NOSIS_HARNESS_Master_Plan.md:145`). The product question for this lens is: **which few, small seams turn the routing brain into something other agents, editors, and pipelines depend on** — without violating THE LAW (small, modular, congruent).

The 2026 landscape has consolidated around exactly three interop currencies:

1. **ACP (Agent Client Protocol)** for *editor ⇄ agent* — Zed-originated, JetBrains co-developed, now with 25+ agents in the ACP registry (Claude Code, Gemini CLI, Codex CLI, Copilot CLI, Goose, Cline, OpenCode…), native support in Zed + JetBrains, community plugins for Neovim/Emacs/VS Code, and — critically for a Windows-first product — **Microsoft's "Intelligent Terminal" fork of Windows Terminal (0.1, June 2, 2026) ships a native agent pane that speaks ACP and auto-detects installed ACP CLIs** (https://codex.danielvaughan.com/2026/06/10/agent-client-protocol-microsoft-intelligent-terminal-codex-cli-multi-agent-ide-ecosystem/, https://agentclientprotocol.com/get-started/introduction, https://www.jetbrains.com/acp/, https://zed.dev/acp, https://github.blog/changelog/2026-01-28-acp-support-in-copilot-cli-is-now-in-public-preview/).
2. **MCP** for *agent ⇄ tool/agent* — the 2026-07-28 final (RC locked May 21) brings the stateless core, the **Tasks extension** (tools/call returns a task handle; clients poll `tasks/get`), MCP Apps, OAuth/OIDC hardening, `.well-known` discovery SEPs, and the official Registry (https://blog.modelcontextprotocol.io/posts/2026-07-28-release-candidate/, https://modelcontextprotocol.io/registry/about). VS Code standardized its agent mode on MCP, not ACP (https://code.visualstudio.com/blogs/2025/04/07/agentMode, https://code.visualstudio.com/api/extension-guides/ai/mcp).
3. **SKILL.md / AGENTS.md** for *knowledge & config sharing* — Anthropic's Agent Skills spec (Dec 18, 2025) crossed **32 tools by March 2026** (Gemini CLI, JetBrains Junie, AWS Kiro, Goose…) and moved under Linux Foundation AAIF governance in May 2026 (https://codex.danielvaughan.com/2026/05/05/agent-skills-open-standard-portable-skills-codex-cli-cross-agent/, https://www.paperclipped.de/en/blog/agent-skills-open-standard-interoperability/, https://agentskills.io/home).

nosis already holds a rare position: it speaks **both** MCP directions (client in `crates/nh-tools/src/mcp.rs`, server in `crates/nh-mcp/src/lib.rs` with a `.well-known/mcp.json` business card at lib.rs:141–148/200–207), has a durable fleet with run-id handles, and has both wire protocols in-tree. Every finding below is a *thin adapter over things that already exist*, which is what keeps this cohesive rather than a feature pile.

---

## Finding 1 — `nh acp`: one stdio adapter buys Zed, JetBrains, Neovim, and Microsoft's Windows Terminal agent pane

**What.** Implement the Agent Client Protocol as a headless front-end: `nh acp` speaks JSON-RPC 2.0 over stdio (initialize → session/new → session/prompt → streamed updates), and maps ACP's `session/request_permission` to the existing trust dial / approval gate. ACP is deliberately LSP-shaped: implement once, run in every ACP client (https://agentclientprotocol.com/get-started/introduction, https://github.com/agentclientprotocol/agent-client-protocol).

**Why now / why #1.** As of March 2026 the ACP registry lists 25+ agents; Zed and JetBrains are native hosts; Copilot CLI shipped ACP in public preview Jan 28, 2026 (https://github.blog/changelog/2026-01-28-acp-support-in-copilot-cli-is-now-in-public-preview/). The kicker for nosis's Windows-first wedge: **Microsoft Intelligent Terminal 0.1 (June 2026) auto-detects ACP-compatible CLIs on the system** and lists them in its agent pane next to Claude Code / Codex / Gemini. A Windows user who installs nosis would see it appear *inside the terminal Microsoft ships* — zero marketing, pure protocol conformance (https://codex.danielvaughan.com/2026/06/10/agent-client-protocol-microsoft-intelligent-terminal-codex-cli-multi-agent-ide-ecosystem/).

**Cohesion.** ACP's permission model (`session/request_permission` before file writes / shell) is *exactly* nosis's law-gated approval model — the constitution stays enforced when nosis runs inside someone else's editor; the honest-identity prompt (fixed in 7faf44b to apply at every agent surface) applies here too, so this becomes the fourth surface of the same one agent loop (run/chat/tui/acp). Differentiators 6 and 7 travel into the editor rather than being TUI-only.

**Design sketch (smallest MVP).** New feature-gated crate `nh-acp` (mirroring how `nh-mcp` wraps `nh-fleet`): stdio JSON-RPC loop; `session/new` builds the same `identity_constitution`-wrapped turn as `cmd_chat` (`crates/nh-cli`, `crates/nh-tui/src/lib.rs` reducers are already headless-testable); `session/prompt` streams agent text + tool-call notifications; permission requests forward the same approval strings the CLI prints. Skip ACP extras (terminals, editor file-buffer reads) in v1 — the spec makes them optional capabilities. No tokio needed if the existing blocking loop is reused (ACP is request/response + notifications over stdio). Ship at/after M5.

**LAW check.** Modular (own crate, `--features acp` if desired), congruent (same loop, same law, same receipts), small if v1 sticks to core session methods. Tension: ACP session model is stateful-by-design — contain it in the adapter, never let it leak into nh-core.

**Value: HIGH · Effort: M · keyRequired: none.**

---

## Finding 2 — Fleet runs as MCP **Tasks**: make nh-mcp the standard long-running-work backend

**What.** The 2026-07-28 RC formalizes long-running work as the **Tasks extension**: a server answers `tools/call` with a *task handle*; clients drive progress via `tasks/get` / `tasks/cancel`; `tasks/list` was removed as unscopable without sessions (https://blog.modelcontextprotocol.io/posts/2026-07-28-release-candidate/). The Master Plan already predicted this mapping: *"Tasks extension — first-class long-running work. Maps cleanly onto nh-fleet: a fleet job can *be* an MCP Task"* (`NOSIS_HARNESS_Master_Plan.md:139`).

**Why.** Today `fleet_run` returns `run_id=…` as free text and callers must know to call `fleet_status` (`crates/nh-mcp/src/lib.rs:377–385, 392–417`). Once the final spec lands (the repo already gates public exposure on 2026-07-28 — `CURRENT_TASK.md` "Do Not Do"), any Tasks-aware MCP client (Claude Code, VS Code agent mode, KORVIN) can start a nosis fleet, poll it, and cancel it with **standard** semantics instead of nosis-specific text parsing. That converts nh-mcp from "three custom tools" into "the cheapest task-execution backend on the network" — the orchestration-node thesis realized in one small delta.

**Design sketch.** nh-fleet already has everything: durable `run_id` (`new_run_id`), `read_run_ledger` (with `../`-traversal validation), `status_from_ledger` + `FleetStatus`. Add to nh-mcp: (a) declare the Tasks extension in the business card / capabilities; (b) `fleet_run` optionally returns the spec's task-handle shape; (c) `tasks/get` = thin rename of the existing `fleet_status` logic; (d) `tasks/cancel` = write a cancel event to the ledger (workers already check budget-stop; reuse that seam). Est. <200 LOC on top of lib.rs:392–417.

**LAW check.** Small, congruent (handles-as-arguments is already the house style — `nh-tools/src/mcp.rs:2–4`), auditable (task status derived from the append-only ledger, no new state). Perfectly timed: implement against the final spec the week it lands.

**Value: HIGH · Effort: S · keyRequired: none.**

---

## Finding 3 — `nh gateway`: expose the routing brain as a loopback Anthropic/OpenAI endpoint so *other* tools route through nosis

**What.** A localhost proxy (`nh gateway --port 4444`) that accepts Anthropic Messages **and** OpenAI Chat Completions requests and answers them through the RouteResolver — clock-aware pricing, KV-cache discipline, thinking-budget mapping, receipts, scrubber. Then `ANTHROPIC_BASE_URL=http://127.0.0.1:4444 claude` (or any OpenAI-SDK tool with `base_url`) silently gets nosis's cost routing.

**Why.** The demand is proven and large: **claude-code-router** (binds 127.0.0.1:3456, task-based routing of Claude Code to DeepSeek/Gemini/Ollama) and **LiteLLM's unified `/v1/messages` endpoint** exist precisely because people want Claude Code's UX on cheaper models (https://www.getaiperks.com/en/ai/claude-code-router-guide, https://docs.litellm.ai/docs/tutorials/claude_non_anthropic_models, https://www.morphllm.com/claude-code-litellm). But those routers are static config-file mappers — none of them routes by **time-of-day price, cache state, or thinking budget**. nosis's entire differentiator stack becomes consumable by every other agent tool *without those tools changing at all*. This is the single strongest "cost brain other CLIs plug into" move, and it works with only the keys already held (DeepSeek, Kimi, MiMo). Note LiteLLM's supply-chain incident (malicious PyPI 1.82.7/1.82.8 flagged by Anthropic per https://www.morphllm.com/claude-code-litellm) — a single vetted Rust binary is a genuine security pitch here.

**Cohesion.** Both wire adapters already exist in nh-core/nh-routes (M1: "wire adapters (OpenAI + Anthropic Messages)" — `MILESTONES.md:22`); the gateway is those adapters run in reverse plus the resolver. Receipts give the user one unified cost ledger across *all* their tools — cost opacity (pain #6) fixed even inside Claude Code.

**Design sketch (MVP).** Reuse the exact nh-mcp server pattern: `tiny_http`, hard 127.0.0.1 bind (`crates/nh-mcp/src/lib.rs:97–99`), optional bearer, scrubbed responses. v1: non-streaming + SSE streaming for the two wires, model name mapped through catalog aliases, every request logged to `.nosis/receipts.jsonl`. Defer: tool-call translation edge cases (pass through unchanged — both wires are already the only two protocols in the catalog).

**LAW check.** Tension with *small* is real — this is a new surface. Mitigations: loopback-only forever (like nh-mcp), no config beyond the existing catalog, one crate. It is highly *congruent* (the product IS a router; this exposes routing at the wire it already speaks) and *harmonic* (one receipts ledger, one vault, one scrubber for everything on the machine). Marked in-scope: routing is the core product.

**Value: HIGH · Effort: M · keyRequired: none** (works with held DeepSeek/Kimi/MiMo keys).

---

## Finding 4 — Read the **Agent Skills (SKILL.md)** standard instead of inventing a skills format

**What.** Support the open Agent Skills spec: skills are folders with a `SKILL.md` (two required YAML fields + markdown body, optional scripts/templates), discovered from the standard locations plus `.nosis/skills/`. Surface them in the `?` palette with state, per the M3 discoverability design.

**Why.** The standard won: released Dec 18, 2025; **32+ tools by March 2026** including Gemini CLI, JetBrains Junie, AWS Kiro, Goose; Linux Foundation AAIF governance since May 2026 (https://codex.danielvaughan.com/2026/05/05/agent-skills-open-standard-portable-skills-codex-cli-cross-agent/, https://www.paperclipped.de/en/blog/agent-skills-open-standard-interoperability/, https://agentskills.io/home). Every skill a user already wrote for Claude Code/Codex/Cursor works in nosis on day one — that removes the biggest switching cost for exactly nosis's target user (a power user who already runs incumbent CLIs). Distribution flows the other way too: skills written for nosis are portable, so publishing a few nosis-flavored skills (e.g. "off-peak refactor batch") is free marketing in every other tool.

**Cohesion.** The constitution loader already defines the layered-knowledge pattern (bundled law → user law → repo `.nosis/law.toml` → AGENTS.md → memory, `MILESTONES.md:35`); SKILL.md slots in as one more *data* layer under the same precedence discipline — and under the same law: skill text is model guidance, never able to override write-holds (differentiator 7 intact). nosis already reads AGENTS.md, so it half-implements the family already.

**Design sketch.** Pure-parsing addition to the constitution loader in nh-context/nh-law: enumerate skill dirs, parse frontmatter (serde_yaml or hand-rolled two-field parse to avoid a dep), inject on-demand by name/description match, list in the palette. No network, no registry, no execution of bundled scripts without the normal exec approval gate.

**LAW check.** Small (a parser + a directory walk), congruent (layers pattern), safe (skills are data; scripts go through approval). This is the whole "config/skills sharing" pillar for the cost of a file format.

**Value: HIGH · Effort: S · keyRequired: none.**

---

## Finding 5 — Headless `nh exec --output json` + a tiny reusable GitHub Action, with the free GLM route as the "$0 agent CI" hook

**What.** M5 already plans headless `nh exec` (`ROADMAP.md:58`, `NOSIS_HARNESS_Master_Plan.md:81` "nh-cli # headless exec, CI mode"). The ecosystem move is to shape it like the two patterns the market standardized on in 2026 — `claude -p` + `anthropics/claude-code-action@v1`, and `codex exec` + `openai/codex-action@v1` (https://code.claude.com/docs/en/github-actions, https://www.developersdigest.tech/blog/codex-exec-ci-headless-guide, https://hidekazu-konishi.com/entry/claude_code_cicd_and_headless_automation.html): final message to stdout, progress to stderr, `--output json` (receipts inline), `--max-turns` / `--budget` hard caps, nonzero exit on gate/budget stop. Then publish a ~40-line composite GitHub Action (`nosistech/nosis-action`) that installs the release binary and runs `nh exec`.

**Why.** CI is where routing economics compound: nightly triage, PR review, scheduled refactors are precisely "deferrable" jobs, and nosis is the only harness whose scheduler can *hold a CI job for off-peak DeepSeek pricing* (fleet `defer_offpeak` already exists — `crates/nh-mcp/src/lib.rs:258,340`). The unique GTM hook is the catalog's free route: *"glm-4.7-flash | FREE (in+cached+out) | Your $0 CI/smoke-test route"* (`NOSIS_HARNESS_Master_Plan.md:393,328`) — "agent CI that costs $0" is a headline no incumbent can print. Git-hooks angle folds in for free: the same headless mode powers the already-planned `nh init` pre-commit secret write-hold (`NOSIS_HARNESS_Master_Plan.md:321`) and user-authored hooks.

**Design sketch.** `nh exec` = `cmd_run` minus TTY, plus JSON envelope {result, receipts, cost, cache_hit_pct, exit_reason}. The Action is YAML in a separate repo — zero harness code. Budget flag reuses the fleet budget-stop machinery.

**LAW check.** Congruent (one agent loop, new presentation), auditable (receipts are the output format), small. The Action lives outside the codebase so the binary stays lean.

**Value: HIGH (launch-critical) · Effort: M (exec) + S (action) · keyRequired: GLM (free registration, 20M tokens — `NOSIS_HARNESS_Master_Plan.md:288`) for the $0-CI story; works with held keys otherwise.**

---

## Finding 6 — Align the business card with SEP-1649 and publish to the official MCP Registry at launch

**What.** nh-mcp already serves `GET /.well-known/mcp.json` (`crates/nh-mcp/src/lib.rs:141–148, 200–207`). Two active SEPs define discovery: **SEP-1649** (server card at `/.well-known/mcp/server-card.json`) and **SEP-1960** (manifest at `/.well-known/mcp`); major clients are implementing both ahead of core-spec merge (https://www.ekamoira.com/blog/mcp-server-discovery-implement-well-known-mcp-json-2026-guide, https://colinknapp.com/specs/mcp-discovery.html). Serve the same JSON at the SEP paths, and at M5 publish nh-mcp's metadata to the official registry (https://registry.modelcontextprotocol.io/, https://modelcontextprotocol.io/registry/about — a metaregistry pointing at GitHub Releases; backed by Anthropic, GitHub, PulseMCP, Microsoft).

**Why.** Discovery is distribution in the MCP world: agents and registry aggregators (PulseMCP etc.) crawl the well-known endpoints and the registry. The Master Plan already plans to *consume* `.well-known` for the `?` palette ("catalog servers from the MCP Registry without a live handshake", `NOSIS_HARNESS_Master_Plan.md:136`) — being a good citizen on the *serving* side is the mirror image, i.e. congruence. Cost: renaming/duplicating one route match arm and filling a richer card (spec version, transport, tools, auth hint).

**LAW check.** Tiny, data-only, honest (the card already carries `"notice": "local/preview only"` — keep truthful flags until the 7/28 final; the repo's own rule forbids public exposure before then).

**Value: MED · Effort: S · keyRequired: none.**

---

## Finding 7 — `ntfy` as the zero-key notification lane beside Telegram; wire fleet lifecycle events to it

**What.** The M3 Telegram hook exists (`crates/nh-tui/src/lib.rs:49–95`, `.nosis/notify.toml`, bot token in vault). Add a second, even smaller channel: **ntfy.sh** — publishing is a bare HTTP POST/PUT to a topic URL, no account, no API key; subscribers get phone + desktop pushes (Windows via the web app/CLI) (https://docs.ntfy.sh/publish/, https://docs.ntfy.sh/, https://docs.ntfy.sh/subscribe/cli/). Then route **fleet** lifecycle events through NotifyConfig, not just TUI semáforo transitions: `WAITING ON YOU` (approval needed), escalation reaching the Opus review-gate, budget stop, off-peak job dispatched, run finished.

**Why.** The walk-away workflow is the product's soul ("walk away from a 2-hour MiMo run and get pinged only when it needs you", `NOSIS_HARNESS_Master_Plan.md:184`) — and M4 made runs *hours long by design* (park-until-off-peak). A notification lane with literally zero setup friction (topic string in a TOML) removes the last excuse not to use deferred runs. Precedent: OpenCode ships an ntfy plugin for exactly this (https://github.com/lannuttia/opencode-ntfy.sh); ntfy is the community-standard for long-job pings (https://slowkow.com/notes/ntfy/). Privacy note in docs: public ntfy.sh topics are guessable — recommend a random topic or self-hosted server; message content is already scrubbed and truncated (MAX_NOTIFY_CHARS=160 exists).

**Design sketch.** `[ntfy] topic = "…" server = "https://ntfy.sh"` in the existing `parse_notify_config`; one `ureq/reqwest` POST beside the Telegram sender; move the notify sender behind a small trait so nh-fleet's `on_event` seam (`crates/nh-mcp/src/lib.rs:374`) can call it headlessly.

**LAW check.** Small (one POST), modular (second variant in an existing config surface), safe (scrubber already in the path). keyRequired: none (Telegram already needs its bot token; ntfy needs nothing).

**Value: MED-HIGH · Effort: S · keyRequired: none.**

---

## Finding 8 — Distribution rails: cargo-dist → GitHub Releases → winget + scoop (+ `cargo binstall`) for a real `winget install nosis`

**What.** Windows-first means the install story must be `winget install NosisTech.nosis`. The 2026 Rust-CLI playbook: **cargo-dist** builds/signs release artifacts and installers from CI (https://axodotdev.github.io/cargo-dist/); publish the *same* GitHub-Releases `.exe` to **winget-pkgs** (manifest via winget-create/komac, pre-installed on Win11) and a **scoop** bucket; `cargo install` stays the Rust-dev path (https://ivaniscoding.github.io/posts/rustpackaging4/, https://rust-cli.github.io/book/tutorial/packaging.html). Key lesson from the field: ship one canonical binary per release — "do not run cargo build multiple times, as builds are not guaranteed to be deterministic" (ivaniscoding post).

**Why.** Every other finding in this lens (ACP auto-detection by Intelligent Terminal, the GitHub Action installing a release binary, MCP Registry pointing at Releases) *depends on clean release artifacts existing at stable URLs*. It also feeds the trust story: a signed, reproducible single binary vs. the npm/pip supply-chain mess incumbent CLIs live in (see the LiteLLM malware incident, Finding 3). Zero product code — pure CI/config, so THE LAW is untouched.

**Design sketch (M5).** `dist init` in the workspace; release workflow on tag; komac-generated winget manifest PR; one-file scoop bucket repo. Optional later: MSIX/winapp if store presence ever matters (https://learn.microsoft.com/en-us/windows/apps/dev-tools/winapp-cli/guides/rust).

**Value: HIGH (launch-critical) · Effort: S · keyRequired: none.**

---

## Finding 9 — Finish the planned nh-mcp surface: `cost_estimate` + `receipts_query` (nosis as the fleet's cost oracle)

**What.** The Master Plan names four nh-mcp tools: *"route resolver, fleet runner, **receipts query, cost estimator**"* (`NOSIS_HARNESS_Master_Plan.md:145`); Slice C shipped three (`route_resolve`, `fleet_run`, `fleet_status` — `crates/nh-mcp/src/lib.rs:221–277`). Add the missing two as read-only tools: `cost_estimate {model?, input_tokens, output_tokens, at?}` → "now vs next off-peak window, cache-hit vs miss" one-liner (pure function over the catalog's `price_at`, already used at lib.rs:323–328); `receipts_query {run_id?|since?}` → aggregated spend from `.nosis/receipts.jsonl` / fleet ledgers, scrubbed.

**Why.** This is what makes *other* agents treat nosis as infrastructure rather than a peer: KORVIN (or Claude Code with nh-mcp in its MCP config — VS Code agent mode too, since it consumes MCP servers natively: https://code.visualstudio.com/api/extension-guides/ai/mcp) can ask "what would this batch cost right now vs. parked?" *before* deciding to delegate, and audit what a delegated run actually cost after. Cost-as-a-queryable-service is the differentiator no other MCP server offers, and it is the stickiest possible hook: once an orchestrator's planning depends on nosis's price oracle, nosis is load-bearing.

**Design sketch.** Two new match arms in `tools_call` + schema entries in `tools_list`, both `readOnlyHint: true`, both one scannable output line (the established house style). `receipts_query` reuses the ledger reader's path-traversal validation; totals only, never raw prompt text, through the scrubber.

**LAW check.** Small (each tool is ~the size of `fleet_status`), auditable (receipts ARE the audit trail — exposing them over MCP is the auditability tenet made interoperable), congruent (completes the surface the plan already specified).

**Value: MED-HIGH · Effort: S · keyRequired: none.**

---

## Deliberately NOT recommended (LAW filter)

- **A custom VS Code / JetBrains extension.** ACP (Finding 1) + MCP (VS Code agent mode consumes nh-mcp, Finding 9) cover both IDE families with protocols, not plugins. Writing TypeScript extensions would bloat and duplicate.
- **Consuming MCP Apps (server-rendered iframes) in v1.** The plan already flags stored-XSS risk ("MCP App HTML as untrusted display-only", Master Plan:167); a TUI has no iframe anyway. Revisit only for the M6 web companion.
- **A plugin/marketplace system of nosis's own.** SKILL.md + `.nosis/mcp.toml` + AGENTS.md are the extension surface; they're data, portable, and already governed by the constitution loader. A bespoke plugin runtime is the opposite of small.
- **npm-wrapper distribution.** Incongruent for a Rust, supply-chain-conscious product; winget/scoop/cargo-dist cover the audience.

## Sequencing (fits the existing roadmap)

- **M5 window:** Finding 5 (`nh exec` is already M5 scope — shape it + action), 8 (release rails), 6 (server card + registry publish, after 7/28 final), 9 (two tools, small), 7 (ntfy, small).
- **Post-launch / M5.5:** Finding 2 (Tasks — implement against the final spec), 4 (SKILL.md reader), 1 (ACP adapter), 3 (gateway).

## Sources

- https://agentclientprotocol.com/get-started/introduction
- https://github.com/agentclientprotocol/agent-client-protocol
- https://zed.dev/acp
- https://www.jetbrains.com/acp/
- https://codex.danielvaughan.com/2026/06/10/agent-client-protocol-microsoft-intelligent-terminal-codex-cli-multi-agent-ide-ecosystem/
- https://github.blog/changelog/2026-01-28-acp-support-in-copilot-cli-is-now-in-public-preview/
- https://www.morphllm.com/agent-client-protocol
- https://blog.modelcontextprotocol.io/posts/2026-07-28-release-candidate/
- https://modelcontextprotocol.io/registry/about
- https://registry.modelcontextprotocol.io/
- https://www.ekamoira.com/blog/mcp-server-discovery-implement-well-known-mcp-json-2026-guide
- https://colinknapp.com/specs/mcp-discovery.html
- https://codex.danielvaughan.com/2026/05/05/agent-skills-open-standard-portable-skills-codex-cli-cross-agent/
- https://www.paperclipped.de/en/blog/agent-skills-open-standard-interoperability/
- https://agentskills.io/home
- https://code.claude.com/docs/en/github-actions
- https://www.developersdigest.tech/blog/codex-exec-ci-headless-guide
- https://hidekazu-konishi.com/entry/claude_code_cicd_and_headless_automation.html
- https://docs.ntfy.sh/publish/
- https://docs.ntfy.sh/
- https://docs.ntfy.sh/subscribe/cli/
- https://github.com/lannuttia/opencode-ntfy.sh
- https://slowkow.com/notes/ntfy/
- https://axodotdev.github.io/cargo-dist/
- https://ivaniscoding.github.io/posts/rustpackaging4/
- https://rust-cli.github.io/book/tutorial/packaging.html
- https://learn.microsoft.com/en-us/windows/apps/dev-tools/winapp-cli/guides/rust
- https://docs.litellm.ai/docs/tutorials/claude_non_anthropic_models
- https://www.morphllm.com/claude-code-litellm
- https://www.getaiperks.com/en/ai/claude-code-router-guide
- https://code.visualstudio.com/api/extension-guides/ai/mcp
- https://code.visualstudio.com/blogs/2025/04/07/agentMode

### Repo files grounding
- `crates/nh-mcp/src/lib.rs` (tools_list/tools_call 221–417, well-known 141–148/200–207, loopback bind 97–99, fleet seams 355–385)
- `crates/nh-tools/src/mcp.rs` (stateless client, .nosis/mcp.toml schema 35–120, OAuth2 struct variant)
- `crates/nh-tui/src/lib.rs` (NotifyConfig/Telegram 49–95)
- `NOSIS_HARNESS_Master_Plan.md` (§4.5 MCP lines 127–174, nh-mcp tool list line 145, GLM free CI 288/328/393, headless exec 81/209, git guard 321)
- `00-start-here/{MILESTONES,ROADMAP,CURRENT_TASK}.md`, `01-product/PRODUCT_BRIEF.md`
