# Lens J — Reliability & Graceful Degradation ("never lose your work; it just works")
Research pass for NOSIS HARNESS · 2026-07-17 · repo HEAD ebea709

## 0. Repo grounding — what exists today (verified by reading code)

- **No retry, no backoff, no failover anywhere in the wire layer.** `nh-core/src/lib.rs:205-217` (`provider_http_error`) turns HTTP 429 into a single friendly string: `" — rate limited; retry later"` — the *user* is the retry loop. Timeouts say `"retry, or switch to another route"` (`nh-core/src/lib.rs:41`). `nh chat` catches the error and "the session keeps going" (`nh-cli/src/cmd_chat.rs:232-233`), which is graceful but manual.
- **Interactive sessions live only in RAM.** `ChatSession.history: Vec<ChatMessage>` (`nh-cli/src/cmd_chat.rs:30-40`). A terminal crash, a Windows renderer bug, or a laptop reboot loses the whole conversation — exactly the "context loss" pain differentiator #6 promises to fix. Only per-turn `Receipt`s reach disk (`.nosis/receipts.jsonl`, `nh-core/src/lib.rs:1047-1071`).
- **The fleet already solved durable resume** with an append-only JSONL ledger + pure fold functions: `LedgerEvent` (`nh-fleet/src/lib.rs:186-241`), `plan_from_ledger` (`:250`), `ladder_position` (`:311`), `resume` (`:455`). Heartbeats are explicitly best-effort ("resume correctness never depends on it", `:230`).
- **The escalation ladder is quality-driven, not availability-driven.** `Ladder::default_ladder()` = Flash→K2.7→Pro-High→Pro-Max (`nh-fleet/src/lib.rs:91-113`); `next_step` retries the same tier twice then escalates (`:124-133`). Nothing reacts to *provider down / 429 / timeout* as a distinct condition — `Outcome::Timeout` and `Outcome::Fail` are treated identically to a verification failure.
- **The resolver is the single mint** (`nh-routes/src/lib.rs:471-605`) with clock-priced quotes (`price_at`, `:173`) and a `provider_default` "cheapest priced api route" rule (`:553`) — i.e., the "cheapest capable" comparison machinery a fallback chain needs already exists.
- **The plan already reserves the local lane**: "a generic OpenAI-compatible route for LiteLLM/Ollama" (Master Plan line 14) and notes DSpark/DeepSpec + "gpt-oss-20b is a realistic local route on the Predator's RTX 5070 Ti" (Master Plan lines 30, 254, 429). `catalog.toml` has no local route yet.
- Providers do fail: DeepSeek had a 7h13m outage on 2026-03-30 (Reuters via downforai.com/deepseek) and enforces concurrency caps (429 over 500 concurrent on v4-pro; chat-deep.ai/docs/api-rate-limits/). GLM free routes are "rate-limited (limits unpublished) and can change" (catalog.toml lines 265-266).

## 1. Current (2026) practice — what the industry converged on

**Gateway-style fallback routing.** LiteLLM's production pattern: `num_retries` per deployment with exponential backoff → sequential fallback chain across model groups; unhealthy deployments enter *cooldown* after `allowed_fails` (default 3) failures/min for `cooldown_time` (default 30 s); error-specific retry policies (RateLimit vs Timeout vs Auth); `enable_pre_call_checks` rejects context-window-exceeding requests *before* sending ([docs.litellm.ai/docs/proxy/reliability](https://docs.litellm.ai/docs/proxy/reliability), [docs.litellm.ai/docs/routing](https://docs.litellm.ai/docs/routing)).

**OpenRouter's two-layer failover.** Provider-layer failover is on by default (5xx/timeout/429 → next provider of the same model); model-layer fallbacks are an opt-in priority-ordered `models` array whose last entry is "your reliability floor". Providers that errored in the last 30 s are deprioritized automatically; you "pay only for the successful run" (zero-completion insurance) ([openrouter.ai/blog/insights/reliability-failover/](https://openrouter.ai/blog/insights/reliability-failover/), [openrouter.ai/docs/guides/routing/provider-selection](https://openrouter.ai/docs/guides/routing/provider-selection)).

