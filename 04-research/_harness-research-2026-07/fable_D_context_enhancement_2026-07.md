# LENS D — CONTEXT ENHANCEMENT: Findings for NOSIS HARNESS
**Date:** 2026-07-16/17 · **Analyst:** Fable 5 research pass · **Scope:** context engine (nh-core / possible nh-context)

---

## 0. Where the harness stands today (repo grounding)

The entire context engine currently lives in `crates/nh-core/src/lib.rs` (`agent` module):

- **Single-stage mechanical compaction** (`compact_history`, lib.rs:1332-1368): at `COMPACT_AT = 0.70` of `context_limit`, drop the smallest earlier prefix of messages (everything between `history[0]` and a user-turn boundary) to reach `COMPACT_TARGET = 0.50`, keeping the last `KEEP_RECENT = 2` user turns. A one-line elision note is **prepended into the first retained message's content** (lib.rs:1362-1365) — i.e., a previously-sent message is mutated.
- **All earlier user messages are deleted outright.** There is no summarization, no tool-result-only pruning, no offload to disk, and no record in receipts of what was elided (only a transient `on_event` line, lib.rs:1160-1164).
- **Byte-stable prefix** is asserted only via `debug_assert_eq!(message_bytes(&history[0]), prefix_bytes)` — debug builds only (lib.rs:1141-1152 etc.).
- **Token estimation fallback** `estimate_tokens` (lib.rs:1316-1328) counts `content` bytes + serialized `tool_calls` only. It does **not** count `reasoning_content` — yet Kimi K2.7 / MiMo routes have `preserve_reasoning = true` and replay full reasoning chains on the wire (lib.rs:283-298; Master Plan A.10.5). It also ignores the tool-spec block sent with every request.
- **Cache metric**: `cache_hit_pct` from `usage.prompt_tokens_details.cached_tokens` (OpenAI wire, lib.rs:362-375) or `cache_read_input_tokens` (Anthropic wire, lib.rs:551-558). DeepSeek's documented fields `prompt_cache_hit_tokens` / `prompt_cache_miss_tokens` are not parsed.
- **`nh chat` history is RAM-only** (`ChatSession.history`, crates/nh-cli/src/cmd_chat.rs:30-40); exit or crash = total context loss — the exact pain point differentiator 6 promises to fix.
- **Fleet jobs already isolate context** (fresh `history: Vec<ChatMessage>` per job, crates/nh-fleet/src/lib.rs:1246), but there is no in-session sub-agent primitive: an agent cannot fork an isolated sub-context and receive back a condensed summary.
- **No `nh-context` crate exists** yet (Master Plan §2 lists it; `crates/` has none) and no memory (retain/recall/reflect is planned as SQLite in ARCHITECTURE_OVERVIEW.md:11, Master Plan §2).
- **Catalog windows are huge**: `context = 1000000` for DeepSeek V4 and MiMo V2.5 routes (catalog.toml:38,66,94,122,228,250) — compaction currently arms at 700k tokens on those routes.
- CONTRACTS_M2.md §3.3 pins the current defaults and defers Anthropic-wire `cache_control` breakpoints (risk table row: "NOT in M2 … LATER hardening pass").

## 0.1 Current external facts (July 2026)

