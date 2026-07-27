# Model Roles

Condensed from plan §A.9 (routing policy v2) — the harness will encode this table in `catalog.toml` + policy TOML; until then it guides manual routing.

## Primary Builder

Model/tool:

GPT-5.6 Terra (Codex CLI delegate); Sol max/ultra for hardest + security-adjacent work.

Use for:

Implementation bursts in the build loop — subscription already paid, quota-scarce.

## Reviewer

Model/tool:

Opus 5 (Claude Code delegate, `claude -p`); Sonnet 4.6 pre-screens to stretch quota.

Use for:

THE LAW conformance, security posture, spec match. Mandatory gate per PR. Batch reviews; receipts + diffs only.

## Research Agent

Model/tool:

Gemini 3.1 Pro via Antigravity CLI delegate (best-effort quota, never critical-path); Claude for synthesis.

Use for:

Web-grounded research subtasks (live docs, CVEs), price/catalog verification.

## Long-Context Synthesizer

Model/tool:

MiMo V2.5-Pro (1M context, no long-context surcharge) — marathon/huge-context jobs route here.

Use for:

500+ tool-call runs, biggest-context analysis.

## Full Task-Shape Table (for the harness policy TOML)

| Task shape | Route |
|---|---|
| CI / smoke tests | GLM-4.7-Flash (**free**) |
| Quick edits, Q&A | DeepSeek V4 Flash, non-think |
| High-volume coding | Kimi K2.7 Code (cache $0.19, always-thinking, preserve_reasoning ON) |
| Coding with screenshots/diagrams | Kimi K2.7 Code (native MoonViT vision) |
| Hard debugging / reasoning | DeepSeek V4 Pro, Think High→Max (daytime = off-peak) |
| Marathon / huge context | MiMo V2.5-Pro |
| Cheap bulk + non-code multimodal | MiMo V2.5 standard (the catalog sleeper: ~$0.105–0.14 in / $0.28 out, 1M ctx, omni) |
| Massively parallel decomposable | Kimi K2.6 Swarm or nh-fleet |
| Free vision for tests | GLM-4.6V-Flash (**free**) |
| Web-grounded research | Gemini 3.1 Pro (Antigravity delegate, best-effort) |
| Implementation bursts | GPT-5.6 Terra/Sol (Codex delegate) |
| Review / gate / security | Opus 5 (Claude Code delegate) |

Escalation ladder: Flash → K2.7 → V4 Pro High → V4 Pro Max → Opus gate. Two failures per tier, receipt attached, never silently retry the same route more than twice.
