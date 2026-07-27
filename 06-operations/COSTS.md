# Costs

NosisTech does not process user payments in v0.1. Users call providers with their own API
accounts and are billed directly by those providers.

| Service | Billing unit | Paid by | Application control |
|---|---|---|---|
| DeepSeek | Provider tokens | User/provider account owner | Trusted catalog rate, per-call input/output caps, reported-usage receipt |
| Moonshot/Kimi | Provider tokens | User/provider account owner | Same; high-speed route is a distinct priced route |
| Xiaomi/MiMo | Provider tokens | User/provider account owner | Same |
| Z.AI/GLM | Provider tokens; some routes currently listed free | User/provider account owner | Freshness-dated catalog; free status must be rechecked |
| GitHub Actions | Repository minutes/storage | NosisTech/GitHub plan | CI timeouts and cancellation of superseded runs |

## Cost Risks

- A provider can change pricing or free-tier status before the local catalog is updated.
- Fleet's token budget is based on completed provider receipts. Already-running calls can finish
  beyond the threshold.
- The budget is a token ceiling, not a hard dollar authorization. Route rates differ.
- A single `nh run` has bounded task and output size but no per-day account budget.
- Provider-side hard caps and alerts are outside the application and are not verified by code.

## Cost Controls

- All production prices have a short `valid_until`; stale values are visibly flagged.
- Interactive profiles cap output at 16,384 tokens by default; max-quality remains bounded.
- Tasks are capped at 64 KiB; Fleet accepts at most 256 tasks and validates IDs/fields first.
- New Fleet runs require a positive observed-token budget. MCP Fleet requests are capped at
  1,000,000 tokens, four active runs, and the configured worker ceiling.
- Provider, MCP, OAuth, tool, and receipt response bodies have hard byte limits.
- The operator must configure provider-account spending alerts or hard limits where available.
  This independent provider-side control is a public-launch operations check, not something `nh`
  can create or verify.

Catalog prices and free/paid status were last rechecked 2026-07-26 and expire for trust purposes
after 2026-08-02 unless reverified.
