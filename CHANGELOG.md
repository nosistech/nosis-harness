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

## [0.1.0] - UNRELEASED

`0.1.0` has not been tagged yet, so this entry is still a draft and every change below is
part of the first release rather than history. Replace `UNRELEASED` above with the tag date
(ISO `YYYY-MM-DD`) as the last edit before tagging - see
[03-execution/RELEASE_CHECKLIST.md](03-execution/RELEASE_CHECKLIST.md).

### Added

- `nh init` - scaffold `.nosis/` in the current repo: receipts directory, `.gitignore`,
  a secret-pattern pre-commit hook, and a starter `catalog.toml`.
- `nh key add <entry>` - store API keys in the OS-native vault (Windows Credential
  Manager / macOS Keychain / Linux Secret Service). Keys are never echoed and never
  written to files. Env fallback for CI/headless: `NH_<ENTRY>_KEY`.
- `nh key remove <entry>` - remove an entry from the OS-native vault; environment/CI
  fallbacks remain an explicit operator responsibility.
- `nh run "<task>"` - run one agent task with `--model`, `--max-turns`, `--think`,
  `--autonomy`, and `--profile`. Defaults: `deepseek-v4-flash`, 20 turns, `balanced`.
  Every turn is logged to the local receipt ledger `.nosis/receipts.jsonl`.
- `nh chat` - interactive session. `/model` and `/provider` switch routes mid-session
  with history and cumulative usage preserved; `/price` quotes the current route at the
  current clock time, including its peak / off-peak window.
- `nh resume` - resume a chat or TUI session that was interrupted. `nh resume` lists the
  interrupted sessions; `nh resume <session-id>` restores that one. Conversation history is
  written turn by turn to a crash-safe session ledger under `.nosis/sessions/`, so a session
  survives a crash or a closed terminal rather than only a clean exit. Cost totals are
  replayed from the ledger rather than stored as money, and the restored session reports
  `resumed`.
- `nh why` - explain the cheapest capable route for a task; `--model` compares a
  specific route against the cheapest capable one.
- `nh profile` - list the execution profiles (`frugal` / `balanced` / `max-quality`)
  and their effective caps for a model.
- `nh tui` - full-screen terminal UI (`--model`, `--budget`, `--profile`).
- `nh fleet run <tasks.json>` and `nh fleet resume [run_id]` - durable, resumable
  worker fleet for independent tasks (`--max-workers`, required `--budget` unless
  `budget_tokens` is in the task file, `--escalate`,
  `--defer-offpeak` on `run`; `--max-workers` on `resume`). Resume picks up the
  latest incomplete run, or a specific run id. Fleet state lives locally under
  `.nosis/fleet/`.
- `nh mcp serve` - **preview** local MCP endpoint. Binds `127.0.0.1` (loopback)
  exclusively; bearer-token guarded. Metered-service tools: `why` (cheapest capable
  route + savings), `route_cost` (price a specific route), and `receipts` (the server's
  own recent metered receipts), alongside `route_resolve`, `fleet_run`, and
  `fleet_status`. The routing, pricing, and receipts tools return MCP `structuredContent`
  next to their text. **Do not expose this endpoint on a public interface.** It is a preview
  in `0.1.0`: loopback-only and bearer-guarded, and public source availability does not
  authorize exposing it. This is not a restriction that lapses on a date.
- Honest metered route advice: execution uses the route the operator explicitly
  selects; `nh why` estimates the cheapest capable catalog route without dispatching it.
  Receipts record reported usage, while peak/off-peak and no-cache comparisons use
  catalog price data. Stale price data is flagged, never guessed.
- Metered context compaction. When long context is compacted, the effect is reported as a
  **net** figure for the next call, because compaction also discards the cached prefix and
  therefore has a cost side as well as a saving side. When the net effect is negative it is
  reported as negative rather than hidden. Compaction figures are estimates, are marked with
  a leading `~`, and are never written into recorded usage.
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
  2026-07-26, normalized current routes to USD, and removed unsupported historical DeepSeek
  peak windows.
- Route prices no longer expire. Nothing in the build ages, expires, or warns about a route
  price, and there is no recheck deadline to satisfy. A hand-serviced deadline cost more
  than it bought for providers that move prices two to four times a year. The accepted
  tradeoff is stated plainly: if a provider changes a price silently, the meter is wrong
  until a human updates the catalog. FX-rate staleness is a separate mechanism and remains -
  a stale rate still refuses to convert rather than guessing.
- The meter now distinguishes what a provider actually reported from what it did not, and
  carries that distinction through every surface instead of substituting zero. Token counts
  that are only a lower bound print with a leading `~`; usage a provider never reported
  prints as unknown rather than as `0`; and a cost is refused outright when it cannot be
  derived honestly. A missing usage figure can no longer be read as a free call.
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
- Tool calls execute only when the provider's termination reason confirms tool use. If the
  reason is absent or unrecognized, the requested calls are refused and the run is reported
  as partial with the reason stated, instead of acting on an unconfirmed instruction.
- The Fleet echo test provider is compiled only under the non-default `test-provider` Cargo
  feature and is absent from ordinary builds, which refuse `NH_FLEET_TEST_PROVIDER` rather
  than honoring it. It previously existed in a default build and could write fabricated
  `pass` receipts into the same `.nosis/receipts.jsonl` that real runs write to. When the
  feature is enabled its receipts are confined to their own directory and can no longer
  reach the canonical ledger.
- Report vulnerabilities per [SECURITY.md](SECURITY.md) - info@nosistech.com,
  5-business-day response SLA.

<!-- Reference-style version links remain local until the intended public remote exists.
     Replace them with the repository compare/release URLs before publishing the tag. -->
[Unreleased]: ./
[0.1.0]: ./
