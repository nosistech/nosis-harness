# Windows package preparation

The supported distribution target is Windows x64. The package contains a portable
`nh.exe`; users need neither a compiler nor the Visual C++ Redistributable. WinGet
will manage the command's PATH entry for a per-user installation.

**Availability:** Windows binaries are distributed through
[GitHub Releases](https://github.com/nosistech/nosis-harness/releases). The WinGet
listing is pending [catalog approval](https://github.com/microsoft/winget-pkgs/pull/438174).
These tools prepare artifacts for review. They do not publish a
release, submit a package, install anything, or change PATH.

## Build a local preview

From the repository root on Windows, with its pinned Rust toolchain installed:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\package-windows.ps1
```

The execution-policy option applies only to that process running the local script;
it does not change a stored policy or override organizational policy.
The script runs a locked release build for `x86_64-pc-windows-msvc`, verifies the
reported version, and writes a new directory under
`target\dist\preview\<version>-<commit>[-dirty]`. It refuses to overwrite an existing
directory. Move a previous output aside before preparing a replacement.

The output contains:

- `nh.exe` for a direct portable download, with `LICENSE` and `START_HERE.md` beside it.
- `nh-<version>-windows-x64.zip` containing only `nh.exe`, `LICENSE`, and
  `START_HERE.md` (the [Windows quickstart](WINDOWS_QUICKSTART.md)).
- `SHA256SUMS` for the executable and ZIP.
- `provenance.json` recording the source revision, working-tree status, target,
  version, and packaging mode.
- Three WinGet manifests under `winget`, using the ZIP's actual SHA-256.

Preview artifacts and their manifests are marked **not for release**. Their
planned download URLs are not proof that a release exists. In particular, the
existing `v0.2.0` source tag does not represent newer local changes.

Validate the generated manifests using the installed WinGet client, substituting
the actual output path:

```powershell
winget validate --manifest .\target\dist\preview\<output>\winget
```

Manifest validation checks structure. It does not prove the planned download URL
is live, test installation, or establish acceptance into the WinGet catalog.

## Prepare a release candidate

Choose a new version, update the workspace version and lockfile, and run the full
repository gate. Commit the reviewed changes and confirm CI on that exact commit
before creating its matching `v<version>` tag. These publication operations require
maintainer authorization.

Then prepare the candidate:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\package-windows.ps1 -Mode release-candidate
```

This mode refuses a dirty tree or a tag that does not resolve to the current commit.
Its output goes under `target\dist\release-candidate\<version>`. Both modes refuse
an existing output directory; neither deletes prior builds.

The manual **Package Windows** GitHub Actions workflow runs the same script and
uploads artifacts with read-only repository permissions. Select the intended tag
and `release-candidate` mode for a candidate. It does not publish a GitHub Release.
Downloading an Actions artifact is a maintainer review path, not the public user
installation path.

## Make installation available

After release approval:

1. Test the candidate on Windows with Defender enabled, including a standard-user
   WinGet install, `nh doctor`, upgrade, and uninstall. Use an isolated test machine
   for local-manifest installation settings. Check that the ZIP extracts correctly
   and its binary matches the standalone executable and checksums.
2. Attach the verified executable, ZIP, `SHA256SUMS`, `LICENSE`, and quickstart to
   the matching GitHub Release. Use the exact ZIP filename from the manifests.
   Publish the provenance record alongside them. Upload the same bytes that were
   validated; rebuilding requires new checksums and manifests.
3. Update README's installation section and `docs/ARCHITECTURE_OVERVIEW.md` together
   with the first binary release. Link actual assets, describe the unsigned Windows
   warning before the first launch, and retain source instructions for contributors.
4. Submit the three generated manifests to
   `microsoft/winget-pkgs` under
   `manifests/n/Nosistech/NosisHarness/<version>/`. The release download must be
   public first. Follow Microsoft's
   [manifest submission guidance](https://learn.microsoft.com/en-us/windows/package-manager/package/manifest).
5. After catalog acceptance, verify the command below from a normal user account
   before advertising it as available.

The intended user command, **not available yet**, is:

```powershell
winget install --id Nosistech.NosisHarness --exact --source winget --scope user
```

Open a fresh terminal afterward, run `nh doctor`, and follow its setup guidance.
Subsequent catalog releases use `winget upgrade --id Nosistech.NosisHarness --exact
--source winget`; removal uses `winget uninstall --id Nosistech.NosisHarness --exact`.
Removal leaves project receipts and separately stored credentials intact.

The first binary is unsigned. Checksums detect changed bytes; they do not replace
code signing or guarantee that a compromised release account published safe bytes.
