# Release Checklist

The release gate for the Nosis Harness (`nh`). Every box must be checked, in order,
before a tag exists. Keep this list congruent with THE LAW: small, simple, secure,
safe, lightweight, readable, auditable, modular, congruent, harmonic.

Quality expectations for what "done" means live in [QUALITY_BAR.md](QUALITY_BAR.md).
The running build history lives in [../00-start-here/BUILD_LOG.md](../00-start-here/BUILD_LOG.md).

## Versioning & Tags

- [ ] Version follows SemVer. Pre-1.0 (`0.y.z`): a **minor** bump MAY contain breaking
      changes; a patch bump must not.
- [ ] The single source of truth for the version is the root `Cargo.toml`
      `[workspace.package] version` (currently `0.1.0`). No crate overrides it.
- [ ] Git tag format is `vX.Y.Z` and matches the workspace version exactly.
- [ ] Tag ONLY from `main`, ONLY after the pre-release gate below is fully green.

## Pre-release gate (all must pass)

- [ ] Working tree contains only reviewed release changes. No temporary tests, generated
      credentials, local receipts, Fleet ledgers, or build logs are staged.
- [ ] Mandatory debug gate is green:
  - [ ] `cargo test --locked --workspace`
  - [ ] `cargo clippy --locked --workspace --all-targets -- -D warnings`
- [ ] `./gate.ps1` is green under the pinned toolchain **1.96.0** (`rust-toolchain.toml`).
      The gate runs, in order, each step's real exit code captured (never piped through `tail`):
  - [ ] `cargo fmt --all --check` — zero formatting drift (contributors format with `cargo fmt`).
  - [ ] `cargo clippy --locked --workspace --all-targets --release -- -D warnings` — zero warnings.
  - [ ] `cargo deny --locked check` — supply-chain gate green (advisories / bans / licenses / sources).
  - [ ] `cargo test --locked --workspace --release` — full suite, **N passed / 0 failed** (ignored count noted).
- [ ] Supply-chain policy (`deny.toml`) is green and enforced by the gate; no RustSec advisory is
      `ignore`d without a documented rationale.
- [ ] The pushed release commit has green GitHub Actions jobs on **Windows, Ubuntu, and macOS**,
      plus the supply-chain job. A configured matrix is not evidence until those remote jobs run.
- [ ] Dependabot is enabled for Cargo and GitHub Actions on the public repository.
- [ ] **No test fixture can age out.** No `valid_until` (or similar date) in a `#[cfg(test)]` fixture
      is a near-future real date. Fixtures that must read "fresh" use the far-future sentinel
      `2099-01-01` (convention set in `crates/nh-mcp/src/lib.rs`); fixtures that must read "stale" use
      an explicitly past date and/or an injected clock. Rationale: on 2026-07-25 the `METER_CATALOG`
      fixture in `crates/nh-tui/src/lib.rs` aged out at its hardcoded `2026-07-24` and turned the gate
      red — the product was correct (fail-closed on stale FX), the fixture was a time bomb, and CI
      would have gone red daily. `cmd_chat.rs` deliberately keeps an injected-clock freshness
      boundary fixture; do not replace that boundary with the far-future sentinel.
- [ ] Unsafe is forbidden workspace-wide via `[workspace.lints.rust] unsafe_code = "forbid"` (each
      crate opts in with `[lints] workspace = true`).
- [ ] Security review of the release diff done (see [../SECURITY.md](../SECURITY.md) for scope
      and the reporting contact).
- [ ] No secrets committed: the canonical `nh-vault`/`nh init` key-shape set is clean across
      both the release diff **and all reachable Git history**. Inspect every match without printing
      the candidate value; documented fake/redaction fixtures are the only allowed matches.
- [ ] [../LICENSE](../LICENSE) (MIT © nosistech LLC) and [../SECURITY.md](../SECURITY.md)
      present and current.
