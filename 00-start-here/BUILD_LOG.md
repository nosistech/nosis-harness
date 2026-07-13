# Build Log

Record every meaningful session here.

## 2026-07-13: M1 build finalized (Fable 5 multi-agent workflow)

Builder:

- Claude (Fable 5, Claude Code) — multi-agent workflow: architect + 4 parallel crate builders, integrator, 4 adversarial reviewers, hardening pass

What changed:

- M1 implemented end-to-end per CONTRACTS_M1.md: full 5-provider catalog with clock-aware pricing, Anthropic Messages wire adapter, thinking-mode dialects + `nh run --think`, stateless MCP client (2026-07-28 draft), and `nh chat` with mid-session `/model`/`/provider` switching and cost HUD. Integration green after merging the crate builders' outputs.
- 4 adversarial review findings addressed (fixes detailed in the M1 hardening entry below).
- Price verification pass (full detail: `../04-research/SOURCE_INDEX.md`, 2026-07-13 section): all four API providers checked against their OWN pricing/docs pages — DeepSeek, Kimi, MiMo, and GLM base prices and base URLs are all first-party CONFIRMED. MiMo plan-B.3 conflict RESOLVED first-party (current `mimo.mi.com/docs/pricing` matches the marketplace figures, superseding the May 27 cut notice; `verify_live` → `confirmed`). Two earlier Kimi "reported" figures were wrong (kimi-k2.6 is $0.16 hit / $0.95 miss / $4.00 out, not ~$0.55–0.60 in / $2.50–2.65 out; k2.7 highspeed bills 2x standard input) and the catalog's old MiMo host was wrong (fixed to `api.xiaomimimo.com`). Still open, honestly: DeepSeek's peak 2x windows are announcement-only (secondary press, not yet on the first-party pricing page — re-verify on/around 2026-07-24), and GLM free-tier rate limits remain unpublished.

Tests/checks run:

- cargo build --workspace: green. cargo test --workspace: 176 passed, 0 failed, 1 ignored (keyring round-trip; nh-cli 49, nh-core lib 21 + integration 5+4+49, nh-routes 38, nh-vault 10). cargo clippy --workspace --all-targets -- -D warnings: clean. M0 smoke: `nh --help` exit 0; `echo /quit | cargo run -p nh-cli -q -- chat` exit 0 with no key configured (friendly warning to stderr, stdout empty). Committed on main as 0ed3d6d 'M1: full catalog, clock pricing, Anthropic wire, thinking dialects, MCP client, chat session'.

Next step:

- Live provider verification (DeepSeek keyed run + GLM free-route run), then M2: context engine + law.

## 2026-07-13: M1 hardening — adversarial-review fixes

Builder:

- Claude (Fable 5, Claude Code) — M1 hardening agent

What changed:

