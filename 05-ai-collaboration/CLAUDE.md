# CLAUDE.md

Guidance for Claude sessions.

Use Claude for:

- Planning and spec ownership (Master Plan updates, milestone briefs for Codex)
- Architecture review and THE LAW conformance
- Security review (nh-vault, MCP posture, prompt-injection surfaces)
- Long-form research synthesis (provider/pricing updates → catalog + Appendix B deltas)
- Review gating via Opus 5 delegate (`claude -p`) — batch reviews, send receipts + diffs only

Quota nuance: plan usage is shared across all Claude surfaces (chat + Code). Two-stage review stretches quota 3–5×: Sonnet 4.6 pre-screens diffs → Opus 5 gates only what passes.

Read first:

- `../00-start-here/MASTER_CONTEXT.md`
- `../00-start-here/CURRENT_TASK.md`
- `../02-architecture/ARCHITECTURE_DECISIONS.md`
- `../NOSIS_HARNESS_Master_Plan.md` (Appendices A/B supersede §1/§3)
