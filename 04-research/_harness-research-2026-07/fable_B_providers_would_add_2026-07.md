# LENS B — Providers We'd Add (GLM/Z.ai, Gemini/Google, Claude/Anthropic, Codex/OpenAI)
**Nosis Harness research — 2026-07-17 (facts verified against live July-2026 sources)**

Repo grounding: `catalog.toml` (routes + delegate stubs), `crates/nh-routes/src/lib.rs` (Wire enum lines 14-17, RouteClass::Delegate parses today at lines 19-23 & 351-359, `price: Option<RoutePrice>` line 163-164 for quota-metered routes), `crates/nh-fleet/src/lib.rs:1318` (fleet bans delegate routes), `NOSIS_HARNESS_Master_Plan.md` Appendices A/B (provider access reality, verified 2026-07-11).

Key reality: we hold API keys ONLY for Kimi, MiMo, DeepSeek. Subscriptions (no API credits) exist for Claude, Codex/ChatGPT, Gemini. Nothing for GLM.

---

## 1. GLM / Z.ai — highest-value key we don't hold

### Current lineup & pricing (verified 2026-07-17, docs.z.ai/guides/overview/pricing)

| Model | Input | Output | Cached | Notes |
|---|---|---|---|---|
| GLM-5.2 | $1.40 | $4.40 | $0.26 | flagship, 1M ctx, MIT license, released 2026-06-16 |
| GLM-5-Turbo | $1.20 | $4.00 | $0.24 | speed-tuned orchestration route |
| GLM-4.7 | $0.60 | $2.20 | $0.11 | prior-gen coding workhorse |
| **GLM-4.7-FlashX** | **$0.07** | **$0.40** | **$0.01** | paid speed-serving of flash — absurdly cheap, NOT in our catalog |
| GLM-4.5-Air | $0.20 | $1.10 | $0.03 | cheap mid-tier, NOT in our catalog |
| GLM-4.7-Flash | FREE | FREE | FREE | already in catalog |
| GLM-4.6V-Flash | FREE | FREE | FREE | free VISION, already in catalog |
| GLM-4.5-Flash | FREE | FREE | FREE | already in catalog |

Cached-input storage is "Limited-time Free" across all paid models (i.e. no storage fee on top of the $0.26/M cache-hit rate — but this can end; `valid_until` discipline applies).

### Quality claim quantified
- Analysts: GLM-5.2 "challenges Anthropic and OpenAI with lower costs" (cryptobriefing.com/zai-glm-5-2-challenges-anthropic-openai/). Digitalapplied's July-2026 comparison: GLM-5.2 list price is ~3.6x cheaper on input and ~5.7x cheaper on output than Claude Opus 4.8 ($5/$25). The Master Plan's "near-Anthropic at ~1/4 cost" claim holds on July-2026 numbers: $1.40 vs $5 input (3.6x), $4.40 vs $25 output (5.7x); blended agent workloads (output-heavy) land near 1/4–1/5 of Opus cost.

### Anthropic-wire compatibility — the standout fact
Z.ai is **the only provider besides DeepSeek in our universe that ships a first-party Anthropic-Messages-compatible endpoint**: `https://api.z.ai/api/anthropic` (docs.z.ai/scenario-example/develop-tools/claude; claudelog.com/faqs/how-to-use-z-ai-in-claude-code/). Claude Code drop-in support shipped day-one for GLM-5.x. This is the deepclaude-proven agent-loop path (plan A.1) — our existing `wire = "anthropic"` adapter should work with a catalog-only TOML entry:

```toml
[routes."glm-5.2-anthropic"]
provider = "glm"
model_id = "glm-5.2"
base_url = "https://api.z.ai/api/anthropic"
wire = "anthropic"
vault_entry = "glm"
class = "api"
...same price block as glm-5.2
```

### Onboarding path (smallest)
1. Register on Z.ai / bigmodel.cn → 20M free tokens (plan A.4) → put key in nh-vault as `glm`.
2. Zero code change: 4 routes already in catalog resolve immediately; free flash routes become the $0 CI/smoke lane (plan B.4) — the test suite burns $0.
3. Catalog-only additions: `glm-5.2-anthropic` (Anthropic wire), `glm-4.7-flashx` ($0.07/$0.40 — cheaper than every paid route we have except MiMo V2.5), optionally `glm-4.5-air`.

