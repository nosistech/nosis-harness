# Costs

NosisTech does not process user payments in v0.1. Users call providers with their own API
accounts and are billed directly by those providers.

| Service | Billing unit | Paid by | Application control |
|---|---|---|---|
| DeepSeek | Provider tokens | User/provider account owner | Trusted catalog rate, per-call input/output caps, reported-usage receipt |
| Moonshot/Kimi | Provider tokens | User/provider account owner | Same; high-speed route is a distinct priced route |
| Xiaomi/MiMo | Provider tokens | User/provider account owner | Same |
| Z.AI/GLM | Provider tokens; some routes currently listed free | User/provider account owner | Free status is recorded with its source date and is not monitored; it can change without notice |
| GitHub Actions | Repository minutes/storage | NosisTech/GitHub plan | CI timeouts and cancellation of superseded runs |

## Cost Risks

- A provider can change pricing or free-tier status before the local catalog is updated.
- Fleet's token budget is based on completed provider receipts. Already-running calls can finish
  beyond the threshold.
- The budget is a token ceiling, not a hard dollar authorization. Route rates differ.
- A single `nh run` has bounded task and output size but no per-day account budget.
- Provider-side hard caps and alerts are outside the application and are not verified by code.

## Cost Controls

- All production prices carry a `price_confidence` label and a first-party citation. Prices do not
  expire and the harness never claims one is currently fresh.
- Interactive profiles cap output at 16,384 tokens by default; max-quality remains bounded.
- Tasks are capped at 64 KiB; Fleet accepts at most 256 tasks and validates IDs/fields first.
- New Fleet runs require a positive observed-token budget. MCP Fleet requests are capped at
  1,000,000 tokens, four active runs, and the configured worker ceiling.
- Provider, MCP, OAuth, tool, and receipt response bodies have hard byte limits.
- The operator must configure provider-account spending alerts or hard limits where available.
  This independent provider-side control is a public-launch operations check, not something `nh`
  can create or verify.

Catalog prices and free/paid status were last checked against first-party sources on 2026-07-26
(Kimi K3 on 2026-07-28). That is a historical fact, not a deadline: prices do not expire, and no
recheck is scheduled or required. Accepted tradeoff — a silent provider price change is metered
wrong until a human notices and edits `catalog.toml`.
