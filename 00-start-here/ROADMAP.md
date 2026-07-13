# Roadmap

Ten-week plan. Milestone detail and exit criteria live in `MILESTONES.md`; full spec in `../NOSIS_HARNESS_Master_Plan.md` §6.

## Phase 0: Foundation (done July 9–12, 2026)

Goal:

Context, direction, research, and first build target.

Deliverables:

- Master Plan v0.1 + Appendices A/B ✅
- Project OS folder ✅
- Repo init + root AGENTS.md ← current
- M0 brief handed to Codex

## Phase 1: Prototype (M0–M1, weeks 1–3)

Goal:

Smallest useful agent loop, then the routing brain.

Deliverables:

- M0: workspace skeleton, turn loop on deepseek-v4-flash, read/edit/exec tools, approval prompt, JSONL receipts, nh-vault.
- M1: RouteResolver + full 5-provider catalog TOML, both wire adapters, thinking dialects, clock-aware pricing, MCP client (stateless 2026-07-28), DeepSeek gotcha tests.

## Phase 2: MVP (M2–M3, weeks 3–7)

Goal:

Something Carlos uses daily instead of the incumbents.

Deliverables:

- M2: cache-first context engine (+cache-hit metric), compaction at 70%, nested constitution loader, mechanical write-holds.
- M3: TUI — semáforo status, cost HUD (dual units: tokens + quota), timeline scrubber + side-git snapshots, trust dial, `?` palette, Telegram notify. Windows renderer matrix.

## Phase 3: Fleet & Integration (M4, weeks 7–9)

Goal:

Parallelism, scheduling, and becoming a node in the orchestration layer.

Deliverables:

- Fleet ledger/workers/resume, off-peak scheduler, Kimi Swarm passthrough, escalation ladder with Opus gate route.
- nh-mcp server: route-resolver + fleet-runner exposed over MCP; KORVIN drives a fleet run.

## Phase 4: Hardening & Launch (M5, weeks 9–10)

Goal:

Ship it.

Deliverables:

- Sandbox tiers, headless `nh exec` for CI, docs.
- nosistech.com launch post (Category: AI Projects, MEDIUM risk disclaimer), CC BY 4.0 footer.
- Optional: flagship of the 55-agent Series 2 line.