### Trap re-confirmed
GLM Coding Plan ($18+/mo) is strictly limited to Z.ai-supported tools (Claude Code, Cline...); Nosis would NOT qualify — calls from an unsupported harness bill as normal API (hboon.com/using-z-ai-with-claude-code-for-cheaper/; gist by conradcaffier03). Pay-per-token or free tier only. Also: OpenRouter GLM hosts vary output caps (32K vs 128K direct) and quantization — prefer direct api.z.ai.

**Verdict: worth the key — it's free to obtain, unlocks the already-catalogued $0 lanes, a free vision route, and the only third Anthropic-wire provider. keyRequired=GLM/Z.ai (free registration).**

---

## 2. Claude / Anthropic — delegate now, API route only if a workload proves it

### July-2026 lineup (claude-api skill reference, cached 2026-06; cross-checked platform.claude.com/docs/en/about-claude/pricing + tldl.io + benchlm.ai July-2026 pages)

| Model | ID | Ctx | $/M in / out | Role for Nosis |
|---|---|---|---|---|
| Claude Fable 5 | `claude-fable-5` | 1M | $10 / $50 | overkill; always-on thinking; 30-day-retention required |
| Claude Opus 4.8 | `claude-opus-4-8` | 1M | $5 / $25 | our AGENTS.md gatekeeper (review/gate) |
| Claude Sonnet 5 | (intro) | 1M | $2 / $10 through 2026-08-31, then $3/$15 | pre-review triage |
| Claude Sonnet 4.6 | `claude-sonnet-4-6` | 1M | $3 / $15 | mid delegate |
| Claude Haiku 4.5 | `claude-haiku-4-5` | 200K | $1 / $5 | receipts, commit messages |

Prompt caching: reads ~0.1x input price, writes 1.25x (5-min TTL) or 2x (1-hr TTL); batch API 50% off. Native Anthropic Messages wire — zero adapter work if credits are ever bought (catalog-only onboarding, `keyRequired=Anthropic` for that path).

### Access reality
We hold a subscription, no API credits. Two zero-new-spend paths:
1. **Delegate via Claude Code headless** (`claude -p`, JSON output) — the planned M4/M5 `class = "delegate"` route; the schema parses this today and the commented `[routes.claude-opus-4-8]` stub exists in catalog.toml lines 355-363. Quota is shared across Claude surfaces (plan A.5) → the Cost HUD delegate panel must show remaining-window estimates.
2. **`ant auth login` OAuth profile** — the Anthropic CLI mints short-lived OAuth Bearer tokens (`ant auth print-credentials --access-token`) usable on `/v1/messages` with the `anthropic-beta: oauth-2025-04-20` header. Technically this is our native Anthropic wire speaking under the subscription — but it is designed for the account holder's own tooling; treat as an experiment behind a flag, not a routed lane, and verify plan-quota accounting before relying on it.

**Verdict: no new key needed for the delegate (OAuth lives in the child CLI); direct API route is a catalog-only TOML away *if* credits are bought — flag keyRequired=Anthropic for that variant. Delegate adapter is the code work (M4/M5 seam: uncomment catalog stub + child-process adapter in nh-tools/nh-cli; nh-fleet already refuses delegates safely at lib.rs:1318).**

---

## 3. Codex / OpenAI — cleanest delegate; direct API blocked by 3rd-wire problem

### July-2026 facts
- GPT-5.6 family GA July 9, 2026: Sol $5/$30, Terra $2.50/$15, Luna $1/$6 per M (aipricing.guru/openai-pricing/; developers.openai.com/api/docs/pricing). 1.05M ctx / 128K out. Cached input −90%, cache writes 1.25x, 30-min minimum cache life.
- `codex exec` is the sanctioned headless mode: streams progress to stderr, final message to stdout; `--json` emits a JSONL event stream (thread.started, turn.completed, item.*, errors); `--output-schema` constrains the final answer to a JSON Schema (developers.openai.com/codex/noninteractive; developersdigest.tech/blog/codex-exec-ci-headless-guide). This maps beautifully onto Nosis receipts.
- Quota: ChatGPT-plan auth draws from the same 5-hour rolling window as interactive sessions — "a CI loop can quietly exhaust the window you wanted for your afternoon coding" (learn.chatgpt.com/docs/non-interactive-mode). Dual-unit Cost HUD is mandatory, and the router must treat Codex delegate as quota-scarce.
- Direct API caveat: Codex/GPT-5.6 is served through the **Responses API**; Chat Completions is not deprecated globally (developers.openai.com/api/docs/deprecations) but the Codex surface has moved. A direct GPT-5.6 API route might not be reachable over our plain OpenAI-chat wire — adding a Responses-API dialect would be a THIRD wire and violates the 2-wire rule. Keep OpenAI as delegate-only unless/until a Chat-Completions-served model is confirmed.

