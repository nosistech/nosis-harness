# Deployment

## Deployment Targets

Local:

- Build and run from source on the operator's machine.
- Windows is locally verified. Linux and macOS remain unverified until remote CI and smoke
  tests execute successfully.

Staging:

- The pushed release commit plus its GitHub Actions matrix is the staging environment.
- Provider calls are not part of CI; tests use local mocks and need no secrets.

Production:

- A public Git tag and source release. There is no hosted NosisTech application server,
  database, CDN, load balancer, signup flow, or payment processor.
- `nh-mcp` remains a loopback-only process on the user's machine. It is not deployed to a
  public interface.

Current blocker (2026-07-26): this checkout has no Git remote, so remote CI and a public tag
cannot yet exist.

## Release Steps

1. Complete every item in `../03-execution/RELEASE_CHECKLIST.md`.
2. Configure and verify the intended NosisTech public remote and protected `main` branch.
3. Push the reviewed release commit; wait for Windows, Ubuntu, macOS, and supply-chain jobs.
4. Tag the exact green commit and publish source release notes.
5. Attach binaries only if built and smoked on each claimed platform; publish checksums and
   state whether each artifact is signed.

## Rollback

1. Mark the affected release as withdrawn and identify the last known-good tag.
2. Tell users to install/build that tag; do not silently move an existing tag.
3. Revoke any affected provider or release credentials.
4. Revert the faulty change on `main`, add a regression test, and rerun the complete gate.
5. Record the event in `INCIDENT_LOG.md`.

## Post-Deploy Checks

- Release tag resolves to the reviewed commit and all required checks remain green.
- Fresh checkout builds with `cargo build --locked --release`.
- `nh --version`, `nh why "quick task"`, and `nh init` smoke successfully.
- A non-loopback `nh mcp serve --addr` attempt is refused.
- README, changelog, security, privacy, and catalog freshness dates match the tag.
- Monitor the private security mailbox and public issue tracker.
