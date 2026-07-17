# Nosis Harness — Deep Improvement Research (July 2026)

**Date:** 2026-07-17 · **Owner:** Carlos Paredes Vargas / NosisTech LLC
**Commissioned:** "deepest and richest" research on how to genuinely improve the harness — product **and** build process — judged against THE LAW (small, simple, secure, safe, lightweight, readable, auditable, modular, congruent, harmonic). Out-of-scope ideas admitted only for intuitiveness/UX, security, or definite huge value.

**Two research engines, 13 lenses:**
- **Fable 5 (high)** — web-current, July-2026, cited landscape + repo grounding (13 lenses A–M). Raw files: `04-research/_harness-research-2026-07/fable_*.md`.
- **GPT-5.6 Sol (xhigh)** — deep design/architecture pass over the actual crate code (lenses A–G, 60-item backlog). Raw file: `04-research/_harness-research-2026-07/_sol_2026-07.md`.
- Where **both models independently flagged the same thing**, it's tagged **[⊕ CONVERGED]** — the strongest confidence signal in this document.

**How to read the tags:** `[VERIFY-LIVE]` = a provider fact to confirm with a live/first-party check before building on it (Sol had no web; Fable's July-2026 web checks resolve many, noted inline). `[NEEDS KEY: x]` = requires an API key we don't hold (today we hold **only** Kimi, MiMo, DeepSeek). Value/Effort/LAW-fit/Scope are per-item.

> **The single most important finding, stated once:** both models, working independently, converged on the same product identity and the same top priority. Identity: **nosis is the agent harness with a meter — it routes every task to the cheapest *capable* model (by clock, cache, modality, thinking budget, and jurisdiction) and hands you the receipt.** Top priority: **make that meter true at every seam and visible in evidence before adding any provider or autonomy.** Everything below serves that.

---

## 1. Executive summary — the 15 highest-leverage moves

Ranked by (value × LAW-fit × cohesion), correctness bugs first. Full detail in §7 (master backlog). "S/M/L" = effort.

1. **Fix the thinking defaults — a live cost bug.** DeepSeek V4 and Kimi K2.6 default thinking **ON**; DeepSeek normalizes `reasoning_effort:"low"→"high"` and auto-escalates recognized harnesses to `max`. Our governor's None/Low tiers silently buy full high-effort thinking. `[⊕ CONVERGED]` **S · in · no key.**
2. **Fix `reasoning_content` passback — a live error path.** K2.6 with thinking+tools *errors* unless prior `reasoning_content` is replayed; DeepSeek's contract requires it too. Catalog `preserve_reasoning=false` + `nh run` default thinking-on = broken today on kimi-k2.6 with tools. **S · in · no key.**
3. **Guard `read_file` / add a `[read]` + `[send]` law class — closes the Lethal Trifecta.** `ReadFile` never consults the law guard, so injected tool output can read `.env`/`*.pem` into a prompt bound for a CN API. Add read-block + egress verdicts (mirrors the existing write verdicts). `[⊕ CONVERGED]` **M · in · no key.**
4. **Bind every vault secret to an approved audience.** Repo-controlled config pairs a vault entry with a URL, so a malicious checkout can redirect a real credential to an attacker origin. Broker `get_scoped(entry, audience)` validating host before the secret materializes. (Sol #1.) **M · in · no key.**
5. **Complete the "cheapest *capable*" resolver + move routing policy to data.** Today it mostly resolves explicit IDs by output sticker price; add a capability/security filter → expected-cost scorer → auditable rejection trace. Move the hard-coded escalation ladder route IDs out of Rust into TOML. `[⊕ CONVERGED]` **M–L · in · no key.**
6. **Ship the "meter made visible": money cost HUD + the counterfactual savings line.** Print actual cost next to naive cost (peak × cache-miss × top-tier over the same tokens) — the 60-second "aha" only nosis *can* print. Plus `/why` route-explain. `[⊕ CONVERGED]` **S–M · in · no key.**
7. **Ship the profiles feature (the owner's ask).** `profiles.toml` (frugal / balanced / max-quality) layered like `law.toml`, one `EffectiveExecutionPolicy` clamping profile wishes to route capabilities, a `/profile` toggle + HUD chip + receipt field. Owns every cost lever. `[⊕ CONVERGED]` **M · in · no key.**
8. **Bound every tool result (envelope) + cap output tokens.** OpenAI wire sends no `max_tokens`; `read_file`/`exec` return unbounded output. One `ToolResultEnvelope {excerpt, handle, digest}` + `max_out` on both wires closes a denial-of-wallet/prompt-injection surface and saves tokens. `[⊕ CONVERGED]` **M · in · no key.**
9. **Cache-aware compaction + never mutate the prefix.** Compaction drops/rewrites `history[1]`, forcing a full-prefix cache MISS next turn (¥0.025→¥3.00, ~120×). Insert the elision note as a *new* message (append-only), and only compact when recache cost < projected savings. `[⊕ CONVERGED]` **S–M · in · no key.**
10. **Privacy-aware routing (a differentiator nobody has).** All 3 keyed providers are Chinese and DeepSeek/Kimi train on API data by default; GLM (Singapore) doesn't. Make jurisdiction a 4th routing dimension: `governance` catalog metadata + a privacy profile filter + `[send]` egress + custody in receipts + one-question `nh init`. Converts "all our keys are Chinese" into a demo. **S–M each · in · GLM key strengthens it.**
11. **The learning router (the moat).** Receipts already carry deepset's failure taxonomy + usage; the fleet ledger already joins route/effort/attempt — but nothing reads it back and the ladder is hard-coded. Fold `.nosis/receipts.jsonl` into a Route Scorecard (cost-per-solved-task), then power an outcome-weighted ladder + failure-class-aware `next_step` + pre-run forecasts. **M · in · no key.**
12. **One resume story: crash-safe session ledger + `nh resume`.** Interactive `nh chat`/`tui` history lives only in RAM — a crash loses it (the exact "context loss" pain #6 promises to fix). Persist it the way the fleet already persists runs; DeepSeek's hours-to-days disk cache makes resume nearly free. `[⊕ CONVERGED]` **M · in · no key.**
13. **Approval UX: prefix rules (y / always-this-session / no), fix the any-key-denies bug, Esc-to-interrupt, taskbar semáforo.** Real approval-fatigue fixes without weaker gates; a genuine bug (any non-`y` key silently denies); OS-level `WORKING` visibility on Windows via OSC 9;4. **S–M · in · no key.**
14. **Onboard GLM (free key) — best key-to-value ratio.** Unlocks the already-catalogued $0 CI + free-vision lanes, a $0.07/$0.40 FlashX route, and — uniquely — a **third Anthropic-wire provider** (`api.z.ai/api/anthropic`) plus the Singapore privacy lane. **S–M · in · [NEEDS KEY: GLM/Z.ai] (free registration).**
15. **Harden the build loop + stand up minimal CI.** No CI exists; frozen-crate checks are manual numstat-eyeballing; the M4 finale lives only as loose working-tree state. Add a `wip/` commit rule, a `gate.ps1` frozen-crate/allowed-files sensor, one GitHub Actions workflow (keyless, 292 mocked tests), `codex exec --output-schema` handoffs, and cargo-nextest with an AV-preflight. `[⊕ CONVERGED]` **S each · in · no key.**

**Scope discipline (a CUT that sharpens identity):** demote "subscription delegates" from a marquee pillar to an escalation-gate footnote. Anthropic moved all programmatic Claude Code to API pricing (2026-06-15) and Gemini CLI died as an open delegate (2026-06-18) — the pillar broke, and open-weight parity (DeepSeek V4 ≈ 83.7% SWE-bench Verified) makes it unnecessary. Reposition: **"open-weight-first harness with a frontier review gate."**

---

## 2. Product cohesion & identity — the one thread (answers "cohesive product that gives the most value")

Read the 7 differentiators side by side and they collapse to one idea: **every unit of agent work is priced, routed, and receipted — and you can always see why.** Clock, cache, modality, and thinking are all *pricing* dimensions; the fleet ledger + receipts are the *accounting*; the HUD/semáforo/trust dial are the accounting made *visible and calm*; THE LAW is the *audit rules*; nh-mcp is the accounting *exported as a service*. Zero new machinery is needed to adopt this identity — every crate already serves the meter. That is what "harmonic" means here.

- **One-sentence identity:** *"nosis is the agent harness with a meter: it routes every task to the cheapest capable model — by clock, cache, modality, and thinking budget — and hands you the receipt."*
- **Best-in-world claim (falsifiable):** *the best tool for converting open-weight model economics (peak/off-peak, ~120× cache-hit discounts, thinking budgets) into a calm, auditable coding agent — natively on Windows.*
- **Why nosis alone can hold this spot (July 2026, cited):** Claude Code / Codex compete on benchmark ceiling but their cost story is quota opacity + rate-limit shock (Claude Code rate-limit-drain complaints, [macrumors](https://www.macrumors.com/2026/03/26/claude-code-users-rapid-rate-limit-drain-bug/); all programmatic use moved to API pricing 2026-06-15, [ccforeveryone](https://ccforeveryone.com/guides/claude-code-limits-and-pricing)). OpenCode is model-agnostic but cost-blind. OpenRouter/proxies aggregate access but "don't reduce bills" and **can't see cache warmth, clock windows, or budget because they aren't the harness** ([clawrouters](https://www.clawrouters.com/blog/why-openrouter-wont-cut-your-ai-bill)). nosis is the only harness whose *router lives inside the harness*.

**The 60-second first-run "aha" — the counterfactual savings line.** After the first task completes, show what it cost *next to what it would have cost naively*:
```
✔ fixed tests/test_parse.rs   route: deepseek-v4-flash (off-peak · cache 82% hit · non-think)
  cost ¥0.11  —  saved 93% vs naive (peak ¥0.44 · cache-miss ¥1.62 · pro-tier ¥3.90)
```
Both numbers come from the same `catalog.toml` price data and the same JSONL token counts — honest by construction, no incumbent *can* print it (their router can't see their cache state). It makes four differentiators visible in one line, compounds (per-turn HUD → session summary → `nh stats` weekly total = the retention loop and the launch screenshot), and is a pure function over data already recorded.

**Three flagship workflows, one meter:**
- **W1 · The Daily Driver** (interactive TUI) — the calm, metered alternative to Claude Code for open-model work. Polish = the savings line + `/why`.
- **W2 · The Overnight Fleet** (hero; no competitor has it) — `nh fleet run tasks.json --defer off-peak --budget ¥20` at 5pm → parks until off-peak → budget hard-stop → kill-safe resume → morning summary "N done, total saved vs peak." Every part exists (Slices A+B); the gap is one-gesture-in / one-summary-out polish. The 30-second asciinema of submit → kill → resume → morning receipts is the single most differentiating demo a 2026 agent CLI could show.
- **W3 · The Agent Node** (headless `nh exec` + nh-mcp) — "the agent other agents call when the work should be cheap." Timely: subscription headless automation is dying exactly as `nh exec` (M5) + nh-mcp ship.

**The v1 cut list (pre-commit against scope creep — the plan's named #1 killer):** M6 multimodal *generation* stays post-launch; web dashboard stays v2; Kimi Swarm stays the minimal seam it is; proactive loop stays v2; **full delegate adapter class cut** (keep commented catalog schema); MCP Apps HTML stays display-only-untrusted; the local/Ollama route ships as a catalog capability, not a marketed pillar.

---

## 3. Live correctness / cost issues in the current code (fix first)

These are not "improvements" — they are gaps the research found in shipped code, most cheap to fix. Ordered by severity.

| # | Issue | Where | Fix | Sev |
|---|---|---|---|---|
| L1 | Governor None/Low silently buys full high thinking; DeepSeek normalizes `low`→`high`, auto-escalates harnesses to `max` | `nh-core apply_thinking` ~301-326; `catalog.toml:201` | send `thinking:{type:disabled}` for None/Low on deepseek-nhm; new `kimi-toggle` dialect for K2.6; always send explicit effort | **cost bug** |
| L2 | `reasoning_content` passback now required in thinking+tools — K2.6 **errors**, DeepSeek contract violated | `catalog.toml:41,202`; `nh-core reasoning_to_send ~276-298` | make replay conditional on effective thinking state (`preserve_when_thinking`) | **error path** |
| L3 | `read_file` never consults the law guard → secrets readable into a CN-bound prompt (Trifecta leg 1) | `nh-tools/src/lib.rs:172-180` | `Access::Read` + `[read] block` + `[send]` egress verdict `[⊕]` | **security** |
| L4 | Vault entry + URL both come from repo-controlled config → credential redirect/exfil | route + `.nosis/mcp.toml` | audience-bound `get_scoped(entry, audience)` broker (Sol #1) | **security** |
| L5 | nh-mcp `authorized()` returns true when no token set; no Origin/Host check → any local process can POST `fleet_run` (spends money) | `nh-mcp/src/lib.rs:189` | default-mint a token; validate Host/Origin (DNS-rebind) `[⊕]` | **security** |
| L6 | Any non-`y` key while `Waiting` silently **denies** the tool call | `nh-tui reduce_key ~1355` | explicit `y`/`n`/`Esc` only, legend in the approval row | **UX bug** |
| L7 | Compaction mutates `history[1]` and drops the prefix → next turn is a full cache MISS (~120×) | `nh-core compact_history ~1332-1368` | insert elision note as a *new* message (append-only); cache-aware trigger `[⊕]` | **cost bug** |
| L8 | `estimate_tokens` ignores `reasoning_content` + tool-spec bytes → late compaction / provider overflow on preserve-reasoning marathon routes | `nh-core ~1316-1328` | count reasoning (when route preserves) + serialized tool specs | correctness |
| L9 | Anthropic-wire `max_tokens` hard-capped at 8192 vs DeepSeek 384K; ignores `output_config` effort | `nh-core ~138-143,408-525` | budget-aware cap; map effort→`output_config` | quality/cost |
| L10 | Interactive session history is RAM-only → crash loses it (pain #6) | `nh-cli/cmd_chat.rs:30-40` | crash-safe session ledger + `nh resume` `[⊕]` | data-loss |
| L11 | MCP tool descriptions/schemas unpinned → rug-pull (CVE-2025-54136); ANSI/invisible chars reach the model unfiltered | `nh-tools/src/mcp.rs:577-643` | TOFU hash pin + `sanitize_untrusted_text()` `[⊕]` | security |
| L12 | Prefix byte-stability guarded by `debug_assert` only → release builds silently tolerate cache-breaking drift | `nh-core ~1141-1152` | `PrefixSeal` verified in all builds + cache-break detector | cost/reliability |

---

## 4. Per-provider findings (July 2026, web-verified)

**Catalog prices re-confirmed accurate** against first-party pages 2026-07-16/17 (DeepSeek, Kimi, MiMo). Key facts and what we under-use:

### Providers we hold keys for
- **DeepSeek V4** (pro/flash, OpenAI + Anthropic wires) — thinking is *default-on*, only `high`/`max` effort exist (L1). `reasoning_content` mandatory in thinking+tools (L2). Caching is automatic, prefix-only, **64-token units from token 0**, evicts in hours-to-days; native usage fields `prompt_cache_hit_tokens`/`prompt_cache_miss_tokens` (parse as fallback — ecosystem tools get this wrong). No batch API. DSpark speculative decoding is server-side (+57–85%, no client work). **Peak/off-peak 2× windows STILL not on the first-party page** as of 2026-07-16 → the catalog's `peak` blocks over-state `price_confidence=confirmed`; re-verify at `valid_until=2026-07-24`. Sources: [thinking](https://api-docs.deepseek.com/guides/thinking_mode/), [kv_cache](https://api-docs.deepseek.com/guides/kv_cache/), [pricing](https://api-docs.deepseek.com/quick_start/pricing/).
- **Kimi / Moonshot** — K2.7-code $0.19/$0.95/$4.00 (256K, always-thinking, max_out 262,144); K2.6 default thinking-on, default `max_tokens` only 32K. **Kimi K3 launched 2026-07-16** (2.8T MoE, 1M ctx, $3.00/$0.30/$15.00) — add as catalog data, keep out of frugal `[VERIFY-LIVE]`. **Batch API bills 0.6× (40% off)** for K2.6/K2.5 (not K2.7/K3) — the only batch lane we hold. Built-in `$web_search` $0.005/call but only on non-thinking K2.6. Strict `json_schema` structured output. **Agent Swarm is a hosted product, not a raw API param** → re-scope the M4 swarm item to "verify availability first." Sources: [K2.6 quickstart](https://platform.kimi.ai/docs/guide/kimi-k2-6-quickstart), [batch](https://platform.kimi.ai/docs/guide/use-batch-api), [K3](https://www.marktechpost.com/2026/07/16/moonshot-ai-releases-kimi-k3-a-2-8-trillion-parameter-open-moe-model-with-kimi-delta-attention-and-1m-context/).
- **MiMo / Xiaomi** — cheapest 1M-ctx multimodal; **cache-hit ~50× cheaper than miss (deepest discount of any held provider)**. **Documented Beijing 00:00–08:00 = 10:00–18:00 La Ceiba 0.8× off-peak window is MISSING from the catalog** (owner's whole workday) `[VERIFY-LIVE: pay-as-you-go applicability]`. New public routes: **UltraSpeed** ($0.0108/$1.305/$2.61 fast lane) and **ASR** ($0.074/audio-hr — cheapest transcription for the modality router). Natively omni (image/video/audio). Sources: [token-plan](https://mimo.mi.com/docs/en-US/price/token-plan), [pay-as-you-go](https://mimo.mi.com/docs/price/pay-as-you-go).

### Providers we'd add (flag [NEEDS KEY])
- **GLM / Z.ai — the highest-value key we don't hold.** GLM-5.2 (MIT, 2026-06-16, 1M ctx) ≈ near-Anthropic at ~1/4–1/5 cost ($1.40/$4.40 vs Opus $5/$25). Free registration = 20M tokens. Already-catalogued free flash/vision routes become the $0 CI/vision lanes the moment a key exists. **GLM-4.7-FlashX $0.07/$0.40** (cheapest paid text route we'd own). Uniquely ships a **first-party Anthropic-Messages endpoint** (`api.z.ai/api/anthropic`) → a *third* Anthropic-wire provider, catalog-only. **Trap:** GLM Coding Plan is tool-restricted; nosis wouldn't qualify → pay-per-token/free only. **[NEEDS KEY: GLM/Z.ai] — do first.** ([docs.z.ai pricing](https://docs.z.ai/guides/overview/pricing))
- **Claude / Anthropic** — Opus 4.8 $5/$25 is our review/gate role. **No new key needed** for the delegate path (`claude -p` or OAuth in the child CLI); direct API is a catalog-only TOML *if credits are ever bought* `[NEEDS KEY: Anthropic for direct]`.
- **Codex / OpenAI** — cleanest delegate (`codex exec --json --output-schema`); **no key needed** (OAuth in codex CLI). Direct API is served via the **Responses API** → would be a *third wire* → **reject** for v1 (2-wire rule).
- **Gemini / Google** — ships a first-party **OpenAI-compatible endpoint** (`generativelanguage.../openai/`) → a paid route is catalog-only `[NEEDS KEY: Google]`. Antigravity headless CLI is unreliable (no `--model`, drops stdout, times out) → **defer** the delegate. Would expose two honest schema gaps worth adding generically: **context-tiered pricing** (Gemini 3.1 Pro doubles >200K) and a **cache-write** multiplier.

**Key-acquisition order (proof-of-value gate):** (1) **GLM** first — free, fits the wire, unlocks $0 + vision + SG-privacy + Anthropic-wire; (2) no OpenAI/Anthropic/Google key until a measured workload proves the delegate insufficient. Record each key decision as a one-page receipt (budget, expiry, one acceptance workload).

---

## 5. Theme summaries (condensed; full designs in the raw lens files)

**Token economy & profiles (the owner's ask).** Profiles as data (`profiles.toml` layered like law, repo may only tighten spend), one `EffectiveExecutionPolicy` that clamps profile wishes to route capabilities, a `/profile` toggle + HUD chip + receipt field. It becomes the single owner of every user-selectable cost lever (Sol's 26-row register): route/clock/cache/output-cap/thinking/turns/compaction/tool-schema/tool-output/off-peak/constitution-trim — while route-required behavior (e.g. Kimi reasoning replay) and law stay immutable. **Never** ask a model to summarize its own authority; `frugal` uses deterministic dedupe, not semantic compression (token-level pruning corrupts code — use extractive/task-conditioned, per Squeez arXiv:2604.04979). MCP tool-schema bloat is the 2026 "context tax" (7 servers ≈ 67K tokens; Anthropic Tool Search −85%) — add per-server allowlists + lazy schema.

**Context engine.** Replace one-stage deletion with the real five-stage ladder (budget-reduce → snip → microcompact → context-collapse → auto-compact), **never delete user messages** (fold verbatim, Codex's proven rule) and **never mutate the prefix** (append-only, Manus). Clamp `effective_context` per route (context rot: models degrade at ~50K on 200K windows — arming compaction at 700K on 1M routes is self-defeating, [Chroma](https://www.trychroma.com/research/context-rot)). Compaction receipts (what/why/lossy). File-based memory v1 (retain/recall/reflect as markdown, not SQLite/embeddings yet — Hindsight interface, flat-file store). In-session `subtask` isolation returning 1–2K-token summaries. Same-route semantic collapse (no new data recipient).

**Route intelligence (the moat).** Route Scorecard = one fold of `.nosis/receipts.jsonl` → per-(route, effort, task-class) cost-per-solved-task; five surfaces read that one file (learned ladder, `/why`, forecasts, `route_stats` MCP tool, `nh bench`). Failure-class-aware `next_step` (escalate on Planning, repair-context on Context) finally *uses* the taxonomy field that is write-only today. Keep it a smoothed win-rate table, **not** a neural router (auditable). Reject: OTel stack (a serde-rename to `gen_ai.*` keys is free future-proofing).

**Reliability & graceful degradation.** Today: no retry/backoff/failover anywhere; a dead provider ends the turn with a string. Add typed `RouteError` + jittered backoff honoring `Retry-After`; **availability re-resolve** = "cheapest capable" applied to failure (reuse the resolver + `switch_to`); a data-only provider cooldown (circuit breaker, no daemon); a **$0 local floor route** (Ollama `gpt-oss:20b` on the RTX 5070 Ti, ~98 tok/s — pure catalog data, the reliability floor + privacy floor); honest degraded-mode UX (every reroute announced, `/health`). Fix the latent bug where a provider *outage* climbs the *quality* ladder to pricier tiers.

**Privacy-aware routing (new differentiator).** `governance` catalog metadata (residency / trains-on-API-data / retention, with the same confidence+staleness discipline as price = "honest-custody"); a privacy profile that filters the route set before cost optimization; a `[send]` egress verdict class in nh-law (block `.env`/secrets from *leaving*, not just from being written); the existing vault Scrubber pointed at the egress path (deterministic → cache-safe); custody in receipts + a jurisdiction glyph in the HUD + `nh privacy`; one-question `nh init`. Fuses routing + security + profiles into a story no incumbent has: *"only if you say so, per repo, verifiably."* Regulatory tailwind: DeepSeek gov-device bans (2025-26), EU AI Act high-risk enforcement 2026-08.

**Security & auditability.** MCP tool pinning (TOFU hash, kills rug-pulls); hash-chained ledger/receipts + `nh verify`; widen Scrubber shapes (ghp_/AKIA/AIza/xox…) + a shared registry seeded from all vault entries; sanitize ANSI/invisible Unicode before the model sees tool text; min-env exec (allowlist, not denylist); a Windows-native sandbox tier (restricted token + Job Object — a genuine differentiator since Anthropic's sandbox-runtime is Linux/macOS only); OAuth `resource` param (RFC 8707, now mandatory); supply-chain gate (`deny.toml` + cargo-audit/deny — crates.io is under active attack, RUSTSEC-2026-0155). Full-fidelity approvals (**never approve a truncated action** — display sanitization ≠ approval fidelity).

**UX / intuitiveness.** Session prefix-rule approvals (y / always-this-session / no — Codex Smart Approvals); fix L6; Esc-to-interrupt + a live working heartbeat (`WORKING · 34s · Esc to stop`); money cost HUD with honest-stale flag; **OSC 9;4 Windows taskbar semáforo** (yellow icon = "waiting on you" from across the room — Windows-first made visceral, zero deps); `/model` picker showing price·modality·peak; "errors that teach" as a tested invariant (the Slice-D OAuth line is the template); context-aware welcome + `nh doctor` + legacy-console hint; `/copy` via OSC 52 (scrubbed); input cursor + history recall.

**Ecosystem & interop.** `nh acp` (one stdio adapter buys Zed + JetBrains + Neovim + **Microsoft's Intelligent Terminal auto-detects ACP CLIs** — Windows wedge); fleet runs as **MCP Tasks** (the RC formalizes exactly `fleet_run`/`fleet_status`); `nh gateway` (loopback OpenAI/Anthropic endpoint so Claude Code et al. route through nosis's cost brain — the strongest "other tools plug into us" move, works with held keys); read **SKILL.md** open standard (a user's existing skills drop in unchanged — and progressive disclosure *is* cache-native); headless `nh exec --output json` + a tiny GitHub Action with the $0 GLM CI hook; finish the nh-mcp surface (`cost_estimate`/`receipts_query` = nosis as the fleet's cost oracle); `ntfy` zero-setup notify; cargo-dist → winget/scoop for `winget install NosisTech.nosis`.

**Agentic surface & extensibility.** Extend the "new capability = new data file, not new code" bet: SKILL.md skills + `/name` commands; agents-as-data that declare a **price ceiling, not a model** (routing stays the differentiator inside user extensions); `.nosis/tools.toml` declared command-tools (argv, never shell-interpolated); one provenance model — **`.nosis/extensions.lock`** (hash-pin skills/tools/MCP descriptions; first-sight approve, any byte change re-approve — Cargo.lock mental model); plus the missing built-ins: `write_file`, ripgrep-engine `grep_files`/`glob_files` (read-only = no approval, Windows-clean), a `plan` tool that feeds the thinking-governor + compaction + fleet, `/fleet` surfaced in the TUI, and a guarded `web_fetch` behind a new `Access::Net` law class (SSRF-aware).

**Architecture & build process.** Complete the resolver (§1.5); route policy as data; a narrow `AgentSessionBuilder` (remove duplicated key/prompt/scrubber setup across run/chat/tui/fleet); internal module splits (nh-tui is 4,107 lines — every "one-file" Sol brief ships a 4K-line context); a differentiator **evidence matrix** (mock/live/security/cost/UX/Windows per differentiator — prevents "ghost differentiators"). **Process:** commit gated-but-unapproved work to `wip/<slice>` (the M4 finale is one AV-quarantine from loss); a `gate.ps1` that mechanizes the frozen-crate/allowed-files check (`cargo public-api` proves additive-only); minimal keyless CI on windows-latest; `codex exec --output-schema` structured Sol handoffs; cargo-nextest with retries + an AV canary preflight (turns "Kaspersky blocked the exe" into `FLAKY`/`EnvironmentBlocked`, not `FAIL`); `[workspace.lints]` + pinned `rust-toolchain.toml`; **MCP 2026-07-28 conformance** (RC mandates `Mcp-Method`/`Mcp-Name`/`MCP-Protocol-Version` headers + reverse-DNS `clientInfo` — we implement neither; deadline 11 days out); fill the empty `EVALUATION_PLAN.md`/`FAILURE_MODES.md` with the real (currently scattered) discipline.

---

## 6. New API keys — value vs. cost

| Provider | Get a key? | Unlocks | Cost/risk | Verdict |
|---|---|---|---|---|
| **GLM / Z.ai** | **Yes, first** | $0 CI + free vision + FlashX $0.07/$0.40 + **3rd Anthropic wire** + SG privacy lane; 20M free signup tokens | free reg; Coding Plan won't cover nosis (pay-per-token only) | **Do it.** Best ratio. |
| Anthropic (direct) | Not yet | direct Opus/Sonnet/Haiku catalog routes; API prompt caching | $5/$25 Opus; only if unattended gating proves needed | Delegate first (no key). |
| OpenAI (direct) | No | — | Responses API = 3rd wire (LAW violation) | Delegate only (no key). |
| Google (direct) | Not yet | Gemini via OpenAI-compat; grounded search | $2/$12; needs 2 schema fields; Antigravity headless unreliable | Defer until a workload proves it. |

Everything else in this document needs **no new key**.

---

## 7. Master ranked backlog

Deduped across both models. **Conf** = confidence (⊕ = both models converged; ● = single-model, strong). **Sc** = scope (in / out-justified / cut). **Key** = new API key needed. Grouped by tier; within a tier, roughly by value/effort.

### Tier 0 — Correctness & security (do first; mostly cheap)
| Initiative | Val | Eff | LAW | Sc | Key | Conf |
|---|:--:|:--:|---|:--:|:--:|:--:|
| Thinking-default fix (L1) | High | S | congruent/honest | in | — | ⊕ |
| `reasoning_content` conditional replay (L2) | High | S | safe | in | — | ● |
| `read_file` guard + `[read]`/`[send]` law class (L3) | High | M | secure/congruent | in | — | ⊕ |
| Credential audience binding (L4) | High | M | secure | in | — | ● |
| nh-mcp inbound token-default + Host/Origin (L5) | High | S | secure | in | — | ⊕ |
| Fix any-key-denies approval bug (L6) | High | S | safe | in | — | ● |
| Cache-aware, append-only compaction (L7) | High | S–M | honest/auditable | in | — | ⊕ |
| `estimate_tokens` counts reasoning + tool specs (L8) | High | S | safe | in | — | ● |
| Anthropic-wire output cap + effort (L9) | High | S | congruent | in | — | ● |
| MCP tool TOFU pinning + ANSI/invisible sanitize (L11) | High | S | secure | in | — | ⊕ |
| `PrefixSeal` release-build invariant + cache-break detector (L12) | High | S | safe | in | — | ● |
| Full-fidelity approvals (never approve truncation) | High | S–M | safe | in | — | ● |
| Min-env exec allowlist | Med | S | secure | in | — | ⊕ |
| Widen Scrubber shapes + shared registry from all vault entries | Med | S | secure | in | — | ⊕ |
| Supply-chain gate (`deny.toml` + cargo-audit/deny) | High | S | secure | out-just | — | ⊕ |
| OAuth `resource` param (RFC 8707) | Med | S | secure | in | — | ⊕ |

### Tier 1 — The meter made visible (identity / cost / UX)
| Initiative | Val | Eff | LAW | Sc | Key | Conf |
|---|:--:|:--:|---|:--:|:--:|:--:|
| Money cost HUD + session cost + budget hard-stop | High | M | honest/auditable | in | — | ⊕ |
| Counterfactual savings line (the 60s aha) | High | S | honest | in | — | ● |
| `/why` route-explain (CLI + TUI chip + receipt) | High | S | auditable | in | — | ⊕ |
| Profiles feature (`profiles.toml` + `/profile`) | High | M | data-driven | in | — | ⊕ |
| Tool-result envelope + output-token caps | High | M | lightweight/safe | in | — | ⊕ |
| Prefix-rule approvals (y / always-session / no) | High | M | safe | in | — | ● |
| Esc-to-interrupt + working heartbeat | High | M | congruent | in | — | ● |
| OSC 9;4 Windows taskbar semáforo | High | S | lightweight | in | — | ● |
| `/model` picker with price·modality·peak rows | Med | S–M | readable | in | — | ● |
| "Errors that teach" helper + tested invariant | Med | S | readable | in | — | ● |
| `nh doctor` + context-aware welcome + legacy-console hint | Med | S–M | self-teaching | in | — | ⊕ |
| `/copy` (OSC 52, scrubbed) + input cursor/history | Med | S–M | lightweight | in | — | ● |
| Private notifications (fixed minimal bodies) | High | S | secure | out-just | — | ⊕ |

### Tier 2 — The learning router (moat)
| Initiative | Val | Eff | LAW | Sc | Key | Conf |
|---|:--:|:--:|---|:--:|:--:|:--:|
| Route Scorecard (fold receipts → cost-per-solved-task) | High | M | auditable | in | — | ● |
| Task-class tag on receipts (6-value enum) | High | S | small | in | — | ● |
| Outcome-weighted escalation ladder | High | M | congruent | in | — | ● |
| Failure-class-aware `next_step` | High | M | congruent | in | — | ⊕ |
| Pre-run cost forecast (ranges + off-peak counterfactual) | High | M | honest | in | — | ● |
| `nh bench` (12-task local mini-bench, cold-start killer) | Med-H | M | auditable | in | — | ● |
| Cost-per-success evaluation corpus | High | M | auditable | in | existing | ⊕ |

### Tier 3 — Context engine
| Initiative | Val | Eff | LAW | Sc | Key | Conf |
|---|:--:|:--:|---|:--:|:--:|:--:|
| Five-stage compaction (never delete user msgs) | High | M–L | modular | in | — | ⊕ |
| Tool-result hot/cold split + insert-time snip | High | S–M | lightweight | in | — | ⊕ |
| `effective_context` clamp (context-rot guard) | High | S | safe | in | — | ● |
| Whole-request context budget (+reasoning/tools) | High | M | safe | in | — | ⊕ |
| Compaction receipts / timeline | High | S–M | auditable | in | — | ⊕ |
| Crash-safe session ledger + `nh resume` | High | M | safe | in | — | ⊕ |
| File-based memory v1 (retain/recall/reflect) | Med-H | M | lightweight | in | — | ⊕ |
| `subtask` in-session context isolation | Med | M | modular | in | — | ⊕ |
| Parse DeepSeek native cache fields (fallback) | Med | S | auditable | in | — | ⊕ |

### Tier 4 — Reliability
| Initiative | Val | Eff | LAW | Sc | Key | Conf |
|---|:--:|:--:|---|:--:|:--:|:--:|
| Typed `RouteError` + jittered backoff (Retry-After) | High | S | small | in | — | ● |
| Availability re-resolve (cheapest-capable on failure) | High | M | congruent | in | — | ● |
| Provider cooldown (circuit breaker as data) | High | S | small | in | — | ● |
| `$0` local floor route (Ollama gpt-oss:20b) | High | S | catalog-data | in | — | ⊕ |
| Honest degraded-mode UX + `/health` | Med-H | S | honest | in | — | ● |
| Pre-flight capability check (context/modality) | Med | S | congruent | in | — | ● |

### Tier 5 — Privacy-aware routing (new differentiator)
| Initiative | Val | Eff | LAW | Sc | Key | Conf |
|---|:--:|:--:|---|:--:|:--:|:--:|
| `governance` catalog metadata (honest-custody) | High | S | congruent | in | — | ● |
| Privacy profile as RouteResolver filter | High | M | congruent | in | — | ● |
| `[send]` egress verdict class in nh-law | High | M | secure | in | — | ⊕ |
| Outbound scrubber (deterministic, cache-safe) | High | M | secure | in | — | ● |
| Custody receipts + jurisdiction glyph + `nh privacy` | Med-H | S | auditable | in | — | ● |
| One-question privacy `nh init` (+ positioning) | Med-H | S | simple | in | — | ● |
| Secret-touching per-turn strictest-profile pin | Med | S | harmonic | in | — | ● |

### Tier 6 — Security & auditability (beyond Tier 0)
| Initiative | Val | Eff | LAW | Sc | Key | Conf |
|---|:--:|:--:|---|:--:|:--:|:--:|
| Hash-chained ledger + receipts + `nh verify` | High | M | auditable | in | — | ⊕ |
| Windows-native sandbox tier (restricted token + Job Object) | High | L | safe | in (M5) | — | ⊕ |
| `nh key doctor` (honest same-user-malware note) | Low-Med | S | honest | out-just | — | ● |
| Sensitive-receipt retention policy | High | M | secure | out-just | — | ● |

### Tier 7 — Providers & wires (data-first)
| Initiative | Val | Eff | LAW | Sc | Key | Conf |
|---|:--:|:--:|---|:--:|:--:|:--:|
| Onboard GLM: free/vision/FlashX + `glm-5.2-anthropic` | High | S–M | congruent | in | GLM | ⊕ |
| MiMo off-peak window in catalog (owner's workday) | High | S | congruent | in | — | ● |
| Kimi Batch API 0.6× (single-turn fleet jobs) | Med-H | M | modular | in | — | ● |
| Multimodal content parts + `read_image` (differentiator #2) | High | M | congruent | in (M5) | existing | ⊕ |
| Structured output (`json_schema`) for internal turns | Med | S | modular | in | existing | ● |
| Kimi `$web_search` on K2.6 (data-only) | Med | M | secure | in | existing | ● |
| Catalog freshness: K3, MiMo UltraSpeed/ASR, Kimi max_out | Med | S | honest | in | — | ● |
| One safe delegate seam (Claude/Codex/Gemini), not 3 wrappers | High | M | modular | in | delegate=none | ⊕ |
| Demote delegates from pillar → escalation footnote (CUT) | High | S | honest | in | — | ● |

### Tier 8 — Ecosystem & extensibility
| Initiative | Val | Eff | LAW | Sc | Key | Conf |
|---|:--:|:--:|---|:--:|:--:|:--:|
| `nh exec --output json` + GitHub Action ($0 GLM CI hook) | High | M+S | congruent | in (M5) | GLM opt | ● |
| Fleet runs as MCP Tasks (2026-07-28) | High | S | congruent | in | — | ● |
| `route_estimate`/`cost_estimate` + `receipts_query` MCP tools | Med-H | S | modular | in | — | ⊕ |
| `nh gateway` (loopback OpenAI/Anthropic → cost brain) | High | M | congruent | in | — | ● |
| Read SKILL.md open standard (skills + `/name`) | High | S | modular | in | — | ⊕ |
| `extensions.lock` provenance (hash-pin all extensions) | High | S | secure | in | — | ⊕ |
| `write_file` + ripgrep `grep_files`/`glob_files` built-ins | High | S–M | congruent | in | — | ● |
| `plan` tool (feeds governor + compaction + fleet) | Med-H | M | harmonic | in | — | ● |
| `/fleet` surfaced in the TUI | Med-H | S | congruent | in | — | ● |
| Agents-as-data (declare price ceiling, not model) | Med | M | modular | in | — | ● |
| `.nosis/tools.toml` declared command-tools | Med | M | modular | in | — | ● |
| `web_fetch` + `Access::Net` law class (SSRF-aware) | Med | M | secure | in | — | ● |
| `nh acp` adapter (Zed/JetBrains/Intelligent Terminal) | High | M | modular | in (M5+) | — | ● |
| `ntfy` zero-key notify + fleet lifecycle events | Med-H | S | small | in | — | ● |
| cargo-dist → winget/scoop distribution | High | S | — | in (M5) | — | ● |
| MCP server-card (SEP-1649) + Registry publish | Med | S | honest | in (M5) | — | ● |

### Tier 9 — Build process & architecture
| Initiative | Val | Eff | LAW | Sc | Key | Conf |
|---|:--:|:--:|---|:--:|:--:|:--:|
| Complete task-aware cheapest-capable resolver | High | M–L | harmonic | in | — | ● |
| Routing policy + escalation ladder as data | High | S–M | congruent | in | — | ⊕ |
| `wip/<slice>` commit rule (durability) | High | S | safe | in | — | ● |
| `gate.ps1` frozen-crate/allowed-files sensor | High | S | auditable | in | — | ● |
| Minimal keyless CI (windows-latest + ubuntu) | High | S | auditable | in | — | ● |
| `codex exec --output-schema` structured Sol handoff | High | S | auditable | in | — | ● |
| cargo-nextest + AV canary preflight | High | S | safe | in | — | ● |
| `[workspace.lints]` + pinned `rust-toolchain.toml` | Med-H | S | lightweight | in | — | ● |
| MCP 2026-07-28 conformance headers | High | M | congruent | in | — | ● |
| `AgentSessionBuilder` (de-dup session setup) | High | M | modular | in | — | ● |
| Internal module splits (nh-tui/core/fleet) | Med-H | M | readable | in | — | ⊕ |
| Differentiator evidence matrix | High | S | auditable | in | — | ● |
| Fill EVALUATION_PLAN / FAILURE_MODES | Med | S | auditable | in | — | ⊕ |

---

## 8. Verify-live ledger (confirm before building on)

Consolidated from both models. Most are one small live probe under a hard token cap (the future `nh verify-live <provider> --budget` command).

1. **DeepSeek peak/off-peak windows** — still not on the first-party page (2026-07-16). Re-check at `valid_until=2026-07-24`; mark the catalog peak block `verify_live`, not `confirmed`.
2. **DeepSeek thinking** — confirm omission-of-`reasoning_effort` = thinking-on (default); confirm `low`→`high` normalization; confirm Anthropic-wire `output_config.effort` + thinking-block replay.
3. **Kimi** — K2.6 thinking-toggle + mandatory `reasoning_content` in tools; K3 dialect/preserve_reasoning; Batch API model support; `$web_search` non-thinking constraint; K2.7 max_out=262,144.
4. **MiMo** — off-peak 0.8× applies to pay-as-you-go (not just Token Plan); reasoning field/mode; UltraSpeed/ASR routes; bounded 25/100-step tool endurance (not the "1,000+" marketing number).
5. **Cache accounting** — per provider: automatic vs explicit, exact prefix/units, min tokens, TTL, which usage fields report hits; assert `cached_tokens>0` on a repeated-prefix second call.
6. **GLM** `[NEEDS KEY]` — free-route limits/region; High/Max thinking mapping; `api.z.ai/api/anthropic` parity; built-in search fee.
7. **MCP 2026-07-28 final** — reconcile assumed wire vs first-party spec; run the official conformance suite once against `nh mcp serve`; confirm OAuth resource-indicator/PKCE requirements.
8. **Structured-output support** with tools + reasoning enabled, per keyed route.
9. **Product metrics** — measured cost-per-successful-task per provider × profile; current prompt breakdown (how much is tool schemas vs law vs history vs reasoning); which differentiators are live- vs mock- vs claim-only.

---

## 9. What NOT to build (LAW rejections — the discipline)

- **A neural/learned router or OTel observability stack** — opaque/heavy; a smoothed win-rate table + receipts.jsonl is the whole "learning" and "observability" story.
- **A third wire** (Google-native or OpenAI Responses) — violates the 2-wire rule; those providers are delegates or catalog-only OpenAI-compat, or rejected.
- **Native provider batch engine now** — only Kimi has one; the off-peak scheduler + cache + free-GLM lane is the batch story; add native only on measured material savings.
- **LLMLingua-style token-level prompt compression** — corrupts code/JSON; use extractive/task-conditioned.
- **Vector/embedding memory or SQLite now** — flat-file retain/recall/reflect first; keep the interface, defer the store.
- **JS/WASM plugin runtime, agent-teams messaging, pre/post shell hooks** — TOML/markdown/MCP data extensions + the ledger cover the need without a code-execution channel.
- **A gateway *dependency* (routing through LiteLLM/OpenRouter)** — nosis *is* the router; steal their published policies as data, not their runtime.
- **Cross-provider mid-turn tool-call replay** — a correctness minefield (reasoning-replay quirks); reroute at turn boundaries only in MVP.
- **Full delegate adapter class in v1** — the economics broke (2026-06 Anthropic/Google changes); keep the commented catalog schema.

---

## 10. Recommended sequencing (post-M4)

Both models independently proposed the same order: **fix security & measurement before increasing autonomy or provider surface.**

1. **M4 close** — commit Slice D; then the Tier-0 hygiene that's hours of work: supply-chain gate, OAuth `resource` param, `wip/` rule, `gate.ps1`, minimal CI, `codex --output-schema`, nextest+AV-preflight.
2. **Security floor slice** — credential audience binding, read/`[send]` guard, full-fidelity approvals, nh-mcp inbound auth, min-env exec, scrubber/registry, MCP TOFU pinning, ANSI sanitize.
3. **Measurement slice** — money cost HUD + counterfactual savings line + `/why`; output caps + tool-result envelope; native cache-field parsing.
4. **Profiles MVP** — the owner's toggle-per-provider ask.
5. **Context slice** — five-stage compaction (stages 1–3) + cache-aware/append-only + `effective_context` clamp + session ledger/`nh resume` + compaction receipts.
6. **Meter-made-visible + learning router** — Route Scorecard → learned ladder + failure-aware next_step + forecasts + `nh bench`.
7. **Privacy-aware routing** — custody v1 (governance metadata + profile filter + `[send]` + receipts + `nh init`).
8. **GLM key** + catalog freshness (K3, MiMo off-peak/UltraSpeed/ASR) + multimodal content parts.
9. **Reliability** (typed errors, re-resolve, cooldown, local floor) + **ecosystem** (`nh exec` + Action, MCP Tasks, gateway, SKILL.md, extensions.lock).
10. **Windows sandbox tier**, `nh acp`, distribution rails — launch hardening.

---

## Appendix — raw research (durable copies)

- **Sol (xhigh) full pass:** `04-research/_harness-research-2026-07/_sol_2026-07.md` (60-item backlog, 56 verify-live questions).
- **Fable (high) 13 lenses:** `04-research/_harness-research-2026-07/fable_{A..M}_*.md` — A providers-have, B providers-add, C token-economy, D context, E UX, F security, G architecture, H route-intelligence, I privacy-routing, J reliability, K cohesion, L ecosystem, M extensibility. Full design sketches, exact line-number grounding, and complete source URLs live in these files.
