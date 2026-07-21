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

- [ ] `./gate.ps1` is green under the pinned toolchain **1.96.0** (`rust-toolchain.toml`).
      The gate runs, in order, each step's real exit code captured (never piped through `tail`):
  - [ ] `cargo fmt --all --check` — zero formatting drift (contributors format with `cargo fmt`).
  - [ ] `cargo clippy --workspace --all-targets --release -- -D warnings` — zero warnings.
  - [ ] `cargo test --workspace --release` — full suite, **N passed / 0 failed** (ignored count noted).
- [ ] `cargo deny check` green. (Note: cargo-deny is being wired into the gate; until it is
      an `Invoke-GateStep`, run it manually and record the result.)
- [ ] `#![forbid(unsafe_code)]` present workspace-wide — verify every crate root carries it.
- [ ] Security review of the release diff done (see [../SECURITY.md](../SECURITY.md) for scope
      and the reporting contact).
- [ ] No secrets committed: secret-pattern scan of the diff is clean (the `nh init`
      pre-commit hook pattern set, applied to the whole release range).
- [ ] [../LICENSE](../LICENSE) (MIT © nosistech LLC) and [../SECURITY.md](../SECURITY.md)
      present and current.
- [ ] [../CHANGELOG.md](../CHANGELOG.md) updated — every user-visible change in `[Unreleased]`.
- [ ] User docs current: [../README.md](../README.md) quickstart matches the actual CLI
      surface, [../PRIVACY.md](../PRIVACY.md) and [../CONTRIBUTING.md](../CONTRIBUTING.md)
      accurate for this release.
- [ ] MCP boundary respected: `nh mcp serve` is a loopback-only preview (binds `127.0.0.1`,
      bearer-token guarded). The MCP server is **NOT exposed publicly** before the MCP final
      spec lands on **2026-07-28** — confirm no docs or defaults suggest otherwise.

## Release steps

- [ ] Bump `[workspace.package] version` in the root `Cargo.toml`.
- [ ] Move the `[Unreleased]` section of [../CHANGELOG.md](../CHANGELOG.md) into
      `[X.Y.Z] - <date>` (today's date, ISO `YYYY-MM-DD`).
- [ ] Commit the version bump + changelog roll on `main`.
- [ ] Re-run `./gate.ps1` on the release commit — still green.
- [ ] Tag: `git tag vX.Y.Z` on that commit.
- [ ] Build the release binary from source (this is the install path):
      `cargo build --release` → `target/release/nh` (`target\release\nh.exe` on Windows).
- [ ] Smoke-test the built binary:
  - [ ] `nh --version` prints the new version.
  - [ ] `nh why "quick task"` returns a cheapest-capable route explanation without error.

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
