# LENS C — Token Economy + "Profiles" Feature (NOSIS Harness)
**Research date: 2026-07-16/17 · Analyst: Fable 5 · Repo HEAD: bd35b4d (M4 Slice D uncommitted)**

---

## 0. Executive summary

The harness already owns the four biggest cost levers as *mechanisms* (clock-aware pricing in `nh-routes`, cache-hit telemetry in `nh-core`, thinking dialects, off-peak fleet deferral in `nh-fleet`), but they are **scattered constants and per-surface defaults with no single user-facing control**. The owner's ask — "ways to save tokens directly with a toggle per provider by profile" — maps cleanly onto a small, data-not-code `profiles.toml` layered exactly like `law.toml`, with a `/profile` toggle and an active-profile chip in the HUD, recorded in every receipt (auditable).

Three concrete money leaks were found in the current code while grounding this lens:

1. **DeepSeek's thinking mode is the DEFAULT** on V4 per the first-party pricing page ("non-thinking and thinking (default) modes") — and `nh-core` *omits* `reasoning_effort` when effort is `None`, assuming omission = non-thinking (`crates/nh-core/src/lib.rs:307-318`). If omission means "provider default = thinking", every quick-edit turn is silently paying reasoning output tokens at ¥2–6/M. Must verify live; a frugal profile must always pin an explicit value.
2. **The OpenAI wire sends no `max_tokens` at all** (only the Anthropic wire caps output, hard-coded `min(route.max_out, 8192)` at `crates/nh-core/src/lib.rs:141`). DeepSeek V4 allows up to 384K output tokens; output is the most expensive token class on every held provider (DeepSeek out ¥6.00/M pro; Kimi $4/M; GLM's lineup prices output ~3× input).
3. **Compaction burns the KV cache**: `compact_history` drains `history[1..start]` and rewrites the first surviving message (`crates/nh-core/src/lib.rs:1332-1368`), so the request immediately after compaction is a near-total cache MISS. On DeepSeek V4-Pro that's ¥3.00/M instead of ¥0.025/M — a 120× cliff the harness triggers on itself, invisibly.

Everything below is designed to respect THE LAW: profiles are TOML data; the code change is one small resolver + threading existing values through seams that already exist.

---

## 1. Provider token-economy facts (verified July 2026, first-party where possible)

### 1.1 DeepSeek (key HELD)
- **Prices (first-party, fetched 2026-07-16):** V4-Flash $0.0028 cache-hit / $0.14 cache-miss / $0.28 out per M; V4-Pro $0.003625 / $0.435 / $0.87. USD page matches the CNY numbers in `catalog.toml` (¥0.02/1.00/2.00 and ¥0.025/3.00/6.00). Source: https://api-docs.deepseek.com/quick_start/pricing
- **Cache-hit is ~50× (flash) to ~120× (pro) cheaper than miss** — the single biggest lever, as the Master Plan §0.1 already states.
- **Caching is automatic, full-prefix-match only.** Cache units form at request boundaries, at detected common prefixes, and at fixed token intervals inside long content; caches expire "within a few hours to a few days" when unused. Usage reports `prompt_cache_hit_tokens` / `prompt_cache_miss_tokens` (native) **and** `prompt_tokens_details.cached_tokens` (OpenAI-compat) — ecosystem tooling has repeatedly gotten this wrong (see any-llm bug). Sources: https://api-docs.deepseek.com/guides/kv_cache · https://github.com/mozilla-ai/any-llm/issues/795
- **Peak/off-peak 2× pricing is still NOT on the first-party pricing page** (fetched 2026-07-16). Press coverage (July 3) calls peak-hour pricing *announced-but-undefined*: "DeepSeek confirms current V4 prices, but not first-party peak windows, dashboard behavior, or migration terms," with the alias migration concluding July 24. The catalog's `valid_until = 2026-07-24` re-check is exactly right; note the peak *block* currently inherits `price_confidence = "confirmed"` from the base table, which overstates it. Source: https://winbuzzer.com/2026/07/03/deepseek-v4-may-add-peak-hour-pricing-to-its-api-xcxwbn/
- **Thinking is the DEFAULT mode on both V4 models** per the pricing page ("support non-thinking and thinking (default) modes"). This interacts badly with `apply_thinking`'s omit-on-None behavior (see §3, Finding 2).
- **No batch API / no batch discount** exists; off-peak windows (when they land) + cache are the batch substitute.

### 1.2 Kimi / Moonshot (key HELD)
- **K2.7-Code (first-party, fetched 2026-07-16):** $0.19 cache-hit / $0.95 cache-miss / $4.00 out; highspeed 2×: $0.38/$1.90/$8.00; 256K context; "supports automatic context caching" — no explicit cache API, no storage fees, no batch API. Source: https://platform.kimi.ai/docs/pricing/chat-k27-code.md
- Cache-hit = 80% off; caching is automatic on prefix reuse; check returned cached-token usage rather than assuming. Sources: https://costgoat.com/pricing/kimi-api · https://benchlm.ai/moonshot/api-pricing
- **Kimi K3 launched 2026-07-16** (yesterday): 2.8T-param open MoE, Kimi Delta Attention, 1M context; pricing $3.00 cache-miss / $0.30 cache-hit / $15.00 out per M, flat across the window. Premium route — the cheap-route preference stays on K2.x, but the catalog (data, not code) should gain the entry with `price_confidence = "verify_live"`. Sources: https://www.marktechpost.com/2026/07/16/moonshot-ai-releases-kimi-k3-a-2-8-trillion-parameter-open-moe-model-with-kimi-delta-attention-and-1m-context/ · https://benchlm.ai/moonshot/api-pricing
- K2.7's always-thinking + `preserve_reasoning` means its history is *bigger* per turn by design — but ~30% fewer thinking tokens than K2.6 (plan A.2). The profile lever for Kimi is **route choice** (never highspeed unless a human is waiting — already catalog doctrine) and **output caps**, not thinking toggles (there is none to send).

### 1.3 MiMo / Xiaomi (key HELD)
- **First-party (fetched 2026-07-16):** V2.5 $0.0028 hit / $0.14 miss / $0.28 out; V2.5-Pro $0.0036 / $0.435 / $0.87; UltraSpeed $0.0108 / $1.305 / $2.61. Matches `catalog.toml` exactly. **Cache-hit on MiMo is ~50× cheaper than miss — the deepest cache discount of any held provider** (hit is 2% of miss). No night-discount windows are published on the pricing page right now (the plan's "night discounts" claim is currently unverifiable first-party → keep `peak`-style data OUT of the MiMo entries until a first-party window is published; honest-cost rule). Source: https://mimo.mi.com/docs/pricing
- MiMo V2.5 remains the cheapest 1M-context multimodal route in the catalog; "no long-context surcharge" still holds (flat pricing).

### 1.4 GLM / Z.ai (NO key held → keyRequired=glm)
- glm-4.7-flash / 4.5-flash / 4.6v-flash are still listed FREE; glm-5.2 $1.40 / $0.26 cached / $4.40 out. Output ≈ 3× input across the lineup; built-in Web Search tool costs $0.01/call on top of tokens (receipt must line-item it if ever used). No batch discount found. Sources: https://docs.z.ai/guides/overview/pricing · https://pricepertoken.com/pricing-page/provider/z-ai
- The $0 CI lane (differentiator for the test suite) is a *profile-selectable* route preference but requires registering a GLM key first.

### 1.5 Cross-provider techniques (2026 state of the art, for citation)
- **MCP/tool-schema bloat is the documented 2026 context tax**: 7 MCP servers ≈ 67,300 tokens of tool definitions (~34% of a 200K window) before the first user prompt. Fixes that shipped: Anthropic Tool Search Tool (defer_loading, −85% tool-def tokens), Anthropic code-execution-with-MCP (150K → 2K tokens, −98.7%), Cloudflare Code Mode, Atlassian's open-source `mcp-compressor` proxy (−70–97% description overhead). Sources: https://www.anthropic.com/engineering/code-execution-with-mcp · https://mcp.directory/blog/mcp-context-bloat-fix-2026-tool-search-code-mode-progressive-disclosure · https://www.stackone.com/blog/mcp-token-optimization/ · https://www.atlassian.com/blog/development/mcp-compression-preventing-tool-bloat-in-ai-agents
- **Prompt compression research**: LLMLingua/LongLLMLingua (perplexity-based token pruning, up to 20× compression, ~1.5% loss on reasoning tasks), LLMLingua-2 (token-classification distillation); 2026 guidance is that token-level pruning corrupts code/JSON structure, so **coding agents should use extractive/verbatim compaction and task-conditioned tool-output pruning** (e.g. "Squeez: Task-Conditioned Tool-Output Pruning for Coding Agents", arXiv 2604.04979). Sources: https://arxiv.org/pdf/2310.06839 · https://arxiv.org/pdf/2604.04979 · https://www.morphllm.com/prompt-compression
- **Batch APIs:** none of the three held providers publishes a batch endpoint or batch discount. The harness's off-peak scheduler + KV-cache discipline + free-GLM lane IS its batch story. (OpenAI/Anthropic batch −50% exists but requires keys the project doesn't hold.)

---

## 2. Every token/cost lever in THIS harness (enumeration, with code refs)

| # | Lever | Where it lives today | State |
|---|---|---|---|
| 1 | KV-cache stable prefix | `nh-core` debug_assert on `history[0]` bytes (`lib.rs:1141-1152`); constitution byte-stable (`nh-law::assemble_constitution`) | mechanism ✅, no user knob |
| 2 | Cache-hit % telemetry | `cache_hit_pct` (`nh-core lib.rs:101`), HUD chip (`nh-tui lib.rs:401-428`) | parses only `prompt_tokens_details.cached_tokens` on OpenAI wire (`lib.rs:403`) — DeepSeek also ships native fields; add fallback + verify-live per provider |
| 3 | Peak/off-peak clock pricing | `price_at`/`peak_status` (`nh-routes lib.rs:173-228`), catalog `peak` blocks | ✅ for quotes; peak block confidence overstated (first-party page still windowless) |
| 4 | Off-peak deferral | `nh-fleet` `defer_offpeak` per task + config (`lib.rs:48,157,996-1016`) | ✅ fleet only; no interactive default, no profile default |
| 5 | Thinking effort | `ThinkingEffort` + `apply_thinking` (`nh-core lib.rs:301-326`), `/effort`, `effort_for` defaults (`nh-tui lib.rs:1198`) | knob exists per session; DeepSeek omit-on-None is a suspected silent thinking-mode default (Finding 2) |
| 6 | Output caps (`max_tokens`) | Anthropic wire only, hard-coded `min(max_out, 8192)` (`nh-core lib.rs:141,410-505`) | OpenAI wire sends NOTHING → unbounded output |
| 7 | Stop sequences | not used anywhere | absent (minor lever) |
| 8 | Context budget cap / compaction | `COMPACT_AT=0.70`, `COMPACT_TARGET=0.50`, `KEEP_RECENT=2` consts; `context_limit = route.context` (1M!) | thresholds hard-coded; effective cap = full route window → a long session can legitimately grow to ~700K tokens/turn before compacting |
| 9 | Compaction ↔ cache interaction | `compact_history` drains + rewrites (`lib.rs:1332-1368`) | cache-hostile: every compaction = full-prefix cache miss next turn |
| 10 | Constitution size | bundled law + user law + repo law + **entire AGENTS.md** + memory (`nh-law lib.rs:143-232`) | no trimming knob; arbitrary repos may carry huge AGENTS.md into every request |
| 11 | Tool schemas | 3 built-in tools (small); MCP toolsets injected wholesale; `tools/list` ttlMs cache (`nh-tools/mcp.rs:19,223-253`) is round-trip caching, not token reduction | no schema slimming / defer-loading |
| 12 | Tool output size | `read_file` returns whole file, `exec` full stdout+stderr — no byte cap (`nh-tools/lib.rs`) | token-bomb risk the plan itself warns about (§5.8 Playwright lesson) |
| 13 | Cheap-route preference | `provider_default` = cheapest by output price (`nh-routes lib.rs:553`); escalation ladder Flash→…→Opus gate (fleet Slice B) | ✅ mechanism; no per-task "frugal vs max" bias knob |
| 14 | Turn budget | `max_turns` (default 20) | CLI flag only |
| 15 | Token budget bar | TUI `self.budget` + budget bar (`nh-tui lib.rs:414-426`) | tokens only — not currency; no hard stop wired to price |
| 16 | Currency cost accounting | `PriceQuote` exists; usage exists; **they are never multiplied** | "session cost so far" chip missing = cost opacity (differentiator 6) unmet |
| 17 | Free-route CI lane | glm-4.7-flash etc. in catalog | needs GLM key (not held) |
| 18 | Route/price freshness | `valid_until` + `stale` flag (`nh-routes lib.rs:173-189`) | ✅; K3 entry missing (new 2026-07-16) |

---

## 3. Findings (ranked)

### Finding 1 — Ship `profiles.toml` + `nh profile` / `/profile`: one data file that owns every knob above
**What.** A first-class cost-profile layer: three bundled profiles (`frugal`, `balanced`, `max`), user-overridable, with per-provider override tables. Catalog stays pure route DATA; profiles are a separate, small TOML that *references* routes — same layering discipline as `law.toml` (bundled → user `~/.nosis/profiles.toml` → repo `.nosis/profiles.toml`, repo can only tighten spend, mirroring nh-law's "repo may add protections, never weaken").

**Schema sketch (data, not code):**
```toml
# ~/.nosis/profiles.toml  (bundled defaults compiled in like BUNDLED_LAW)
active = "balanced"                     # or via `nh profile use frugal` / `/profile`

[profiles.frugal]
thinking       = "low"        # pinned explicit value, never provider-default
context_cap    = 65536        # effective window = min(route.context, this)
max_out        = 4096         # OpenAI + Anthropic wires both send it
prefer         = "cheapest"   # route bias: cheapest capable in modality
defer_offpeak  = true         # fleet default; interactive gets a peak warning
constitution   = "core"       # drop Memory + AGENTS.md sections at session start
tool_output_cap = 16384       # bytes per tool result before head/tail elision
compact_at     = 0.60
compact_target = 0.35         # compact earlier but DEEPER (fewer cache burns)
max_turns      = 12

[profiles.frugal.provider.deepseek]
thinking = "low"              # per-provider override (owner's explicit ask)
[profiles.frugal.provider.kimi]
route = "kimi-k2.6"           # never highspeed under frugal

[profiles.max]
thinking = "max"
context_cap = 0               # 0 = route window
max_out = 0                   # 0 = route max_out
prefer = "best"
defer_offpeak = false
constitution = "full"
```
**Toggle UX.** `/profile` in chat/TUI opens the same live menu pattern as `/model` (Slice E); `nh profile use <name>` for headless; `nh run --profile frugal`. The HUD line (`nh-tui lib.rs:401`) gains one chip: `· frugal`. Every receipt records `profile: "frugal"` (auditable tenet). Switching mid-session re-derives the constitution + effort exactly like `/model` already re-derives `identity_constitution` (`cmd_chat.rs:284-295`) — cache warmth resets, which the user already accepts for `/model`.

**Smallest MVP (1 slice):** parse + resolve profiles (one small module in `nh-cli` or a ~150-line `nh-profiles` file inside nh-routes' consumer, NOT a new crate); apply only `thinking`, `max_out`, `context_cap`, `defer_offpeak`; `/profile` + HUD chip + receipt field. The remaining knobs (constitution trim, tool_output_cap, compact thresholds) land as follow-ups threading through seams that already exist (`AgentLoop.constitution`, `context_limit`, consts → fields).

**Law fit.** Small (one TOML + one resolver), simple, auditable (receipt records profile), modular (each knob threads an existing seam), congruent (mirrors law.toml layering and catalog-is-data). Tension: knob count must stay bounded — cap the schema at the levers enumerated here; anything else goes to LATER.md.

---

### Finding 2 — DeepSeek "thinking (default)" trap: omitting `reasoning_effort` may be silently buying reasoning tokens
**What.** DeepSeek's first-party pricing page says both V4 models "support non-thinking and thinking (**default**) modes" (fetched 2026-07-16). `apply_thinking` (`nh-core/src/lib.rs:307-318`) omits the field entirely for `ThinkingEffort::None`, on the belief that "none is invalid, so omit for non-thinking". If the server default is *thinking*, then `nh run`'s default (`effort_for(DeepseekNhm) = None`, `nh-tui lib.rs:1198-1202`) pays reasoning output tokens (billed at output rate, ¥2.00–¥6.00/M) on every quick edit — precisely the "cheapest capable" turns the harness exists to protect.

**Recommendation.** Verify live (one probe per model: same prompt, omitted vs `low`, compare `completion_tokens` and presence/length of `reasoning_content`), then either (a) pin `low` as the frugal/balanced floor for DeepSeek, or (b) if a live-valid non-thinking token exists in the confirmed enum (`high|low|medium|max|xhigh` was live-confirmed 2026-07-14 per the code comment — note there is no "off"), document that omission is the only non-think path with a receipt-visible marker. Profiles (Finding 1) then always send an explicit, chosen value. Effort S; potentially the highest per-dollar finding in this report.

**Evidence.** https://api-docs.deepseek.com/quick_start/pricing · `crates/nh-core/src/lib.rs:300-326` · `crates/nh-tui/src/lib.rs:1198-1202`.

---

### Finding 3 — Session cost in CURRENCY + budget hard-stop (multiply the two things you already have)
**What.** The HUD shows tokens and cache-% but never money; `PriceQuote` (`nh-routes lib.rs:131-141`) and split usage (`cached_tokens` vs `prompt_tokens`) are both live, but no code multiplies them. Cost opacity is documented CLI pain the plan promises to fix (differentiator 6, "Cost HUD… session cost so far, projected cost-to-goal, budget bar with hard stop").

**Recommendation.** Add `fn turn_cost(quote: &PriceQuote, usage: &Usage) -> f64` = `(cached×hit + (prompt−cached)×miss + completion×output)/1e6`, accumulate per session, render one HUD chip (`¥0.42` / `$0.13`), and honor a per-profile `budget_currency` hard stop (frugal: e.g. $0.50/session). Quote at the *time of each turn* so peak turns bill 2× honestly; flag `stale`/`verify_live` quotes with the existing confidence marks. Delegate routes (no price table) show "quota" as planned. Effort S–M; entirely local math; makes every other profile knob *visible* so the owner can feel the savings.

**Evidence.** `crates/nh-routes/src/lib.rs:131-189` · `crates/nh-tui/src/lib.rs:401-428` · NOSIS_HARNESS_Master_Plan.md §5.2.

---

### Finding 4 — Cache-aware compaction: stop paying the 120× cliff every time you compact
**What.** DeepSeek/Kimi/MiMo caching is automatic **full-prefix matching** (DeepSeek: "a subsequent request can only hit the cache if it fully matches a cache prefix unit"). `compact_history` (`nh-core lib.rs:1332-1368`) drains `history[1..start]` AND rewrites the content of the first surviving message — so the very next request shares only the system message with any cache unit: a near-total cache miss. On V4-Pro that single turn bills at ¥3.00/M for what was ¥0.025/M — on a 500K-token history that's ~¥1.5 vs ~¥0.0125 for one turn. Compacting at 70%→50% repeatedly on a long session triggers this cliff over and over.

**Recommendation.** Three small, data-driven changes: (1) make `compact_at`/`compact_target` profile knobs (frugal compacts *earlier and deeper* — fewer, bigger compactions = fewer cache burns; max-quality compacts late); (2) don't rewrite the survivor message — insert the `[nosis] earlier context compacted…` marker as a NEW message so surviving bytes stay byte-identical (preserves any interior cache units DeepSeek formed at request boundaries); (3) log `cache_burn_estimate` in the compaction receipt/timeline marker so the cost is auditable, and have the TUI's degradation-guard marker show it. Optionally: since cache misses are what cost money, schedule compaction to coincide with route switches (cache is already cold then). Effort M.

**Evidence.** https://api-docs.deepseek.com/guides/kv_cache · `crates/nh-core/src/lib.rs:1082-1084,1332-1368` · catalog.toml:44-51.

---

### Finding 5 — Output-token discipline: send `max_tokens` on the OpenAI wire, make the Anthropic 8192 cap data
**What.** Output is the priciest token class everywhere (DeepSeek pro ¥6.00/M out vs ¥3.00 miss-in; Kimi $4.00 out vs $0.95 in; GLM's whole lineup prices output ~3× input; MiMo $0.87 out vs $0.435 in). The OpenAI-wire client builds its body with **no `max_tokens` at all** (`build_openai_body`, `nh-core` — only the Anthropic client sends one, hard-coded `route.max_out.unwrap_or(8192).min(8192)` at `lib.rs:141`). DeepSeek V4 permits up to 384K output tokens; one runaway generation can cost more than a whole session.

**Recommendation.** Thread `max_out` from the profile into BOTH wire clients (`ChatRequest` gains `max_tokens: Option<u64>`; OpenAI body sets it when Some; Anthropic replaces the magic 8192). Frugal = 4096, balanced = 16384, max = route.max_out. Keep the route's `max_out` as the ceiling (`min`). Add `stop` sequences only if a live need appears (LAW: don't pre-build). Note Kimi/MiMo always-thinking routes need headroom for reasoning tokens inside the output budget — set per-provider overrides (e.g. kimi frugal = 8192). Effort S.

**Evidence.** `crates/nh-core/src/lib.rs:141,410-505` · https://platform.kimi.ai/docs/pricing/chat-k27-code.md · https://docs.z.ai/guides/overview/pricing · catalog.toml `max_out = 384000`.

---

### Finding 6 — Tool-result caps + MCP schema slimming (the 2026 "context tax", pre-empted)
**What.** Two related leaks: (a) `read_file`/`exec` return unbounded content — the plan's own Playwright token-bomb lesson (§5.8) applies to the harness's first-party tools; (b) MCP toolsets inject every server's full JSON schema into every request — the documented 2026 industry failure mode (7 servers ≈ 67K tokens, ~34% of a 200K window, before the user types; Anthropic's Tool Search cut tool-def tokens 85%, code-execution-with-MCP cut a workflow 150K→2K, Atlassian's mcp-compressor proxy trims descriptions 70–97%).

**Recommendation.** (a) Profile knob `tool_output_cap` (bytes): head+tail elision with an honest `[nosis] tool output elided: N bytes …` marker — task-conditioned tool-output pruning is the technique the coding-agent literature currently endorses over token-level compression (Squeez, arXiv 2604.04979; token pruning corrupts code structure). (b) For MCP: under `frugal`, include only tool `name` + first sentence of `description` + required params in the schema sent to the model (full schema stays client-side for validation); a `tools = ["allowlist"]` per server in `.nosis/mcp.toml` so a repo only carries the tools it uses. The existing ttlMs cache (`nh-tools/mcp.rs:223-253`) already avoids re-fetch round-trips; this cuts the *per-request token* cost, which ttlMs does not touch. Defer a full Tool-Search-style lazy loader unless MCP tool counts actually grow (LAW: smallest change). Effort M.

**Evidence.** `crates/nh-tools/src/lib.rs:158-256` (no caps) · `crates/nh-tools/src/mcp.rs:19,223-253` · https://www.anthropic.com/engineering/code-execution-with-mcp · https://www.stackone.com/blog/mcp-token-optimization/ · https://www.atlassian.com/blog/development/mcp-compression-preventing-tool-bloat-in-ai-agents · https://arxiv.org/pdf/2604.04979

---

### Finding 7 — Off-peak: make deferral a profile default, and mark DeepSeek peak data honestly
**What.** The fleet already parks peak-priced tasks (`defer_offpeak`, Slice B, `nh-fleet lib.rs:996-1016`) — but it's per-task/per-config with no global stance, and interactive surfaces only show the peak chip. Meanwhile the catalog's DeepSeek `peak` blocks carry the base table's `price_confidence = "confirmed"`, yet the first-party pricing page STILL shows no peak windows (fetched 2026-07-16) and July press explicitly says windows/dashboard behavior are unconfirmed. The catalog comment acknowledges this; the *data model* doesn't.

**Recommendation.** (1) Profile knob `defer_offpeak` (frugal=true) becomes the fleet default; (2) interactive marathon guard: when a `nh run`/TUI task enters a peak window on a peak-priced route, emit one amber line "peak 2× until HH:MM — /profile frugal defers, or continue" (the plan's promised warn-before-marathon, §3); (3) add optional `confidence` inside the `[.price.peak]` table (defaults to the price table's) so peak can be `verify_live` while base prices stay `confirmed` — honest-cost rule applied to the peak dimension; re-verify at the existing `valid_until = 2026-07-24`. Effort S (knob + one field + one warning line; scheduler already exists).

**Evidence.** `crates/nh-fleet/src/lib.rs:48,996-1016` · catalog.toml:21-28,53-56 · https://winbuzzer.com/2026/07/03/deepseek-v4-may-add-peak-hour-pricing-to-its-api-xcxwbn/ · https://api-docs.deepseek.com/quick_start/pricing (no peak windows listed).

---

### Finding 8 — Constitution/system-prefix trimming, byte-stable per session
**What.** `assemble_constitution` (`nh-law lib.rs:143-232`) inlines bundled law + user law + repo law + the **entire AGENTS.md** + memory into every request's system message. On the nosis repo that's small; on arbitrary user repos AGENTS.md/memory can be thousands of tokens, resent every turn. Because cache-hit pricing makes a stable prefix nearly free ON HIT (¥0.025/M), the real cost is (a) every cache-miss turn (first turn, post-compaction, post-route-switch, cache expiry "hours to days"), and (b) attention/quality dilution.

**Recommendation.** Profile knob `constitution = "full" | "core"`: `core` keeps Operating law + Project law (the enforceable parts; policy compilation is UNTOUCHED — write-holds never depend on prompt text) and drops Memory + AGENTS.md sections, or caps each section at N bytes with an honest `[section truncated]` marker. Applied ONLY at session start / route switch so the prefix stays byte-stable within a session (the `debug_assert_eq!(message_bytes(&history[0]))` invariant at `nh-core lib.rs:1141-1152` keeps enforcing this for free). Effort S: `ConstitutionSources` already separates the sections; the trim is a filter before `assemble_constitution`.

**Evidence.** `crates/nh-law/src/lib.rs:143-232` · `crates/nh-core/src/lib.rs:1141-1152` · DeepSeek cache-expiry: https://api-docs.deepseek.com/guides/kv_cache

---

### Finding 9 — Cache-telemetry correctness: parse DeepSeek's native cache fields as fallback
**What.** The OpenAI-wire client reads only `prompt_tokens_details.cached_tokens` (`nh-core lib.rs:403`). DeepSeek returns BOTH that OpenAI-compat field and its native `prompt_cache_hit_tokens`/`prompt_cache_miss_tokens`; ecosystem bug reports (any-llm #795, earendil-works/pi #3880) show integrations silently reading 0% cache on DeepSeek by trusting one shape. Kimi/MiMo are OpenAI-compat but their exact usage field shapes should be verify-live'd too. The cache-hit % chip is the plan's "key metric of 2026" — it must never silently read 0, because the frugal profile's whole feedback loop (and Finding 3's cost math) keys off it.

**Recommendation.** In `WireUsage`, deserialize `prompt_cache_hit_tokens` as a fallback when `prompt_tokens_details.cached_tokens` is absent (`cached = details.cached_tokens.or(prompt_cache_hit_tokens)`), plus one recorded verify-live probe per provider in the M-next gate (assert `cached_tokens > 0` on a repeated-prefix second call). Effort S (a serde field + one line).

**Evidence.** `crates/nh-core/src/lib.rs:374-410` · https://api-docs.deepseek.com/guides/kv_cache · https://github.com/mozilla-ai/any-llm/issues/795

---

### Finding 10 — Catalog freshness: Kimi K3 landed 2026-07-16; add as data, keep it OUT of frugal
**What.** Moonshot released Kimi K3 yesterday (2.8T-param open MoE, Kimi Delta Attention, 1M context). First-party pricing per aggregator readings of platform.kimi.ai: $3.00 cache-miss / $0.30 cache-hit / $15.00 out per M, flat across the 1M window — a premium route (90% cache discount though). The catalog's own doctrine ("new models are a TOML entry, not a release") makes this a pure data change; the profiles feature gives it a home: `max` profile may prefer it after live eval, `frugal`/`balanced` never route to it.

**Recommendation.** Add `[routes."kimi-k3"]` with `price_confidence = "verify_live"` + `valid_until` ≈ +2 weeks, `context = 1000000`; verify thinking dialect / preserve_reasoning against Moonshot docs before first live call (K2.7's always-thinking rule may not carry over). No adapter code should change (OpenAI wire). Effort S.

**Evidence.** https://www.marktechpost.com/2026/07/16/moonshot-ai-releases-kimi-k3-a-2-8-trillion-parameter-open-moe-model-with-kimi-delta-attention-and-1m-context/ · https://benchlm.ai/moonshot/api-pricing · catalog.toml:142-211 · NOSIS_HARNESS_Master_Plan.md §7 ("catalog is data").

---

### Finding 11 (out-justified, keyRequired=glm) — Activate the $0 GLM lane so `frugal` can route smoke work at zero cost
**What.** glm-4.7-flash / glm-4.5-flash / glm-4.6v-flash remain listed FREE on Z.ai's pricing page, already fully described in catalog.toml — but no GLM key is held, so the harness's cheapest possible route (and its $0 CI story, plan A.4) is dark. A `frugal` profile that can say `provider.glm.route = "glm-4.7-flash"` for classification/smoke/summarize tasks makes the profile feature's floor literally $0. Registration on bigmodel.cn also grants 20M free tokens (plan A.4).

**Recommendation.** One `nh key add glm` away; no code. Under profiles: `[profiles.frugal] smoke_route = "glm-4.7-flash"` used by self-tests and receipt-summarization-class tasks. Respect the honesty caveat: free tiers are rate-limited with unpublished limits — mark `best-effort`, never critical-path.

**Evidence.** https://docs.z.ai/guides/overview/pricing · catalog.toml:264-348 · Master Plan A.4/B.4.

---

## 4. What was deliberately NOT proposed (LAW discipline)
- **LLMLingua-style token-level prompt compression in the harness**: current research says perplexity pruning corrupts code/JSON at agent-relevant ratios; verbatim/extractive compaction (which nh already does) is the right family for coding agents. No heavy dep, no model-in-the-loop compressor. (https://www.morphllm.com/prompt-compression · arXiv 2604.04979)
- **A batch-API adapter**: no held provider offers one; the off-peak scheduler already fills that role.
- **Explicit cache-management APIs**: all three held providers are automatic-caching; there is nothing to manage — the lever is prefix discipline, which exists.
- **A new `nh-profiles` crate**: a module + TOML suffices; a ninth crate would violate small/lightweight for ~150 lines.

## 5. Sources
- https://api-docs.deepseek.com/quick_start/pricing
- https://api-docs.deepseek.com/guides/kv_cache
- https://winbuzzer.com/2026/07/03/deepseek-v4-may-add-peak-hour-pricing-to-its-api-xcxwbn/
- https://platform.kimi.ai/docs/pricing/chat-k27-code.md
- https://mimo.mi.com/docs/pricing
- https://docs.z.ai/guides/overview/pricing
- https://www.anthropic.com/engineering/code-execution-with-mcp
- https://mcp.directory/blog/mcp-context-bloat-fix-2026-tool-search-code-mode-progressive-disclosure
- https://www.stackone.com/blog/mcp-token-optimization/
- https://www.atlassian.com/blog/development/mcp-compression-preventing-tool-bloat-in-ai-agents
- https://github.com/mozilla-ai/any-llm/issues/795
- https://www.marktechpost.com/2026/07/16/moonshot-ai-releases-kimi-k3-a-2-8-trillion-parameter-open-moe-model-with-kimi-delta-attention-and-1m-context/
- https://benchlm.ai/moonshot/api-pricing
- https://costgoat.com/pricing/kimi-api
- https://pricepertoken.com/pricing-page/provider/z-ai
- https://arxiv.org/pdf/2310.06839 (LongLLMLingua)
- https://arxiv.org/pdf/2604.04979 (Squeez: task-conditioned tool-output pruning)
- https://www.morphllm.com/prompt-compression