**Verdict: keyRequired=none for the delegate (OAuth in codex CLI). Delegate adapter should capture `--json` events into the receipt and parse `--output-schema` outputs. Do NOT plan a direct OpenAI API route (2-wire rule).**

---

## 4. Gemini / Google — one good surprise (OpenAI-compat endpoint), one confirmed mess (Antigravity headless)

### Pricing (July 2026; benchlm.ai/blog/posts/gemini-api-pricing, ai.google.dev/gemini-api/docs/pricing, tokenmix.ai)
| Model | $/M in / out | Notes |
|---|---|---|
| Gemini 3.1 Pro | $2 / $12 (→ $4 / $18 above 200K ctx) | frontier; Deep Think Ultra-gated |
| Gemini 3.5 Flash | $1.50 / $9 (cache hit $0.15) | ~192 tok/s class |
| Gemini 3 Flash | $0.50 / $3 | fast triage |
| Flash-Lite | $0.25 / $1.50 | high-volume |
All tiers 1M ctx; Batch mode 50% off.

### Wire compatibility — better than the Master Plan assumed
Google ships a **first-party OpenAI-compatible endpoint**: `https://generativelanguage.googleapis.com/v1beta/openai/` — chat completions, streaming, function calling, structured outputs, image/audio understanding, and `reasoning_effort` all supported (still labeled beta) (ai.google.dev/gemini-api/docs/openai). So a *paid* Gemini route is catalog-only TOML on our existing OpenAI adapter — no code. `keyRequired=Google` (we have no API credits; paid-only for 3.1 Pro).

Two schema gaps it would expose (small, honest-cost-rule fixes):
1. **Context-tiered pricing** — 3.1 Pro doubles above 200K input; our `RoutePrice` is flat. Needs an optional `[price.long_context] threshold/multiplier` block (mirrors the existing `price.peak` pattern in nh-routes).
2. **Cache-write premium** — Gemini/Anthropic/OpenAI all bill cache *writes* (1.25–2x); our schema has only `cache_hit`/`cache_miss`. A `cache_write` multiplier field would make the KV-cache-first router honest for any of these providers.

### Antigravity CLI (subscription path) — confirm best-effort, defer the delegate
Gemini CLI was sunset for individual Pro/Ultra users (developers.googleblog.com/an-important-update-transitioning-gemini-cli-to-antigravity-cli/); Antigravity (`agy`) is the replacement. July-2026 field reports: headless requires `--headless --approve all` (auto-approves writes/exec — sandbox only), `agy -p` can **drop stdout under a pipe/subprocess**, users report headless runs that "time out or run forever", there is **no `--model` flag** (auto-selects, defaults 3.5 Flash), and `GEMINI_API_KEY` is ignored in favor of `agy auth login` / `ANTIGRAVITY_TOKEN` (aibuilderclub.com/blog/antigravity-cli-guide; botmonster.com/coding/gemini-cli-dead-migrate-antigravity-cli-2026/). On Windows-first + auditable-receipts requirements, this is a poor delegate. Recommendation: mark Gemini `best-effort`, do NOT build the Antigravity delegate in M4/M5; revisit only for the Search-grounding research niche, or take the paid OpenAI-compat API route when a workload justifies a Google key.

---

## 5. Ranked catalog impact summary

| Add | Class | Wire | Onboarding | Key | Verdict |
|---|---|---|---|---|---|
| GLM free flash x3 (already catalogued) | api | openai | key only | GLM (free reg.) | DO FIRST — $0 CI + free vision |
| glm-5.2-anthropic | api | anthropic | catalog-only | GLM | 2nd Anthropic-wire agent-loop provider |
| glm-4.7-flashx | api | openai | catalog-only | GLM | $0.07/$0.40 — cheapest paid text route we'd own |
| claude-opus-4-8 delegate | delegate | anthropic | M4/M5 adapter (stub exists) | none | review/gate; quota-scarce |
| gpt-5.6 delegate (`codex exec --json`) | delegate | openai | M4/M5 adapter (stub exists) | none | implementer bursts; 5h shared window |
| gemini-3-flash / 3.1-pro via OpenAI-compat | api | openai | catalog-only + 2 small schema fields | Google | defer until workload proves it |
| anthropic direct API (haiku/sonnet/opus) | api | anthropic | catalog-only | Anthropic | only if credits bought; caching aligns with KV-first engine |
| Antigravity delegate | delegate | n/a | code (fragile) | none | DEFER — headless unreliable |

Sources are cited inline above; consolidated list in the structured output.