- **Claude Code's compaction** (the model the Master Plan cites as the five-stage reference) is now well documented from the March 31, 2026 source-map leak: a three-tier system — (1) *microcompaction*: hot/cold split of tool outputs, older results persisted to disk with path references (applies to Read/Bash/Grep/Glob/WebSearch/WebFetch/Edit/Write); (2) *auto-compaction*: headroom accounting (`effectiveWindow = contextWindow − max(maxOutput, 20k)`, threshold ≈ `effectiveWindow − 13k`), LLM 9-section structured summary that quotes key phrases verbatim; (3) manual `/compact` with focus hints; post-compaction it rehydrates the 5 most-recent files, todos, and a continuation instruction. Sources: https://decodeclaude.com/compaction-deep-dive/ ; https://karanprasad.com/blog/how-claude-code-actually-works-reverse-engineering-512k-lines ; https://oldeucryptoboi.com/blog/context-compaction-deep-dive/
- **Codex CLI vs Claude Code vs OpenCode compaction** (April 2026 comparison): Codex preserves **all user messages verbatim** and deletes assistant/tool messages, replacing history with a "handoff memo"; Claude Code trims tool results first (zero LLM cost) and keeps the message prefix stable to protect the prompt cache; OpenCode hides messages with a `compacted` timestamp (reversible), protects the most recent 40k tokens, only prunes when it can free >20k tokens, and replays the last user message after summarizing. Source: https://justin3go.com/en/posts/2026/04/09-context-compaction-in-codex-claude-code-and-opencode
- **Anthropic now ships server-side compaction** as an API feature (`context_management.edits: [{"type":"compact_20260112"}]`, beta `compact-2026-01-12`; default trigger 150k input tokens, min 50k; `pause_after_compaction`; usage reports per-iteration tokens). Anthropic-API-only — relevant as a design reference and as a delegate-route option, not for DeepSeek's Anthropic wire. Source: https://platform.claude.com/docs/en/build-with-claude/compaction
- **Anthropic context-engineering guidance**: compaction, structured note-taking (agentic memory persisted outside the window and re-pulled), and sub-agent architectures where subagents return 1,000–2,000-token condensed summaries. Sources: https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents ; https://platform.claude.com/cookbook/tool-use-context-engineering-context-engineering-tools
- **Context rot** (Chroma): 18 frontier models degrade measurably as input grows, *even on simple retrieval*, and degradation appears far below the window limit (significant degradation at ~50k on 200k-window models; distractors and middle-of-context placement make it worse). Source: https://www.trychroma.com/research/context-rot
- **Manus KV-cache lessons** (the canonical agent-builder reference): KV-cache hit rate is *the* production metric; stable prompt prefix (a single-token change invalidates everything after), **append-only context — never modify previous actions or observations**, deterministic serialization, explicit cache breakpoints where the provider needs them, mask tools rather than removing them. Source: https://manus.im/blog/Context-Engineering-for-AI-Agents-Lessons-from-Building-Manus
- **DeepSeek context caching**: on-disk, automatic, best-effort; cache **units** form at request boundaries and fixed intervals; a request hits only on a **full prefix-unit match**; cache cleared after hours-to-days idle; usage exposes `prompt_cache_hit_tokens` / `prompt_cache_miss_tokens`. Source: https://api-docs.deepseek.com/guides/kv_cache/ ; https://api-docs.deepseek.com/news/news0802/
- **Kimi/Moonshot caching**: automatic context caching cuts repeated-prefix input cost 80–87% across K2 family (K2.7 hit $0.19 vs $0.95 miss — matches catalog.toml:163-164). Sources: https://www.cometapi.com/kimi-k2-api-pricing/ ; https://benchlm.ai/moonshot/api-pricing
- **Memory research**: "Hindsight is 20/20: Building Agent Memory that Retains, Recalls, and Reflects" (arXiv:2512.12818) — the retain/recall/reflect interface the Master Plan already names; lifts LongMemEval accuracy 39%→83.6% with a 20B open model. Source: https://arxiv.org/abs/2512.12818
- **Multi-agent context isolation** is the consensus 2026 pattern: isolated sub-contexts returning structured 1–2k-token results; five context-quality criteria (relevance, sufficiency, isolation, economy, provenance). Sources: https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents ; https://arxiv.org/abs/2603.09619

---

## Findings (ranked)

