# LENS I — Data-Sovereignty & Privacy-Aware Routing for Nosis Harness

Research date: 2026-07-17. Analyst lens: product-level, cohesion-first, judged against THE LAW
(small, simple, secure, safe, lightweight, readable, auditable, modular, congruent, harmonic).

---

## 0. Why this lens is a killer differentiator (the 2026 fact base)

**The provider reality Nosis lives in is asymmetric.** All three keyed providers are Chinese
open-weight APIs, and their data postures are materially different from the US delegates:

- **DeepSeek** first-party privacy policy: "we directly collect, process and store your Personal
  Data in People's Republic of China" and uses data "to train and improve our technology, such as
  our machine learning models and algorithms" (an opt-out *right* exists but no self-serve API
  mechanism). Source: https://cdn.deepseek.com/policies/en-US/deepseek-privacy-policy.html.
  Third-party analysis of the API ToS concludes training on API-submitted data is the *default* —
  unlike OpenAI/Anthropic/Google which exclude API data from training:
  https://tokenmix.ai/blog/deepseek-api-is-it-safe.
- **Kimi / Moonshot**: OpenPlatform privacy policy permits use of user content for model
  improvement; the community flagged this loudly on the K2 model card discussion ("Their API
  Takes Your Data — PRIVACY RISK"): https://huggingface.co/moonshotai/Kimi-K2-Thinking/discussions/24 ;
  first-party policy: https://platform.kimi.ai/docs/agreement/userprivacy.
- **GLM / Z.ai** is the *odd one out in a good way*: services provided from **Singapore**, personal
  data generally processed in Singapore, and API data is stated as not used for training:
  https://docs.z.ai/legal-agreement/privacy-policy. That makes the (currently un-keyed) GLM lane
  not just the "$0 CI lane" but the **regional-privacy lane**.
- **Regulatory context**: US federal agencies (NASA, Navy, Congress) and states (NY, TX, VA)
  restricted DeepSeek on government devices starting Feb 2025
  (https://www.insideglobaltech.com/2025/02/18/u-s-federal-and-state-governments-moving-quickly-to-restrict-use-of-deepseek/);
  OpenAI publicly lobbied for bans on "PRC-produced" models
  (https://techcrunch.com/2025/03/13/openai-calls-deepseek-state-controlled-calls-for-bans-on-prc-produced-models/);
  scrutiny escalated into 2026 (https://www.insurancejournal.com/news/international/2026/01/07/853376.htm).
  EU AI Act high-risk obligations hit full enforcement **August 2026**
  (https://digital-strategy.ec.europa.eu/en/policies/regulatory-framework-ai), and the 2026 EU
  discussion has shifted from "data residency" to "technical sovereignty — who controls the stack"
  (https://lyceum.technology/magazine/eu-data-residency-ai-infrastructure/).
- **The leak problem is measured, not hypothetical**: GitGuardian's 2026 report counts **29M
  hardcoded secrets** in public GitHub in 2025 (+34% YoY) and AI-assisted commits leak secrets at
  ~**3.2% vs 1.6%** for human-only commits — double
  (https://www.turbogeek.co.uk/ai-coding-tools-secrets-leaked-2026/). CVE-2025-55284 showed a
  coding agent (Claude Code < v1.0.4) exfiltrating `.env` contents via DNS after prompt injection
  (https://brightsec.com/blog/is-your-ai-assistant-leaking-secrets-a-look-at-data-exfiltration-in-code-generation/).
- **Incumbent precedent proves the demand but not the shape**: Cursor sells "Privacy Mode" +
  zero-data-retention agreements (https://cursor.com/data-use); Anthropic sells ZDR as a
  gated enterprise feature (https://code.claude.com/docs/en/zero-data-retention); GitHub shipped
  secret scanning *inside the agent loop* via the GitHub MCP server in March 2026
  (https://github.blog/changelog/2026-03-17-secret-scanning-in-ai-coding-agents-via-the-github-mcp-server/).
  **Nobody routes by privacy.** Incumbents offer a binary privacy toggle on a single-provider
  product. Nosis is a *router* — it can make privacy a first-class routing dimension, exactly like
  clock and cache. That is the cohesion play: privacy routing isn't an 8th bolt-on feature, it is
  the same RouteResolver decision with one more constraint, the same law.toml with one more rule
  class, the same receipts with one more field.

**The honest product story**: "Nosis sends your code to the cheapest capable model — and it is the
only harness that will tell you, per turn, *which jurisdiction your code went to*, let you forbid
it per repo, and physically strip secrets before anything leaves the machine."

---

## Finding 1 — Residency & data-governance metadata in `catalog.toml` (the foundation, data-not-code)

**What.** The catalog already treats *price* as verified first-party data with confidence and
staleness flags (`price_confidence`, `valid_until` — catalog.toml:14-19). Do the *identical* thing
for data governance. Per route, add:

```toml
[routes.deepseek-v4-pro.governance]
residency = "CN"                  # jurisdiction of inference + storage
trains_on_api_data = "default-on" # "default-on" | "opt-out" | "no"
retention = "indefinite"          # "zero" | "30d" | "indefinite" | "unknown"
source_confidence = "confirmed"   # same discipline as price_confidence
valid_until = "2026-10-01"        # policies change; stale = flagged, never guessed
```

Verified values as of 2026-07: DeepSeek → CN / default-on training
(https://cdn.deepseek.com/policies/en-US/deepseek-privacy-policy.html,
https://tokenmix.ai/blog/deepseek-api-is-it-safe); Kimi → CN / trains
(https://platform.kimi.ai/docs/agreement/userprivacy); MiMo → CN; GLM/Z.ai → SG / API not trained
(https://docs.z.ai/legal-agreement/privacy-policy); delegates → US, provider-default no-training
(https://code.claude.com/docs/en/zero-data-retention).

**Why it's first.** The router cannot enforce a constraint it cannot see. Every other finding in
this lens keys off this field. And it costs almost nothing: the schema change is pure TOML +
one struct field in `RawCatalog`/`ResolvedRoute` (crates/nh-routes/src/lib.rs:144-160, 480-525).

**LAW fit.** Congruent (mirrors the existing honest-cost/price-confidence discipline exactly —
"honest-cost" becomes "honest-custody"); small; auditable; data-not-code per AGENTS.md hard rule.

**Effort: S. keyRequired: none.**

---

## Finding 2 — Privacy profile as a RouteResolver constraint (privacy joins clock/cache/modality as a routing dimension)

**What.** A per-session (user law) / per-repo (repo law) privacy profile that *filters the route
set before cost optimization*:

```toml
# .nosis/law.toml  (repo may only tighten — reuses the existing monotonic merge)
[privacy]
profile = "regional"   # "open" | "regional" (no trains_on_api_data=default-on, no CN)
                       # | "local-only" (nothing leaves the machine)
# or explicit: allow_residency = ["SG", "US", "local"]
```

**Seam.** `RouteResolver` is *the only component allowed to mint a resolved route*
(02-architecture/ARCHITECTURE_OVERVIEW.md:5). One filter inside `resolve()` /
`provider_default()` (crates/nh-routes/src/lib.rs:529-580) — routes whose `governance` violates
the profile are excluded, exactly like unpriced routes are excluded from `provider_default`
today. Because the TUI, `nh exec`, nh-fleet, and nh-mcp's `route_resolve` all funnel through this
one seam, one ~40-line change covers every surface *including the fleet and KORVIN* — that is the
harmonic payoff of the existing architecture.

**Fail-closed UX** (reuses the friendly-error pattern at nh-routes/src/lib.rs:541-545):
`"profile 'regional' excludes deepseek-v4-flash (CN, trains on API data) — capable regional
routes: glm-5.2 (SG, needs GLM key: nh vault set glm)"`. The error itself sells the GLM key
acquisition at the exact moment it matters.

**Why it's killer.** Cursor/Claude offer privacy as an account-level binary
(https://cursor.com/data-use, https://code.claude.com/docs/en/zero-data-retention). No harness
lets a *repo* say "this codebase never goes to a provider that trains on it" while other repos
still enjoy CN off-peak pricing. It converts the "all our keys are Chinese" liability into the
demo: `client-repo/` runs regional, `hobby-repo/` runs cheapest-on-earth, same tool, zero
ceremony. Directly answers the 2025-26 enterprise bans wave
(https://www.insideglobaltech.com/2025/02/18/u-s-federal-and-state-governments-moving-quickly-to-restrict-use-of-deepseek/).

**LAW fit.** Congruent (a 4th routing dimension in the existing resolver, not a new subsystem);
modular; secure. Tension: "local-only" needs Finding 6's local route to be non-empty — ship the
profile with "regional" as the flagship and "local-only" honest-failing until a local route exists.

**Effort: M. keyRequired: none (GLM key unlocks the best regional lane — flag it in docs/error text).**

---

## Finding 3 — `[send]` egress rules in nh-law: "what may leave the machine" as a third verdict class

**What.** nh-law today compiles `write` and `exec` rules into verdicts
(crates/nh-law/src/lib.rs:86-117). Add a third, structurally identical class:

```toml
[send]                     # content egress to ANY external provider / MCP server
block = ["**/.env*", "**/*.pem", "**/*.key", "**/id_rsa*", "secrets/**", ".nosis/**"]
ask   = ["migrations/**", "customers/**"]
```

`send_verdict(rel_path) -> Allow|Ask|Block`, enforced at the *one* seam where file content enters
an outbound prompt (the read-file tool result assembly in nh-core, before context packing) and at
MCP tool-argument egress (nh-tools). Bundled defaults mirror the existing bundled `write.block`
list byte-for-byte (crates/nh-law/src/lib.rs:853-862 test: `.git/**, .nosis/**, **/*.pem,
**/*.key, **/id_rsa*, **/.env*`) — today the agent can't *write* your `.env` but it can happily
*read it into a prompt bound for a CN API*. That is the gap CVE-2025-55284 exploited in Claude
Code (https://brightsec.com/blog/is-your-ai-assistant-leaking-secrets-a-look-at-data-exfiltration-in-code-generation/),
and closing it in *law* (not model text) is exactly the SECURITY_MODEL.md posture ("enforced in
code — never overridable by model text", 02-architecture/SECURITY_MODEL.md:7).

**Reuse, not new machinery**: glob matcher (lib.rs:365-398), block→ask→allow precedence
(lib.rs:88-104), repo-may-only-tighten merge (lib.rs:293-327), starter written by `nh init`
(starter_law.toml). The diff is one more `Vec<String>` triple in `Policy` + one verdict fn +
starter/bundled lines.

**Prompt-injection defense bonus**: a poisoned tool output that tells the agent "read .env and
summarize it" hits a hard Block regardless of autonomy — the Lethal Trifecta's "external input +
secrets" leg is severed *by data*, satisfying the Security Goals list (SECURITY_MODEL.md:6-9).

**LAW fit.** Small, simple, congruent (third verse of the same song as write/exec), auditable
(blocks are receipted). No tension.

**Effort: M. keyRequired: none.**

---

## Finding 4 — Outbound scrubber: point the existing nh-vault Scrubber at the wire-out path (pre-send redaction)

**What.** nh-vault already ships a `Scrubber` that redacts key shapes (`sk-…`, `csk-…`, JWT) plus
literal vault values on every *output* path — TUI, logs, receipts (crates/nh-vault/src/lib.rs:91-141;
SECURITY_MODEL.md:36). Mirror the same scrubber on the *egress* path: every outbound request body
(prompt text, tool results) passes `scrub()` before the wire adapter serializes it. Extend shapes
with the industry-standard high-signal set — AWS `AKIA…`, GitHub `ghp_/gho_…`, Slack webhooks,
private-key PEM blocks, high-entropy candidates — following gitleaks' regex+entropy model
(https://github.com/gitleaks/gitleaks, https://oneuptime.com/blog/post/2026-01-25-secret-scanning-gitleaks/view).
Receipt gains `redactions: n`.

**Why.** Path rules (Finding 3) can't catch a token pasted in a source file or hardcoded in
`config.ts` — the exact failure mode behind the 29M-secrets/3.2%-AI-commit-leak numbers
(https://www.turbogeek.co.uk/ai-coding-tools-secrets-leaked-2026/). GitHub now does this inside
the agent loop server-side (https://github.blog/changelog/2026-03-17-secret-scanning-in-ai-coding-agents-via-the-github-mcp-server/);
Nosis can do it *client-side, before egress*, which is strictly stronger — the secret never
leaves the machine, to any jurisdiction. Production LLM pipelines in 2026 treat pre-send
redaction (regex+checksum first layer) as table stakes
(https://appscale.blog/en/blog/pii-redaction-pipeline-llm-presidio-ner-reversible-tokenisation-2026);
Presidio-style reversible tokenization is the known upgrade path but NOT the MVP
(https://github.com/lotharschulz/pii-redaction-guard) — redact-and-count is small; NER/PII
engines are bloat for v1 (LAW: drop-if-hard).

**KV-cache interaction (must design, trivial to satisfy)**: redaction must be *deterministic*
(same input → same `[REDACTED]` output) so the stable prefix stays byte-stable and cache hits
(~120x cheaper) survive. A pure function already is.

**LAW fit.** Secure, safe, lightweight (regex set, no ML), congruent (same Scrubber type, second
call site — "one scrubber, both directions" is a readable invariant). Tension: false positives
could mangle code being legitimately edited — mitigate with the gitleaks-style allowlist and an
`Ask` surface ("3 strings look like live keys — send redacted / send raw / cancel").

**Effort: M. keyRequired: none.**

---

## Finding 5 — Custody in the receipts + jurisdiction glyph in the cost HUD (`nh privacy` report)

**What.** Receipts already log route/cost/tools per turn (SECURITY_MODEL.md:42-45). Add two
fields: `residency: "CN"` and `redactions: 2`. In the TUI cost HUD (nh-tui semáforo area,
ARCHITECTURE_OVERVIEW.md:9), show one glyph next to the price: `CN`, `SG`, `US`, `⌂` (local) —
the jurisdiction your bytes went to *this turn*. Add `nh privacy` (headless too): aggregates the
JSONL — "this week: 1.2M tokens → CN (deepseek, kimi), 40k → SG (glm-free CI), 0 send-blocks
triggered, 14 redactions."

**Why.** Differentiator #6 is "fixes cost opacity" — this fixes *custody opacity*, the identical
UX pain one layer up, using the identical mechanism (receipt → HUD → report). Enterprises
evaluating the 2026 bans wave need exactly this artifact for review
(https://www.insideglobaltech.com/2025/02/18/u-s-federal-and-state-governments-moving-quickly-to-restrict-use-of-deepseek/,
https://witness.ai/blog/deepseek-security-concerns/ — "security tooling had no way to flag"
conversational-AI data flows; Nosis flags it natively). It is also the *trust-building* move that
lets Nosis honestly market Chinese-provider price advantages: we never hide where the tokens go.

**LAW fit.** Auditable (its whole point), harmonic (receipt→HUD→report is the established
pattern), tiny. No tension.

**Effort: S. keyRequired: none.**

---

## Finding 6 — Zero-egress local route: `residency = "local"` via any OpenAI-wire local server (catalog data, ~zero code)

**What.** Ollama / llama.cpp / LM Studio all speak the OpenAI wire on localhost. Because the
catalog is data and the OpenAI adapter already exists, a local route is *a catalog entry, not a
feature*:

```toml
[routes."local-qwen-coder"]
provider = "local"
base_url = "http://127.0.0.1:11434/v1"
wire = "openai"
class = "api"
vault_entry = ""            # only code change: vault lookup optional for local
[routes."local-qwen-coder".governance]
residency = "local"
trains_on_api_data = "no"
[routes."local-qwen-coder".price]
currency = "USD"; cache_hit = 0.0; cache_miss = 0.0; output = 0.0
```

This makes Finding 2's `profile = "local-only"` real, gives the escalation ladder a floor below
GLM-free, and provides the only route class immune to *every* residency argument — the 2026 EU
framing is explicit that self-hosted inference "eliminates this entire category of compliance
work" (https://lyceum.technology/magazine/eu-data-residency-ai-infrastructure/), and even the US
CLOUD Act critique of "US servers ≠ sovereignty" doesn't touch localhost.

**Scope note.** This is the one idea flirting with scope creep ("now we support local models!").
Keep it brutally minimal: no model management, no downloads, no Ollama lifecycle — the user runs
their own server; Nosis just routes to it like any other OpenAI endpoint. If the vault-optional
change exceeds ~20 lines, defer.

**LAW fit.** Congruent (catalog-is-data doctrine proves itself), modular, lightweight. Tension:
capability honesty — a 7B local model is *not* "capable" for most tasks; the resolver's
capable-route logic must be allowed to say "no local route is capable of this; profile forbids
egress; here is your choice" rather than silently producing garbage.

**Effort: S (catalog + vault-optional). keyRequired: none.**

---

## Finding 7 — Wire the existing `secret-touching` classifier tag into route policy (private-pin)

**What.** The data-flow design *already* classifies each task `{modality, horizon, complexity,
deferrable, secret-touching}` (02-architecture/ARCHITECTURE_OVERVIEW.md:20). Today only
modality/clock/quota feed the resolver. Rule: a `secret-touching` turn (task references vault,
auth code, `send.ask` paths…) is resolved under the *strictest* profile available in the session
— typically local or a no-training route — regardless of price, or surfaces one calm Ask:
"this turn touches `customers/**` — run on glm-5.2 (SG, no-training) for ~$0.02 more? [y/N]".

**Why.** This is dynamic privacy routing — per *turn*, not per repo — and it's nearly free
because the tag and the policy-table seam both already exist in the design (plan §A.9). It
mirrors how the thinking-budget governor already varies spend by task; now data-custody varies
by task sensitivity too. Product story: "Nosis doesn't just pick the cheapest capable route — it
picks the cheapest capable route *your data can afford*."

**LAW fit.** Harmonic (three governors — cost, thinking, custody — same shape), simple (one row
in the policy table). Tension: classifier false negatives mean this is defense-in-*depth*, never
the only line — Findings 3/4 remain the hard floor.

**Effort: S (once the M-classifier exists; the rule itself is a policy-table row). keyRequired: none.**

---

## Finding 8 — One-question privacy onboarding in `nh init` (+ position it as differentiator #8)

**What.** `nh init` already writes `starter_law.toml` and installs the secret-blocking pre-commit
hook (SECURITY_MODEL.md:54). Add exactly one question:

```
Where may code from this repo be sent?
  1) Anywhere cheapest (CN ok — DeepSeek/Kimi/MiMo off-peak)   [default]
  2) Regional only (no providers that train on API data; SG/US/local)
  3) Never leaves this machine (local-only)
```

…and write `[privacy] profile = "…"` into the repo's `.nosis/law.toml`. That's the entire UX.
No settings page, no re-prompting, changeable by editing one TOML line or `/privacy` in the TUI
(which shows the current profile + last-10-turns custody from receipts — read-only view over
Findings 2+5).

Then update 01-product/BRAND_AND_POSITIONING.md and PRODUCT_BRIEF.md (currently templates):
privacy-aware routing is the honest answer to the #1 objection every prospect will raise in 2026
— "you route my code to China?" — now answered with "only if you say so, per repo, verifiably."
Cursor made "Privacy Mode on by default for Business" a top-of-page selling point
(https://cursor.com/data-use); Nosis's version is *stronger* (per-repo, per-route, receipted) and
costs one question.

**Why product-cohesion.** This is what turns Findings 1-7 from security plumbing into a *felt
product moment*: the calm, single decision at repo birth, honored forever, visible in the HUD.
It's the anti-approval-fatigue way to do governance (differentiator #6's philosophy applied to
privacy), and it forecloses the reputational failure mode where a launch reviewer discovers
"Windows harness silently ships your code to Chinese APIs that train on it by default"
(https://tokenmix.ai/blog/deepseek-api-is-it-safe) — instead the reviewer finds the answer in the
first 10 seconds of `nh init`.

**LAW fit.** Simple, harmonic (init → law → resolver → receipt is one straight line through
existing components). No tension.

**Effort: S. keyRequired: none (option 2 becomes far more attractive with a GLM key — the
onboarding text can say "add a free GLM key to unlock the $0 regional lane").**

---

## Interaction map (why this is ONE feature, not eight)

```
catalog.toml governance fields (F1)  ──feeds──▶ RouteResolver profile filter (F2)
        ▲                                            ▲            │
  honest-custody discipline                    [privacy] in law.toml (F8 writes it)
        │                                            │            ▼
nh-law [send] verdicts (F3) ──gates content──▶ nh-core egress seam ──▶ wire adapters
                                                     │                    ▲
nh-vault Scrubber, 2nd call site (F4) ───────────────┘        local route (F6)
        │
receipts residency+redactions (F5) ──▶ HUD glyph / nh privacy / KORVIN via nh-mcp
secret-touching tag (F7) ──▶ per-turn strictest-profile pin
```

Every arrow lands on a seam that already exists. Nothing new is invented except the fields and
verdicts themselves. That is the LAW-shaped version of a "data governance platform."

## Sequencing recommendation

M5-adjacent, in THE LAW's smallest-first order: **F1 (S) → F5 (S) → F3 (M) → F2 (M) → F8 (S)**
ship together as "custody v1" (visible + enforceable + one-question UX);
**F4 (M)** next (egress scrubber); **F6, F7 (S each)** opportunistically after.

## All sources

- https://cdn.deepseek.com/policies/en-US/deepseek-privacy-policy.html
- https://tokenmix.ai/blog/deepseek-api-is-it-safe
- https://platform.kimi.ai/docs/agreement/userprivacy
- https://huggingface.co/moonshotai/Kimi-K2-Thinking/discussions/24
- https://docs.z.ai/legal-agreement/privacy-policy
- https://www.insideglobaltech.com/2025/02/18/u-s-federal-and-state-governments-moving-quickly-to-restrict-use-of-deepseek/
- https://techcrunch.com/2025/03/13/openai-calls-deepseek-state-controlled-calls-for-bans-on-prc-produced-models/
- https://www.insurancejournal.com/news/international/2026/01/07/853376.htm
- https://witness.ai/blog/deepseek-security-concerns/
- https://digital-strategy.ec.europa.eu/en/policies/regulatory-framework-ai
- https://lyceum.technology/magazine/eu-data-residency-ai-infrastructure/
- https://www.turbogeek.co.uk/ai-coding-tools-secrets-leaked-2026/
- https://brightsec.com/blog/is-your-ai-assistant-leaking-secrets-a-look-at-data-exfiltration-in-code-generation/
- https://github.blog/changelog/2026-03-17-secret-scanning-in-ai-coding-agents-via-the-github-mcp-server/
- https://cursor.com/data-use
- https://code.claude.com/docs/en/zero-data-retention
- https://github.com/gitleaks/gitleaks
- https://oneuptime.com/blog/post/2026-01-25-secret-scanning-gitleaks/view
- https://appscale.blog/en/blog/pii-redaction-pipeline-llm-presidio-ner-reversible-tokenisation-2026
- https://github.com/lotharschulz/pii-redaction-guard

Repo grounding: catalog.toml (schema lines 1-28, routes 30-348); crates/nh-routes/src/lib.rs
(ResolvedRoute 144-160, RouteResolver 471-604); crates/nh-law/src/lib.rs (Policy verdicts 86-133,
monotonic merge 293-327, glob 365-398, bundled block list asserted at 853-862);
crates/nh-law/src/starter_law.toml; crates/nh-vault/src/lib.rs (Scrubber 91-141);
02-architecture/SECURITY_MODEL.md; 02-architecture/ARCHITECTURE_OVERVIEW.md (data flow 20-23).
