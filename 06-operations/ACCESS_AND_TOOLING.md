# Access And Tooling

## Tools

- Git and the public repository host: source, review, CI, release tags, private vulnerability intake.
- Rust 1.96.0: build, test, clippy, rustfmt check.
- `cargo-deny` 0.20.2: advisories, bans, licenses, and source policy.
- Provider consoles: key creation/revocation, spend limits, billing alerts, and usage investigation.
- OS credential manager: local `nosis-harness` entries.

## Accounts

Do not store passwords or API keys here.

| Account | Needed for | Secret location |
|---|---|---|
| NosisTech repository owner | branch protection, CI, releases, security reports | Repository host account controls |
| DeepSeek/Moonshot/Xiaomi/Z.AI operator accounts | direct model routes | Provider console; local copy in OS vault |
| Release signer (only if binaries are signed) | artifact signing | Dedicated signing service/store, never this repo |

## Local Services

| Service | URL/port | Purpose |
|---|---|---|
| `nh-mcp` preview | `http://127.0.0.1:8765` by default | Local bearer-guarded routing/Fleet tools |

## Secrets Rule

Never commit secrets or paste them into docs, prompts, receipts, issue reports, or test fixtures.
Use `nh key add <entry>` for interactive storage. Use repository/CI secret controls for the
`NH_<ENTRY>_KEY` fallback and expose it only to jobs that need it. Public CI must stay keyless.

## First Hour After a Credential Leak

1. Revoke the credential at the provider first. Local deletion alone does not stop a copied key.
2. Run `nh key remove <entry>` and remove every matching CI/environment fallback.
3. Create a replacement with the least provider scope and a spending limit/alert, then store it
   with `nh key add <entry>`.
4. Inspect provider usage/billing and the time window of exposure. Stop active Fleet runs if needed.
5. Scan the working tree and complete Git history without printing matches. If a secret reached
   history, treat it as permanently disclosed even after a normal deletion.
6. Record impact, rotation, and regression work in `INCIDENT_LOG.md`; notify affected users if a
   public release or shared system was involved.

## Release-Credential Leak

Revoke the repository/release credential, remove active sessions, audit branch/tag/release changes,
rotate signing material if applicable, and do not reuse or silently move a compromised tag.