**Retry discipline.** 2026 consensus: exponential backoff **with jitter** (thundering-herd prevention), **honor `Retry-After`** when present, cap total retries, and treat 429/5xx/timeout as retryable but 401/403/400 as terminal; circuit-breaker consensus ≈ 5 failures to trip, 60 s cooldown ([getmaxim.ai — retries/fallbacks/circuit breakers](https://www.getmaxim.ai/articles/retries-fallbacks-and-circuit-breakers-in-llm-apps-a-production-guide/), [getmaxim.ai — handle 429](https://www.getmaxim.ai/articles/handle-429-errors-in-production-llm-applications/), [fast.io/resources/ai-agent-retry-patterns/](https://fast.io/resources/ai-agent-retry-patterns/), [truefoundry.com — LLM failover](https://www.truefoundry.com/blog/llm-failover-load-balancing-provider-outages)).

**Durable interactive sessions.** Claude Code persists every conversation as append-only JSONL under `~/.claude/projects/<project>/<session-id>.jsonl`; "crash recovery is built in — since each line is independently valid, a crash mid-write only loses the last partial line". Documented weakness: its parser *refuses to resume* from a corrupt last record ([code.claude.com/docs/en/sessions](https://code.claude.com/docs/en/sessions), [claude-world.com/tutorials/s16-session-storage/](https://claude-world.com/tutorials/s16-session-storage/), [neural-llm.com — recovering sessions mid-task](https://neural-llm.com/blog/guides/recovering-claude-code-sessions-mid-task)).

**Local inference floor.** Ollama's OpenAI-compatible endpoint (`http://localhost:11434/v1`) now covers chat completions, streaming, tool calling, and structured outputs ([docs.ollama.com/api/openai-compatibility](https://docs.ollama.com/api/openai-compatibility), [ollama.com/blog/tool-support](https://ollama.com/blog/tool-support)). `gpt-oss:20b` (MXFP4, ~14 GB) fits a 16 GB RTX 5070 Ti with ~98 tok/s — Blackwell runs 4-bit microscaling float natively ([modelfit.io/gpu/rtx-5070-ti/](https://modelfit.io/gpu/rtx-5070-ti/), [toolhalla.ai — RTX 50-series guide](https://toolhalla.ai/blog/best-local-llms-rtx-50-series-gpu-2026)). Speculative decoding: DeepSeek open-sourced **DSpark** (2026-06-27; 60–85% faster V4 generation, confidence-scheduled verification) and MIT-licensed **DeepSpec** for training drafters ([marktechpost.com 2026-06-27](https://www.marktechpost.com/2026/06/27/deepseek-releases-dspark-a-speculative-decoding-framework-that-accelerates-deepseek-v4-per-user-generation-60-85-over-mtp-1/), [deepseek.ai/blog/deepseek-dspark-speculative-decoding](https://deepseek.ai/blog/deepseek-dspark-speculative-decoding)); locally, llama.cpp `--model-draft` and Ollama 5.x `OLLAMA_SPECULATIVE_DECODE=1` give 1.8–2.5x on code (acceptance >80% on code patterns) ([github.com/ggml-org/llama.cpp/blob/master/docs/speculative.md](https://github.com/ggml-org/llama.cpp/blob/master/docs/speculative.md), [vucense.com — speculative decoding 2026](https://vucense.com/dev-corner/speculative-decoding-explained-2x-faster-local-llms-ollama-llama-cpp-2026/)).

## 2. Findings (ranked)

### F1 — Availability re-resolve: "next cheapest capable route" as the harness's second ladder axis (HIGH / M)
Today a dead provider ends the turn with a string error. The cohesive move: when retries exhaust, the RouteResolver — the only mint — re-resolves to the *next cheapest capable* route (same modality ⊇ task needs, context ≥ estimated tokens, class=api, not in cooldown, priced by `price_at(now)`), and the turn continues on the same history. This is OpenRouter's model-array fallback ([openrouter.ai/blog/insights/reliability-failover/](https://openrouter.ai/blog/insights/reliability-failover/)) and LiteLLM's fallback chains ([docs.litellm.ai/docs/proxy/reliability](https://docs.litellm.ai/docs/proxy/reliability)) — but NOSIS's version is *better because it's price-aware by clock*: at Beijing peak the fallback order itself changes. It's the product thesis ("always the cheapest capable route") applied to the failure domain — maximum cohesion, no new concepts.
**Seam:** `nh-routes`: add `RouteResolver::fallback_chain(&self, from: &ResolvedRoute, needs: &Needs, at: DateTime<Utc>) -> Vec<ResolvedRoute>` — a pure function ordering capable routes by blended `price_at(at)` (the comparison logic in `provider_default`, lib.rs:553-580, generalizes). `nh-fleet`: extend `Step` (lib.rs:116-121) with `Step::Reroute(route_id)`, emitted by `next_step` when the receipt's failure is availability-class, and a `TaskRerouted` ledger event mirroring `TaskEscalated` (lib.rs:212-217). `nh chat`: on wire failure, offer/perform the same re-resolve via the existing `switch_to` (cmd_chat.rs:275) which already preserves history across switches — the M1 exit criterion becomes the failover mechanism for free.
**MVP:** chain over api routes only, triggered by the typed availability errors of F2; delegate routes and cross-wire replay quirks excluded.
**LAW:** congruent (reuses resolver + ladder + switch_to), auditable (ledger event per reroute), simple (pure ordering function). Tension: silent rerouting could surprise — resolved by F6's mandatory announcement.

### F2 — Resilient wire client: typed RouteError + jittered backoff honoring Retry-After (HIGH / S)
Foundation for everything else. Replace stringly failures with a small enum at the two wire clients: `RateLimited { retry_after: Option<Duration> }`, `Overloaded` (5xx), `TimedOut`, `AuthRejected`, `ContextExceeded`, `Other`. Policy (2026 consensus): retry `RateLimited`/`Overloaded`/`TimedOut` up to 2 times with exponential backoff + full jitter, honoring `Retry-After` when the provider sends it; never retry `AuthRejected` ([getmaxim.ai — handle 429](https://www.getmaxim.ai/articles/handle-429-errors-in-production-llm-applications/), [fast.io retry patterns](https://fast.io/resources/ai-agent-retry-patterns/), [staskoltsov.medium.com — client-side 429](https://staskoltsov.medium.com/handling-http-429-too-many-requests-on-the-client-side-from-exponential-backoff-to-internal-4401d4345322)). LiteLLM ships error-specific retry policies for exactly this taxonomy ([docs.litellm.ai/docs/proxy/reliability](https://docs.litellm.ai/docs/proxy/reliability)). This also fixes a latent ladder bug: today `Outcome::Timeout` from a *provider outage* climbs the quality ladder to pricier tiers (nh-fleet lib.rs:124-133) — burning money on a network problem. Availability failures should reroute sideways (F1), not escalate up.
**Seam:** `nh-core` wire module — `provider_http_error` (lib.rs:205-217) and `send_error` (lib.rs:35) become constructors of `RouteError`; retry loop wraps the two `send()` sites; parse `Retry-After` header. Friendly strings stay as `Display` — zero UX regression.
**LAW:** small (one enum + one loop), readable, secure (401 never retried, key never logged — scrubber already in place).

### F3 — One resume story: crash-safe session ledger + a single `nh resume` verb (HIGH / M)
"Never lose your work" is currently true for fleet runs and false for the surface people actually live in. Persist `nh chat`/`nh tui` history the way the fleet already persists runs: append each `ChatMessage` (+ route switches, + usage) as one scrubbed JSONL line to `.nosis/sessions/<session-id>.jsonl`; resume = fold lines (the exact `plan_from_ledger` idiom, nh-fleet lib.rs:250-286). Claude Code proved the format — and documented the failure NOSIS should beat: its parser refuses to resume on a torn last record ([claude-world.com/tutorials/s16-session-storage/](https://claude-world.com/tutorials/s16-session-storage/), [neural-llm.com guide](https://neural-llm.com/blog/guides/recovering-claude-code-sessions-mid-task)); NOSIS's fold simply drops the one malformed trailing line and says so. Then unify the verb: `nh resume` lists interrupted *sessions and fleet runs* from their ledgers and continues either — one mental model, one word, both ledgers. Windows-first bonus: crash-of-the-terminal is precisely the documented Windows CLI pain differentiator #6 targets (Master Plan line 180).
**Seam:** new ~150-line `session_ledger` module in `nh-cli` (or `nh-core::receipt`'s sibling), reusing `ReceiptWriter`'s append+scrub pattern (nh-core lib.rs:1047-1071); `ChatMessage` already derives Serialize (lib.rs:50). `preserve_reasoning` routes (Kimi/MiMo) get their reasoning blocks persisted for free — history *is* the ledger.
**LAW:** congruent (fleet pattern reused verbatim), auditable (the session becomes a receipt trail), secure (Scrubber before write — switched-away keys stay scrubbed, cmd_chat.rs:40).

### F4 — Provider cooldown state: a circuit breaker as data, not a daemon (HIGH / S)
LiteLLM cools deployments down after 3 fails/min for 30 s ([docs.litellm.ai/docs/routing](https://docs.litellm.ai/docs/routing)); OpenRouter deprioritizes providers that errored in the last 30 s ([openrouter.ai/docs/guides/routing/provider-selection](https://openrouter.ai/docs/guides/routing/provider-selection)); production consensus ≈ 5 failures / 60 s cooldown ([getmaxim.ai production guide](https://www.getmaxim.ai/articles/retries-fallbacks-and-circuit-breakers-in-llm-apps-a-production-guide/)). NOSIS needs no background health-checker (LAW-lightweight): a tiny `HealthState` map `{route_id → consecutive_fails, cooled_until}` updated from F2's typed errors, consulted by F1's `fallback_chain` ("skip routes in cooldown"), and appended to the ledger/receipts so every skip is auditable. In-memory per process; the fleet run and the chat session each own one. This prevents the worst failure smell — hammering a downed DeepSeek during its next 7-hour outage ([downforai.com/deepseek](https://downforai.com/deepseek)) while 429s pile up against the 500-concurrency cap ([chat-deep.ai/docs/api-rate-limits/](https://chat-deep.ai/docs/api-rate-limits/)).
**Seam:** ~60-line struct in `nh-routes` (pure, clock-injected — the fleet's `Clock` trait, nh-fleet lib.rs:61-71, already exists for testability); threaded as `&mut HealthState` through dispatch.
**LAW:** small, simple, auditable. Tension: none — it is literally a HashMap and two constants.

### F5 — The $0 always-available floor is pure catalog data: a `local` provider route (HIGH / S)
The most NOSIS-shaped finding: the offline lane requires **zero new wire code**. Ollama exposes OpenAI-compatible `/v1/chat/completions` with tool calling, streaming, and structured outputs ([docs.ollama.com/api/openai-compatibility](https://docs.ollama.com/api/openai-compatibility)); NOSIS's OpenAI client + `catalog.toml` schema already accept it. Add:

```toml
[routes."local-gpt-oss-20b"]
provider = "local"
model_id = "gpt-oss:20b"
base_url = "http://localhost:11434/v1"
wire = "openai"
vault_entry = "local"          # Ollama ignores the bearer token
class = "api"
modality = ["text"]
context = 131072
thinking_dialect = "none"
[routes."local-gpt-oss-20b".price]
currency = "USD"
unit = "per_million_tokens"
cache_hit = 0.0
cache_miss = 0.0
output = 0.0
price_confidence = "confirmed"  # it's your electricity
```

`gpt-oss:20b` (~14 GB MXFP4) fits the owner's RTX 5070 Ti 16 GB at ~98 tok/s ([modelfit.io/gpu/rtx-5070-ti/](https://modelfit.io/gpu/rtx-5070-ti/), [compute-market.com RTX 50 guide](https://www.compute-market.com/blog/best-local-llm-rtx-50-series-2026)) — a genuinely capable agent model, and the Master Plan already names it "a realistic local route" (line 429) and promises "a generic OpenAI-compatible route for LiteLLM/Ollama" (line 14). Product effect: it is the guaranteed **last entry of every F1 fallback chain** — OpenRouter calls the last array entry "your reliability floor" ([openrouter.ai blog](https://openrouter.ai/blog/insights/reliability-failover/)) — making "NOSIS never fully goes down" a true sentence in the launch post, and it joins the free-GLM $0 CI lane as a second zero-cost tier (cohesion with differentiator #1's cost story). Needs: a one-line vault special-case ("local" entries skip key requirement, or accept any placeholder) and an unreachable-localhost error that says `ollama serve` + `ollama pull gpt-oss:20b`.
**LAW:** small (data, not code — the catalog-is-data hard rule is *proven* by this feature), harmonic ($0 floor completes the price spectrum), safe (offline = no data leaves the machine). keyRequired: none.

### F6 — Honest degraded-mode UX: every reroute announced, `/health` visible, cost of failure counted (MED-HIGH / S)
The trust contract: NOSIS may auto-reroute (F1) but never silently. Three small surfaces: (a) a one-line announcement in the existing footer idiom (cmd_chat.rs:346-352): `⚠ kimi-k2.7-code unreachable (2 retries) → deepseek-v4-flash off-peak · history intact`; the TUI semáforo gains one state (degraded/amber). (b) `/health` command printing the F4 cooldown table — provider, state, cooled-until, last error class — the CLI answer to OpenRouter's live uptime widgets ([openrouter.ai/docs/guides/routing/provider-selection](https://openrouter.ai/docs/guides/routing/provider-selection)). (c) honest failure accounting: failed-attempt tokens are still spent money (OpenRouter's caveat: some 429 paths consume credits despite zero-completion insurance — [openrouter.ai blog](https://openrouter.ai/blog/insights/reliability-failover/)); receipts already carry `usage` (nh-core lib.rs:1033-1045), so the cost HUD shows `this task: $0.014 (incl. 1 failed attempt $0.003)`. This *is* differentiator #6 (ambiguous status, cost opacity) extended into the failure domain, and it operationalizes FAILURE_MODES.md's only rule: "Fail visibly, safely, and with enough context to recover" (02-architecture/FAILURE_MODES.md:19).
**Seam:** `nh-cli` footer + one TUI chip + a `/health` handler over F4's map. **LAW:** honest-cost congruence; readable.

### F7 — Pre-flight capability check: reroute before the request, not after the error (MED / S)
LiteLLM's `enable_pre_call_checks` rejects context-window violations *before* sending ([docs.litellm.ai/docs/proxy/reliability](https://docs.litellm.ai/docs/proxy/reliability)); OpenRouter's model-layer fallback covers context-length validation errors ([openrouter.ai blog](https://openrouter.ai/blog/insights/reliability-failover/)). NOSIS already estimates input tokens every turn for compaction (`COMPACT_AT`, nh-core lib.rs:1082-1085, 1154+) and every route carries `context`/`max_out` (nh-routes lib.rs:156-157) — so a 5-line check can catch "this history no longer fits kimi-k2.6's 262 K" at dispatch and hand F1 a `ContextExceeded` *without a paid failed call*. Same pre-flight slot cheaply verifies modality (image task on a text-only route) — turning modality-aware dispatch (differentiator #2) from a routing preference into a guarantee.
**Seam:** one function in `nh-core::agent` before the `send()`; errors feed the same F1/F2 path. **LAW:** small, congruent (reuses the compaction estimator).

### F8 — DSpark-inspired speed floor: speculative decoding on the local lane (LOW-MED / S, optional)
DeepSeek's hosted API already gives DSpark speedups "for free" (Master Plan line 254; [marktechpost.com](https://www.marktechpost.com/2026/06/27/deepseek-releases-dspark-a-speculative-decoding-framework-that-accelerates-deepseek-v4-per-user-generation-60-85-over-mtp-1/)). For F5's local floor, the same idea costs one documented env var: Ollama 5.x `OLLAMA_SPECULATIVE_DECODE=1`, or llama.cpp `--model-draft` with a small drafter — 1.8–2.5x on code workloads, >80% draft acceptance on code patterns ([vucense.com](https://vucense.com/dev-corner/speculative-decoding-explained-2x-faster-local-llms-ollama-llama-cpp-2026/), [llama.cpp speculative.md](https://github.com/ggml-org/llama.cpp/blob/master/docs/speculative.md), [unsloth.ai/docs — speculative decoding](https://unsloth.ai/docs/basics/inference-and-deployment/saving-to-gguf/speculative-decoding)). Keep it as **setup documentation + an optional catalog comment**, not harness code — the backend owns decoding. Value: degraded mode that is $0 *and* not painfully slow, which is what makes people actually leave the local lane on.
**Seam:** `nh init` doctor-check note + docs; zero crates touched. **LAW:** lightweight by construction (drop-if-hard compliant).

## 3. Anti-recommendations (LAW screens)
- **No background health-check pinger/daemon** — passive failure-driven cooldown (F4) suffices; a pinger adds a process, wakeups, and false confidence (violates lightweight/simple).
- **No gateway dependency (don't route through LiteLLM proxy or OpenRouter)** — NOSIS *is* the router; adopting one hollows the core differentiator and adds a hop of cost/latency/keys. Steal their published policies (cooldown numbers, two-layer failover) as data, not their runtime.
- **No cross-provider mid-turn tool-call replay in MVP** — replaying a half-finished tool-call exchange onto a different wire/dialect (Kimi's preserve_reasoning, DeepSeek's empty-reasoning-on-tool-replay quirk, catalog.toml:42) is a correctness minefield; MVP reroutes at turn boundaries only. Document as the known limitation.

## 4. Cohesion summary
One sentence ties the lens together: **the RouteResolver's "cheapest capable" rule, applied to failure** — retries (F2) are the resolver refusing to give up on a price; reroutes (F1) are the resolver re-quoting the market; cooldowns (F4) are stale-route flags like stale-price flags; the local route (F5) is the catalog's zero lower bound; the session ledger (F3) is the fleet ledger grown to the chat surface; and the degraded-mode UX (F6) is the honest-cost rule speaking during a bad day. Nothing new is invented — every mechanism is an existing NOSIS idiom extended one notch, which is exactly what "congruent + harmonic" demands.

## Sources
- https://docs.litellm.ai/docs/proxy/reliability
- https://docs.litellm.ai/docs/routing
- https://openrouter.ai/blog/insights/reliability-failover/
- https://openrouter.ai/docs/guides/routing/provider-selection
- https://www.getmaxim.ai/articles/retries-fallbacks-and-circuit-breakers-in-llm-apps-a-production-guide/
- https://www.getmaxim.ai/articles/handle-429-errors-in-production-llm-applications/
- https://fast.io/resources/ai-agent-retry-patterns/
- https://www.truefoundry.com/blog/llm-failover-load-balancing-provider-outages
- https://staskoltsov.medium.com/handling-http-429-too-many-requests-on-the-client-side-from-exponential-backoff-to-internal-4401d4345322
- https://code.claude.com/docs/en/sessions
- https://claude-world.com/tutorials/s16-session-storage/
- https://neural-llm.com/blog/guides/recovering-claude-code-sessions-mid-task
- https://docs.ollama.com/api/openai-compatibility
- https://ollama.com/blog/tool-support
- https://modelfit.io/gpu/rtx-5070-ti/
- https://toolhalla.ai/blog/best-local-llms-rtx-50-series-gpu-2026
- https://www.compute-market.com/blog/best-local-llm-rtx-50-series-2026
- https://www.marktechpost.com/2026/06/27/deepseek-releases-dspark-a-speculative-decoding-framework-that-accelerates-deepseek-v4-per-user-generation-60-85-over-mtp-1/
- https://deepseek.ai/blog/deepseek-dspark-speculative-decoding
- https://github.com/ggml-org/llama.cpp/blob/master/docs/speculative.md
- https://vucense.com/dev-corner/speculative-decoding-explained-2x-faster-local-llms-ollama-llama-cpp-2026/
- https://unsloth.ai/docs/basics/inference-and-deployment/saving-to-gguf/speculative-decoding
- https://downforai.com/deepseek
- https://chat-deep.ai/docs/api-rate-limits/
- https://status.deepseek.com/
