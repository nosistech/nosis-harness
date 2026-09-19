# Start using Nosis on Windows

This guide accompanies the portable Windows x64 package. You do not need Rust,
Visual Studio, or the Visual C++ Redistributable to run it.

## Check the download

This initial package is unsigned. Windows may show **Windows protected your PC**
and **Unknown publisher**. Verify the download came from the official
[nosistech/nosis-harness releases](https://github.com/nosistech/nosis-harness/releases)
and compare its SHA-256 with the accompanying `SHA256SUMS` before choosing
**More info**, then **Run anyway**. If Windows blocks it without that option,
follow your organization's policy; do not disable Windows protection.

To calculate the ZIP's checksum, use its actual filename:

```powershell
Get-FileHash -Algorithm SHA256 .\nh-VERSION-windows-x64.zip
```

The checksum detects a changed download. It is not a publisher signature.
Local preview builds are for review and are not published releases.

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
dialog, then open a new terminal. A WinGet installation manages this for you.

From the project folder, once `nh` is on PATH:

```powershell
nh init
```

This creates local configuration, receipts folders, and a secret-pattern Git hook.
Existing Git hooks are preserved. Try a route explanation without a key:

```powershell
nh why "review the diff"
```

This explains a route and estimated cost without calling a model or changing files.
Then store a provider key securely:

```powershell
nh key add deepseek
```

Enter the key at the hidden prompt; do not put it in the command line or a file.
It is stored in Windows Credential Manager. Start a session with:

```powershell
nh chat
```

Provider calls may incur charges. The program asks before executing shell commands.
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
