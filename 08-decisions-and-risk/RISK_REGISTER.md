# Risk Register

Track risks early so they become managed facts, not vague anxiety. Source: plan §7 + Appendix deltas.

| Risk | Impact | Likelihood | Owner | Mitigation | Status |
|---|---|---|---|---|---|
| Scope creep (the #1 killer) | high | high | Carlos | 5 providers, 7 differentiators, nothing else in v1; everything else → TASK_BACKLOG under LATER | open |
| Model catalog rot (K3, V5, Opus 5 will land) | medium | high | Claude | Catalog is TOML data; 2 wire protocols only; new model = data entry | open |
| DeepSeek V4 pricing/behavior shifts at official launch | medium | medium | Claude | Prices are data with `valid_until`; harness flags stale data, never invents (honest-cost rule); 24h email notice monitored | monitoring |
| MiMo pricing sources conflict | low | high | Codex (M1) | `verify_live = true` flag; read platform pricing page at integration | open |
| Windows sandboxing genuinely hard | medium | high | Carlos | v1 = approval-gating + restricted tokens on Windows, full sandbox Linux-only; honest docs | open |
| Review debt (one person + two AI builders) | high | medium | Opus 4.8 | Mandatory Opus gate per PR, no direct-to-main, receipts on every merge; Sonnet pre-screen stretches quota | open |
| MCP final spec deltas vs frozen RC (July 28) | medium | low | Codex | Pin SDK to frozen RC, conformance check in CI, 2025-11-25 fallback; no public nh-mcp server until final | monitoring |
| Dead model strings in existing configs (MiMo V2 gone June 30; DeepSeek aliases die July 24) | high | high | Carlos | Audit KORVIN/LiteLLM/LECTOR configs NOW; banned-string rejection with tests in adapter | open |
| Gemini delegate quota unreliable (unpublished, repeatedly cut) | low | high | — | Router marks all Gemini routes best-effort, never critical-path | closed (by design) |
| Delegate quota starvation (Claude plan shared across surfaces) | medium | medium | Carlos | Cost HUD shows remaining-window estimates; batch Opus reviews | open |

## Risk Template

Risk:

Why it matters:

Trigger:

Mitigation:

Owner:

Review date:
