# LENS H — Route Intelligence: The Router That Learns (2026-07-17)

Research pass for NOSIS HARNESS. Focus: making the single RouteResolver (nh-routes) smarter over time from OUTCOMES, while staying LAW-simple (local, file-based, no telemetry stack).

## 0. Repo grounding — what already exists (this lens builds on it, not beside it)

The striking discovery of this pass: **the raw material for a learning router is already committed.** Nothing needs a new data pipeline; the harness already emits every signal the 2026 routing literature says a learned router needs.

- `crates/nh-core/src/lib.rs:1016-1045` — `Receipt` already carries `outcome: Outcome` (Pass/Fail/Partial/Skip/Timeout), `failure_class: Option<FailureClass>` with **exactly deepset's taxonomy** (`Context, Constraint, Verification, Planning`), `usage` (prompt/completion/cached tokens), `turns`, `tool_calls`, `model_id`, `ts_utc`.
- `crates/nh-core/src/lib.rs:1048-1071` — `ReceiptWriter` appends scrubbed JSONL to `.nosis/receipts.jsonl`. Local, append-only, auditable. This IS the training log.
- `crates/nh-fleet/src/lib.rs:185-211` — the fleet ledger records `TaskQueued{route_id}`, `TaskStarted{route_id, effort, attempt}`, `TaskReceipt{receipt}` — so route↔outcome joins are already durable per attempt, including which ladder tier and thinking effort was in play.
- `crates/nh-fleet/src/lib.rs:90-133` — `Ladder::default_ladder()` is a **hard-coded static list** (flash → k2.7 → v4-pro High → v4-pro Max) and `next_step()` decides Retry/Escalate/Gate purely from `Outcome` + attempt count. It ignores `failure_class` entirely — the taxonomy is captured but never consulted. This is the seam.
- `crates/nh-routes/src/lib.rs:471-605` — `RouteResolver` picks by static catalog data only (price, modality, dialect, clock via `price_at`). `price_at()`/`peak_status()` (lines 173-228) already produce everything an "explain" needs: peak flag, multiplier, end-of-window, price confidence, staleness.
- `crates/nh-mcp/src/lib.rs:221-289` — nh-mcp exposes `route_resolve`, `fleet_run`, `fleet_status`; adding a read-only stats tool is a 1-arm extension of an existing match.
- `NOSIS_HARNESS_Master_Plan.md:185` — Cost HUD already promises "projected cost-to-goal (rolling estimate from tokens/turn × remaining plan items)" — pre-run forecasting is *already congruent* with the spec, it just has no data source yet. Line 190 explicitly defers dashboards to v2 web — so all Lens-H surfaces must stay in the TUI/CLI.

Conclusion: Lens H is not "add a learning system"; it is **close the loop that the receipts/ledger already opened**. That's why every finding below is S/M effort.

## 1. Current-2026 landscape (citations)