- [ ] **[../SECURITY.md](../SECURITY.md) audit statement is true for THIS release.** It names both
      the 2026-07-20 audit and the stricter 2026-07-21 result (**2 critical / 14 high**), links the
      report, and identifies current regression evidence for C-01 and C-02 without claiming a release
      before the tag exists.
- [ ] [../CHANGELOG.md](../CHANGELOG.md) updated — every user-visible change in `[Unreleased]`.
- [ ] User docs current: [../README.md](../README.md) quickstart matches the actual CLI
      surface, [../PRIVACY.md](../PRIVACY.md) and [../CONTRIBUTING.md](../CONTRIBUTING.md)
      accurate for this release.
- [ ] Current product/architecture documents do not claim automatic routing, delegate execution,
      snapshot restore, OS sandboxing, remote notifications, or telemetry that the build does not have.
- [ ] `catalog.toml` prices, limits, URLs, and free/paid status were rechecked against first-party
      sources recently enough that every production `valid_until` includes the release date.
- [ ] Owner completes the Windows FEEL pass in the actual terminal used day to day.
- [ ] MCP boundary respected: `nh mcp serve` is a loopback-only preview (binds `127.0.0.1`,
      bearer-token guarded). Public source availability does not authorize exposing it on a public
      interface; confirm no docs, defaults, proxy examples, or deployment files suggest otherwise.

## Public repository readiness

- [ ] A public Git remote exists, `origin` points to the intended NosisTech repository, branch
      protection is configured for `main`, and required CI jobs are enforced.
- [ ] Private vulnerability reporting or an equivalent private intake is enabled, and
      `info@nosistech.com` is monitored for the five-business-day response target.
- [ ] Repository description, topics, license, security policy, privacy statement, and support
      boundary describe a local source-install CLI, not a hosted service.
- [ ] The operations documents name the credential-rotation, catalog-expiry, incident, and rollback
      procedures. No blank template is presented as an active runbook.

## Release steps

- [ ] Bump `[workspace.package] version` in the root `Cargo.toml`.
- [ ] Move the `[Unreleased]` section of [../CHANGELOG.md](../CHANGELOG.md) into
      `[X.Y.Z] - <date>` (today's date, ISO `YYYY-MM-DD`).
- [ ] Commit the version bump + changelog roll on `main`.
- [ ] Re-run `./gate.ps1` on the release commit — still green.
- [ ] Push the release commit and wait for every required remote CI job to pass.
- [ ] Tag: `git tag vX.Y.Z` on that commit.
- [ ] Build the release binary from source (this is the install path):
      `cargo build --release` → `target/release/nh` (`target\release\nh.exe` on Windows).
- [ ] Smoke-test the built binary:
  - [ ] `nh --version` prints the new version.
  - [ ] `nh why "quick task"` returns a cheapest-capable route explanation without error.
  - [ ] `nh init` in a temporary Git repository is idempotent and reports an existing user hook.
  - [ ] `nh mcp serve --addr 0.0.0.0:8765` fails before binding.
- [ ] If binaries are attached, build each on its claimed platform, publish SHA-256 checksums,
      identify them as signed or unsigned, and smoke-test the exact uploaded bytes. Otherwise publish
      a source-install release only and say so explicitly.

## Post-release

- [ ] Push the tag: `git push origin vX.Y.Z` (and the release commit on `main`).
- [ ] Publish release notes from the `[X.Y.Z]` CHANGELOG entry.
- [ ] Watch [../06-operations/INCIDENT_LOG.md](../06-operations/INCIDENT_LOG.md) for
      post-release reports; triage anything filed against the new tag first.

## Rollback

- [ ] Revert to the previous tag / release: check out the prior `vX.Y.Z`, rebuild with
      `cargo build --release`, and point users at that tag as current.
- [ ] Record the incident — what shipped, what broke, why the gate missed it — in
      [../06-operations/INCIDENT_LOG.md](../06-operations/INCIDENT_LOG.md).
- [ ] Do not re-release until the gate has a check that would have caught the failure.