### F1 — Staged compaction pipeline: make the five stages real, in one small module (HIGH / M-L)
Today one function does everything by deleting user intent — the exact "context loss" pain the harness exists to fix. Codex-CLI keeps user messages verbatim; Claude Code trims tool results before ever calling an LLM; NOSIS deletes both. Build `nh_core::context` (or a small `nh-context` crate per Master Plan §2) with an ordered `Vec<Stage>` pipeline; each stage is a pure `fn(&mut Vec<ChatMessage>, &Budget) -> Option<StageReport>`:
1. **budget-reduce** — clamp `max_out` and thinking effort when near threshold (cheap headroom; mirrors Claude Code headroom accounting).
2. **snip** — truncate any single oversized tool result at insertion time (F2).
3. **microcompact** — replace tool results older than the last N user turns with a one-line placeholder + on-disk reference (F2). Zero LLM cost; preserves every user/assistant message.
4. **context-collapse** — the existing prefix-drop, amended: **never delete user messages — fold them verbatim into the elision note** (Codex's proven rule), and insert the note as a *new* message instead of mutating `history[1]` (append-only rule, Manus).
5. **auto-compact** — only if 1–4 can't reach target: one LLM summary call routed via RouteResolver to the cheapest capable route (mimo-v2.5 $0.14/M in, catalog.toml:256-262, or deepseek-v4-flash off-peak), with a Claude-Code-style structured summary prompt ("quote key phrases verbatim; state, decisions, files touched, errors, next steps"), then rehydrate: last user message replayed (OpenCode pattern).
Each stage returns a `StageReport` that feeds F3 receipts. MVP = stages 3+4 amendments (mechanical, no new deps); stage 5 behind a config flag.
*Evidence:* https://justin3go.com/en/posts/2026/04/09-context-compaction-in-codex-claude-code-and-opencode ; https://decodeclaude.com/compaction-deep-dive/ ; https://platform.claude.com/docs/en/build-with-claude/compaction ; nh-core/src/lib.rs:1332-1368; Master Plan §0 diff.4.
*LAW:* modular (stages), auditable (reports), small (each stage <100 lines); auto-compact adds an LLM call — gated behind config to stay congruent with "cheapest capable".

### F2 — Tool-result hot/cold split + insert-time snip (token-bomb guard) (HIGH / S-M)
Tool outputs are the bulk of agent context and the least valuable after a few turns; Claude Code offloads older tool results to disk with path references and this is its zero-cost first line of defense. At `AgentLoop` tool-result push (lib.rs:1225-1231): (a) **snip**: if a single result exceeds a cap (e.g. 24k chars), write the full output to `.nosis/tool-outputs/<sha8>.txt` (scrubbed via `nh_vault::Scrubber`), keep head+tail + `"[nosis] full output: .nosis/tool-outputs/… — read_file to retrieve"`; (b) **microcompact**: when the 70% trigger fires, first replace tool-result messages older than the last 2 user turns with the same placeholder form before any message is dropped. The model can always re-read what it actually needs — Anthropic calls this "tool clearing" and ships it as `clear_tool_uses` context editing. This also encodes the owner's documented Playwright token-bomb lesson (Master Plan §5.8) at the engine level, not just per-tool defaults.
*Evidence:* https://decodeclaude.com/compaction-deep-dive/ ; https://platform.claude.com/cookbook/tool-use-context-engineering-context-engineering-tools ; nh-core/src/lib.rs:1225-1231; Master Plan §5.8.
*LAW:* small, lightweight (no deps), safe (nothing is lost — offloaded, scrubbed), auditable (files on disk).

### F3 — Compaction receipts: what was compacted and why, in the ledger (HIGH / S)
Compaction currently leaves no durable trace (only the `on_event` line, lib.rs:1160-1164). Add a `compaction` receipt line to `.nosis/receipts.jsonl` (extend `receipt` module with a small `CompactionRecord { ts_utc, stage, trigger_pct, messages_elided, tokens_before, tokens_after, elided_digest, cache_invalidated: bool }`) written through the existing `ReceiptWriter` (scrubbed, append-only). The TUI timeline (M3, Master Plan §5.4/5.8) gets its visible compaction marker from the same record. This is the owner-requested "receipts of what was compacted & why" and directly serves the auditable tenet; OpenCode's reversible `compacted`-timestamp design shows the industry moving to non-destructive, inspectable compaction.
*Evidence:* nh-core/src/lib.rs:1009-1071 (ReceiptWriter), 1160-1164; https://justin3go.com/en/posts/2026/04/09-context-compaction-in-codex-claude-code-and-opencode ; Master Plan §5.8.
*LAW:* auditable, small, congruent (reuses the receipt seam).

### F4 — Promote the stable prefix to a HARD, release-build invariant (`PrefixSeal`) + live cache-break detector (HIGH / S)
The ~120× cache economics (¥0.025 vs ¥3.00, Master Plan §0.1) hang on byte-stability, but the guard is `debug_assert` only — release builds silently tolerate prefix mutation. Manus: one token of drift invalidates everything after; DeepSeek requires a **full prefix-unit match**. Design: a tiny `PrefixSeal` newtype minted once per session holding `blake3/sha256(history[0] bytes ‖ canonical tool-spec bytes)` (tool specs are part of the wire prefix too — `build_body` serializes them every request, lib.rs:256-271, but nothing pins their order/content). Before every `complete()`, verify in all builds; on mismatch: fail the turn with a typed error + receipt, never send. Second half: a **cache-break detector** — track `cached_tokens` per turn; if turn N's cached count drops below turn N−1's prompt size by a large margin mid-session, emit `on_event("cache broken — prefix changed or provider evicted")` and a receipt. This turns the invariant from an assumption into a monitored SLO and powers the cache-hit chip's trustworthiness (M2 exit: >60% over 50 turns, CONTRACTS_M2 §3.4). Also adopt the append-only corollary: compaction must insert, never mutate (fix lib.rs:1362-1365, see F1).
*Evidence:* https://manus.im/blog/Context-Engineering-for-AI-Agents-Lessons-from-Building-Manus ; https://api-docs.deepseek.com/guides/kv_cache/ ; nh-core/src/lib.rs:1141-1152, 1362-1365; CONTRACTS_M2.md §3.1/3.4.
*LAW:* secure/safe (fail loud, not silent cost blowout), small (one hash + one check), harmonic (protects differentiator 4 across all crates).

### F5 — `effective_context` clamp per route: stop arming compaction at 700k tokens (context-rot guard) (HIGH / S)
Chroma's research shows frontier models degrade well below their advertised windows — significant degradation at ~50k tokens on 200k-window models, worse with distractors and middle-placement. NOSIS routes advertise 1M (catalog.toml:38 etc.) so compaction arms at 700k — deep inside rot territory; sessions will get dumb long before they get compacted. Since the catalog is data (THE LAW: catalog is TOML, not code), add an optional per-route `effective_context` field (e.g. DeepSeek/MiMo 1M routes → 200-262k default; absent → `min(context, 262144)`), parsed in nh-routes and used by `AgentLoop.context_limit` (cmd_run.rs:121, cmd_chat.rs:120, nh-fleet lib.rs:1239). The full window stays available for explicit long-document jobs via an override flag. Cheap, data-only, and it makes every other compaction improvement fire at the point where it actually preserves model quality.
*Evidence:* https://www.trychroma.com/research/context-rot ; catalog.toml:8,38,228,250; nh-routes/src/lib.rs:156; crates/nh-cli/src/cmd_run.rs:121.
*LAW:* small (one TOML field + one min()), congruent (catalog-is-data), safe.

### F6 — Fix `estimate_tokens`: count `reasoning_content` and tool-spec overhead (HIGH / S — bug-level)
`estimate_tokens` (lib.rs:1316-1328) sums content + tool_calls only. On `preserve_reasoning` routes (Kimi K2.7 always-thinking, MiMo — Master Plan A.10.5, catalog-enforced) the wire replays full reasoning chains that can dominate the payload, and the tool-spec block rides on every request; both are invisible to the estimator. Consequence: on exactly the marathon routes NOSIS advertises (MiMo 1,000+ tool calls), the 70% trigger fires late and requests can exceed the real window with a provider error instead of a graceful compaction. Fix: add `reasoning_content` bytes (only when the route preserves them — pass a `counts_reasoning: bool` from route policy) and a per-request `tools_overhead` term computed once from the serialized specs. ~15 lines + tests.
*Evidence:* nh-core/src/lib.rs:1316-1328 vs 283-298 (reasoning replay); Master Plan A.2/A.10.5; https://www.cometapi.com/kimi-k2-api-pricing/ (K2.7 always-thinking).
*LAW:* small, safe, congruent (respects per-route policy already in the catalog).

### F7 — Session persistence + `nh chat --resume` (context-loss pain, and a cache-warm bonus) (HIGH / M)
`ChatSession.history` is RAM-only (cmd_chat.rs:30-40): a crash, an accidental Ctrl-C, or a Windows terminal death (documented pain #5) erases the session — "context loss" is literally differentiator 6's target. Design: append each turn's messages as scrubbed JSONL to `.nosis/sessions/<session-id>.jsonl` (same append-only + scrubber pattern as receipts, reuse `ReceiptWriter`'s shape; `.nosis/.gitignore` already covers artifacts per A.8.6). `nh chat --resume [id]` replays the file into `history` and re-seals the prefix (F4). Bonus grounded in provider docs: DeepSeek's disk cache persists **hours to days** — resuming with byte-identical history means the *entire* prior conversation re-prefills at the ¥0.025 cache-hit rate instead of ¥3.00, so resume is nearly free. That is a marketable, measurable win no incumbent surfaces.
*Evidence:* crates/nh-cli/src/cmd_chat.rs:30-40,125-133; https://api-docs.deepseek.com/guides/kv_cache/ ("usually within a few hours to a few days"); Master Plan §5 pain list.
*LAW:* safe (crash-proof), auditable (session file), small (reuses JSONL+scrubber seam).

### F8 — File-based memory v1: retain/recall/reflect as flat files behind the cache breakpoint (MED-HIGH / M)
The plan schedules SQLite memory (§2), but THE LAW and this project's own working practice (the meta-observation: NOSIS itself is being built with file-based memory — MEMORY.md + topic files) argue for a flat-file v1: `.nosis/memory/MEMORY.md` (+ optional topic files), a `memory_write(section, text)` tool (write-gated by nh-law like any file write), and injection of the memory file **once per session, immediately after the constitution** — session-stable, so it extends the cached prefix rather than breaking it (it changes between sessions, not within). Anthropic's "structured note-taking" is exactly this and is their recommended first memory lever; Hindsight (arXiv:2512.12818) validates the retain/recall/reflect interface the plan already names (39%→83.6% LongMemEval with an open 20B model) — the interface can stay, the store can be a file until scale demands SQLite. Recall v1 = whole-file injection (small file, capped, e.g. 4k tokens); reflect v1 = a `/reflect` command that runs one cheap-route summarization of the session into MEMORY.md at session end. No new deps, no vector store, no embedding key.
*Evidence:* https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents ; https://arxiv.org/abs/2512.12818 ; Master Plan §2 "Memory: retain/recall/reflect (Hindsight-style), pluggable"; ARCHITECTURE_OVERVIEW.md:11.
*LAW:* small/lightweight (files, not DB), readable (memory is human-editable markdown), auditable, modular (store swappable later).

### F9 — `subtask` tool: in-session sub-agent context isolation with condensed returns (MED / M)
Fleet already isolates per-job context (nh-fleet lib.rs:1246) but only for batch runs; within a live session the agent has no way to fork a throwaway context. The 2026 consensus pattern (Anthropic multi-agent research system; corporate multi-agent survey) is sub-agents with isolated windows returning 1,000–2,000-token structured summaries — detailed exploration context never pollutes the parent. Design: an `nh-tools` tool `subtask { task, route_hint?, max_turns }` that constructs a nested `AgentLoop` (fresh history, same `ToolCtx`, read-mostly tool set by default, trust dial inherited) on a resolver-chosen cheap route, and returns only the final text (capped). Parent context cost: one tool call + one summary. This composes with routing (differentiator 1-3): a Flash sub-task can explore while the Pro parent keeps its expensive cached context clean. Receipt per subtask via the existing writer. Guard: depth ≤ 1, subtask cannot spawn subtasks (small, safe).
*Evidence:* https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents (1-2k token subagent summaries); https://arxiv.org/abs/2603.09619 (isolation/economy/provenance criteria); crates/nh-fleet/src/lib.rs:1239-1248; nh-core AgentLoop (lib.rs:1087-1106).
*LAW:* modular (reuses AgentLoop), small MVP, secure (inherited gates, depth cap); tension: adds one tool — justified by direct context-quality payoff.

### F10 — Cache-aware compaction economics: compact when the cache is already cold (MED / S)
Compaction is not free on a cache-first engine: dropping/rewriting history invalidates every cached token after the prefix, so the next turn re-prefills the retained suffix at cache-miss price (~120× on V4-Pro). Two zero-dep improvements: (a) **compact at route-switch moments** — `/model` / `/provider` in `nh chat` lands on a provider whose cache is cold anyway (cmd_chat.rs keeps history across switches); run the pipeline opportunistically there even below 70%; (b) **HUD pre-warning** — emit "context 65% — compaction at 70% (next turn will re-prefill ~N tokens at miss price ≈ $X)" using the route's own price table, so the user can choose to `/compact` at a task boundary (Claude Code's manual-compact-at-boundaries guidance). Both are a few lines in the trigger block (lib.rs:1154-1166) + one event string; the cost math already exists in nh-routes price entries.
*Evidence:* https://api-docs.deepseek.com/guides/kv_cache/ (full-prefix-unit matching); Master Plan §0.1 (120×); https://decodeclaude.com/compaction-deep-dive/ (manual compaction at task boundaries); nh-core/src/lib.rs:1154-1166; catalog.toml price tables.
*LAW:* small, congruent (marries differentiators 1 and 4), honest-cost.

### F11 — Parse DeepSeek's native cache fields as a fallback (`prompt_cache_hit_tokens`/`prompt_cache_miss_tokens`) (MED / S)
`WireUsage` reads only OpenAI-convention `prompt_tokens_details.cached_tokens` (lib.rs:362-375). DeepSeek's documented usage fields are `prompt_cache_hit_tokens` and `prompt_cache_miss_tokens`; if a DeepSeek response ever ships only the native fields (or the details object is absent on some gateway path), the cache chip silently reads 0% and the F4 cache-break detector false-alarms. Add both fields to `WireUsage` with `cached_tokens` precedence `prompt_tokens_details.cached_tokens → prompt_cache_hit_tokens`; also lets receipts line-item hit vs miss tokens exactly, which the Cost HUD (M3) can price with the per-route hit/miss rates already in catalog.toml (¥0.025/¥3.00). ~10 lines + a parse test.
*Evidence:* https://api-docs.deepseek.com/guides/kv_cache/ ; https://api-docs.deepseek.com/news/news0802/ ; nh-core/src/lib.rs:362-375; catalog.toml:44-50.
*LAW:* small, auditable (exact cost attribution), congruent.

---

## Explicitly considered and NOT recommended
- **Anthropic server-side compaction (`compact_20260112`)** as the engine: Anthropic-API-only (needs an Anthropic key the project doesn't hold; DeepSeek's Anthropic-compatible wire won't implement a beta context_management feature). Client-side pipeline (F1) keeps it provider-neutral. Reference only.
- **Vector/embedding retrieval memory**: needs an embedding route (no key holdings fit cheaply), adds a store dep, violates lightweight for v1. Flat-file recall (F8) first; the Hindsight interface leaves the seam open.
- **SQLite memory now**: deferred per F8 rationale; interface-compatible upgrade later.

## Sources
- https://decodeclaude.com/compaction-deep-dive/
- https://karanprasad.com/blog/how-claude-code-actually-works-reverse-engineering-512k-lines
- https://oldeucryptoboi.com/blog/context-compaction-deep-dive/
- https://justin3go.com/en/posts/2026/04/09-context-compaction-in-codex-claude-code-and-opencode
- https://platform.claude.com/docs/en/build-with-claude/compaction
- https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents
- https://platform.claude.com/cookbook/tool-use-context-engineering-context-engineering-tools
- https://www.trychroma.com/research/context-rot
- https://manus.im/blog/Context-Engineering-for-AI-Agents-Lessons-from-Building-Manus
- https://api-docs.deepseek.com/guides/kv_cache/
- https://api-docs.deepseek.com/news/news0802/
- https://www.cometapi.com/kimi-k2-api-pricing/
- https://benchlm.ai/moonshot/api-pricing
- https://arxiv.org/abs/2512.12818
- https://arxiv.org/abs/2603.09619
- https://codex.danielvaughan.com/2026/04/10/context-compaction-showdown-coding-agents/
