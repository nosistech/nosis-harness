#requires -Version 5.1

[CmdletBinding()]
param(
    [ValidateSet("preview", "release-candidate")]
    [string]$Mode = "preview"
)

Set-StrictMode -Version 2.0
$ErrorActionPreference = "Stop"

$TargetTriple = "x86_64-pc-windows-msvc"
$PackageIdentifier = "Nosistech.NosisHarness"
$ManifestVersion = "1.12.0"
$Utf8NoBom = New-Object System.Text.UTF8Encoding($false)

function Write-Utf8NoBom {
    param(
        [Parameter(Mandatory = $true)]
        [string]$LiteralPath,
        [Parameter(Mandatory = $true)]
        [string]$Content
    )

    [System.IO.File]::WriteAllText(
        $LiteralPath,
        $Content.TrimEnd() + [Environment]::NewLine,
        $script:Utf8NoBom
    )
}

function Find-Dumpbin {
    $OnPath = Get-Command dumpbin.exe -CommandType Application -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($null -ne $OnPath) {
        return $OnPath.Source
    }

    $ProgramFilesX86 = [Environment]::GetEnvironmentVariable("ProgramFiles(x86)")
    if ([string]::IsNullOrEmpty($ProgramFilesX86)) {
        throw "dumpbin.exe is not on PATH and ProgramFiles(x86) is unavailable."
    }
    $VswherePath = Join-Path $ProgramFilesX86 "Microsoft Visual Studio\Installer\vswhere.exe"
    if (-not (Test-Path -LiteralPath $VswherePath -PathType Leaf)) {
        throw "dumpbin.exe is not on PATH and Visual Studio Installer's vswhere.exe was not found."
    }

    $Found = @(& $VswherePath -latest -products "*" `
        -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
        -find "VC\Tools\MSVC\**\bin\Hostx64\x64\dumpbin.exe")
    if ($LASTEXITCODE -ne 0) {
        throw "vswhere failed while locating dumpbin.exe with exit code $LASTEXITCODE."
    }
    $Candidates = @(
        $Found |
            ForEach-Object { $_.Trim() } |
            Where-Object { $_ -and (Test-Path -LiteralPath $_ -PathType Leaf) }
    )
    if ($Candidates.Count -eq 0) {
        throw "Visual Studio C++ tools are installed, but dumpbin.exe could not be found."
    }
    return [System.IO.Path]::GetFullPath(($Candidates | Select-Object -First 1))
}

if ($env:OS -ne "Windows_NT") {
    throw "Windows packaging must run on Windows."
}

Get-Command cargo -CommandType Application -ErrorAction Stop | Out-Null
Get-Command git -CommandType Application -ErrorAction Stop | Out-Null

$RepoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$CargoConfigPath = Join-Path $RepoRoot ".cargo\config.toml"
$LicensePath = Join-Path $RepoRoot "LICENSE"
$QuickstartPath = Join-Path $RepoRoot "docs\WINDOWS_QUICKSTART.md"

foreach ($RequiredPath in @($CargoConfigPath, $LicensePath, $QuickstartPath)) {
    if (-not (Test-Path -LiteralPath $RequiredPath -PathType Leaf)) {
        throw "Required packaging input is missing: $RequiredPath"
    }
}

$CargoConfig = Get-Content -Raw -LiteralPath $CargoConfigPath
$TargetSection = [regex]::Match(
    $CargoConfig,
    '(?ms)^\[target\.x86_64-pc-windows-msvc\]\s*\r?\n(?<body>.*?)(?=^\[|\z)'
)
if (-not $TargetSection.Success -or
    $TargetSection.Groups["body"].Value -notmatch 'target-feature=\+crt-static') {
    throw ".cargo/config.toml must enable +crt-static for $TargetTriple."
}

foreach ($VariableName in @(
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
        "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS"
    )) {
    if (-not [string]::IsNullOrEmpty([Environment]::GetEnvironmentVariable($VariableName))) {
        throw "$VariableName can override the repository's static CRT settings; unset it before packaging."
    }
}

Push-Location $RepoRoot
try {
    $MetadataOutput = @(& cargo metadata --locked --no-deps --format-version 1)
    if ($LASTEXITCODE -ne 0) {
        throw "cargo metadata failed with exit code $LASTEXITCODE."
    }
    $Metadata = ($MetadataOutput -join [Environment]::NewLine) | ConvertFrom-Json
    $CliPackages = @($Metadata.packages | Where-Object { $_.name -eq "nh-cli" })
    if ($CliPackages.Count -ne 1) {
        throw "Expected exactly one nh-cli package in cargo metadata."
    }

    $Version = [string]$CliPackages[0].version
    if ($Version -notmatch '^\d+\.\d+\.\d+(?:-[0-9A-Za-z]+(?:\.[0-9A-Za-z]+)*)?$') {
        throw "Workspace version '$Version' is not a supported package version."
    }

    $GitRootOutput = @(& git rev-parse --show-toplevel)
    if ($LASTEXITCODE -ne 0) {
        throw "Unable to locate the Git repository."
    }
    $GitRoot = [System.IO.Path]::GetFullPath(($GitRootOutput -join "").Trim())
    $TrimChars = [char[]]@('\', '/')
    if (-not $GitRoot.TrimEnd($TrimChars).Equals(
            $RepoRoot.TrimEnd($TrimChars),
            [StringComparison]::OrdinalIgnoreCase
        )) {
        throw "The packaging script must live in the repository it packages."
    }

    $CommitOutput = @(& git rev-parse --verify HEAD)
    if ($LASTEXITCODE -ne 0) {
        throw "Unable to resolve the source commit."
    }
    $SourceCommit = ($CommitOutput -join "").Trim().ToLowerInvariant()
    if ($SourceCommit -notmatch '^[0-9a-f]{40}$') {
        throw "Git returned an invalid source commit."
    }

    $StatusOutput = @(& git status --porcelain=v1 --untracked-files=all)
    if ($LASTEXITCODE -ne 0) {
        throw "Unable to inspect the working tree."
    }
    $SourceDirty = $StatusOutput.Count -gt 0
    $ExpectedTag = "v$Version"
    $SourceTag = $null

    if ($Mode -eq "release-candidate") {
        if ($SourceDirty) {
            throw "Release-candidate mode requires a clean tracked and untracked working tree."
        }

        $TaggedCommitOutput = @(& git rev-parse --verify "refs/tags/$ExpectedTag^{commit}")
        if ($LASTEXITCODE -ne 0) {
            throw "Release-candidate mode requires tag $ExpectedTag."
        }
        $TaggedCommit = ($TaggedCommitOutput -join "").Trim().ToLowerInvariant()
        if ($TaggedCommit -ne $SourceCommit) {
            throw "Tag $ExpectedTag does not resolve to HEAD ($SourceCommit)."
        }
        $SourceTag = $ExpectedTag
    }

    $ShortCommit = $SourceCommit.Substring(0, 12)
    $DistRoot = [System.IO.Path]::GetFullPath((Join-Path $RepoRoot "target\dist"))
    if ($Mode -eq "preview") {
        $OutputName = "$Version-$ShortCommit"
        if ($SourceDirty) {
            $OutputName += "-dirty"
        }
        $OutputPath = Join-Path (Join-Path $DistRoot "preview") $OutputName
    } else {
        $OutputPath = Join-Path (Join-Path $DistRoot "release-candidate") $Version
    }
    $OutputPath = [System.IO.Path]::GetFullPath($OutputPath)
    $DistPrefix = $DistRoot.TrimEnd($TrimChars) + [System.IO.Path]::DirectorySeparatorChar
    if (-not $OutputPath.StartsWith($DistPrefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing an output path outside target\dist."
    }
    if (Test-Path -LiteralPath $OutputPath) {
        throw "Output already exists; move it aside before packaging: $OutputPath"
    }

    & cargo build --locked --release -p nh-cli --target $TargetTriple
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build failed with exit code $LASTEXITCODE."
    }

    $CargoTargetRoot = [System.IO.Path]::GetFullPath([string]$Metadata.target_directory)
    $BuiltBinary = Join-Path $CargoTargetRoot "$TargetTriple\release\nh.exe"
    if (-not (Test-Path -LiteralPath $BuiltBinary -PathType Leaf)) {
        throw "The release build did not produce $BuiltBinary."
    }

    $VersionOutput = @(& $BuiltBinary --version)
    if ($LASTEXITCODE -ne 0) {
        throw "The built binary failed its version check with exit code $LASTEXITCODE."
    }
    $ReportedVersion = ($VersionOutput -join [Environment]::NewLine).Trim()
    if ($ReportedVersion -ne "nh $Version") {
        throw "The built binary reported '$ReportedVersion'; expected 'nh $Version'."
    }

    $DumpbinPath = Find-Dumpbin
    $DumpbinOutput = @(& $DumpbinPath /NOLOGO /DEPENDENTS $BuiltBinary)
    if ($LASTEXITCODE -ne 0) {
        throw "dumpbin /DEPENDENTS failed with exit code $LASTEXITCODE."
    }
    $PeImports = @(
        $DumpbinOutput |
            ForEach-Object {
                [regex]::Matches(
                    [string]$_,
                    '(?i)(?<![a-z0-9_.-])(?<dll>[a-z0-9_.-]+\.dll)(?![a-z0-9_.-])'
                ) | ForEach-Object { $_.Groups["dll"].Value.ToLowerInvariant() }
            } |
            Sort-Object -Unique
    )
    if ($PeImports.Count -eq 0) {
        throw "dumpbin /DEPENDENTS returned no parseable DLL imports for nh.exe."
    }
    $ForbiddenImports = @(
        $PeImports | Where-Object {
            $_ -match '^(?:vcruntime140.*|msvcp140.*|msvcr[0-9]+.*|ucrtbase|api-ms-win-crt-.*)\.dll$'
        }
    )
    if ($ForbiddenImports.Count -gt 0) {
        throw "nh.exe dynamically imports a C runtime DLL: $($ForbiddenImports -join ', ')."
    }
    $StaticCrtVerified = $true

    $PostBuildCommitOutput = @(& git rev-parse --verify HEAD)
    if ($LASTEXITCODE -ne 0) {
        throw "Unable to recheck the source commit after building."
    }
    $PostBuildCommit = ($PostBuildCommitOutput -join "").Trim().ToLowerInvariant()
    if ($PostBuildCommit -ne $SourceCommit) {
        throw "HEAD changed while the package was building."
    }

    $PostBuildStatus = @(& git status --porcelain=v1 --untracked-files=all)
    if ($LASTEXITCODE -ne 0) {
        throw "Unable to recheck the working tree after building."
    }
    if (($PostBuildStatus -join "`n") -ne ($StatusOutput -join "`n")) {
        throw "The working tree changed while the package was building."
    }
    $SourceDirty = $PostBuildStatus.Count -gt 0

    if ($Mode -eq "release-candidate") {
        if ($SourceDirty) {
            throw "The working tree became dirty while the package was building."
        }
        $PostBuildTagOutput = @(& git rev-parse --verify "refs/tags/$ExpectedTag^{commit}")
        if ($LASTEXITCODE -ne 0) {
            throw "Unable to recheck release tag $ExpectedTag after building."
        }
        $PostBuildTagCommit = ($PostBuildTagOutput -join "").Trim().ToLowerInvariant()
        if ($PostBuildTagCommit -ne $PostBuildCommit) {
            throw "Tag $ExpectedTag changed while the package was building."
        }
    }

    $WingetPath = Join-Path $OutputPath "winget"
    New-Item -ItemType Directory -Path $WingetPath | Out-Null

    $PortableBinary = Join-Path $OutputPath "nh.exe"
    $OutputLicense = Join-Path $OutputPath "LICENSE"
    $OutputGuide = Join-Path $OutputPath "START_HERE.md"
    Copy-Item -LiteralPath $BuiltBinary -Destination $PortableBinary
    Copy-Item -LiteralPath $LicensePath -Destination $OutputLicense
    if ($Mode -eq "preview") {
        $Quickstart = Get-Content -Raw -LiteralPath $QuickstartPath
        Write-Utf8NoBom -LiteralPath $OutputGuide -Content @"
> **PREVIEW ONLY - NOT FOR RELEASE OR WINGET SUBMISSION**
>
> This package was built locally from commit $SourceCommit (dirty: $($SourceDirty.ToString().ToLowerInvariant())).
> Its planned release URL may not exist.

$Quickstart
"@
    } else {
        Copy-Item -LiteralPath $QuickstartPath -Destination $OutputGuide
    }

    $ArchiveName = "nh-$Version-windows-x64.zip"
    $ArchivePath = Join-Path $OutputPath $ArchiveName
    $ArchiveInputs = @($PortableBinary, $OutputLicense, $OutputGuide)
    Compress-Archive -LiteralPath $ArchiveInputs -DestinationPath $ArchivePath -CompressionLevel Optimal

    $BinaryHash = (Get-FileHash -LiteralPath $PortableBinary -Algorithm SHA256).Hash.ToUpperInvariant()
    $ArchiveHash = (Get-FileHash -LiteralPath $ArchivePath -Algorithm SHA256).Hash.ToUpperInvariant()
    Write-Utf8NoBom -LiteralPath (Join-Path $OutputPath "SHA256SUMS") -Content @"
$BinaryHash  nh.exe
$ArchiveHash  $ArchiveName
"@

    $ReleaseUrl = "https://github.com/nosistech/nosis-harness/releases/download/$ExpectedTag/$ArchiveName"
    $PreviewNotice = if ($Mode -eq "preview") {
        "# PREVIEW ONLY - NOT FOR SUBMISSION. The planned release URL may not exist.`r`n"
    } else {
        ""
    }

    Write-Utf8NoBom -LiteralPath (Join-Path $WingetPath "$PackageIdentifier.yaml") -Content @"
# yaml-language-server: `$schema=https://aka.ms/winget-manifest.version.1.12.0.schema.json
${PreviewNotice}PackageIdentifier: $PackageIdentifier
PackageVersion: $Version
DefaultLocale: en-US
ManifestType: version
ManifestVersion: $ManifestVersion
"@

    Write-Utf8NoBom -LiteralPath (Join-Path $WingetPath "$PackageIdentifier.installer.yaml") -Content @"
# yaml-language-server: `$schema=https://aka.ms/winget-manifest.installer.1.12.0.schema.json
${PreviewNotice}PackageIdentifier: $PackageIdentifier
PackageVersion: $Version
InstallerType: zip
NestedInstallerType: portable
UpgradeBehavior: install
ElevationRequirement: elevationProhibited
Installers:
  - Architecture: x64
    InstallerUrl: $ReleaseUrl
    InstallerSha256: $ArchiveHash
    NestedInstallerFiles:
      - RelativeFilePath: nh.exe
        PortableCommandAlias: nh
ManifestType: installer
ManifestVersion: $ManifestVersion
"@

    Write-Utf8NoBom -LiteralPath (Join-Path $WingetPath "$PackageIdentifier.locale.en-US.yaml") -Content @"
# yaml-language-server: `$schema=https://aka.ms/winget-manifest.defaultLocale.1.12.0.schema.json
${PreviewNotice}PackageIdentifier: $PackageIdentifier
PackageVersion: $Version
PackageLocale: en-US
Publisher: nosistech LLC
PublisherUrl: https://github.com/nosistech
PublisherSupportUrl: https://github.com/nosistech/nosis-harness/issues
PackageName: Nosis Harness
PackageUrl: https://github.com/nosistech/nosis-harness
License: MIT
LicenseUrl: https://github.com/nosistech/nosis-harness/blob/$ExpectedTag/LICENSE
ShortDescription: A metered terminal agent for user-selected open-weight model routes.
Moniker: nh
Tags:
  - ai
  - cli
  - developer-tools
ManifestType: defaultLocale
ManifestVersion: $ManifestVersion
"@

    $BuildEnvironment = if ($env:GITHUB_ACTIONS -eq "true") {
        "github-actions"
    } else {
        "local"
    }
    $CiMetadata = $null
    if ($BuildEnvironment -eq "github-actions") {
        $CiMetadata = [ordered]@{}
        $CiFields = [ordered]@{
            run_id       = $env:GITHUB_RUN_ID
            run_attempt  = $env:GITHUB_RUN_ATTEMPT
            sha          = $env:GITHUB_SHA
            workflow_ref = $env:GITHUB_WORKFLOW_REF
        }
        foreach ($Field in $CiFields.GetEnumerator()) {
            if (-not [string]::IsNullOrEmpty([string]$Field.Value)) {
                $CiMetadata[$Field.Key] = [string]$Field.Value
            }
        }
        $ImageParts = @(
            @($env:ImageOS, $env:ImageVersion) |
                Where-Object { -not [string]::IsNullOrEmpty([string]$_) }
        )
        if ($ImageParts.Count -gt 0) {
            $CiMetadata["image"] = $ImageParts -join "@"
        }
    }

    $Provenance = [ordered]@{
        format_version       = 1
        mode                 = $Mode
        publication_state    = "not-published"
        build_environment    = $BuildEnvironment
        ci                   = $CiMetadata
        version              = $Version
        target               = $TargetTriple
        source_commit        = $SourceCommit
        source_dirty         = $SourceDirty
        source_tag           = $SourceTag
        static_crt           = $StaticCrtVerified
        pe_inspector         = "dumpbin /DEPENDENTS"
        pe_imports           = $PeImports
        archive              = $ArchiveName
        archive_sha256       = $ArchiveHash
        binary_sha256        = $BinaryHash
        planned_release_url  = $ReleaseUrl
        built_utc            = [DateTime]::UtcNow.ToString("o")
    }
    Write-Utf8NoBom -LiteralPath (Join-Path $OutputPath "provenance.json") -Content (
        $Provenance | ConvertTo-Json
    )

    if ($Mode -eq "preview") {
        Write-Utf8NoBom -LiteralPath (Join-Path $OutputPath "PREVIEW-NOT-FOR-RELEASE.txt") -Content @"
This is a local preview built from commit $SourceCommit (dirty: $($SourceDirty.ToString().ToLowerInvariant())).
It has not been published as a GitHub Release or submitted to WinGet.
The generated manifests describe the planned immutable release URL only and are not for submission.
"@
    }

    Write-Host "Prepared $Mode Windows package:"
    Write-Host $OutputPath
} finally {
    Pop-Location
}
