# Changelog

All notable, user-facing changes to the Nosis Harness (`nh`) are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
The initial public release is `0.1.0`. Its tag is created only after the reviewed release
commit passes local and remote gates.

For the full engineering history behind these entries, see
[00-start-here/BUILD_LOG.md](00-start-here/BUILD_LOG.md).

## [Unreleased]

No changes yet.

## [0.1.0] - 2026-07-26

### Added

- `nh init` — scaffold `.nosis/` in the current repo: receipts directory, `.gitignore`,
  a secret-pattern pre-commit hook, and a starter `catalog.toml`.
- `nh key add <entry>` — store API keys in the OS-native vault (Windows Credential
  Manager / macOS Keychain / Linux Secret Service). Keys are never echoed and never
  written to files. Env fallback for CI/headless: `NH_<ENTRY>_KEY`.
- `nh key remove <entry>` — remove an entry from the OS-native vault; environment/CI
  fallbacks remain an explicit operator responsibility.
- `nh run "<task>"` — run one agent task with `--model`, `--max-turns`, `--think`,
  `--autonomy`, and `--profile`. Defaults: `deepseek-v4-flash`, 20 turns, `balanced`.
  Every turn is logged to the local receipt ledger `.nosis/receipts.jsonl`.
- `nh chat` — interactive session. `/model` and `/provider` switch routes mid-session
  with history and cumulative usage preserved; `/price` evaluates freshness-dated catalog
  pricing at the current clock time.
- `nh why` — explain the cheapest capable route for a task; `--model` compares a
  specific route against the cheapest capable one.
- `nh profile` — list the execution profiles (`frugal` / `balanced` / `max-quality`)
  and their effective caps for a model.
- `nh tui` — full-screen terminal UI (`--model`, `--budget`, `--profile`).
- `nh fleet run <tasks.json>` and `nh fleet resume [run_id]` — durable, resumable
  worker fleet for independent tasks (`--max-workers`, required `--budget` unless
  `budget_tokens` is in the task file, `--escalate`,
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
- Honest metered route advice: execution uses the route the operator explicitly
  selects; `nh why` estimates the cheapest capable catalog route without dispatching it.
  Receipts record reported usage, while peak/off-peak and no-cache comparisons use
  catalog price data. Stale price data is flagged, never guessed.
- Route catalog (`catalog.toml`) for open-weight providers: DeepSeek V4
  (`deepseek-v4-pro`, `deepseek-v4-flash`, plus Anthropic-wire variants), Kimi
  (`kimi-k2.7-code`, `kimi-k2.7-code-highspeed`, `kimi-k2.6`), MiMo (`mimo-v2.5-pro`,
  `mimo-v2.5`), and GLM (`glm-5.2`, plus the free rate-limited `glm-4.7-flash`,
  `glm-4.6v-flash`, and `glm-4.5-flash`).
- Law-based approval guardrails: every shell command stops at a y/N prompt that
  defaults to deny.
- No Nosis-operated telemetry, analytics, beacons, or crash reporting. Provider requests
  go directly to the selected approved origin; operator-approved MCP tools and shell
  commands can create additional egress. Receipts stay local by default. See
  [PRIVACY.md](PRIVACY.md).

### Changed

- Reverified every production catalog price against first-party provider documentation on
  2026-07-26, normalized current routes to USD, removed unsupported historical DeepSeek
  peak windows, and set a short 2026-08-02 recheck deadline.
- Repository catalogs are accepted only when byte-identical to the bundled catalog or to an
  exact operator-reviewed user-global copy.
- Active credentials now share one zeroizing registry across provider, Fleet, TUI, CLI, and
  MCP paths instead of bulk-materializing every catalog key into ordinary strings.
- GitHub Actions now pins third-party actions by commit, disables persisted checkout
  credentials, adds macOS to Windows/Linux checks, uses locked dependencies, sets timeouts,
  and is paired with weekly Dependabot updates.
- Security, privacy, architecture, release, and operations documents now distinguish current
  implementation from historical/aspirational plans.
- Internal workspace crates are marked `publish = false`; public v0.1 is distributed from the
  reviewed source release rather than as independently publishable crates.io packages.
- Provider credentials, runtime path containment, TUI terminal ownership, and TUI worker
  lifecycle now live behind shared modules instead of being duplicated across UI surfaces.
- TUI, Core, Fleet, MCP, tools, routing, law, CLI, and vault tests are organized into named
  responsibility modules while preserving public commands, APIs, wire formats, and ledgers.
- Provider wire formats, catalog validation, cache-safe context compaction, trusted CLI
  configuration, OAuth refresh, Fleet scheduling, and TUI input/render/worker state now live in
  explicit responsibility modules behind the same public facades.
- Recoverable HTTP-construction, synchronization-poisoning, process-result, token-generation, and
  worker-join failures now return bounded errors instead of panicking. Strict warning-free rustdoc
  is part of both CI and the local release gate.

### Removed

- Removed Telegram notification code, configuration, credential access, HTTP dependency,
  background sender, and documentation from public v0.1. The local bell/taskbar signal remains.

### Security

- Fail-closed audience checks on key egress: a key is released only to its verified
  provider destination; anything unverifiable is refused.
- Application-controlled terminal, receipt, tool-result, and MCP-result paths redact known
  key shapes plus active literal credentials.
- Fleet state is guarded by a single-writer lock; readers never mutate state.
- Bearer-token auth on the MCP preview uses constant-time comparison.
- Repository catalogs cannot redefine credential destinations or spend metadata unless
  they exactly match the bundled catalog or an operator-reviewed user-global catalog.
- Default interactive output is capped at 16,384 tokens per provider call; MCP Fleet
  runs require a positive budget and enforce a 1,000,000-token request ceiling.
- The generated pre-commit guard now shares the runtime key-shape registry and reports,
  without overwriting, an existing user hook that needs manual chaining.
- Report vulnerabilities per [SECURITY.md](SECURITY.md) — info@nosistech.com,
  5-business-day response SLA.

<!-- Reference-style version links remain local until the intended public remote exists.
     Replace them with the repository compare/release URLs before publishing the tag. -->
[Unreleased]: ./
[0.1.0]: ./
