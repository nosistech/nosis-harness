# CODEX.md

Guidance for Codex/OpenAI sessions.

## Models

- Default implementer: `gpt-5.6-terra` (GPT-5.5-class at half price).
- Hardest changes: `gpt-5.6-sol` with `max` effort; `ultra` mode (parallel subagents) for M2 context engine and anything touching nh-law/security.
- Quota stretcher when approaching plan limits: `gpt-5.4-mini`.
- Deprecated under ChatGPT sign-in — never select: `gpt-5.2*`, `gpt-5.3-codex`.
- Update the Codex CLI binary first; outdated clients don't show 5.6. Codex + ChatGPT Work share one usage pool — `/status` shows remaining.

## Use Codex for

- Implementation (small PRs), tests, local verification, repo inspection, documentation updates.

## Workflow

1. Read context (AGENTS.md first-read order).
2. Inspect files.
3. Make scoped changes — small PRs, never direct-to-main.
4. Verify: `cargo test && cargo clippy -- -D warnings` must pass before handoff.
5. Update `../00-start-here/BUILD_LOG.md`.
6. Hand off to Opus 4.8 gate with receipt + diff.

## M0 First Prompt (paste into Codex after repo init)

> **Model: gpt-5.6-sol, effort max, ultra mode** (or gpt-5.6-terra if quota is tight). Read AGENTS.md and NOSIS_HARNESS_Master_Plan.md (including Appendices A and B — they supersede Sections 1 and 3 where they conflict). Implement Milestone M0 only: a Rust workspace `nosis-harness` with crates nh-core, nh-routes (stub), nh-tools, nh-vault, nh-cli. nh-core runs a turn loop against DeepSeek `deepseek-v4-flash` via the OpenAI-compatible endpoint (base_url https://api.deepseek.com), with tools read_file, edit_file, exec_shell (approval prompt before every exec). Keys come from nh-vault (OS-native secret store via the `keyring` crate, `zeroize` after use) with env-var `NH_DEEPSEEK_KEY` as fallback only; no plaintext keys at rest, and a redaction scrubber on all log/receipt output paths. Every turn writes a JSONL receipt to .nosis/receipts.jsonl; `nh init` installs the `.nosis/.gitignore` and secret-pattern pre-commit hook. The adapter must reject banned model strings (deepseek-chat, deepseek-reasoner, mimo-v2-\*, gpt-5.2\*, gpt-5.3-codex, moonshot-v1-\*) with tests. Follow THE LAW: small, simple, secure, readable, auditable. No TUI yet. Include an integration test with a mocked provider. Run `cargo test` and `cargo clippy -- -D warnings` before finishing. Small PRs only; do not touch main directly.
