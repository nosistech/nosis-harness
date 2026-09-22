#requires -Version 5.1
[CmdletBinding()]
param(
    [string]$Executable = '',
    [switch]$Check
)
Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'
$RepoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
if (-not $Executable) { $Executable = Join-Path $RepoRoot 'target/release/nh.exe' }
$Executable = (Resolve-Path -LiteralPath $Executable -ErrorAction Stop).Path
$Destination = Join-Path $RepoRoot 'docs/CLI_REFERENCE.md'
$Sections = @('', 'setup', 'init', 'catalog', 'catalog migrate', 'model',
    'model set', 'model show', 'model clear', 'key', 'key add', 'key remove',
    'run', 'chat', 'doctor', 'resume', 'why', 'profile', 'tui',
    'fleet', 'fleet run', 'fleet resume', 'mcp', 'mcp serve')
$Document = [Collections.Generic.List[string]]::new()
$Document.Add('# Command reference')
$Document.Add('')
$Document.Add('Generated from the current source build. Unreleased commands may not exist in the')
$Document.Add('latest downloadable package. Run `nh --help` to check your installed version.')
$Document.Add('For a learning path, start with [three small tasks](TUTORIALS.md).')
$Document.Add('')
foreach ($Section in $Sections) {
    $Arguments = @()
    if ($Section) { $Arguments += $Section.Split(' ') }
    $Arguments += '--help'
    $Lines = @(& $Executable @Arguments)
    if ($LASTEXITCODE -ne 0) { throw "Help failed for nh $Section." }
    $Heading = if ($Section) { "nh $Section" } else { 'nh' }
    $Document.Add("## $Heading")
    $Document.Add('')
    $Document.Add('```text')
    $Document.Add((($Lines | ForEach-Object { $_.TrimEnd() }) -join "`n").TrimEnd())
    $Document.Add('```')
    $Document.Add('')
}
$Text = ($Document -join "`n").TrimEnd() + "`n"
if ($Check) {
    if (-not (Test-Path -LiteralPath $Destination) -or
        [IO.File]::ReadAllText($Destination).Replace("`r`n", "`n") -cne $Text) {
        throw 'CLI reference is stale. Build nh-cli, then run scripts/generate-cli-reference.ps1.'
    }
    Write-Output 'CLI reference matches executable help.'
} else {
    [IO.File]::WriteAllText($Destination, $Text, [Text.UTF8Encoding]::new($false))
    Write-Output 'Updated docs/CLI_REFERENCE.md from executable help.'
}
