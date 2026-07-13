# Prompt Library

## Project Handoff Prompt

Read `00-start-here/MASTER_CONTEXT.md`, `00-start-here/CURRENT_TASK.md`, and `00-start-here/BUILD_LOG.md`. Continue from the latest task. Do not restart from scratch.

## M0 Implementation Prompt (Codex)

See `CODEX.md` — kept there so it stays next to the model-selection guidance.

## Opus 4.8 Review Gate Prompt

Read AGENTS.md and the attached receipt + diff (never raw transcripts). Review against: (1) THE LAW conformance — small, simple, secure, readable, auditable; (2) security posture — nh-vault rules, no plaintext secrets, tool outputs treated as data, banned model strings rejected; (3) spec match against NOSIS_HARNESS_Master_Plan.md for the current milestone. Verdict: APPROVE or REJECT with a written receipt listing each violation and the minimal fix. Findings first.

## Code Review Prompt (general)

Review for bugs, security risks, missing tests, data leaks, and maintainability. Findings first.

## Catalog Verification Prompt (M1, recurring)

Verify live pricing and model availability for every route in catalog.toml against the provider's own pricing page (not aggregators). Record per route: price, output cap, quantization if disclosed, `price_confidence`, `valid_until`. Flag any entry where sources conflict (known: MiMo first-party rates). Do not invent numbers — stale data gets flagged, never guessed (honest-cost rule).

## Research Prompt

Analyze the source for patterns, risks, opportunities, and direct project implications. Do not copy code (CodeWhale: patterns yes, code never).
