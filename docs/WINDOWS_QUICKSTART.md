# Start using Nosis on Windows

This guide accompanies the portable Windows x64 package. You do not need Rust,
Visual Studio, or the Visual C++ Redistributable to run it.

For the published version, download the ZIP from the
[v0.2.2 release](https://github.com/nosistech/nosis-harness/releases/tag/v0.2.2).
The minimum path is: verify the download, extract it, open your project in a
terminal and run setup. Adding Nosis to PATH is optional.

## Check the download

This package is unsigned. Windows may show **Windows protected your PC**
and **Unknown publisher**. Verify the download came from the official
[nosistech/nosis-harness releases](https://github.com/nosistech/nosis-harness/releases)
and compare its SHA-256 with the accompanying `SHA256SUMS` before choosing
**More info**, then **Run anyway**. If Windows blocks it without that option,
follow your organization's policy; do not disable Windows protection.

In the folder containing the download, open a terminal and calculate the checksums
of Nosis ZIP files. Compare the row for the file you downloaded:

```powershell
Get-FileHash -Algorithm SHA256 .\nh-*-windows-x64.zip
```

The checksum detects a changed download. It is not a publisher signature.
If antivirus reports a malware detection, stop and report it; do not add an
exclusion or treat that as the ordinary unknown-publisher warning above.
Local preview builds are for review and are not published releases.
Future releases that include build attestations can also be checked using the
[release verification guide](https://github.com/nosistech/nosis-harness/blob/main/docs/RELEASE_VERIFICATION.md).
Published v0.2.2 does not include that attestation.

## Open a terminal

Extract the ZIP to a folder you want to keep. In File Explorer, open that folder,
right-click an empty area, and choose **Open in Terminal**. Run:

```powershell
.\nh.exe doctor
```

The report needs no API key. It tells you what is configured and how to fix anything
missing. `PATH: nh not found` is expected when running a portable copy directly;
you can continue using `.\nh.exe` from its folder.

## Use it on a project

Open a terminal in your project folder. Call the executable using its full path,
for example `& 'C:\Users\you\Apps\Nosis\nh.exe' doctor`.
For shorter commands from any folder, add the folder containing `nh.exe` to your
**user** PATH through Windows' **Edit environment variables for your account**
dialog, then open a new terminal. Once the pending WinGet listing is approved,
its installation will manage this for you.

For v0.2.2 or later, start the guided setup from your project folder:

```powershell
& 'C:\Users\you\Apps\Nosis\nh.exe' setup
```

Use your actual executable path. If `nh` is on PATH, just run `nh setup` or `nh`.
The guide confirms the folder, checks the installation, and lets you choose a cloud
model. It offers a free price preview, secure key entry, and an optional chat.
Current unreleased source offers a read-only first task instead; the downloaded
v0.2.2 does not enforce read-only task permissions.
Existing configuration and keys are preserved; missing ignore entries may be added.
Press Enter to decline a yes/no question, or type `cancel` to stop.

You need an API key from your chosen provider to send model requests. Enter it only
at the hidden prompt; it is stored in Windows Credential Manager. Do not put it in
the command line, a task message, or a file. Provider calls may incur charges.
The program asks before executing shell commands.

Setup prints commands for returning to the selected model. For the full walkthrough,
see the [guided setup guide](https://github.com/nosistech/nosis-harness/blob/main/docs/GETTING_STARTED.md).
The [practice tasks](https://github.com/nosistech/nosis-harness/blob/main/docs/TUTORIALS.md)
walk through a free preview, a small reviewed edit and returning to work. v0.3.0-rc.1 candidate
commands are labeled so you can distinguish them from your installed version.

Run `nh --help` for commands. For a local model instead of a cloud API, follow
the [local models guide](https://github.com/nosistech/nosis-harness/blob/main/docs/LOCAL_MODELS.md).

## Update or remove

For a portable download, close Nosis and replace its executable with the newer
verified download. To remove it, delete its program folder and remove any PATH
entry you added. Your project files and `.nosis` receipts remain in your projects.
Use `nh key remove deepseek` before deleting the executable if you also want to
remove that saved key. See the
[privacy guide](https://github.com/nosistech/nosis-harness/blob/main/PRIVACY.md)
for configuration, receipts, and session locations.

If setup reports an untrusted catalog after an upgrade, it has preserved your older
project catalog. Back it up outside the project before intentionally replacing it
with the new bundled catalog; the guided setup guide explains the steps. Never trust
a changed catalog without reviewing its provider destinations.
v0.3.0-rc.1 candidate builds also offer an explicit `nh catalog migrate` command for
recognized older bundled catalogs. This command is not in v0.2.2; see the current
[configuration guide](https://github.com/nosistech/nosis-harness/blob/main/docs/CONFIGURATION.md)
for its consent, backup and recovery behavior.