- Wire-client HTTP config (nh-core): both clients now build via one `http_client()` — explicit 600 s request timeout + 10 s connect timeout (reqwest's blocking default silently aborted every request at 30 s, killing long thinking turns) and `redirect::Policy::none()` (reqwest forwards custom headers like `x-api-key` across cross-host redirects — a redirecting endpoint is now a friendly HTTP error, never a key leak). Timeout failures get their own actionable line ("provider at <url> did not answer within 600s — retry, or switch to another route") instead of the misleading "could not reach provider". `McpClient` got the same explicit timeouts.
- `nh chat` keyless recovery (nh-cli): a keyless start now retries the real connection at the next task, so `nh key add <provider>` in another terminal works without restarting the session; reconnect registers the new key on every scrub path (shared `install_client` helper with `/model`/`/provider`).
- CONTRACTS_M1.md §7 amendment 4 (orchestrator authority): ratified `nh run --think none|low|high|max` + per-dialect defaults (always-thinking/glm-hm → High, deepseek-nhm/none → None) — closes the frozen-surface gap flagged in review.
- Deleted the reviewer's truncated `adv_redirect_probe.rs` (marked DELETE AFTER REVIEW, did not compile); its concern lives on as the `cross_host_redirects_are_refused_never_followed` regression test in `wire_clients.rs`.

Tests/checks run:

- `cargo test --workspace` (180 passed, 1 ignored keyring round-trip, 0 failed; 4 new tests: timeout error line, timeout floor guard, redirect refusal on both wires, keyless reconnect), `cargo clippy --workspace --all-targets -- -D warnings` clean. Smoke: `echo /quit | nh chat` exit 0 keyless; `nh run --help` surface unchanged.

Next step:

- M1 exit demo against a live key (unchanged from integration entry).

## 2026-07-13: M1 integration — workspace green, contracts reconciled

Builder:

- Claude (Fable 5, Claude Code) — M1 integrator

What changed:

- Reconciled the 3 nh-routes tests asserting pre-verification catalog values with the 2026-07-13 live-verified first-party prices (kimi-k2.6 output 4.00 confirmed, mimo B.3 conflict resolved confirmed, glm free tier confirmed); `mimo_prices_are_verify_live` renamed to `mimo_prices_are_confirmed_first_party`. Catalog is data-of-record — tests follow data.
- `nh chat` keyless startup: connect failure at start is now one `warning:` line plus a stand-in client that re-surfaces the vault error only when a task runs; commands work keyless and `echo /quit | nh chat` exits 0 on a fresh machine (new test).
- CONTRACTS_M1.md: §5.2 amendment 3 (`AgentLoop.thinking`), §5.3 nh-cli `serde_json` dev-dep, §6 ledger rows marked RESOLVED (base URLs, MiMo prices, Kimi K2.6 cache-hit $0.16) + DeepSeek 2026-07-24 re-verify row, new §7 integration amendments (keyless startup, `peak <mult>x until HH:MM` format blessed, nh-tools crate-root re-exports).

Tests/checks run:

- `cargo build --workspace`, `cargo test --workspace` (176 passed, 1 ignored keyring round-trip, 0 failed), `cargo clippy --workspace --all-targets -- -D warnings` — all green. M0 smoke: `nh --help` exit 0; `echo /quit | nh chat` exit 0 keyless; `/price` keyless shows live peak HUD.

Next step:

- M1 exit demo against a live key: mid-session `/model`+`/provider` switch, one stateless MCP call with handle passback; verify-live ledger items (`reasoning_effort`, `ttlMs`, GLM rate limits; DeepSeek prices on/around 2026-07-24).

## 2026-07-13: M1 nh-cli (`nh chat` REPL + `nh run --think`)

Builder:

- Claude (Fable 5, Claude Code) — nh-cli builder (CONTRACTS_M1.md §4)

What changed:

- New `nh chat [--model <id>]` (cmd_chat.rs): line REPL over an injected line-source/Write abstraction (piped-stdin friendly, BOM-tolerant for Windows pipes); prompt `nh> ` on stderr, stdout carries only answers and command output. Plain lines run through `AgentLoop::run_with_history` on ONE persistent history; `/model` and `/provider` switch routes mid-session preserving history and cumulative usage (M1 exit criterion); `/price` and the per-answer footer are the first cost HUD lines — peak indicator renders "peak 2x until HH:MM" with the window boundary converted to the user's local time; `/tools` lists builtin then MCP tools; unknown commands get the one-line help. MCP tools load at chat start from `.nosis/mcp.toml` via `nh_tools::mcp` (broken file = one warning, chat continues).
- Scrubbing: one shared `Arc<RwLock<Scrubber>>` across stdout/stderr/approval/receipt paths, rebuilt on every switch so switched-away keys stay redacted for the whole session.
- `nh run --think none|low|high|max` mapped to `ThinkingEffort` (`effort_for`): absent flag defaults per dialect — High for always-thinking/glm-hm, None for deepseek-nhm/none. `nh run` now uses `make_client` (both wires) and refuses delegate routes with the M4 message.
- Deps per §5.3: `chrono` (workspace) added to nh-cli; `serde_json` (workspace) added as dev-dependency so tests build `ChatMessage` via serde instead of struct literals (§5.2 grep rule).

Tests/checks run:

- `cargo test -p nh-cli` 48 passed (23 new chat tests: switching, price/footer formats, scrubbing, loop basics, MCP load); `cargo clippy -p nh-cli --all-targets -- -D warnings` clean; workspace clippy clean. Manual piped smoke: `/price` (live peak boundary in local time), friendly missing-key error keeps route, `/quit` exits 0. Workspace tests: all crates green EXCEPT 3 nh-routes tests asserting pre-verification catalog values (kimi-k2.6 output 2.65→4.00, mimo 0.87, glm confirmed) — catalog owner's reconciliation, not nh-cli.

Next step:

- nh-routes tests vs live-verified catalog.toml reconciliation; M1 live MCP call + `reasoning_effort` verify-live ledger.

## 2026-07-13: M1 nh-tools (MCP client, stateless 2026-07-28)

Builder:

- Claude (Fable 5, Claude Code) — nh-tools builder (CONTRACTS_M1.md §3)

What changed:

- New `nh_tools::mcp` module (re-exported at crate root; existing tools untouched): `.nosis/mcp.toml` parsing (`load_mcp_config`, unknown keys ignored, friendly errors naming valid auth/trust/spec values), `McpClient` (blocking JSON-RPC 2.0 over Streamable HTTP POST, `tools/list` with `ttlMs` cache — absent→60s, 0→no cache — `tools/call` text/non-text block rendering, discovery via GET `.well-known/mcp.json` with `server/discover` POST fallback).
- Statelessness invariant: no `initialize`, no `Mcp-Session-Id` ever; every request's params carry `_meta` (protocolVersion echoing config spec, clientInfo, capabilities). State handles are ordinary tool arguments (handle-passthrough test).
- Auth §3.4 (none / apikey via nh-vault per call / oauth2 deferred to M4 with one message) + §3.5 outbound header lint choke point (`Mcp-*`/`x-mcp-*` with Scrubber-shaped values refused; `Authorization` exempt).
- §3.6 adapters: `mcp_tools` builds `mcp__<server>__<tool>` adapters (`[MCP <server>] ` description prefix); trust=ask gates via `ctx.approve("mcp <server> <tool> <args>")` with Ok-shaped denial; trust=auto skips the gate only for server-annotated `readOnlyHint` tools; trust=block servers are never contacted and offer no tools; failing servers contribute one warning line, never a hard failure.
- Deps added to nh-tools per §5.3: `reqwest`, `toml` (workspace), `nh-vault` (path).

Tests/checks run:

- `cargo test -p nh-tools` 49 passed (37 new MCP tests against a hand-rolled `std::net::TcpListener` mock — no live calls, no heavy dev-deps); `cargo clippy -p nh-tools --all-targets -- -D warnings` clean. Workspace: nh-vault/nh-routes/nh-core lib+agent green; nh-cli has a pre-existing mid-flight compile error (`cmd_run::run` arity, cli owner's area); nh-core `wire_clients` test binary transiently file-locked by a concurrent build (os error 32), retried.

Next step:

- nh-cli `/tools` wires `mcp_tools` + warnings through the Scrubber; M1 live verify of `ttlMs` location against a real 2026-07-28 server.

## 2026-07-13: M1 nh-core (wire clients, thinking dialects, session history)

Builder:

- Claude (Fable 5, Claude Code) — nh-core builder (CONTRACTS_M1.md §2)

What changed:

- nh-core wire: `AnthropicMessagesClient` (POST `{base_url}/v1/messages`, `x-api-key` + `anthropic-version: 2023-06-01`, required `max_tokens`, full tool_use/tool_result mapping with consecutive-tool-result merge, usage from `input_tokens`/`output_tokens`/`cache_read_input_tokens`).
- nh-core wire: `make_client` factory — dispatches on `route.wire`, captures per-route policy (thinking dialect, `preserve_reasoning`, deepseek tool-replay quirk); `max_tokens = min(max_out, 8192)` on the anthropic wire.
- nh-core wire: `ThinkingEffort` enum + one-function `(dialect, effort)` mapping (deepseek-nhm → `reasoning_effort`, flagged verify-live; always-thinking/glm-hm/none → no toggle); `ChatRequest.thinking` and `ChatMessage.reasoning_content` amendments per §5.2; reasoning replay rules (preserve / strip / quirk empty-string, stored value wins) in one function.
- nh-core agent: `run_with_history` for `nh chat` sessions (`run` is now a thin wrapper); additive `AgentLoop.thinking` field — nh-cli's one literal updated in step (`thinking: None`).

Tests/checks run:

- `cargo test -p nh-core` 30 passed (incl. loopback mock-server tests for both wires — no live calls); `cargo clippy -p nh-core --all-targets -- -D warnings` clean; workspace clippy clean. Workspace tests: nh-core/nh-cli/nh-tools/nh-vault green; 3 pre-existing nh-routes failures from catalog.toml (2026-07-13 confirmed prices) vs nh-routes test expectations (verify_live/reported/2.65) — routes owner's area, untouched here.

Next step:

- Routes owner: reconcile nh-routes tests with the updated catalog.toml. nh-cli builder: `nh chat` on top of `run_with_history` + `make_client`.

## 2026-07-13: M0 hardening (adversarial review fixes)

Builder:

- Claude (Fable 5, Claude Code) — hardening agent

What changed:

- nh-cli: every stderr path now passes the Scrubber — progress lines, the approval prompt, and the final `nh:` error line; model-supplied text is also control-char-escaped (`sanitize_line`) so \r/ANSI cannot spoof the approval gate, with a visible truncation marker past 500 chars.
- nh-tools: `exec_shell` strips `NH_*_KEY` env vars from the child, closing the key-exfiltration-to-disk path via the env fallback.
- nh-routes: `from_toml` rejects banned route keys AND banned `model_id` values (clean alias can no longer smuggle a dead id onto the wire).
- nh-cli: `nh init` writes a starter catalog.toml (embedded repo-root catalog — still data), so `nh run` works in a fresh repo; missing-catalog error now says "run `nh init` to create one".

Tests/checks run:

- `cargo test --workspace` (62 passed, 1 ignored keyring round-trip), `cargo clippy --workspace --all-targets -- -D warnings` — green. Manual: `nh init` + `nh run` flow in a fresh temp dir reaches the key prompt.

Next step:

- M1: live route/pricing verification against providers.

## 2026-07-12: M0 build finalized (Fable 5 multi-agent workflow)

Builder:

- Claude (Fable 5, Claude Code) — multi-agent workflow: 5 parallel crate builders, integrator, 3 adversarial reviewers, hardening pass

What changed:

- M0 implemented end-to-end across all five crates via the multi-agent workflow; integration green after merging the crate builders' outputs.
- 6 adversarial review findings addressed (fixes detailed in the M0 hardening entry above).

Tests/checks run:

- 53 passed; 0 failed; 1 ignored (keyring_round_trip) across 6 test binaries: nh-core unit 12, nh-cli 8, nh-core integration 3, nh-routes 10, nh-tools 10, nh-vault 10 (+1 ignored); doc-tests 0; clippy -D warnings clean.

Next step:

- Verify live against DeepSeek (`nh key add deepseek`, then `nh run` on a sample repo), then M1.

## 2026-07-12: M0 implemented (turn loop, tools, vault, routes, CLI)

Builder:

- Claude (Fable 5, Claude Code) — 5 parallel crate builders + integrator

What changed:

- Implemented all locked `todo!()` contracts across `nh-core` (AgentLoop, OpenAiCompatClient, receipts), `nh-routes` (RouteResolver, catalog parsing, banned-string rejection), `nh-tools` (read_file / edit_file / exec_shell behind approval gate; denial is an Ok-shaped "user denied: <command>" tool result), `nh-vault` (OS keyring + env fallback + Scrubber), `nh-cli` (init / key / run).
- Sanctioned contract addition: `AgentLoop.on_event: Option<Box<dyn Fn(&str) + Send>>` for progress lines; nh-cli wires it to stderr. Field set is now frozen.

Tests/checks run:

- `cargo build --workspace`, `cargo test --workspace` (53 passed, 1 ignored keyring round-trip), `cargo clippy --workspace --all-targets -- -D warnings` — all green.

Next step:

- M1: live route/pricing verification against providers.

## 2026-07-12: Project OS created

Builder:

- Carlos + Claude (Fable 5, Claude Code)

What changed:

- Adapted ProjectStarterTemplate into this folder as the project operating system.
- Filled core docs from Master Plan v0.1: master context, current task, roadmap, milestones, decision log, product brief, architecture overview/decisions, security model, AI-collaboration set (AGENTS/CLAUDE/CODEX/MODEL_ROLES/CONTEXT_HANDOFF/PROMPT_LIBRARY), risk register, one-page summary.
- Re-pointed the M0 implementer prompt from "Codex 5.5" to GPT-5.6 (Terra default / Sol for hardest), added nh-vault to M0 scope per plan §A.10.7.

Files changed:

- All of `00-start-here/`, `05-ai-collaboration/`, plus README, PRODUCT_BRIEF, ARCHITECTURE_OVERVIEW, ARCHITECTURE_DECISIONS, SECURITY_MODEL, RISK_REGISTER, ONE_PAGE_SUMMARY.

Decisions made:

- Adopted the template as project OS (see DECISION_LOG 2026-07-12).

Tests/checks run:

- None (no code yet).

Next step:

- `git init`, root AGENTS.md, first commit, hand M0 to Codex (prompt in `../05-ai-collaboration/CODEX.md`).

Risks:

- All Appendix B prices are `reported`, not confirmed — verify live at M1. MiMo first-party pricing sources conflict.

## 2026-07-09 → 2026-07-11: Master Plan v0.1 + Appendices A/B

Builder:

- Carlos + Claude (research/planning)

What changed:

- Master Plan v0.1 written (verdict, capability matrix, architecture, routing brain, fleet, MCP 2026-07-28 strategy, UX, build plan M0–M5, risks, Codex first prompt).
- Appendix A: two-backend access architecture (API routes vs subscription delegates), per-provider deep dives, nh-vault spec.
- Appendix B: complete verified model catalog (July 11) with delta logs.

Next step:

- Organize project folder, then pre-M0 setup.
