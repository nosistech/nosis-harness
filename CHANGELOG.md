# Changelog

All notable, user-facing changes to the Nosis Harness (`nh`) are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
The first public release will be tagged `v0.1.0`; until then, the full current feature
set lives under **Unreleased**.

For the full engineering history behind these entries, see
[00-start-here/BUILD_LOG.md](00-start-here/BUILD_LOG.md).

## [Unreleased]

### Added

- `nh init` — scaffold `.nosis/` in the current repo: receipts directory, `.gitignore`,
  a secret-pattern pre-commit hook, and a starter `catalog.toml`.
- `nh key add <entry>` — store API keys in the OS-native vault (Windows Credential
  Manager / macOS Keychain / Linux Secret Service). Keys are never echoed and never
  written to files. Env fallback for CI/headless: `NH_<ENTRY>_KEY`.
- `nh run "<task>"` — run one agent task with `--model`, `--max-turns`, `--think`,
  `--autonomy`, and `--profile`. Defaults: `deepseek-v4-flash`, 20 turns, `balanced`.
  Every turn is logged to the local receipt ledger `.nosis/receipts.jsonl`.
- `nh chat` — interactive session. `/model` and `/provider` switch routes mid-session
  with history and cumulative usage preserved; `/price` shows live peak/off-peak pricing.
- `nh why` — explain the cheapest capable route for a task; `--model` compares a
  specific route against the cheapest capable one.
- `nh profile` — list the execution profiles (`frugal` / `balanced` / `max-quality`)
  and their effective caps for a model.
- `nh tui` — full-screen terminal UI (`--model`, `--budget`, `--profile`).
- `nh fleet run <tasks.json>` and `nh fleet resume [run_id]` — durable, resumable
  worker fleet for independent tasks (`--max-workers`, `--budget`, `--escalate`,
  `--defer-offpeak` on `run`; `--max-workers` on `resume`). Resume picks up the
  latest incomplete run, or a specific run id. Fleet state lives locally under
  `.nosis/fleet/`.
- `nh mcp serve` — **preview** local MCP endpoint. Binds `127.0.0.1` (loopback)
  exclusively; bearer-token guarded. Metered-service tools: `why` (cheapest capable
  route + savings), `route_cost` (price a specific route), and `receipts` (the server's
  own recent metered receipts), alongside `route_resolve`, `fleet_run`, and
  `fleet_status`. The routing, pricing, and receipts tools return MCP `structuredContent`
  next to their text. Do not expose this endpoint publicly before the MCP final spec
  lands on 2026-07-28.
- Honest metered routing: `nh` picks the cheapest capable route for each task and
  hands you the receipt — cost, token usage, and savings versus no-cache. Peak and
  off-peak pricing is applied per route; stale price data is flagged, never guessed.
- Route catalog (`catalog.toml`) for open-weight providers: DeepSeek V4
  (`deepseek-v4-pro`, `deepseek-v4-flash`, plus Anthropic-wire variants), Kimi
  (`kimi-k2.7-code`, `kimi-k2.7-code-highspeed`, `kimi-k2.6`), MiMo (`mimo-v2.5-pro`,
  `mimo-v2.5`), and GLM (`glm-5.2`, plus the free rate-limited `glm-4.7-flash`,
  `glm-4.6v-flash`, and `glm-4.5-flash`).
- Law-based approval guardrails: every shell command stops at a y/N prompt that
  defaults to deny.
- No telemetry: prompts go only to the provider you explicitly select, over TLS;
  `nh` adds no intermediary and never phones home. Receipts stay local and are never
  uploaded. See [PRIVACY.md](PRIVACY.md).

### Security

- Fail-closed audience checks on key egress: a key is released only to its verified
  provider destination; anything unverifiable is refused.
- Secrets are scrubbed from all logs and all tool output before egress.
- Fleet state is guarded by a single-writer lock; readers never mutate state.
- Bearer-token auth on the MCP preview uses constant-time comparison.
- Report vulnerabilities per [SECURITY.md](SECURITY.md) — info@nosistech.com,
  5-business-day response SLA.

<!-- Reference-style version links. Replace the Unreleased target with the
     compare URL (.../compare/v0.1.0...HEAD) once the public remote and the
     v0.1.0 tag exist. -->
[Unreleased]: ./