### Learning routers
- **BaRP — "Learning to Route LLMs from Bandit Feedback: One Policy, Many Trade-offs"** (https://arxiv.org/abs/2510.07429, HTML https://arxiv.org/html/2510.07429v1): frames routing as a contextual bandit trained from *partial, per-call outcome feedback* (exactly what receipts are), with a **preference vector to dial cost/accuracy at inference time without retraining**. Lesson for nosis: the online-update-from-outcomes framing is right, but a neural policy is LAW-overkill; a smoothed empirical win-rate table gets most of the benefit at ~1% of the complexity.
- **"Dynamic Model Routing and Cascading for Efficient LLM Inference: A Survey"** (https://arxiv.org/pdf/2603.04445, 2026): surveys routing vs cascading; nosis's escalation ladder IS a cascade — the survey's key point is cascades dominate when weak-model verification is cheap, and cascade thresholds should be set from observed per-tier solve rates, not fixed.
- **OpenRouter Auto Router** (https://openrouter.ai/docs/guides/routing/routers/auto-router, https://openrouter.ai/blog/insights/model-routing/): powered by NotDiamond, exposes only *which* model was picked (`model` field) — the docs contain **no explanation of WHY**; selection criteria are explicitly vague ("prompt complexity, task type, model capabilities"), with a 0–10 cost/quality dial. This is the biggest incumbent transparency gap nosis can attack: routing exists everywhere in 2026, *explainable* routing does not.

### Router evaluation
- **RouterBench** (https://www.emergentmind.com/topics/routerbench-dataset): formalizes routing evaluation as points/curves in **2-D cost–quality space with a non-decreasing convex hull = the Pareto frontier**; 405k precomputed outcomes. The methodology (not the dataset) is what nosis should copy: every route = a (cost, quality) point per task class; dominated routes are visible mechanically.
- **LLMRouterBench** (https://arxiv.org/html/2601.07206v1, 2026): unified framework, confirms routing beats the single strongest model as the pool grows.
- **mini-swe-agent** (https://github.com/SWE-agent/mini-swe-agent): 100-line bash-only harness scoring >74% on SWE-bench Verified — proof that a *tiny* eval harness is credible in 2026; vals.ai tracks a bash-only slice as its own leaderboard (https://www.vals.ai/benchmarks/swebench).
- **Claw-SWE-Bench Lite** (https://arxiv.org/pdf/2606.12344, 2026): an 80-instance subset preserving rankings at 22.9% of full cost (Pass@1 0.643 vs 0.639) — evidence that a small, cheap, well-chosen task set ranks routes almost as faithfully as a big one. A nosis-local 10–20-task bench is methodologically defensible.
- **Terminal-Bench** (https://arxiv.org/pdf/2601.11868): CLI-native agent benchmark — the task format (command + verifiable check) is the right shape for `nh bench` tasks.

### Failure classification
- **deepset, "Harness Engineering: How to Build Reliable AI Agents by Engineering the System, Not the Model"** (https://www.deepset.ai/blog/harness-engineering, May 2026): the context/constraint/verification/planning failure taxonomy, with each failure mode mapped to a harness component and evidence that harness-only fixes move agents 20+ leaderboard positions. nh-core already encodes this enum verbatim — nosis is possibly the only harness with the taxonomy *in the wire format*.
- **AgentRx: Diagnosing AI Agent Failures from Execution Trajectories** (https://arxiv.org/pdf/2602.02475, 2026): trajectory-based failure diagnosis; supports the "cheap post-hoc classification from the transcript you already have" approach.
- **awesome-harness-engineering** (https://github.com/ai-boost/awesome-harness-engineering): 2026 index of evals/observability/orchestration patterns; confirms "harness engineering" is now the recognized discipline nosis is positioned in.

### Cost forecasting & observability
- **"How Do AI Agents Spend Your Money? Analyzing and Predicting Token Consumption in Agentic Coding Tasks"** (https://arxiv.org/abs/2604.22750, 2026): agentic coding uses ~3,500× the tokens of single-shot reasoning; consumption is stochastic with up to **30× variance across runs of the same task** — so honest forecasts must be *ranges* (median + p90), never point estimates. Agents' own pre-run token predictions are usable for budget alerts. This slots perfectly under the catalog's honest-cost rule (`PriceConfidence`, `stale` flags in nh-routes:74-141).
- **OpenTelemetry GenAI semantic conventions** (https://callsphere.ai/blog/vw3c-opentelemetry-genai-conventions-ai-agents-2026, https://techbytes.app/posts/opentelemetry-genai-agent-semconv-cheat-sheet-2026/): client spans exited experimental in early 2026; `gen_ai.client.token.usage` / `invoke_agent` / `execute_tool` are the stable core. nosis should NOT adopt an OTel stack (LAW: lightweight), but aligning receipt *field names* to `gen_ai.*` keys costs nothing and makes receipts ingestible by any OTLP tool later.
- **The Register, June 2026** (https://www.theregister.com/ai-and-ml/2026/06/24/ai-coding-agents-could-soon-cost-more-than-the-developers-using-them/5260864): agent-cost anxiety is mainstream; cost forecasting is a product wedge, not a nicety.

## 2. Findings (ranked)

### F1. Route Scorecard: fold `.nosis/receipts.jsonl` into per-(route, effort, task-class) stats — the keystone
**What:** A read-side aggregation, no new writer: `nh routes stats` folds receipts + fleet ledgers into a table keyed by `(route_id, thinking_effort, task_class)`: attempts, pass rate (Laplace-smoothed), Partial rate, median/p90 tokens, median latency, cache-hit %, failure-class histogram, and the single headline metric **cost-per-solved-task** = Σcost / Σpasses (in the route's currency, priced via `price_at` at each receipt's `ts_utc`). Optionally cache the fold in `.nosis/route_stats.json` with the source-file byte offset, so re-folds are incremental.
**Why keystone:** F2 (learning ladder), F3 (explain), F4 (forecast), F6 (bench), F8 (MCP) all read this one artifact. One derived file, five surfaces — that is cohesion by construction, and it is the RouterBench cost–quality methodology (https://www.emergentmind.com/topics/routerbench-dataset) reduced to a JSON file.
**Seam:** new small module `nh-routes::stats` (or `nh-core::receipt::fold`) — receipts already have everything (`nh-core/src/lib.rs:1034-1045`); the ledger joins route/effort/attempt (`nh-fleet/src/lib.rs:196-211`). Missing input: `route_id` on the Receipt itself (it has `model_id`; add `route_id` — 1 field) and `task_class` (see F5).
**LAW:** small (one fold function), auditable (derived from append-only sources, deletable/rebuildable at will), local (no telemetry). Effort M. Key: none.

### F2. Outcome-weighted escalation ladder: de-prefer routes that keep failing a task class
**What:** `Ladder::default_ladder()` (`nh-fleet/src/lib.rs:91-113`) stays as the *shape*, but tier order per task class is re-ranked by expected cost-per-solved-task from F1, with strict guardrails: (a) minimum 5 samples before any reorder (Laplace prior = catalog order); (b) exponential decay (half-life ~30 days) so a route recovers after provider fixes; (c) the Opus review gate and budget stop are NEVER learned away; (d) every reorder is written to the ledger as a `LadderAdjusted` event with the numbers that caused it — the learning is itself auditable. Add ε≈0.1 exploration (occasionally try the de-preferred route) so stats don't fossilize — this is the LAW-sized version of BaRP's bandit updating (https://arxiv.org/abs/2510.07429) and the survey's "cascade thresholds from observed per-tier solve rates" (https://arxiv.org/pdf/2603.04445).
**Why:** this is the actual "router that learns." E.g., if kimi-k2.7 keeps timing out on `test-fix` tasks, the fleet stops burning two attempts there before escalating. It compounds the cost differentiators: cheapest-capable becomes cheapest-*proven*-capable.
**Seam:** `Ladder::for_task_class(stats)` constructor + `next_step()` unchanged; fleet coordinator passes stats at run start (frozen per run — no mid-run drift, keeps runs reproducible).
**LAW tension & resolution:** "learning" risks un-auditable behavior → resolved by frozen-per-run stats, ledger-logged reorders, and deterministic math (no RNG in the ranking, ε-exploration seeded and logged). Effort M. Key: none.

### F3. `nh route explain` / TUI "why this route" chip — attack the incumbents' opacity
**What:** A `RouteDecision` struct: chosen route + per-candidate rejection reasons (`modality mismatch`, `peak 2x until 18:00`, `price stale since 2026-07-24`, `pass rate 41% on task-class refactor (12 samples)`, `no API key in vault`). Rendered three ways from one struct: `nh route explain <task>` CLI, a one-line TUI chip on dispatch ("deepseek-v4-flash — off-peak ¥0.4/Mtok-out, 92% pass on edits, cache-warm"), and embedded in the receipt so *post-hoc* audits show why the route was chosen.
**Why:** OpenRouter's Auto Router — the category leader — documents *no* explanation of why NotDiamond picks a model (https://openrouter.ai/docs/guides/routing/routers/auto-router); nosis's routing inputs are all already legible data (`price_at`, `peak_status`, modality flags, F1 stats). "The only router that shows its work" is a marketing-grade sentence that also serves the constitution-native differentiator (#7) and cost-opacity pain (#6).
**Seam:** nh-routes — the resolver already computes everything except stats; `peak_status()` (lib.rs:193-228) is the template for terse human strings. Effort S (without stats: trivially S; with F1 wired in: still S). Key: none.

### F4. Pre-run cost forecast: honest ranges + "wait for off-peak saves ¥X"
**What:** Before a run/fleet dispatch: `forecast = task_count × historical (median, p90) tokens for this task class × price_at(now)`, shown as a **range with sample count** ("est ¥3–¥11 across 8 similar tasks"), never a point estimate — because measured variance is up to 30× per task (https://arxiv.org/abs/2604.22750). Add the counterfactual the off-peak scheduler already implies: price the same run at the next off-peak window and print "defer 40min → save ~¥6 (2x peak ends 18:00 CST)". Cold start (no samples): show only the price quote + confidence and say "no history yet" — the honest-cost rule (`PriceConfidence`, `stale`) extended to forecasts.
**Why:** fulfills the Master Plan's promised "projected cost-to-goal" HUD chip (NOSIS_HARNESS_Master_Plan.md:185) with a real data source; kills the "rate-limit shock" pain; makes the off-peak differentiator *quantified before commit* rather than discovered after. 2026 context: 85% of companies miss AI cost forecasts by >10% (https://fluidattacks.com/blog/ai-token-economics-cost-control) and agent-cost anxiety is headline news (https://www.theregister.com/ai-and-ml/2026/06/24/ai-coding-agents-could-soon-cost-more-than-the-developers-using-them/5260864).
**Seam:** consumes F1 stats + existing `price_at`/`ready_to_dispatch` (`nh-fleet/src/lib.rs:75-77`); surfaces in `nh fleet run --dry-run` and the TUI cost HUD. Effort M. Key: none.

### F5. Task-class tag on every receipt (the 6-word classifier)
**What:** Add `task_class: Option<TaskClass>` to Receipt with a tiny closed enum (~6: `edit`, `refactor`, `test-fix`, `greenfield`, `review`, `ops`). Classification is a two-tier heuristic, zero ML: (1) keyword match on the task string; (2) fallback: dominant tool mix from the transcript (e.g. mostly `exec`+test commands → `test-fix`). Never blocks; `None` is fine.
**Why:** stats keyed only by route are misleading — routes fail *per task class* (deepset's whole point is failures are situational: https://www.deepset.ai/blog/harness-engineering). This one field turns F1/F2 from "global averages" into "route X is bad at Y," which is the actual intelligence. It is deliberately the smallest possible feature vector — the 2026 literature uses embeddings (PILOT, BaRP), but a 6-value enum is auditable and THE-LAW-readable.
**Seam:** `nh-core::receipt` + one function in the agent loop where receipts are minted. Effort S. Key: none.

### F6. `nh bench`: a 12-task local mini-bench that seeds the scorecard (cold-start killer)
**What:** A benchmark runner that is just a fleet run over a checked-in `bench/tasks.toml`: ~12 small, deterministic, repo-agnostic tasks (2 per task class), each with a machine check (Terminal-Bench-style command + expected result, https://arxiv.org/pdf/2601.11868). `nh bench --routes deepseek-v4-flash,kimi-k2.6` executes each task per route via the *existing* fleet (ledger, budget stop, receipts all come free) and prints the cost–quality table; receipts flow into F1 as priors. Run it off-peak via the existing scheduler; on free GLM routes it costs ¥0 once a GLM key exists (optional — runs fine on held DeepSeek/Kimi/MiMo keys off-peak).
**Why:** solves the F2 cold-start (no reordering until 5 samples — bench provides them in one evening), catches provider regressions/model-version drift ("kimi dropped 20 points on refactor since last month"), and validates catalog claims. Methodological cover: an 80-instance subset preserves full-benchmark rankings at 23% cost (https://arxiv.org/pdf/2606.12344), and a 100-line harness is a credible eval vehicle (https://github.com/SWE-agent/mini-swe-agent). 12 tasks is enough to rank 5 providers for one user's workload.
**Seam:** thin `nh-cli` subcommand over `nh-fleet` — the runner is ~a TaskSpec loader + check executor; NO new execution machinery. Effort M. Key: none (GLM key optional for free-tier runs).

### F7. Failure-class-aware next_step: escalate on Planning, repair on Context
**What:** `next_step()` (`nh-fleet/src/lib.rs:124-133`) currently treats all failures identically (retry ×2 → next tier → gate). Use the `failure_class` the receipt already carries: **Planning/Verification → escalate thinking effort/tier** (the model was too weak); **Context → retry same tier with compaction/fresh context** (a bigger model with the same overflowing context fails the same way — escalating wastes money); **Constraint → retry same tier once with the violated rule surfaced** (it's a harness/constitution problem, per deepset's failure→component mapping, https://www.deepset.ai/blog/harness-engineering; trajectory-diagnosis precedent: https://arxiv.org/pdf/2602.02475).
**Why:** this makes the ladder *diagnostic* instead of brute-force — the cheapest fix is picked per failure type, which directly cuts escalation spend and is a story no incumbent CLI tells ("nosis knows *why* it failed and fixes that, instead of just paying more"). It also finally *uses* the taxonomy field that is currently write-only.
**Seam:** extend `next_step(ladder, tier_idx, attempt, outcome, failure_class)` — a match-arm change plus one compaction hook; the enum exists (`nh-core/src/lib.rs:1026-1031`). Effort M. Key: none.

### F8. `route_stats` MCP tool: the fleet's memory becomes an orchestration primitive
**What:** Add a read-only `route_stats` tool to nh-mcp (next to `route_resolve`/`fleet_run`/`fleet_status`, `crates/nh-mcp/src/lib.rs:221-289`) returning the F1 scorecard (optionally filtered by task_class), plus fold the F3 explanation into `route_resolve`'s response.
**Why:** KORVIN (or any MCP orchestrator) currently gets a route with no evidence; with stats it can make budget decisions *across* harness instances. It also means the learning loop is observable by machines, not just the TUI — congruent with the "node in the orchestration layer" M4 goal (ROADMAP.md Phase 3), and it makes nh-mcp the only 2026 MCP server exposing *evidenced* model routing.
**Seam:** one match arm + serializing the already-computed F1 struct. Effort S. Key: none.

### F9 (out-justified). OTel GenAI-aligned receipt field names — future-proof, zero-stack
**What:** When touching Receipt for F1/F5, rename/alias serde keys to OpenTelemetry GenAI semconv vocabulary where a 1:1 mapping exists (`gen_ai.usage.input_tokens`, `gen_ai.usage.output_tokens`, `gen_ai.response.model`, operation duration), keeping the JSONL/local format — NO exporter, NO collector, NO OTLP dependency.
**Why out-justified:** not harness-functional today, but client-span semconv went stable in early 2026 and the ecosystem (Datadog, Langfuse, Greptime) ingests it natively (https://callsphere.ai/blog/vw3c-opentelemetry-genai-conventions-ai-agents-2026, https://techbytes.app/posts/opentelemetry-genai-agent-semconv-cheat-sheet-2026/, https://greptime.com/blogs/2026-05-09-opentelemetry-genai-semantic-conventions) — a serde-rename now means any future dashboard/v2 web view (Master Plan line 190) reads receipts unmodified. Costs a diff of ~10 lines if done during F1/F5; costs a migration if done later.
**LAW:** congruent (speaks the ecosystem's language), lightweight (no runtime change). Effort S. Key: none.

## 3. What NOT to build (LAW rejections)
- **A learned/neural router** (BaRP-style policy, NotDiamond embedding router): opaque, heavy, un-auditable — fails readable/auditable. The smoothed win-rate table captures the compounding value.
- **An OTel/Langfuse observability stack**: fails lightweight/local; receipts.jsonl + one fold function is the whole stack (F9 keeps the door open for free).
- **Full SWE-bench integration**: hours of compute and Docker on Windows; the 12-task local bench preserves the ranking signal (Claw-SWE-Bench Lite evidence) at ~zero cost.
- **Mid-run ladder mutation**: learning updates apply between runs only (frozen stats per run) — keeps every run reproducible from its ledger.

## 4. Cohesion story (one paragraph)
All nine findings are one loop: receipts (exists) → task-class tag (F5) → fold (F1) → three read surfaces — explain (F3), forecast (F4), MCP (F8) — and two decision surfaces — learned ladder (F2), failure-aware next_step (F7) — with the bench (F6) as the loop's ignition and OTel naming (F9) as its passport. No new daemon, no new store, no new protocol: the append-only files the harness already writes become the product's memory, and every differentiator (clock, cache, modality, thinking, honest cost, calm UX, constitution) becomes *visible and self-improving* through the same JSON. That is "cohesive product," not feature pile.

## Sources
- https://arxiv.org/abs/2510.07429 — BaRP: Learning to Route LLMs from Bandit Feedback
- https://arxiv.org/html/2510.07429v1 — BaRP HTML
- https://arxiv.org/pdf/2603.04445 — Dynamic Model Routing and Cascading survey (2026)
- https://www.deepset.ai/blog/harness-engineering — deepset failure taxonomy / harness engineering (May 2026)
- https://arxiv.org/pdf/2602.02475 — AgentRx: diagnosing agent failures from trajectories
- https://github.com/ai-boost/awesome-harness-engineering — harness engineering index
- https://github.com/SWE-agent/mini-swe-agent — 100-line agent, >74% SWE-bench Verified
- https://arxiv.org/pdf/2606.12344 — Claw-SWE-Bench Lite (80-instance ranking-preserving subset)
- https://arxiv.org/pdf/2601.11868 — Terminal-Bench
- https://www.vals.ai/benchmarks/swebench — bash-only harness leaderboard slice
- https://www.emergentmind.com/topics/routerbench-dataset — RouterBench cost–quality frontier methodology
- https://arxiv.org/html/2601.07206v1 — LLMRouterBench (2026)
- https://openrouter.ai/docs/guides/routing/routers/auto-router — Auto Router docs (opacity evidence)
- https://openrouter.ai/blog/insights/model-routing/ — OpenRouter routing blog
- https://arxiv.org/abs/2604.22750 — How Do AI Agents Spend Your Money? (token prediction, 30× variance)
- https://www.theregister.com/ai-and-ml/2026/06/24/ai-coding-agents-could-soon-cost-more-than-the-developers-using-them/5260864 — cost anxiety mainstream
- https://fluidattacks.com/blog/ai-token-economics-cost-control — forecast miss rates
- https://callsphere.ai/blog/vw3c-opentelemetry-genai-conventions-ai-agents-2026 — OTel GenAI conventions status 2026
- https://techbytes.app/posts/opentelemetry-genai-agent-semconv-cheat-sheet-2026/ — GenAI semconv cheat sheet
- https://greptime.com/blogs/2026-05-09-opentelemetry-genai-semantic-conventions — OTel GenAI tracing
