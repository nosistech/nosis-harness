# LENS A — Providers we hold keys for (DeepSeek, Kimi/Moonshot, MiMo/Xiaomi)
**Research date: 2026-07-17 · Analyst: Fable 5 · Scope: under-used API capabilities → concrete nosis-harness changes**

Repo grounding: `catalog.toml` (route data, verified 2026-07-13 by the team), `crates/nh-core/src/lib.rs` (wire clients + agent loop), `crates/nh-routes/src/lib.rs` (RouteResolver, peak pricing), `crates/nh-fleet/src/lib.rs`, `NOSIS_HARNESS_Master_Plan.md` Appendices A/B.

Web verification: first-party docs re-checked 2026-07-16/17 (api-docs.deepseek.com, platform.kimi.ai, mimo.mi.com) plus secondary trackers. Catalog prices **re-confirmed accurate**: DeepSeek V4 Pro ¥0.025/¥3.00/¥6.00 ≈ $0.003625/$0.435/$0.87 ([api-docs pricing](https://api-docs.deepseek.com/quick_start/pricing/), [TLDL](https://www.tldl.io/resources/deepseek-api-pricing)); Kimi K2.7-code $0.19/$0.95/$4.00 ([OpenRouter](https://openrouter.ai/moonshotai/kimi-k2.7-code)); MiMo V2.5 $0.0028/$0.14/$0.28 and V2.5-Pro $0.0036/$0.435/$0.87 ([mimo.mi.com pay-as-you-go](https://mimo.mi.com/docs/price/pay-as-you-go)).

---

## F1 (HIGH / S) — Thinking defaults are wrong-way-round: DeepSeek V4 and Kimi K2.6 ship with thinking **enabled by default**, so the thinking-budget governor's "None/Low" tiers silently buy full high-effort thinking

**The facts (July 2026, first-party):**
- DeepSeek V4 thinking mode is toggled by `thinking: {"type": "enabled"|"disabled"}` and **the default is `enabled`**. Valid `reasoning_effort` values are only `high` and `max`; *"low and medium are mapped to high, and xhigh is mapped to max"* — there is **no cheap "low" thinking tier**; the only cheaper state is thinking *disabled*. Default effort is `high`, and DeepSeek auto-escalates to `max` for requests it recognizes as complex agent harnesses (Claude Code, OpenCode). Source: [DeepSeek Thinking Mode guide](https://api-docs.deepseek.com/guides/thinking_mode/), corroborated by [litellm #27439](https://github.com/BerriAI/litellm/issues/27439) and [Together's normalization docs](https://docs.together.ai/docs/deepseek-v4-quickstart).
- Kimi `kimi-k2.6` also **defaults to `thinking: {"type": "enabled"}`**; disable with `"thinking": {"type": "disabled"}`. Source: [K2.6 quickstart](https://platform.kimi.ai/docs/guide/kimi-k2-6-quickstart).

**The repo today:** `crates/nh-core/src/lib.rs:301-326` (`apply_thinking`) omits everything for `ThinkingEffort::None`, and sends `reasoning_effort: "low"` for `Low` on the `deepseek-nhm` dialect. Both are wrong against the July API:
- `None` ⇒ nothing sent ⇒ server default `thinking: enabled` at effort `high` ⇒ the harness pays thinking output tokens (at ¥6/M output, often 2–5× the visible answer) on every "no-thinking" turn.
- `Low` ⇒ `"low"` ⇒ normalized server-side to **`high`** ⇒ the "cheap thinking" tier does not exist; nosis is billing users High while displaying Low.
- `kimi-k2.6` has `thinking_dialect = "none"` in `catalog.toml:201` ("send no toggle") ⇒ K2.6 always runs in Thinking mode even for typeahead-cheap turns.

**Change (nh-core, one function + one catalog line):**
1. `deepseek-nhm` mapping: `None|Low → body["thinking"] = {"type":"disabled"}`; `High → reasoning_effort:"high"`; `Max → reasoning_effort:"max"` (and keep `thinking` enabled implicitly or explicitly). Document that Low==None on this dialect in the HUD ("low → non-thinking on DeepSeek").
2. New dialect for K2.6, e.g. `thinking_dialect = "kimi-toggle"`: `None|Low → {"type":"disabled"}` (Instant mode), `High|Max → {"type":"enabled"}`. Catalog change is one string; adapter arm is ~6 lines next to the existing match in `apply_thinking`.
3. Beware DeepSeek's *harness auto-escalation to max*: since effort defaults can be overridden by DeepSeek's agent-detection, always send an explicit value when thinking is on, so cost is what the governor chose, not what DeepSeek guessed.

**LAW fit:** congruent (differentiator #3 finally does what it claims), auditable (cost shown == cost charged), small (2 files).

---

## F2 (HIGH / S) — `reasoning_content` persistence must be *conditional on thinking+tools*, and the current catalog policy will error on K2.6 and degrade DeepSeek

**The facts:**
- DeepSeek thinking mode + tool calls: *"`reasoning_content` must be fully passed back to the API in all subsequent requests"* — mandatory, unlike plain conversations. Source: [Thinking Mode guide](https://api-docs.deepseek.com/guides/thinking_mode/).
- Kimi K2.6 with thinking enabled during multi-step tool calling: you must retain the assistant's `reasoning_content` in context **"or an error will occur"**. Source: [K2.6 quickstart](https://platform.kimi.ai/docs/guide/kimi-k2-6-quickstart).
- MiMo: keeping all previous `reasoning_content` in thinking-mode tool loops is "recommended … for best performance" (already honored: `preserve_reasoning = true`). Source: [MiMo first-api-call docs](https://mimo.mi.com/docs/en-US/quick-start/summary/first-api-call).

**The repo today:** `catalog.toml` sets `preserve_reasoning = false` for all four DeepSeek routes (line 41 etc.) and for `kimi-k2.6` (line 202). `reasoning_to_send` (`nh-core/src/lib.rs:276-298`) then strips reasoning and only sends the DeepSeek empty-string quirk. Combined with F1 (thinking is ON by default), **`nh run` on kimi-k2.6 with tools is on an error path today**, and DeepSeek thinking+tools runs violate the documented contract (quality degradation, possible cache churn).

**Change:** make replay policy a function of the *effective thinking state*, not a static route bool. In `OpenAiPolicy` add `preserve_when_thinking: bool`; in `reasoning_to_send`, treat `preserve_reasoning || (preserve_when_thinking && effort != None)` as the preserve condition (the effort is already in `ChatRequest`). Catalog: DeepSeek routes + kimi-k2.6 get `preserve_when_thinking`-style quirk or a new column. Keep the empty-string tool-replay quirk for DeepSeek *non-thinking* mode only. ~20 lines + tests mirroring the existing `reasoning_to_send` test block (`nh-core/src/lib.rs:750-815`).

**LAW fit:** secure/safe (no surprise API errors mid-fleet-run), congruent with plan A.10.5.

---

## F3 (HIGH / S) — Time-of-day routing is DeepSeek-only; MiMo has a **documented Beijing 00:00–08:00 off-peak 0.8× coefficient** the catalog doesn't know about

**The facts:** Xiaomi's Token Plan lists off-peak hours **Beijing 00:00–08:00 (UTC 16:00–24:00) with a 0.8× consumption coefficient — 20% off**; all Token Plan levels include it. Sources: [mimo.mi.com token-plan](https://mimo.mi.com/docs/en-US/price/token-plan), [platform.xiaomimimo.com/token-plan](https://platform.xiaomimimo.com/token-plan), [Toknary summary](https://ai-token-plan.com/xiaomi-mimo). Caveat: first-party page ties the 20% to Token Plan; whether pay-as-you-go gets it is not stated → catalog should carry `price_confidence = "verify_live"` on the window until confirmed with one live off-peak call.

**The repo today:** only the four DeepSeek routes carry `[price.peak]` windows (`catalog.toml:53-140`); MiMo routes have none, so `price_at()` (`nh-routes/src/lib.rs:173-189`) quotes MiMo flat 24/7 and the router can't see that **Beijing 00:00–08:00 = 10:00–18:00 La Ceiba (UTC-6) — the owner's entire working day is MiMo's discount window**, the same structural advantage the plan celebrates for DeepSeek off-peak.

**Change (data + one label):** the schema already supports it — `parse_peak` only requires `multiplier > 0` (`nh-routes/src/lib.rs:438-441`), so add to both MiMo routes:
```toml
[routes."mimo-v2.5".price.peak]
multiplier = 0.8
timezone = "Asia/Shanghai"
windows = ["00:00-08:00"]
```
Then fix the one UX seam: `peak_status` (`nh-routes/src/lib.rs:191-228`) renders "peak 0.8x until …" — when `multiplier < 1.0`, label it `"discount 0.8x until …"` (3-line change). Optionally rename the concept in docs to "clock windows".

**LAW fit:** small (pure data + 3 lines), congruent (extends differentiator #1 from 1 provider to 2), auditable (verify_live flag until confirmed).

---

## F4 (HIGH / S) — The Anthropic-wire client hard-caps `max_tokens` at 8,192 (vs DeepSeek's 384K max-out) and ignores the documented `output_config` effort control

**The facts:** DeepSeek's Anthropic-compatible endpoint controls thinking effort via `output_config: {"effort": "high"|"max"}` ([Thinking Mode guide](https://api-docs.deepseek.com/guides/thinking_mode/)); V4 max output is 384K ([pricing page](https://api-docs.deepseek.com/quick_start/pricing/)); the Master Plan itself notes "Think Max wants ≥384K headroom" (Appendix B, line 360).

**The repo today:** `make_client` (`nh-core/src/lib.rs:138-143`) builds `AnthropicMessagesClient` with `route.max_out.unwrap_or(8192).min(8192)` — i.e. **always 8,192**, and `build_anthropic_body` sends no thinking/effort field at all (`lib.rs:408-525`). So the deepclaude-proven `deepseek-v4-*-anthropic` routes truncate any long answer (`stop_reason: "max_tokens"`) and can't express the thinking governor — the two catalog routes exist but are strictly worse than their OpenAI-wire twins.

**Change:** (a) make the cap budget-aware: default `min(route.max_out, 32_768)` for normal turns and raise toward `max_out` when `thinking >= High` (the loop already knows the effort); (b) map `ThinkingEffort` → `output_config.effort` for the `deepseek-nhm` dialect on this wire (the client currently receives no dialect — pass the same `OpenAiPolicy`-style struct `make_client` already builds). ~25 lines; test with a mock asserting `output_config` and `max_tokens`.

**LAW fit:** congruent (both wires obey the same governor), honest (no silent truncation).

---

## F5 (HIGH / M) — Kimi Batch API: 40% off (0.6× real-time rates) for exactly the non-interactive work nh-fleet already queues

**The facts:** Moonshot's Batch API takes a JSONL file (≤100MB, unique `custom_id`s) via `${BASE}/files`, creates jobs via `${BASE}/batches` with completion windows `24h`/`3d`/`7d`, polls `/batches/{id}`, and bills **60% of real-time rates**; supported models are **`kimi-k2.6` and `kimi-k2.5` (K2.7-code and K3 are NOT supported)**. Sources: [platform.kimi.ai use-batch-api](https://platform.kimi.ai/docs/guide/use-batch-api), [CometAPI pricing roundup](https://www.cometapi.com/kimi-k2-api-pricing/). DeepSeek explicitly has **no batch tier** ([chat-deep.ai pricing tracker](https://chat-deep.ai/pricing/)); MiMo docs show none either — Kimi is the only batch lane we have a key for.

**The repo today:** nh-fleet (2,243 lines) runs workers over the synchronous `ChatClient`; the off-peak scheduler concept exists (M4/Master Plan §207) but the only "cheaper later" mechanism is DeepSeek clock windows. Batch is a *second, orthogonal* cheaper-later mechanism, and it composes with cache pricing.

**Change (MVP, keeps THE LAW):** don't build a general batch engine. Add one `BatchClient` in `nh-core::wire` (3 endpoints: upload, create, poll — same reqwest blocking client, ~150 lines) and one fleet job class `deferred-batch` that (a) compiles a set of single-turn fleet tasks (evals, mass classification, repo-wide analysis prompts — *not* multi-turn tool loops, which batch can't do) into one JSONL, (b) records the batch id in the fleet ledger for kill-resume, (c) ingests results as normal receipts with `price × 0.6`. Catalog gains an optional `batch_multiplier = 0.6` under `[price]` so the router can quote it honestly.

**LAW fit:** modular (one client, one job class), tension: it's the largest item here — keep it single-turn-only to stay small.

---

## F6 (MED-HIGH / M) — Cache-aware compaction: DeepSeek caches in 64-token prefix units from token 0, and every compaction is a *paid* full-prefix cache miss the engine currently ignores

**The facts:** DeepSeek context caching is automatic, prefix-only, matched **from the 0th token** in 64-token storage units (content <64 tokens never caches; practical reliability from ~1K-token prefixes); usage returns `prompt_cache_hit_tokens` / `prompt_cache_miss_tokens`; unused entries evict in "hours to a few days". Sources: [Context Caching guide](https://api-docs.deepseek.com/guides/kv_cache/), [cache-hit rules deep-dive](https://deepseekv4pro.com/guides/deepseek-context-caching-hit-rules), [news0802](https://api-docs.deepseek.com/news/news0802/). Kimi and MiMo caching are likewise automatic-prefix with published hit rates ($0.19 vs $0.95; $0.0028 vs $0.14) — same discipline applies ([Kimi pricing](https://www.cometapi.com/kimi-k2-api-pricing/), [MiMo pricing](https://mimo.mi.com/docs/price/pay-as-you-go)).

**The repo today:** the loop compacts at 70% → 50% (`nh-core/src/lib.rs:1082-1084`, `compact_history` at 1332) by *dropping an earlier prefix span* — which by definition changes every byte after the system message, so the next request re-pays cache-miss input on the entire retained history (~¥3/M on V4-Pro vs ¥0.025 cached: the compaction itself can cost more than several ordinary turns). The engine guards prefix stability of message[0] (debug asserts at 1142-1169) but treats compaction as free.

**Change:** teach the compaction decision the route's own prices: `recache_cost = retained_tokens × (cache_miss − cache_hit)`; `savings_per_turn = dropped_tokens × cache_hit_rate_estimate`; only compact when `recache_cost < savings × expected_remaining_turns` (a constant like 5 is fine for v1), and emit the price in the event line: `"context 72% — compacted 14 messages (one-time recache ≈ ¥0.41)"`. Bonus: keep compaction boundaries on whole messages (already true) and never touch message[0] (already true). ~40 lines in `compact_history`'s caller + the existing `on_event` string.

**LAW fit:** auditable/honest-cost (the HUD stops lying about "free" compaction), congruent with differentiator #4.

---

## F7 (MED / S) — Cache telemetry: parse DeepSeek's first-party `prompt_cache_hit_tokens`/`prompt_cache_miss_tokens` as a fallback for `prompt_tokens_details.cached_tokens`

**The facts:** DeepSeek's documented usage fields are `prompt_cache_hit_tokens` and `prompt_cache_miss_tokens` ([kv_cache guide](https://api-docs.deepseek.com/guides/kv_cache/), [cache-hit rules](https://deepseekv4pro.com/guides/deepseek-context-caching-hit-rules)). The OpenAI-shaped `prompt_tokens_details.cached_tokens` mirror is what nh-core parses today (`nh-core/src/lib.rs:362-375`) — it works, but a single-field dependency on a compat mirror is fragile across the V4 official-launch API churn the catalog itself flags (`valid_until = 2026-07-24`).

**Change:** in `WireUsage`, add `#[serde(default)] prompt_cache_hit_tokens: Option<u64>` and use it when `prompt_tokens_details` is absent. 6 lines + one test fixture. The cache-hit % HUD (differentiator #6's "key metric of 2026", `cache_hit_pct` at lib.rs:101) then survives either representation.

**LAW fit:** small, lightweight, auditable.

---

## F8 (MED / M) — Modality-aware dispatch (differentiator #2) has no wire support: Kimi K2.7/K2.6 and MiMo accept base64 image/video content parts, but `ChatMessage.content` is a plain string

**The facts:** K2.6/K2.7 accept images (png/jpeg/webp/gif) and videos (mp4/mov/webm/…) via base64 or file upload, images ≤4K, video ≤1080p ([K2.6 quickstart](https://platform.kimi.ai/docs/guide/kimi-k2-6-quickstart)); MiMo V2.5 "natively processes images, video, audio and text" at 1M context ([mimo.mi.com pricing](https://mimo.mi.com/docs/en-US/pricing)) and is the plan's "cheapest 1M-context multimodal route" (Appendix B line 381). The catalog already declares `modality = ["text","image","video"(,"audio")]` on five routes we hold keys for.

**The repo today:** `ChatMessage { content: Option<String> … }` (`nh-core/src/lib.rs:49-62`) — the OpenAI-wire body builder can only emit string content, so a modality-routed task physically cannot deliver the image. Differentiator #2 is currently routing metadata with no payload path.

**Change (MVP):** add `parts: Option<Vec<ContentPart>>` (`Text{text}`, `ImageB64{media_type,data}`) to `ChatMessage`; `build_body` emits the OpenAI content-array form when parts are present, string otherwise (Anthropic wire can map the same enum later). One `read_image` tool (path → base64, size-capped, LAW-gated like other fs tools) makes it usable end-to-end from `nh run`. ~120 lines total across nh-core + nh-tools.

**LAW fit:** congruent (makes an advertised differentiator real); tension: keep video/file-upload out of v1 — base64 images only.

---

## F9 (MED / S) — Structured output: Kimi has strict `json_schema`, DeepSeek has JSON output + Chat Prefix Completion — use them for the harness's *internal* structured turns

**The facts:** Kimi supports `response_format: {"type":"json_object"}` and **strict Structured Output** `{"type":"json_schema"}` (recommended; do not mix `json_object` with partial-mode prefill — use json_schema, or `partial:true` + `{` prefill) ([Moonshot JSON-mode guide](https://platform.moonshot.ai/docs/guide/use-json-mode-feature-of-kimi-api)). DeepSeek V4 lists "Json Output" and "Chat Prefix Completion (Beta)" as supported features on both models ([pricing/feature matrix](https://api-docs.deepseek.com/quick_start/pricing/)). MiMo exposes OpenAI-style `tool_choice: required/forced-function` ([AI/ML API model docs](https://docs.aimlapi.com/api-references/text-models-llm/xiaomi/mimo-v2.5)), which is the classic strict-JSON workaround.

**The repo today:** `ChatRequest` (`nh-core/src/lib.rs:84-89`) has no `response_format`; every internal structured exchange (future compaction summaries, fleet task decomposition, MCP structured results, eval graders in `EVALUATION_PLAN.md`) would have to parse free text.

**Change:** add `response_format: Option<serde_json::Value>` to `ChatRequest`, passed through verbatim on the OpenAI wire; provide one helper `json_schema(name, schema)` and use forced `tool_choice` as the fallback dialect where `json_schema` is unsupported. Keep it OFF for normal agent turns (schema constraints fight tool calling). ~30 lines.

**LAW fit:** small, modular; feeds M4/M5 fleet + eval work.

---

## F10 (MED / M) — Kimi's built-in `$web_search` tool: $0.005/call web research with **no new API key** — but only on non-thinking routes (K2.6 Instant), never K2.7-code

**The facts:** Moonshot bills `$web_search` at $0.005 per successful call plus result-processing tokens; the built-in WebSearch **requires thinking disabled**, and K2.7-code has no non-thinking mode — so K2.6 (with `thinking: disabled`, see F1's new dialect) is the only compatible route we hold a key for. Sources: [CometAPI](https://www.cometapi.com/kimi-k2-api-pricing/), [K2.6 quickstart](https://platform.kimi.ai/docs/guide/kimi-k2-6-quickstart).

**The repo today:** nh-tools has no web capability at all; any "look this up" subtask either fails or hallucinates. The Master Plan's escalation ladder has no research lane.

**Change (MVP):** a `web_research` pseudo-tool in nh-tools that, when invoked by *any* route, runs a one-shot sub-request on `kimi-k2.6` (thinking disabled) with the provider-side `$web_search` builtin enabled, and returns the summarized answer **as data** (the SECURITY_MODEL "tool outputs are always data" rule already covers web-injection). Router hint: `requires = ["web"]` routes to K2.6. Cost lands on the receipt as tool cost.

**LAW fit:** secure (data-only ingestion), out-of-scope risk: none — it's a provider feature of a key we own. Tension: adds a nested model call; keep it single-shot, no loops.

---

## F11 (MED / S) — Catalog refresh from first-party pages: MiMo UltraSpeed is now publicly priced (fast lane #2), MiMo-ASR audio exists, K2.7-code max_out = 262,144, K2.6 default max_tokens is only 32K

**The facts (all first-party):**
- **MiMo-V2.5-Pro-UltraSpeed** is now on the public pay-as-you-go page at $0.0108 cache-hit / $1.305 cache-miss / $2.61 out (exactly 3× Pro) — the catalog note "application-gated, off-by-default" (Master Plan B line 382) is stale; it's a listable human-waiting fast lane beside kimi-highspeed. Source: [mimo.mi.com pay-as-you-go](https://mimo.mi.com/docs/price/pay-as-you-go).
- **MiMo-V2.5-ASR**: $0.074 per audio hour — the cheapest transcription lane in the whole catalog for the modality router (audio → text). Same source.
- **kimi-k2.7-code max output = 262,144 tokens** ([OpenRouter model page](https://openrouter.ai/moonshotai/kimi-k2.7-code)); the catalog leaves `max_out` unset for all Kimi routes, which matters because the Anthropic-wire cap logic (F4) and output budgeting read `max_out`.
- **kimi-k2.6 default `max_tokens` = 32,768** ([K2.6 quickstart](https://platform.kimi.ai/docs/guide/kimi-k2-6-quickstart)) — long K2.6 outputs need an explicit `max_tokens`, which the OpenAI-wire body never sends today (`build_body`, nh-core/src/lib.rs:225-274).
- DeepSeek **DSpark** speculative decoding is server-side (+57–78% speed on Pro, +60–85% on Flash, Master Plan line 254, no client work) — after the official V4 launch settles (catalog `valid_until = 2026-07-24`), re-measure latency tiers so "highspeed/UltraSpeed only when a human waits" decisions stay honest.

**Change:** pure `catalog.toml` data (+ optionally an explicit `max_tokens` passthrough on the OpenAI wire, ~5 lines). Also worth recording: Kimi Agent Swarm (300 sub-agents/4,000 steps) is a **hosted product surface, not a raw `kimi-k2.6` API parameter** ([kimi-ai.chat K2.6 explainer](https://kimi-ai.chat/models/kimi-k2-6/), [MarkTechPost launch coverage](https://www.marktechpost.com/2026/04/20/moonshot-ai-releases-kimi-k2-6-with-long-horizon-coding-agent-swarm-scaling-to-300-sub-agents-and-4000-coordinated-steps/)) — the M4 "Swarm passthrough" milestone item should be re-scoped to "verify API availability first" before any code is planned.

**LAW fit:** small, honest-cost (data stays confirmed), congruent with catalog-is-data rule.

---

## Cross-checks & non-findings
- **Prices in catalog.toml are all still correct** against July 16–17 first-party pages (see header). `valid_until = 2026-07-24` re-verification stands: legacy `deepseek-chat`/`deepseek-reasoner` die 2026-07-24 15:59 UTC ([pricing page](https://api-docs.deepseek.com/quick_start/pricing/)) — the ban list in `nh-routes/src/lib.rs:247-254` already covers this.
- **DeepSeek peak 2× windows** (Beijing 09–12/14–18) still not on the first-party pricing page as of this check — keep the catalog's re-verify note; secondary trackers disagree with each other ([NxCode](https://www.nxcode.io/resources/news/deepseek-api-pricing-complete-guide-2026) shows stale/conflicting data), trust only api-docs at the July 24 re-check.
- **Parallel tool calls**: the loop already executes every call in a multi-call turn sequentially and returns per-id tool messages — no change needed; MiMo's historical streamed-tool-args quirk is moot while the clients are non-streaming.
- **Temperature**: K2.6 fixes temperature (1.0 thinking / 0.6 non-thinking) and errors on custom values — nh-core never sends temperature, so we're accidentally correct; keep it that way.
- **1M-context handling**: DeepSeek/MiMo bill flat across context lengths (no long-context surcharge) — no router change needed beyond the existing `context` field.

## Sources
- https://api-docs.deepseek.com/quick_start/pricing/
- https://api-docs.deepseek.com/guides/thinking_mode/
- https://api-docs.deepseek.com/guides/kv_cache/
- https://api-docs.deepseek.com/news/news0802/
- https://deepseekv4pro.com/guides/deepseek-context-caching-hit-rules
- https://github.com/BerriAI/litellm/issues/27439
- https://docs.together.ai/docs/deepseek-v4-quickstart
- https://www.tldl.io/resources/deepseek-api-pricing
- https://chat-deep.ai/pricing/
- https://www.nxcode.io/resources/news/deepseek-api-pricing-complete-guide-2026
- https://platform.kimi.ai/docs/guide/kimi-k2-6-quickstart
- https://platform.kimi.ai/docs/guide/use-batch-api
- https://platform.moonshot.ai/docs/guide/use-json-mode-feature-of-kimi-api
- https://www.cometapi.com/kimi-k2-api-pricing/
- https://openrouter.ai/moonshotai/kimi-k2.7-code
- https://kimi-ai.chat/models/kimi-k2-6/
- https://www.marktechpost.com/2026/04/20/moonshot-ai-releases-kimi-k2-6-with-long-horizon-coding-agent-swarm-scaling-to-300-sub-agents-and-4000-coordinated-steps/
- https://mimo.mi.com/docs/en-US/pricing
- https://mimo.mi.com/docs/price/pay-as-you-go
- https://mimo.mi.com/docs/en-US/price/token-plan
- https://platform.xiaomimimo.com/token-plan
- https://ai-token-plan.com/xiaomi-mimo
- https://mimo.mi.com/docs/en-US/quick-start/summary/first-api-call
- https://docs.aimlapi.com/api-references/text-models-llm/xiaomi/mimo-v2.5
- https://hyper.ai/en/stories/188287194b08082101b20f8e8fdf6b18
