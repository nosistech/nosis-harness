# Check a Windows installation

Use this checklist to record an actual installation, not to infer success from CI.
Keep your normal antivirus, firewall and Windows download protections enabled.
Kaspersky or another active antivirus is a valid test environment; Defender does
not have to run alongside it. Do not switch products or add an exclusion for this test.

## Record the environment

Record Windows version, standard/admin user status, antivirus product and protection
status, executable version and SHA-256. Omit usernames, machine IDs and license keys.
Note whether this machine already has Rust, Node.js, Visual C++ runtimes or Nosis
configuration. A developer laptop test is useful but is not a clean-machine test.

## Download and first start

1. Download the release ZIP using a browser and follow the
   [Windows quickstart](WINDOWS_QUICKSTART.md). Record the actual download/publisher
   warnings. An antivirus detection is different from an unknown-publisher warning:
   stop and report a detection rather than bypassing it.
2. Verify the checksum and any supplied [build provenance](RELEASE_VERIFICATION.md).
   Record the exact ZIP/executable hashes. Unsigned status remains a disclosed limit.
3. Extract to a new user-owned folder whose name includes spaces. Call the binary
   directly and check `--version`, `doctor` and `--help` without changing system PATH.
4. In a new practice project, run setup, decline key entry and any provider request.
   Confirm the preview is clearly an estimate. Cancel once and inspect what remains.
5. With an already stored key, verify setup recognizes it without showing its value.
   An actual model task is separate and can incur provider charges.

## Upgrade and removal

1. Keep an older verified executable and a practice project with recorded file hashes.
   Close only your test Nosis instance and replace that test copy with the new binary.
2. Confirm project files, receipts and stored-key presence are retained. Inspect the
   [catalog migration](CONFIGURATION.md) preview, decline once, then accept only a
   recognized bundled catalog. Verify the retained backup matches the original bytes.
3. Confirm a customized catalog is refused and preserved. Check saved-model recovery
   and explicit model override if supported by the tested version.
4. Remove only the test program folder and any user PATH entry you deliberately added.
   Confirm project files and history remain. Credential removal is separate and explicit;
   see [privacy and deletion](../PRIVACY.md). Never delete real credentials for this test.
5. Once WinGet accepts the package, verify installation, discovery in a fresh terminal,
   upgrade and uninstall through the public source. A passing manifest check is not
   proof of a successful public installation. Do not advertise availability prematurely.

## Result record

| Check | Observed result | Evidence |
| --- | --- | --- |
| Download and warnings | Not run | |
| Version/hash identity | Not run | |
| No-key setup and cancellation | Not run | |
| Existing-key setup | Not run | |
| Upgrade preserves data | Not run | |
| Catalog backup/refusal | Not run | |
| Removal preserves user data | Not run | |
| Public WinGet lifecycle | Not run | |

Fill in only observed results. Record limitations separately, such as an existing
developer environment or unavailable clean VM. A completed checklist is evidence
for that configuration, not a promise that antivirus will accept every future build.
