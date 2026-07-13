# NOSIS HARNESS — Master Plan v0.1
**Project:** Terminal agent harness for open-weight models (DeepSeek V4, Kimi K2.x, MiMo V2.5) + Claude/Codex as peers
**Owner:** Carlos Paredes Vargas / NosisTech LLC
**Governance:** THE LAW (small, simple, secure, safe, lightweight, readable, auditable, modular, congruent, harmonic)
**Build loop:** Claude (plan/spec) → Codex 5.5 (implement) → Opus 4.8 (review/gate) → ship
**Date:** July 9, 2026 · **Research current through:** July 9, 2026

---

## 0. Verdict and strategy

Yes — this is buildable with the Codex-builds / Opus-checks loop. But CodeWhale is ~4,300 commits of Rust across many crates. Rebuilding all of it violates THE LAW. Two routes:

**Route A — Greenfield, narrow scope (RECOMMENDED).** Original IP (House of Nosis portfolio value, same posture as KORVIN: influenced-by, not forked). Support exactly 5 providers at launch: DeepSeek, Moonshot (Kimi), Xiaomi (MiMo), Anthropic, OpenAI — plus a generic OpenAI-compatible route for LiteLLM/Ollama. Steal *patterns* from CodeWhale (route resolver, nested constitution, fleet ledger), not code.

**Route B — Fork CodeWhale (MIT permits it)** and ship Nosis Harness as an opinionated layer. Fastest to feature parity, but you inherit 100k+ lines you didn't write, a moving upstream, and it's not original IP.

This plan assumes Route A. If you choose B, Phases 1–2 collapse into "strip and rebrand" and Phase 3+ stays the same.

