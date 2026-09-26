# Model updates checked September 22, 2026

**Unreleased source catalog.** These additions are not in the v0.2.2 download or
the existing v0.3.0-rc.1 draft. All four routes passed a small live text/tool-call
check on September 22, 2026: read a harmless file and return its marker word.
GLM required another run after initial connection failures; those failed attempts
did not report usage, so their cost is unknown. This verifies that
small workflow, not general reliability, image support or comparative task quality.

| New route | Why consider it | Current integration |
| --- | --- | --- |
| `mimo-v2.6-flash` | Lower token price in the new MiMo family | Text tasks |
| `mimo-v2.6-pro` | Alternative for tasks you want to compare with Flash | Text tasks |
| `glm-5.3-flash` | Lower token price than the GLM flagship; visual inputs | Text and image tasks |
| `glm-5.3` | Current GLM flagship | Text tasks |

A cheaper token rate does not guarantee a cheaper completed task. Keep the model
that produces correct results with acceptable time and cost for your work.
Premium speed variants are not included in this update.

## Existing MiMo users

In its notice checked September 22, 2026, Xiaomi says `mimo-v2.5` and `mimo-v2.5-pro` stop working on **October 21, 2026 at
10:00 Beijing time**, without automatic replacement.
[Official retirement notice](https://mimo.mi.com/docs/en-US/updates/deprecate).

**For v0.2.2 users:** no published Nosis build contains these new routes yet.
Keep your current installation and watch the [releases page](https://github.com/nosistech/nosis-harness/releases)
for a build that includes them. Rerunning v0.2.2 setup cannot add these routes.
If you need them before a release, the source must first be published and built
using [the contributor instructions](../CONTRIBUTING.md); there is no ready download
for this working tree. You can explicitly choose another supported provider meanwhile.

The commands below require a build containing the new catalog. After installing it, run this in your project:

```powershell
nh catalog migrate
```

Review the changes and backup location before confirming. Existing model choices
are not silently replaced. Custom catalogs require manual review. See
[catalog migration](CONFIGURATION.md#upgrade-your-project-catalog-v030-rc1-candidate).

To try the new Flash route explicitly:

```powershell
nh run "Explain the purpose of this project's README" --read-only --model mimo-v2.6-flash
```

The route uses your existing `mimo` credential entry. To make it your saved choice:

```powershell
nh model set mimo-v2.6-flash
```

Use `mimo-v2.6-pro` instead if that is the route you choose. New MiMo routes currently
advertise text only because the provider's new modality documentation is inconsistent.
This update does not claim verified image, video or audio integration for them.
[MiMo API contract](https://mimo.mi.com/docs/en-US/api/chat/openai-api).

## GLM and other providers

The new GLM routes use the ordinary API endpoint and `glm` credential entry.
Subscription coding-plan quota is a separate service. GLM 5.3 requires reasoning;
`--think none` becomes `low` for these routes: reasoning stays enabled and its
tokens can be billed. It is not a way to turn reasoning charges off.
Its default posture uses the provider's documented `max` effort; explicit
`--think high` requests less effort, and `--profile frugal` uses `low`. More effort
can consume more reasoning tokens. Inspect `nh profile --model glm-5.3` before a
long task; choosing a profile does not guarantee a final bill.
[GLM model contract](https://docs.z.ai/guides/llm/glm-5.3).

Existing DeepSeek Flash already calls the current canonical provider model. Existing
Kimi routes remain available. Their presence is not a promise of account eligibility,
rate-limit availability or permanent provider pricing. Catalog prices are estimates;
the provider's bill remains authoritative.

MiMo routes and Kimi K3 now use the documented `max_completion_tokens` request field
for their output ceiling. This includes reasoning tokens where the provider specifies
that behavior. It does not create a hard spending cap across calls or retries.
