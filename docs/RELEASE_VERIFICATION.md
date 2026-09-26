# Verify a Windows release

**Version boundary:** published v0.2.2 is unsigned and does not have the new build
attestation. Use its release checksum instructions. The v0.3.0-rc.1 candidate
[packaging and attestation run](https://github.com/nosistech/nosis-harness/actions/runs/35791434747)
passed. Both downloaded subjects were verified with the saved bundle and GitHub
lookup against source `796f94840b8f528532e1a2e24e3a098e412e0c74` and tag
`v0.3.0-rc.1`. The candidate remains a draft and lacks the newer source changes.
Clean installation verification is still pending; publisher signing is optional
future work. Do not assume an old asset has been signed or attested
retroactively.

## Check the source and bytes

Download from [nosistech/nosis-harness releases](https://github.com/nosistech/nosis-harness/releases).
Keep the ZIP and its `SHA256SUMS` together. Substitute the downloaded version:

```powershell
Get-FileHash -Algorithm SHA256 .\nh-VERSION-windows-x64.zip
```

Compare the entire hash with the ZIP's line in `SHA256SUMS`. A mismatch means stop:
do not extract or run that download. A matching checksum detects corruption or
substitution relative to that checksum file, but does not establish publisher identity.

## Check build provenance when the release includes it

This optional advanced check uses the [GitHub CLI](https://cli.github.com/).
Obtain the release's full source commit from the repository's tag/commit view.
Use the commit the tag points to, not an annotated tag object's own SHA.
Replace `VERSION` and `FULL_COMMIT_SHA` before running:

```powershell
gh attestation verify .\nh-VERSION-windows-x64.zip --repo nosistech/nosis-harness --predicate-type https://slsa.dev/provenance/v1 --signer-workflow nosistech/nosis-harness/.github/workflows/package-windows.yml --source-ref refs/tags/vVERSION --source-digest FULL_COMMIT_SHA --deny-self-hosted-runners
```

The same command can verify the standalone `nh.exe` by changing the file argument.
The CLI retrieves the attestation from GitHub; a downloaded bundle can instead be
supplied with `--bundle .\attestation.json`. Do not remove repository, workflow,
tag or commit restrictions to make a failed check pass. Missing or failed provenance
for a release that promises it needs investigation before use.
One JSON bundle covers both subjects: the executable and the ZIP.

Build provenance binds the artifact digest to the signing workflow and source
identity. It does not establish that the code is bug-free, reproduce the build,
or provide Windows Authenticode signing. An unsigned executable can still display
Unknown publisher or SmartScreen warnings. Follow your organization's policy.
The workflow comes from the selected source revision; maintainers must still review
that source and protect release tags. The separate `provenance.json` file is unsigned
packaging metadata, not the cryptographically verified attestation bundle.

## Maintainer release checklist

1. Finish review and the local gate, commit, then verify required CI on that exact
   source. Dispatch Package Windows on its matching version tag in release-candidate mode.
2. Require both packaging and attestation jobs to pass. Download both artifacts;
   preserve the exact executable, ZIP and verification bundle. Do not rebuild them.
   Actions artifacts are retained for 14 days; retain the bundle with the release.
3. Verify the downloaded ZIP and executable with the command above and confirm
   the ZIP's contents, version and key-free first run. A preview build is ineligible.
   Test the command without `--bundle` too, allowing for GitHub indexing delay,
   before claiming the public attestation lookup works.
4. Complete a clean standard-user installation, upgrade and removal test, including
   paths with spaces, existing configuration and credentials. Record actual results;
   manifest validation is not a substitute.
5. Stage all assets in a draft release. Review GitHub's immutable-release setting
   before publishing; this workflow does not enable it. Verify the public downloads
   again after publication. Never replace an already published asset silently.

Publisher signing still requires a verified signing account and a separately
reviewed sign-before-package step. This work does not enroll or purchase one.

References: [GitHub attestations](https://docs.github.com/en/actions/concepts/security/artifact-attestations),
[verification flags](https://cli.github.com/manual/gh_attestation_verify),
[immutable releases](https://docs.github.com/en/code-security/concepts/supply-chain-security/immutable-releases).
