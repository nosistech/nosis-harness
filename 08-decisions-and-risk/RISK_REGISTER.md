# Risk Register

Track risks early so they become managed facts, not vague anxiety. Source: plan §7 + Appendix deltas.

| Risk | Impact | Likelihood | Owner | Mitigation | Status |
|---|---|---|---|---|---|
| Scope creep (the #1 killer) | high | high | Carlos | 5 providers, 7 differentiators, nothing else in v1; everything else → TASK_BACKLOG under LATER | open |
| Model catalog rot (K3, V5, Opus 5 will land) | medium | high | Claude | Catalog is TOML data; 2 wire protocols only; new model = data entry | open |
| DeepSeek V4 pricing/behavior shifts at official launch | medium | medium | Claude | Prices are data with `valid_until`; harness flags stale data, never invents (honest-cost rule); 24h email notice monitored | monitoring |
| MiMo pricing sources conflict | low | high | Codex (M1) | `verify_live = true` flag; read platform pricing page at integration | open |
| Windows sandboxing genuinely hard | medium | high | Carlos | v1 ships NO OS-level sandbox. Containment is policy-level and honest about it: exec refused on a `Block` verdict and requiring explicit approval for every other verdict at the op boundary, law `exec_block` patterns, null stdin, 300s deadline, env allowlist, verified `taskkill /T /F` that reports honestly when a re-parented descendant survives; filesystem symlink-rejection + canonical workspace containment; egress bound by the `[send]` law class and https+exact-origin credential audiences; `unsafe_code = "forbid"` workspace-wide. Job Objects REJECTED 2026-07-24 (require `unsafe`); restricted tokens are NOT implemented. | accepted — documented honestly |
| Review debt (one person + two AI builders) | high | medium | Opus 5 | Per-wave adversarial review by the orchestrator before any commit + the 4-step `gate.ps1` (fmt / clippy -D / cargo-deny / test --release) as the mechanical floor. NB: the originally-planned "no direct-to-main" rule was never adopted — the gate superseded it (ratified 2026-07-25). | open |
| MCP final spec deltas vs frozen RC (July 28) | medium | low | Codex | No SDK to pin: the client is hand-rolled (`crates/nh-tools/src/mcp.rs`) and the server is `tiny_http`. Real mitigation = `nh mcp serve` stays a loopback-only preview and no public server ships before the final spec lands 2026-07-28. There is NO MCP conformance job in CI. | monitoring |
| Dead model strings in existing configs (MiMo V2 gone June 30; DeepSeek aliases die July 24) | high | high | Carlos | Audit KORVIN/LiteLLM/LECTOR configs NOW; banned-string rejection with tests in adapter | open |
| Gemini delegate quota unreliable (unpublished, repeatedly cut) | low | high | — | Moot for v1: the subscription-delegate backend class was CUT from v1 (2026-07-16/17); only a commented catalog schema stub remains | closed (feature cut from v1) |
| Delegate quota starvation (Claude plan shared across surfaces) | medium | medium | Carlos | Moot for v1: see above — no delegate routes ship in v0.1.0 | closed (feature cut from v1) |

## Risk Template

Risk:

Why it matters:

Trigger:

Mitigation:

Owner:

Review date:
