# Source Index

Track important sources here.

| Source | Type | URL/path | Why it matters | Date checked |
|---|---|---|---|---|
|  |  |  |  |  |

## 2026-07-13 — M1 price verification pass (plan B.3 task, honest-cost rule)

All four API providers checked against their OWN pricing/docs pages (no aggregators).
Every confirmed number was written to `catalog.toml` with `price_confidence = "confirmed"`.

### DeepSeek — CONFIRMED (base rates + base URLs); peak windows announced, not yet on the pricing page

- https://api-docs.deepseek.com/quick_start/pricing (first-party, USD): deepseek-v4-flash
  $0.0028 hit / $0.14 miss / $0.28 out; deepseek-v4-pro $0.003625 / $0.435 / $0.87.
  Confirms base URLs `https://api.deepseek.com` and `https://api.deepseek.com/anthropic`,
  and the 2026-07-24 kill date for the legacy model names.
- https://api-docs.deepseek.com/zh-cn/quick_start/pricing (first-party, CNY): flash
  ¥0.02 / ¥1 / ¥2; pro ¥0.025 / ¥3 / ¥6 — matches catalog.toml exactly. Confidence stays "confirmed".
- Peak 2x (Beijing 09:00-12:00 & 14:00-18:00): NOT shown on either first-party pricing
  page as of today. Corroborated by press quoting DeepSeek's announcement (TechNode
  2026-06-30, SCMP, thenextweb): takes effect at the mid-July V4 official launch, pro
  peak = ¥0.05 / ¥6 / ¥12 — matches the catalog peak table. Left untouched;
  re-verify at valid_until 2026-07-24 when the launch settles.

### Kimi / Moonshot — CONFIRMED (all three routes + base URL); two earlier "reported" figures were wrong

- Docs host moved: platform.moonshot.ai now 301-redirects to platform.kimi.ai. The API
  host is unchanged: https://platform.kimi.ai/docs/api/overview.md confirms
  `https://api.moonshot.ai/v1` as the active base URL (no deprecation notice).
- https://platform.kimi.ai/docs/pricing/chat-k27-code.md: kimi-k2.7-code $0.19 hit /
  $0.95 miss / $4.00 out (matches catalog — confirmed); kimi-k2.7-code-highspeed
  $0.38 / $1.90 / $8.00 — CONTRADICTS the earlier reported $0.19/$0.95 input rates
  (highspeed bills 2x standard input). Catalog updated to first-party numbers.
- https://platform.kimi.ai/docs/pricing/chat-k26.md (table read verbatim, twice):
  kimi-k2.6 $0.16 hit / $0.95 miss / $4.00 out — CONTRADICTS the earlier reported
  ~$0.55-0.60 in / $2.50-2.65 out range, and the cache-hit rate is now published.
  Catalog updated. Note: prices exclude taxes (billed by jurisdiction at checkout).

### MiMo / Xiaomi — CONFIRMED (prices + base URL); plan B.3 first-party-vs-marketplace conflict RESOLVED

- platform.xiaomimimo.com docs paths 302-redirect to mimo.mi.com (Xiaomi first-party).
- https://mimo.mi.com/docs/pricing (table read verbatim, twice): mimo-v2.5-pro
  $0.0036 hit / $0.435 miss / $0.87 out; mimo-v2.5 $0.0028 / $0.14 / $0.28.
  RESOLUTION of the plan B.3 conflict: the current first-party page matches the
  marketplace-side figures ($0.435/$0.87), superseding the May 27 permanent-cut
  notice ($1 in / $3 out / $0.20 cached). The old "~$0.20 cached" figure was also
  wrong — published cache-hit rates are two orders of magnitude lower
  (DeepSeek-style cache pricing). Catalog updated, verify_live → confirmed.
- https://mimo.mi.com/docs/en-US/quick-start/summary/first-api-call: base URLs
  `https://api.xiaomimimo.com/v1` (OpenAI wire) and
  `https://api.xiaomimimo.com/anthropic` (Anthropic wire). The catalog's old
  `platform.xiaomimimo.com/v1` host was wrong — fixed on both mimo routes.
- Also listed first-party: MiMo-V2.5-Pro-UltraSpeed $0.0108 / $1.305 / $2.61 (3x
  standard, application-gated — not in catalog, recorded here for M4/backlog).

### GLM / Z.ai — CONFIRMED (all four routes + base URL)

- https://docs.z.ai/guides/overview/pricing (first-party): glm-5.2 $1.4 in / $0.26
  cached / $4.4 out — matches catalog exactly. glm-4.7-flash, glm-4.5-flash,
  glm-4.6v-flash all listed Free (input, cached, output) — zeros confirmed.
  Footnote: "Cached Input Storage" is limited-time free on paid models. Free-tier
  rate limits remain unpublished (CONTRACTS_M1 §6 ledger row stays open).
- https://docs.z.ai/guides/overview/quick-start: base URL
  `https://api.z.ai/api/paas/v4` confirmed verbatim.

### Unresolved / open items

1. DeepSeek peak 2x multiplier: announcement-only (secondary press quoting DeepSeek);
   not yet on the first-party pricing page. Re-verify on/around 2026-07-24.
2. GLM free-tier rate limits: still unpublished anywhere first-party.
3. Catalog test coupling: three nh-routes tests assert the OLD kimi-k2.6 / mimo
   confidence+prices and glm-4.7-flash "reported" — architect must refresh them
   (see PRICE VERIFICATION report, 2026-07-13).