**What "better than CodeWhale" means concretely (the 7 differentiators):**
1. **Time-of-day cost routing** — nobody has this. DeepSeek's V4 official launch (mid-July 2026, confirmed June 29) introduces peak/off-peak pricing for the first time on a frontier API: peak = 2× rate, Beijing 9:00–12:00 and 14:00–18:00, off-peak unchanged. Confirmed rates (¥/M tokens): **V4-Pro** off-peak ¥0.025 cache-hit / ¥3.00 cache-miss / ¥6.00 out → peak ¥0.05 / ¥6.00 / ¥12.00. **V4-Flash** ¥0.02 / ¥1.00 / ¥2.00 → peak ¥0.04 / ¥2.00 / ¥4.00. Cache hits stay cheap even at peak. MiMo has night discounts too. The harness schedules deferrable work (fleet jobs, batch refactors, eval sweeps, test-fix loops) into cheap windows automatically and warns before running a marathon job into a peak block. DeepSeek gives 24h email notice before any pricing change — the catalog stores a `valid_until` and flags stale prices.
2. **Modality-aware dispatch** — DeepSeek V4 Pro is text-only; Kimi K2.6 is natively multimodal (image + video); MiMo V2.5 Pro is multimodal on Xiaomi's native platform but exposed text-only on some aggregators (OpenRouter lists it text-only). The harness knows this per-route and auto-delegates vision subtasks instead of erroring.
3. **Thinking-budget governor** — DeepSeek V4 has Non-Think / Think High / Think Max; Kimi K2.6 ships Instant / Thinking / Agent / Swarm variants; MiMo sustains 1,000+ tool calls. The harness maps task complexity → reasoning effort per provider dialect instead of one global setting.
4. **KV-cache-first context engine** — cache-hit input is ~120× cheaper than cache-miss on V4-Pro (¥0.025 vs ¥3.00). Stable prefix ordering is a first-class invariant, not an accident. Compaction follows Claude Code's five-stage progressive model (reverse-engineered April 2026): budget reduction → snip → microcompact → context collapse → auto-compact.
5. **MCP 2.0 native (Section 4.5)** — the **MCP 2026-07-28 spec** (final ships **July 28, 2026** — 19 days out) is the biggest revision since launch: stateless core, extensions framework, MCP Apps, Tasks extension, OAuth-native auth, `.well-known` discovery. Every other CLI is now migrating *away* from session-based MCP. Nosis Harness is stateless-native from commit one — it never carries the technical debt the incumbents are paying down.
6. **UX that fixes the documented pain** (Section 5) — approval fatigue, cost opacity, ambiguous status, context loss, Windows instability.
7. **Constitution-native** — THE LAW + AGENTS.md are the top authority layer, enforced in code (CodeWhale's nested-constitution idea, merged with your existing nosis-orchestration skill).

**Also new since the April/May model data (fold into build, not marketing):**
- **DSpark** (DeepSeek, June 27) — speculative-decoding framework, +60–85% generation speed on V4-Flash, +57–78% on V4-Pro vs the prior MTP-1 baseline. DeepSeek also open-sourced **DeepSpec** (MIT), the full training stack for speculative draft models, usable with Qwen3/Gemma. → Directly relevant to your local LECTOR inference on the RTX 5070 Ti; the harness's local route (LiteLLM/Ollama) should support DSpark-style speculative draft models where the backend exposes them.
- **deepclaude** — an open project that ports Claude Code's *full agent loop* onto DeepSeek V4 Pro via the Anthropic-compatible endpoint. This is the strongest public proof that **loop architecture, not model identity, determines agent behavior** — validating the whole thesis of this project. Worth reading before M0; it's a reference, not a dependency.
- **GLM5.2** (Z.ai, formerly Zhipu) — analysts call it "almost equal to Anthropic for the corporate market" at ~1/4 the cost/token. Candidate 6th route post-v1; add as a TOML catalog entry, no code change needed.

---

### 0.1 Confirmed DeepSeek V4 official pricing (¥/M tokens, load into catalog)

| Model | Window | Cache-hit in | Cache-miss in | Output |
|---|---|---|---|---|
| V4-Pro | Off-peak | ¥0.025 | ¥3.00 | ¥6.00 |
| V4-Pro | **Peak (2×)** | ¥0.05 | ¥6.00 | ¥12.00 |
| V4-Flash | Off-peak | ¥0.02 | ¥1.00 | ¥2.00 |
| V4-Flash | **Peak (2×)** | ¥0.04 | ¥2.00 | ¥4.00 |

Peak = Beijing 9:00–12:00 and 14:00–18:00 daily. Off-peak unchanged from preview. 24h email notice before any change. Cache-hit is ~120× cheaper than cache-miss — the cache-first engine is the single biggest cost lever. (MiMo V2.5 Pro ≈ $0.435 in / $0.87 out per M, no long-context surcharge, night discount. Kimi via subscription tiers.)

---

## 1. Model capability matrix (bake into the route catalog)

| | DeepSeek V4 Pro | DeepSeek V4 Flash | Kimi K2.6 | Kimi K2.7 Code | MiMo V2.5 Pro |
|---|---|---|---|---|---|
| Params (total/active) | 1.6T / 49B | 284B / 13B | 1T / 32B | K2 family | 1.02T / 42B |
| Context | 1M (384K max out) | 1M | 262K | 256K-class | 1M (131K out typical) |
| Modality | **Text only** | Text only | **Image + video native** | Code-focused | Multimodal native platform; text-only on some routes |
| Thinking modes | Non / High / Max (xhigh→max) | Non / High / Max | Instant / Thinking / Agent / Swarm | low-token coding | long-horizon, MTP 3× output speed |
| Signature strength | Reasoning + agentic coding SOTA open (80.6% SWE-V) | Cheap default | Agent Swarm (300 agents / 4,000 steps), 12h sessions | Best Kimi coding, low token burn | 1,000+ tool-call coherence, token efficiency (40–60% fewer tokens) |
| Wire | OpenAI + **Anthropic-format** (api.deepseek.com/anthropic) | same | OpenAI-compat | OpenAI-compat | OpenAI-compat |
| Pricing quirks | Peak/off-peak 2× from official launch; cache-hit ≫ cheaper | cheapest | subscription tiers | low tokens | no long-context surcharge; night discounts |

**Hard-coded gotchas (must be in the adapter layer, with tests):**
- DeepSeek legacy aliases `deepseek-chat` / `deepseek-reasoner` **die July 24, 2026 15:59 UTC**. Never emit them.
- DeepSeek V4 validator quirk: assistant turns with pure tool calls may need `reasoning_content: ""` (empty string, not null) on replay.
- Kimi K3 and DeepSeek V5/R2 are **not released** as of early July 2026 — catalog must be data, not code, so new models are a TOML entry, not a release.
- MiMo modality differs by provider route → capability flags live on the *route*, not the model.

---

## 2. Architecture (Rust workspace, KORVIN-adjacent)

```
nosis-harness/
├── crates/
│   ├── nh-core        # agent loop, turn state machine, receipts
│   ├── nh-routes      # RouteResolver: provider × model × wire × price × modality × clock
│   ├── nh-context     # budget, compaction, KV-cache prefix discipline, memory hooks
│   ├── nh-tools       # read/edit/exec/search + MCP client & server
│   ├── nh-law         # nested constitution loader + mechanical invariants (write-holds)
│   ├── nh-fleet       # append-only JSONL ledger, workers, receipts, resume
│   ├── nh-tui         # ratatui frontend (Section 5)
│   └── nh-cli         # headless exec, CI mode
├── AGENTS.md          # THE LAW, Opus-orchestrator/Codex-implementer roles
└── .nosis/            # per-repo law, hooks, snapshots
```

Design rules (from the June 2026 harness literature — a harness = agent loop + tool interface + context management + control mechanisms):
- **Single RouteResolver** is the only component that can mint a resolved route (CodeWhale's best idea — keep it). A resolved route carries: endpoint, wire protocol (OpenAI / Anthropic Messages), model ID, context limit, price *at the current clock time*, modality flags, thinking-mode dialect.
- **Loop taxonomy** (Anthropic, June 2026): turn-based (default), goal-based (`/goal` with deterministic stop conditions + token budget), time-based (`/schedule` — this is where off-peak routing shines), proactive (v2, not v1).
- **Verification-first**: every phase gate runs the repo's verification policy (tests/lint/build) before the loop advances; failure classification per deepset's framework (context / constraint / verification / planning failure) is logged in the receipt so you can see *why* runs fail, not just that they failed.
- **Rollback**: side-git snapshots outside the repo's `.git` (CodeWhale pattern), one per turn, addressable from the TUI timeline.
- **Memory**: retain / recall / reflect interface (Hindsight-style, June 2026), pluggable — v1 is SQLite (you already run this pattern in KORVIN).
- **Prompt-injection posture**: tool outputs are data; constitution + approval + sandbox enforced in code, never overridable by model text (avoid the Lethal Trifecta: external input + secrets + state mutation without gates).

---

## 3. The routing brain (differentiators 1–4)

**Task classifier → route policy.** Each task gets tags: `{modality, horizon, complexity, deferrable, secret-touching}`. Policy table (user-editable TOML):

| Task shape | Default route | Thinking |
|---|---|---|
| Quick edit / Q&A | DeepSeek V4 Flash | Non-think |
| Complex refactor / debugging | DeepSeek V4 Pro | Think High |
| Hardest reasoning, stuck loops | V4 Pro | Think Max (needs ≥384K window) |
| Anything with an image/video/screenshot | Kimi K2.6 (or MiMo native route) | Agent |
| High-volume coding, token-sensitive | Kimi K2.7 Code or MiMo V2.5 Pro | — |
| 500+ tool-call marathons | MiMo V2.5 Pro | — |
| Massively parallel decomposable work | Kimi K2.6 Swarm or nh-fleet | — |
| Review/gate | Opus 4.8 (Anthropic Messages, native) | adaptive |

**Clock-aware pricing.** Each route's price entry is `fn price(at: DateTime) -> Cost` with a `valid_until` field. Deferrable fleet jobs auto-queue into off-peak windows (DeepSeek Beijing peak 9–12 & 14–18 = 2×; MiMo night discount). TUI shows a peak/off-peak chip and warns before running a marathon job into a peak block. Confirmed V4 rates are in Section 0.1. Batch/eval/synthetic-data sweeps moved off-peak = up to 50% saving on DeepSeek alone; cache hits stay cheap even at peak, so the cache-first engine compounds the win.

**Cache discipline.** System prompt + constitution + AGENTS.md render into a byte-stable prefix; dynamic content (memory, file reads) appends after the cache breakpoint. A `cache-hit %` metric lives in the status line — this is the "key metric of 2026" and no CLI surfaces it today.

**Escalation ladder.** On verification failure ×2 at a given tier, escalate one tier (Flash → Pro High → Pro Max → Opus 4.8 review) with the failure receipt attached. Never silently retry the same route more than twice.

---

## 4. Fleet & swarm

- `nh fleet run tasks.json --max-workers N` with append-only ledger, heartbeats, typed receipts (pass/fail/partial/skip/timeout), idempotent `resume` — CodeWhale's proven shape.
- **Two parallelism backends:** (a) native nh-fleet workers (any model), (b) Kimi Agent Swarm passthrough for tasks that decompose well — the harness writes the swarm brief, Kimi coordinates up to 300 sub-agents internally, harness verifies outputs. Cheaper than orchestrating 300 API loops yourself.
- Every fleet job is deferrable by default → off-peak scheduler.

---

## 4.5 MCP — first-class, stateless-native (differentiator 5)

**Yes, the harness uses MCP — both directions — and it's built against the new spec, not the old one.**

The timing is a gift. The **MCP 2026-07-28 specification** locks its final on July 28, 2026 (release candidate frozen May 21). It's the largest revision since launch, with a 12-month deprecation window for the old 2025-11-25 version. Every incumbent CLI (Claude Code, Codex, Cline, OpenCode) now has to *migrate* session-based servers off sticky routing. Nosis Harness has no legacy to migrate — it targets 2026-07-28 from the first commit and supports 2025-11-25 only as a fallback client. That's a structural head start.

**What changed in the spec that the harness must implement:**
- **Stateless core (SEP-2567, SEP-2575).** The `initialize` handshake and `Mcp-Session-Id` header are gone. Every request is self-contained: protocol version, client info, and capabilities travel in `_meta`. Any server instance can serve any request → clients (us) must send full context per call and not assume a pinned instance.
- **Explicit state handles, not hidden sessions.** Servers that need cross-call state mint a handle (`browser_id`, `repo_id`) returned from a tool; the model passes it back as an ordinary argument. The harness surfaces these handles in the receipt/timeline so state is auditable — this fits THE LAW's "auditable" tenet perfectly.
- **`.well-known` discovery + `server/discover`.** Clients can read a server's "business card" without connecting. → powers the `?` palette (Section 5) and lets Nosis Harness catalog servers from the MCP Registry without a live handshake.
- **Response caching (`ttlMs`).** `tools/list` results are cacheable → fewer round-trips, and cache-hit discipline extends to tool metadata, not just prompt prefix.
- **OAuth 2.1 / OIDC-native auth.** Servers are formally OAuth 2.1 resource servers with Resource Indicators (RFC 8707). → the harness ships a proper token store with refresh, killing the #1 cross-tool bug of 2026 (auth-refresh failures) instead of silent retry loops.
- **Tasks extension** — first-class long-running work. Maps cleanly onto nh-fleet: a fleet job can *be* an MCP Task, or drive one.
- **Extensions framework (SEP-2133), reverse-DNS IDs, negotiated via capabilities map.** → Nosis can publish its own `com.nosistech.*` extensions (e.g. a route-cost extension) without forking the spec.
- **W3C Trace Context in `_meta` (SEP-414).** traceparent/tracestate flow through tool calls → one OpenTelemetry span tree across harness → client → server → downstream. Free observability.

**Two roles:**
1. **MCP client (nh-tools).** Connects to any 2026-07-28 or 2025-11-25 server. Brings your whole stack along: filesystem, GitHub, Playwright, databases, n8n webhooks, KORVIN. Config in `.nosis/mcp.toml` (schema below), per-repo.
2. **MCP server (nh-mcp).** Nosis Harness exposes its own tools — route resolver, fleet runner, receipts query, cost estimator — over MCP so KORVIN or another agent can drive it. This is what stops it being a dead-end CLI and makes it a node in your orchestration layer.

**`.nosis/mcp.toml` (client config schema):**
```toml
[servers.playwright]
url = "http://localhost:8931/mcp"     # stateless Streamable HTTP
spec = "2026-07-28"                    # or "2025-11-25" fallback
auth = "oauth2"                        # none | apikey | oauth2
scopes = ["browse"]
default_mode = "snapshot"             # avoid token-bomb screenshots (your Playwright lesson, encoded)
trust = "ask"                          # inherits trust dial: auto | ask | block

[servers.github]
url = "https://api.githubcopilot.com/mcp"
spec = "2026-07-28"
auth = "oauth2"
trust = "ask"
```

**Security posture — MCP is where the Lethal Trifecta bites, and the new spec adds surfaces (per Akamai's July analysis):**
- **Header leakage.** Never map secrets/PII into `Mcp-Method`/`Mcp-Name` or `x-mcp-*` — they become visible to every proxy and log. nh-mcp lints outbound headers for secret patterns.
- **Desync / protocol-confusion.** Stateless HTTP + custom headers open request-smuggling vectors → strict header validation, reject ambiguous framing.
- **MCP Apps = web risk.** Server-rendered UIs in sandboxed iframes reintroduce stored-XSS. If Nosis ever consumes MCP Apps UIs, they render sandboxed with CSP; v1 treats MCP App HTML as untrusted display-only.
- **Tasks = DoS vector.** Task creation is cheap for the client, expensive for the server → the harness rate-limits task creation and treats task output as data, never instructions.
- **Tool outputs are always data.** A poisoned tool result can never auto-approve its own `exec`; state-mutating MCP calls route through the trust dial regardless of autonomy level.

**MCP milestone exit criteria (testable):**
- *M1 exit adds:* connect to one 2026-07-28 stateless server, call a tool, pass a returned handle back on the next call — no session header anywhere on the wire.
- *M3 exit adds:* `?` palette lists every configured MCP server + tool with live state (enabled / auth-ok / stale / discover-only via `.well-known`).
- *M4 exit adds:* nh-mcp exposes route-resolver + fleet-runner as an MCP server; KORVIN connects and triggers a fleet run; OAuth token refresh survives a forced expiry mid-session.

---

## 5. UX/UI — fixing the documented pain

Documented complaints across Claude Code / Codex / Gemini CLI / CodeWhale (2025–2026): approval-prompt fatigue, agents losing context in long sessions, tool-call retry loops, ambiguous status indicators, mouse-tracking loss on exit, opaque cost and rate-limit shock, hooks/plugins being undiscoverable, Windows color/stability bugs, renderer crashes.

**Nosis Harness answers:**

1. **Semáforo status model.** Exactly one state at all times, color + word + icon: `WORKING` (green) / `WAITING ON YOU` (amber, bell) / `BLOCKED` (red, reason) / `IDLE`. No ambiguous spinners. State changes emit OS notification *and* optional Telegram push via your existing KORVIN bot — walk away from a 2-hour MiMo run and get pinged only when it needs you.
2. **Cost HUD.** Footer chips: session cost so far, cache-hit %, current route + peak/off-peak indicator, projected cost-to-goal (rolling estimate from tokens/turn × remaining plan items), budget bar with hard stop. Kills "rate-limit shock."
3. **Trust dial, not YOLO binary.** Autonomy is per-path/per-command, compiled from `.nosis/law.toml` invariants: e.g. `src/**` auto-approve edits, `migrations/**` always ask, `rm|curl|ssh` always ask, protected paths hard-block even in max autonomy. Approval fatigue drops because you only get asked about things you declared sensitive.
4. **Timeline scrubber.** Left-rail vertical timeline of turns; each entry = snapshot + receipt + cost. Arrow keys scrub, `Enter` inspects diff, `R` restores. Rollback becomes visual, not a slash-command you have to remember.
5. **Discoverability palette.** `?` opens a fuzzy palette listing every command, hook, skill, and MCP tool *with its current state* (enabled/auth-ok/stale). No more opaque plugin ecosystem.
6. **Windows-first.** You run Windows 11 on both machines; Claude Code's sandbox still doesn't support native Windows. Target: first-class native Windows (crossterm, tested renderer, Job Objects + restricted tokens for containment) + Linux (Landlock/seccomp) + macOS (Seatbelt). This alone is a real wedge.
7. **Companion dashboard (v2).** Axum web view (KORVIN pattern) for fleet monitoring and diff review from the phone — TUI stays minimal per THE LAW.
8. **Degradation guard.** At 70% of route context: auto-compact with a visible marker in the timeline (your Claude Code `/compact` habit, automated). Screenshot-type tools default to snapshot/text modes to avoid token bombs — the lesson from your MCP Playwright incident, encoded as a default.

---

## 6. Build plan (Codex 5.5 implements, Opus 4.8 gates)

**Roles (AGENTS.md):** Claude = planner/spec owner. Codex 5.5 = implementer, small PRs only, must run `cargo test && cargo clippy -- -D warnings` before handoff. Opus 4.8 = reviewer: checks THE LAW conformance, security posture, and that the code matches this spec; may reject with a written receipt. Build on ASUS / verify over SSH on Predator, per your established loop.

**M0 — Skeleton (week 1).** Workspace, nh-core turn loop against ONE route (deepseek-v4-flash, OpenAI wire), read/edit/exec tools, plain approval prompt, receipts to JSONL. *Exit: fixes a failing test in a sample repo end-to-end.*

**M1 — RouteResolver + matrix + MCP client (weeks 2–3).** Catalog TOML with all 5 providers, wire adapters (OpenAI + Anthropic Messages), thinking-mode dialects, modality flags, clock-aware pricing, DeepSeek gotcha tests (alias ban, reasoning_content). MCP client against one **stateless 2026-07-28** server with handle passthrough. *Exit: `/model` and `/provider` switch mid-session; peak/off-peak price shown correctly; MCP tool call with no session header on the wire.*

**M2 — Context engine + law (weeks 3–5).** Stable-prefix cache discipline + cache-hit metric, compaction at 70%, nested constitution loader (bundled law → user law → repo `.nosis/law.toml` → AGENTS.md → memory), mechanical write-holds for protected paths. *Exit: cache-hit % >60% on a 50-turn session; protected path blocked in max autonomy.*

**M3 — TUI (weeks 5–7).** Semáforo, cost HUD, timeline scrubber + side-git snapshots, trust dial, `?` palette, Telegram notify hook. Windows renderer test matrix (Windows Terminal, VS Code terminal, ConHost). *Exit: full session on the Predator natively, zero renderer artifacts.*

**M4 — Fleet + swarm + scheduler + nh-mcp server (weeks 7–9).** Ledger, workers, resume, receipts; off-peak scheduler; Kimi Swarm passthrough; escalation ladder with Opus 4.8 gate route; nh-mcp exposes route-resolver + fleet-runner over MCP. *Exit: 10-task fleet run survives a kill -9 and resumes idempotently; deferred job executes off-peak; KORVIN connects to nh-mcp and triggers a fleet run; OAuth refresh survives forced expiry.*

**M5 — Hardening + launch (weeks 9–10).** Sandbox tiers, headless `nh exec` for CI, docs, nosistech.com launch post (Category: AI Projects, MEDIUM risk disclaimer), CC BY 4.0 footer. Optional: publish as the flagship of the 55-agent Series 2 line.

---

## 7. Risks (stated, then handled)

- **Scope creep** is the #1 killer. Mitigation: 5 providers, 6 differentiators, nothing else in v1. Every feature request goes to a `LATER.md`.
- **Maintenance treadmill** — model catalogs rot (K3, V5, Opus 5 will land). Mitigation: catalog is data (TOML), adapters are 2 wire protocols only.
- **DeepSeek V4 is still preview** until mid-July official launch; pricing/behavior may shift, and peak-hour definitions could change. Mitigation: pricing is data with a `valid_until` field; harness flags stale price data instead of inventing numbers (CodeWhale's honest-cost rule — keep it).
- **Windows sandboxing is genuinely hard** (no Landlock equivalent). Mitigation: v1 ships approval-gating + restricted tokens on Windows, full syscall sandboxing on Linux; be honest in docs.
- **One person + two AI builders** is real leverage but review debt accumulates. Mitigation: Opus 4.8 gate is mandatory per PR, no direct-to-main, receipts on every merge.
- **MCP spec finalizes July 28** — the RC is frozen but the final could carry small deltas, and Tier-1 SDK support lands within the 10-week window. Mitigation: target the frozen RC now, pin the SDK version, add a conformance-suite check in CI, treat 2025-11-25 as fallback. Don't ship nh-mcp *server* to the public until the final lands.
- Note: the official CodeWhale site is **codewhale.net** (per the repo). Treat codewhale.ai content as unofficial.

---

## 8. First prompt for Codex 5.5 (paste into Codex after repo init)

> Read AGENTS.md and NOSIS_HARNESS_Master_Plan.md. Implement Milestone M0 only: a Rust workspace `nosis-harness` with crates nh-core, nh-routes (stub), nh-tools, nh-cli. nh-core runs a turn loop against DeepSeek `deepseek-v4-flash` via the OpenAI-compatible endpoint (base_url https://api.deepseek.com, key from env NH_DEEPSEEK_KEY), with tools read_file, edit_file, exec_shell (approval prompt before every exec). Every turn writes a JSONL receipt to .nosis/receipts.jsonl. Follow THE LAW: small, simple, readable, auditable. No TUI yet. Include integration test with a mocked provider. Run cargo test and cargo clippy -D warnings before finishing.

---
---

# APPENDIX A — Provider Deep-Dive & Access Architecture
**Research current: July 11, 2026. Supersedes Section 1 where they conflict.**

## A.0 The two-backend architecture (this is the key structural decision)

Your access reality: **API credits** for DeepSeek, Kimi, MiMo. **Subscriptions only** (no API credits) for Claude, Codex/ChatGPT, Gemini. **Nothing yet** for GLM. So Nosis Harness needs two backend classes, and this becomes a core `nh-routes` concept:

**Class 1 — Direct API routes (token-metered).** DeepSeek, Kimi, MiMo, GLM. Speak OpenAI or Anthropic wire directly, keys in nh-vault, costs in tokens × price(clock).

**Class 2 — Delegate routes (subscription-metered).** Claude, Codex, Gemini. No API key exists — access is OAuth via each vendor's own agent. The harness drives them **headless as subprocesses**:
- Claude → `claude -p "..."` (Claude Code headless; included in your subscription)
- Codex → `codex exec "..."` (Codex CLI; included in ChatGPT plans; OAuth sign-in)
- Gemini → **Antigravity CLI** (NOT Gemini CLI — see A.6)

A delegate route wraps the child CLI: passes the brief + files, captures output + diffs, writes a normal Nosis receipt. Cost accounting is in **plan quota units** (messages/credits per window), not tokens — the Cost HUD must show dual units. Delegate routes are "already paid for," so the router treats them as zero-marginal-cost but **quota-scarce**: reserve them for what they're uniquely good at (Opus = review/gate, Codex = implementation bursts) and push bulk/batch work to Class 1 off-peak.

This also resolves the GLM question: no credits needed on day one — see A.5.

## A.1 DeepSeek (API credits ✅) — the reasoning + off-peak workhorse

**Current models (safe):** `deepseek-v4-pro`, `deepseek-v4-flash`. **Banned:** `deepseek-chat`, `deepseek-reasoner` — dead July 24, 2026 15:59 UTC; adapter refuses to emit them.
- Official V4 launch mid-July: same model IDs, peak/off-peak pricing activates (Section 0.1), 功能优化/performance improvements, 24h email notice before billing changes.
- **DSpark** (June 27): server-side speculative decoding, +57–78% speed on Pro, +60–85% on Flash — you get this for free on the hosted API; expect materially faster turns after official launch. **DeepSpec** open-sourced (MIT) for local draft-model training.
- Modality: **text only** (both models). Any image/video/audio subtask must dispatch elsewhere.
- Thinking: Non / High / Max (xhigh→max). Think Max wants ≥384K context headroom.
- Wire: OpenAI (`api.deepseek.com`) **and Anthropic** (`api.deepseek.com/anthropic`). Prefer the Anthropic wire for agent loops — it's the deepclaude-proven path.
- Gotcha (test-covered): assistant turns with pure tool calls need `reasoning_content: ""` (empty string, not null) on replay.
- Efficiency levers, ranked: (1) cache-hit discipline — ¥0.025 vs ¥3.00 is ~120×; (2) off-peak scheduling — 2× peak Beijing 9–12/14–18 = **La Ceiba (UTC-6) 19:00–22:00 and 00:00–04:00** — so your *daytime* is DeepSeek's off-peak. You're structurally advantaged: schedule freely 05:00–18:00 local, avoid 19:00–22:00 local; (3) Flash-first escalation ladder; (4) thinking budget matched to task.

## A.2 Kimi / Moonshot (API credits ✅) — the multimodal coder

**Current models (safe):** `kimi-k2.7-code` (flagship coder, June 12), `kimi-k2.7-code-highspeed`, `kimi-k2.6` (general flagship, April 20). K2.5 exists but K2.6/K2.7 supersede it — don't target it. Older Moonshot-V1 line is being retired — never emit it. K3: **not released** (rumored 3–4T) — catalog entry when real.
- **K2.7 Code protocol nuances (critical, adapter-enforced):**
  - **Always-thinking**: no non-thinking mode exists; don't send a mode toggle.
  - **`preserve_thinking` on**: the full reasoning chain must be **persisted across multi-turn conversations** — the context engine must carry reasoning_content in history for Kimi routes (opposite of the usual strip-reasoning compaction habit!). Compaction for Kimi routes must preserve reasoning blocks or the model degrades.
  - ~30% fewer thinking tokens than K2.6 for better results — effective cost per task is lower than sticker.
  - **Multimodal**: MoonViT 400M vision encoder — text + image + video into the *coding* model. Screenshots/diagrams go straight into Kimi coding turns; no delegation hop needed.
- Pricing: $0.95/M in (cache-miss), **$0.19 cache-hit (80% off)**, $4.00/M out. Highspeed variant: $8.00/M out at ~180 tok/s (260 short-context) — latency-critical interactive turns only.
- K2.6: $0.55–0.60/M in, $2.50–2.65/M out, 262K context, image+video, **Agent Swarm** (300 agents/4,000 steps), 12h autonomous sessions, Instant/Thinking/Agent/Swarm variants.
- Wire: OpenAI **and Anthropic**-compatible. Kimi Code subscription plans exist (weekly quotas) but you're on API credits — stay there for harness use.
- Efficiency levers: cache-hit discipline, K2.7's token efficiency (route high-volume coding here), Swarm passthrough for parallel decomposable work, highspeed only when a human is waiting.

## A.3 MiMo / Xiaomi (API credits ✅) — the marathon runner

**⚠️ Deprecation alert (verify your configs TODAY):** the entire **MiMo-V2 series went offline June 30, 2026 — old model names are invalid.** If any KORVIN/LiteLLM config still says `mimo-v2-pro` or `mimo-v2-flash`, it is broken now. Current: `mimo-v2.5-pro` (flagship), `mimo-v2.5` (standard, cheaper, omni-modal).
- V2.5-Pro: 1.02T/42B, 1M context, hybrid SWA+GA attention (7× KV-cache reduction), MTP 3× output speed, sustains 1,000+ tool calls. Xiaomi's own launch demos used **Claude Code as the harness** — it's literally built to be driven by a harness like yours.
- Modality: **route-dependent.** Xiaomi's native platform is omni-modal (image/video/audio/text) and improving TTS/ASR; aggregator routes (OpenRouter) expose text-only. Catalog flags modality per-route, defaulting native platform = multimodal, aggregator = text.
- Same `reasoning_content` persistence requirement as Kimi in thinking mode with tool calls (documented in V2-Flash docs, applies to family): **persist reasoning history in messages**.
- Pricing: ~$0.435/$0.87 per M (≈ DeepSeek Pro), **no long-context surcharge** (same rate at 10K or 900K — unique; route your longest-context jobs here), night discounts, Token Plans auto-renew, 40–60% fewer tokens per task than Western flagships.
- **UltraSpeed**: 1,000–1,200 tok/s serving mode of the same 1T model on commodity 8-GPU nodes (FP4 experts + DFlash + TileRT), 3× price for ~10× speed, application-gated (June trial ended; enterprise via business-mimo@xiaomi.com). Catalog it as a gated route: off by default, note the application path. The **MiMo-V2.5-Pro-FP4-DFlash checkpoint is open on Hugging Face** — long-term self-host option for Predator-class successors, not current hardware.
- Wire: OpenAI **and Anthropic**-compatible on platform.xiaomimimo.com.
- Efficiency levers: longest-horizon jobs and biggest-context jobs route here; night discount + no context surcharge stack; MTP makes output cheap in wall-clock terms.

## A.4 GLM / Z.ai (no credits — solved) — the free on-ramp

**Current models:** `glm-5.2` (June 13–16, 744B MoE, 1M ctx, 128K out, MIT weights, thinking High/Max only), `glm-5-turbo`, `glm-4.7`, **`glm-4.7-flash` — listed as fully FREE on Z.ai's pricing table.**
- **Day-one path with zero purchase:** (1) register on bigmodel.cn → **20M free tokens**; (2) `glm-4.7-flash` free tier for harness integration testing — it's the perfect CI/smoke-test route so your test suite burns $0; (3) buy GLM-5.2 credits only when a workload proves it.
- GLM-5.2 pricing when you do: $1.40/M in, **$0.26 cached (−81%, storage limited-time free)**, $4.40/M out direct; OpenRouter marketplace runs $0.93–$1.20 in across ~20 hosts but **watch output caps (32K on some hosts vs 128K direct) and quantization (fp8/fp4 varies, undisclosed on some)** — catalog must record output-cap and quant per route, not just price.
- **GLM Coding Plan trap:** the $18/$72/$160 subscription is *strictly limited to Z.ai-supported tools* (Claude Code, Cline, etc.). Nosis Harness would not qualify — calls outside supported tools bill as normal API. Don't buy the Coding Plan for the harness; pay-per-token or free tier only.
- Benchmarks worth noting: Intelligence 51.1 (99th pct), SWE-bench Pro 62.1 (reported > GPT-5.5's 58.6) at ~1/6 the closed-frontier cost. Wire: OpenAI-compatible.

## A.5 Claude / Anthropic (subscription ✅, no API credits) — the gate

- Route: **delegate via Claude Code headless** (`claude -p`) and/or the Agent SDK under your existing plan. Your reviewer stays **Opus 4.8** ($5/$25/M, 1M context, SWE-bench Verified ~88.6% independent) — the strongest independent track record in the lineup and already your AGENTS.md gatekeeper.
- Quota nuance: plan usage is **shared across Claude surfaces** — a heavy chat day eats the same allowance as Opus review runs. The Cost HUD's delegate-route panel should show remaining-window estimates so a fleet review pass doesn't starve your interactive work.
- Efficiency lever: Opus reviews are the most expensive quota you own — batch them. The escalation ladder sends *receipts + diffs*, never raw transcripts, to the review route.

## A.6 Gemini / Google (subscription ✅) — the changed one

- **Your access path changed June 18, 2026:** Gemini CLI and Code Assist extensions **stopped serving Google AI Pro/Ultra and free individual users**. Individual subscription access now goes through **Antigravity CLI** (Go, closed-source; Skills/Hooks/Subagents/Extensions migrate as Antigravity plugins). If you have any scripts calling `gemini -p`, they are dead for your account class.
- Current models via Antigravity: **Gemini 3.5 Flash, Gemini 3.1 Pro, Gemini 3 Flash** (Antigravity also exposes Claude Sonnet/Opus 4.6 and gpt-oss-120b — ignore; you have better native routes).
- API alternative exists but is paid-only for 3.1 Pro (3.1 Flash: $0.50/$3.00) — you have no API credits, so the delegate route via Antigravity CLI is the only zero-new-spend path. Free-tier quotas have been cut repeatedly and are unpublished — treat Gemini delegate quota as unreliable; router marks it `best-effort`, never on the critical path.
- What it's uniquely good for: built-in Google Search grounding (live docs/CVEs pulled into a coding task). Use as the "research subtask" delegate.

## A.7 OpenAI / Codex (subscription ✅) — your implementer just upgraded

- **GPT-5.6 went GA July 9, 2026 — two days ago.** Family: **Sol** (flagship: complex reasoning, coding, cybersecurity, long agentic tasks; new `max` reasoning effort; **`ultra` mode** = parallel subagents; Cerebras-served variant at 750 tok/s), **Terra** (≈GPT-5.5 performance at half price — "work you previously gave GPT-5.5"), **Luna** (fast/cheap). All: **1.05M context, 128K out.**
- **Action for AGENTS.md:** your implementer role "Codex 5.5" should be re-pointed. OpenAI's own guidance: Terra is the natural successor for GPT-5.5-class work; Sol for the hardest changes. Suggested: **implementer = GPT-5.6 Terra default, Sol for M2 (context engine) and anything touching nh-law/security.** `gpt-5.2`/`gpt-5.3-codex` are **deprecated under ChatGPT sign-in** — update any `codex exec --model` references. Update your Codex CLI binary first: outdated clients don't show 5.6 at all.
- Quota nuances: Codex and **ChatGPT Work share one usage pool**; credits schedule Sol 125/12.5/750 (in/cached/out per M), Terra half, Luna one-fifth; Plus-plan local messages ~15–90 (Sol) per 5h window; `ultra` available in Codex on Plus+. API list (if you ever buy credits): Sol $5/$30, Terra $2.50/$15, Luna $1/$6, **cached input −90%, cache writes 1.25×, 30-min minimum cache life**.
- Codex CLI is Rust-native, open-source, has `codex exec` for headless, three-tier sandbox — the cleanest delegate to wrap.

## A.8 Key & credential security — `nh-vault` (new crate, required before M1)

THE LAW: minimize surface. Rules, enforced in code:
1. **No plaintext keys anywhere at rest.** Not in `.nosis/*.toml`, not in shell profiles, not in receipts. Storage = OS-native secret store via one interface: **Windows Credential Manager (DPAPI)** on the Predator/ASUS, macOS Keychain, Linux secret-service/keyring. Rust: `keyring` crate. `nh key add deepseek` prompts, stores, never echoes.
2. **Injection at spawn, memory-only.** Keys are read from vault at request time and injected into the provider client (or child-CLI env for delegates) per-call; never written to disk, never logged, zeroized after use (`zeroize` crate).
3. **Redaction filter on every output path.** TUI, logs, receipts, and MCP responses pass through a scrubber matching known key formats (`sk-`, `csk-`, JWT shapes) + the literal values currently in vault. A leaked key in a stack trace is a test failure.
4. **Per-route scoping.** Each route names its vault entry; a DeepSeek call can never read the Kimi key. Delegate routes hold **OAuth tokens, not keys** — stored in the same vault, refresh handled by the child CLI itself where possible (claude/codex manage their own auth; harness never touches those token files).
5. **MCP header lint** (from Section 4.5): outbound `Mcp-*`/`x-mcp-*` headers scanned for secret patterns before send — the Akamai leak vector, closed.
6. **Git guard:** `.nosis/` ships a `.gitignore` covering receipts and any auth artifacts; nh-law adds a mechanical write-hold on committing files matching secret patterns (pre-commit hook installed by `nh init`).
7. **LiteLLM option:** you already run a LiteLLM gateway — supported as a single-upstream mode (harness holds one gateway key; provider keys live only on the VPS). Trade-off stated honestly: simpler key surface, but you lose per-provider Anthropic-wire features and clock-aware pricing must then read LiteLLM's cost tables. Default = direct + vault; gateway mode = flag.

## A.9 Routing policy v2 (supersedes Section 3 table)

| Task shape | Route | Why |
|---|---|---|
| CI / smoke tests / harness self-test | **GLM-4.7-Flash (free)** | $0 test suite |
| Quick edits, Q&A | DeepSeek V4 Flash, non-think | cheapest capable |
| High-volume coding | **Kimi K2.7 Code** | 30% token efficiency + cache $0.19 |
| Coding with screenshots/diagrams | Kimi K2.7 Code | native MoonViT vision in the coder |
| Hard debugging / reasoning | DeepSeek V4 Pro, Think High→Max | best open reasoning; your daytime = its off-peak |
| Marathon (500+ tool calls) / huge context | **MiMo V2.5-Pro** | 1,000+ call coherence, no context surcharge |
| Massively parallel decomposable | Kimi K2.6 Swarm or nh-fleet | 300-agent native |
| Non-code multimodal (audio/video heavy) | MiMo V2.5 (native platform) | omni-modal |
| Web-grounded research subtask | Gemini 3.1 Pro (Antigravity delegate) | Search grounding; best-effort quota |
| Implementation bursts (build loop) | **GPT-5.6 Terra/Sol (Codex delegate)** | subscription already paid; ultra for parallel |
| Review / gate / security | **Opus 4.8 (Claude Code delegate)** | your AGENTS.md gatekeeper; batch it |
| Escalation ladder | Flash → K2.7 → V4 Pro High → V4 Pro Max → Opus gate | 2 failures per tier, receipt attached |

Deprecation-safe catalog (all routes above are current, none scheduled for removal): deepseek-v4-pro/flash · kimi-k2.7-code(+highspeed) · kimi-k2.6 · mimo-v2.5-pro · mimo-v2.5 · glm-5.2 · glm-4.7-flash · gpt-5.6-sol/terra/luna · gemini-3.1-pro · gemini-3.5-flash · claude-opus-4.8. **Banned strings (adapter-rejected): deepseek-chat, deepseek-reasoner, mimo-v2-*, gpt-5.2*, gpt-5.3-codex, moonshot-v1-*.**

## A.10 What changed vs the earlier sections (delta log, July 11)
1. GPT-5.6 Sol/Terra/Luna GA July 9 → implementer re-point; "Codex 5.5" references in AGENTS.md are now legacy.
2. MiMo V2 series hard-deprecated June 30 → audit all existing configs (KORVIN, LiteLLM) today.
3. Gemini subscription access = Antigravity CLI only since June 18 → delegate adapter targets Antigravity, not gemini-cli.
4. GLM day-one path = free tier (4.7-Flash + 20M bigmodel tokens); Coding Plan explicitly NOT usable by an unsupported harness.
5. Kimi/MiMo reasoning-persistence requirement → context engine gets a per-route `preserve_reasoning: bool`; compaction respects it.
6. Two-backend architecture (API routes vs delegate routes) promoted to core nh-routes concept; Cost HUD gets dual units (tokens vs quota).
7. New crate: nh-vault (A.8) — lands in M0, not later; keys exist from the first commit, so security does too.

---

# APPENDIX B — Complete Model Catalog (verified July 11, 2026)
**This is the full routable catalog. Supersedes Appendix A tables where they conflict. Every model here is current and not scheduled for deprecation. Load into `catalog.toml`.**

## B.1 DeepSeek — 2 models (complete; the API surface really is this small)
| Model ID | Ctx / Out | Modality | Price ¥/M off-peak (in-hit / in-miss / out) | Notes |
|---|---|---|---|---|
| `deepseek-v4-pro` | 1M / 384K | Text | 0.025 / 3.00 / 6.00 (peak 2×) | Non/High/Max thinking; Anthropic wire available; Think Max wants ≥384K headroom |
| `deepseek-v4-flash` | 1M / 384K | Text | 0.02 / 1.00 / 2.00 (peak 2×) | Cheap default; escalation-ladder floor |

Dead July 24: `deepseek-chat`, `deepseek-reasoner`. Quirk: `reasoning_content: ""` on tool-only replay turns. DSpark speedups server-side after official launch.

## B.2 Kimi / Moonshot — 4 current models
| Model ID | Ctx | Modality | Price $/M (in-miss / in-hit / out) | Notes |
|---|---|---|---|---|
| `kimi-k2.7-code` | 262K | Text+**image+video** (MoonViT) | 0.95 / 0.19 / 4.00 | **Always-thinking, preserve_thinking forced ON** — persist reasoning across turns; ~30% fewer thinking tokens than K2.6 |
| `kimi-k2.7-code-highspeed` | 262K | same | 0.95 / 0.19 / **8.00** | ~180 tok/s (260 short-ctx); human-waiting turns only |
| `kimi-k2.6` | 262K | Text+image+video | ~0.55–0.60 / — / 2.50–2.65 | Instant/Thinking/Agent/**Swarm** (300 agents, 4,000 steps, 12h sessions) |
| `kimi-k2-thinking` / `kimi-k2-instruct` | 256K | Text | ~0.60 / — / 2.50 | Legacy-gen deep-reasoning / light-general variants; still served, K2.6/K2.7 usually better — catalog as fallback only |

Retiring: Moonshot-V1 line (never emit). K3: unreleased. Wire: OpenAI + Anthropic. Weights: Modified MIT.

## B.3 MiMo / Xiaomi — 3 current text/omni models + TTS (V2 series: ALL DEAD June 30)
Official deprecation map (from Xiaomi, confirmed): `mimo-v2-pro`→`mimo-v2.5-pro` (params fully adapted) · `mimo-v2-omni`→`mimo-v2.5` (params fully adapted) · `mimo-v2-flash`→`mimo-v2.5` (**⚠️ parameter DEFAULTS changed** — do not assume old defaults; Codex must diff the migration doc before wiring) · `mimo-v2-tts`→`mimo-v2.5-tts` (**timbre remap: `mimo_default` → 冰糖 on Chinese clusters, `mia` on other clusters** — if LECTOR/KORVIN voice configs reference mimo_default, output voice changed under you).

| Model ID | Ctx / Out | Modality | Price $/M (marketplace low → first-party) | Notes |
|---|---|---|---|---|
| `mimo-v2.5-pro` | 1M / 131K | **Omni** on native platform (image/video/audio/text); text-only on some aggregators | in 0.435→~1.00 / out 0.87→~3.00 / cached ~0.20 | 1,000+ tool-call coherence; MTP 3× output; flat price across context lengths; night discounts; reasoning persistence required in thinking+tools mode |
| `mimo-v2.5` | 1M / 131K | **Omni** | in **0.105–0.14** / out **0.28** | "Pro-level agentic performance at ~half the inference cost," beats old V2-Omni on image/video — **the cheapest 1M-context multimodal route in the entire catalog**; default multimodal + default cheap-bulk route |
| `mimo-v2.5-pro` UltraSpeed | 1M | text-first serving mode | 3× standard | 1,000–1,200 tok/s; application-gated (business-mimo@xiaomi.com); catalog off-by-default |
| `mimo-v2.5-tts` | — | TTS | see platform | Voice design/cloning from one sentence; relevant to LECTOR, not the harness core |

**⚠️ Pricing conflict logged:** sources disagree on first-party rates (Xiaomi's own post implies ¥0.025/¥3/¥6 ≈ OpenRouter's $0.435/$0.87; a May 27 permanent-cut notice says $1/$3/$0.20-cached flat). Catalog marks MiMo prices `verify_live = true`; M1 task: read platform pricing page at integration time. Known API edge cases: streamed tool arguments and parallel tool calls have OpenAI-compat quirks — test both in M1; TTFT is on the slow side (~1.3–2.8s), so not the typeahead route.

## B.4 GLM / Z.ai — 9 routable models (I under-catalogued this one; now complete)
| Model ID | Ctx | Modality | Price $/M (in / cached / out) | Role |
|---|---|---|---|---|
| `glm-5.2` | 1M / 128K out | Text | 1.40 / 0.26 / 4.40 | Flagship; thinking High/Max; SWE-bench Pro 62.1 (vendor-cited > GPT-5.5); MIT weights |
| `glm-5-turbo` | 262K | Text (no vision) | 1.20 / 0.24 / 4.00 | Speed-tuned GLM-5; outstanding agentic index (65.9), tau2 0.985 instruction adherence — orchestration route, not coding lead |
| `glm-4.7` | ~203K | Text | 0.60 / 0.11 / 2.20 | Value champion; frontend-code focus |
| **`glm-4.7-flash`** | — | Text | **FREE (in+cached+out)** | Your $0 CI/smoke-test route |
| `glm-4.7-flashx` | — | Text | 0.07 / — / 0.40 | Lowest-latency paid tier |
| **`glm-4.5-flash`** | — | Text | **FREE** | Second free text route (rate-limited) |
| **`glm-4.6v-flash`** | — | **Vision** | **FREE** | ⭐ Free VISION model — $0 image-understanding route for tests and light multimodal; I missed this entirely in Appendix A |
| `glm-4.5-air` | 131K | Text | 0.20 / 0.03 / 1.10 | Cheapest paid; high-volume extraction/summarization |
| `glm-4.5-x` / `glm-4.5-airx` | 131K | Text | 2.20/8.90 · 1.10/4.50 | Premium/fast legacy variants — skip unless latency-critical |

Extra nuances: built-in Web Search tool = **$0.01 per call on top of tokens** (agents that search every turn accrue per-call fees token math won't show — receipt must line-item it). Output is ~3× input across the lineup → cap max_output and request concise diffs. Coding Plan (Lite $18/Pro $72/Max $160, quota 3× peak / 2× off-peak / 1× off-peak promo through Sep 2026) remains **unusable by Nosis Harness** (supported-tools-only) — noted so you never buy it by mistake; API + free tiers are your lane. Free credits: bigmodel.cn 20M tokens on signup.

## B.5 Gemini / Google — via Antigravity delegate (subscription) + paid API
| Model | Access for you | Notes |
|---|---|---|
| `gemini-3.1-pro` | Antigravity CLI delegate (Pro/Ultra sub) or paid API (no free API tier) | Deep Think mode Ultra-gated; Search grounding = its unique value |
| `gemini-3.5-flash` | Antigravity delegate; API cheap tier | ~192 tok/s class — fast triage |
| `gemini-3-flash` | Antigravity delegate | legacy fast tier |
| `gemini-3.1-flash` (API) | $0.50 / $3.00 per M | only if you ever buy Google API credits |

Since March 25 free-tier users are Flash-only and quotas are unpublished/cut repeatedly → router marks all Gemini routes `best-effort`, never critical-path. Antigravity also exposes Claude Sonnet/Opus 4.6 and `gpt-oss-120b` inside itself — ignore (you have better native routes for both).

## B.6 Claude / Anthropic — 4 current models, delegate via Claude Code (subscription)
| Model (API string) | Ctx | Role in harness |
|---|---|---|
| Claude Fable 5 (`claude-fable-5`) | — | Newest flagship (Mythos-class tier). Available in Claude products; check availability in your Claude Code plan when wiring — use if selectable |
| **Claude Opus 4.8** (`claude-opus-4-8`) | 1M | **Your reviewer/gatekeeper** (AGENTS.md); $5/$25/M if ever on API; SWE-V ~88.6% independent — strongest verified coding record |
| Claude Sonnet 4.6 (`claude-sonnet-4-6`) | — | Mid-tier delegate — good for pre-review triage so Opus quota is spent only on final gates |
| Claude Haiku 4.5 (`claude-haiku-4-5`) | — | Cheap/fast delegate — receipt summarization, commit messages |

Quota nuance: plan usage shared across all Claude surfaces (chat + Code). Two-stage review pattern: Sonnet 4.6 pre-screens diffs → Opus 4.8 gates only what passes. Stretches your subscription 3–5×.

## B.7 OpenAI — GPT-5.6 family + quota stretchers, delegate via Codex CLI (subscription)
| Model | Ctx / Out | Price $/M (in / cached / out) | Role |
|---|---|---|---|
| `gpt-5.6-sol` (alias `gpt-5.6`) | 1.05M / 128K | 5 / 0.50 / 30 | Hardest implementation, security-adjacent code; `max` effort; `ultra` = parallel subagents (Plus+ in Codex); Cerebras-served variant ~750 tok/s |
| `gpt-5.6-terra` | 1.05M / 128K | 2.50 / 0.25 / 15 | **Default implementer** — GPT-5.5-class at half price |
| `gpt-5.6-luna` | 1.05M / 128K | 1 / 0.10 / 6 | Fast/cheap delegate tasks |
| `gpt-5.4-mini` | — | plan quota | OpenAI's own documented **quota stretcher** inside Codex — switch to it when approaching plan limits |
| `gpt-oss-120b` / `gpt-oss-20b` | — | open weights, Apache 2.0 | **gpt-oss-20b is a realistic local route on the Predator's RTX 5070 Ti** (cu128 rules apply); 120b is not. Candidate KorvinEngine sibling |

Cache economics: −90% cached input, but cache **writes bill 1.25×** and minimum cache life 30 min — only worth it for prompts reused within the window. Deprecated under ChatGPT sign-in: `gpt-5.2*`, `gpt-5.3-codex` (some remain API-only). Codex + ChatGPT Work share one usage pool; `/status` in CLI shows remaining. Chat Completions API deprecated in Codex — Responses API is the surface. Update the CLI binary or 5.6 won't appear.

## B.8 Corrections vs Appendix A (delta log #2, July 11)
1. **GLM catalog was 4× larger than I reported**: added glm-5-turbo, 4.7-flashx, 4.5-flash, **4.6v-flash (free vision!)**, 4.5-air/x/airx, plus the $0.01/call Web Search fee and the 3× output-to-input price shape.
2. **mimo-v2.5 standard is the sleeper of the whole catalog**: $0.105–0.14 in / $0.28 out with 1M context and full omni-modality — new default for cheap-bulk AND multimodal routing; Kimi K2.7 stays the coding-with-vision route, mimo-v2.5 takes everything-else-with-vision.
3. **MiMo TTS timbre remap** (your table): `mimo_default` no longer exists as-was — audit LECTOR/Reels pipeline configs if any call MiMo TTS.
4. **mimo-v2-flash→v2.5 changed parameter defaults** — Codex must read Xiaomi's migration doc and pin explicit params rather than trusting defaults.
5. MiMo first-party pricing sources conflict → `verify_live` flag added; catalog schema gains `price_confidence: confirmed|reported|verify_live`.
6. Claude catalog completed with all 4 current models + two-stage Sonnet→Opus review pattern to stretch subscription quota.
7. OpenAI catalog completed with gpt-5.4-mini quota stretcher + gpt-oss-20b as a future local route.
8. Kimi legacy K2 Thinking/Instruct catalogued as fallbacks only.
