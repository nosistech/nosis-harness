#requires -Version 5.1
Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'
$global:ReleaseCiFixtureCommit = 'a' * 40
$global:ReleaseCiFixtureScenario = 'pass'

# Offline fixture: never contact GitHub or modify repository settings.
function gh {
    param([string]$Command, [string]$Endpoint)
    $global:LASTEXITCODE = 0
    if ($global:ReleaseCiFixtureScenario -eq 'api-error') { $global:LASTEXITCODE = 1; return '' }
    if ($Endpoint -like '*/jobs?*') {
        $Jobs = @('Checks (windows-latest)', 'Checks (ubuntu-latest)',
            'Checks (macos-latest)', 'Supply chain') | ForEach-Object {
            @{ name = $_; status = 'completed'; conclusion = 'success'; head_sha = $global:ReleaseCiFixtureCommit }
        }
        if ($global:ReleaseCiFixtureScenario -eq 'missing-job') { $Jobs = $Jobs[0..2] }
        if ($global:ReleaseCiFixtureScenario -eq 'empty-jobs') { $Jobs = @() }
        if ($global:ReleaseCiFixtureScenario -eq 'duplicate-job') { $Jobs += $Jobs[0].Clone() }
        if ($global:ReleaseCiFixtureScenario -eq 'skipped-job') { $Jobs[2].conclusion = 'skipped' }
        if ($global:ReleaseCiFixtureScenario -eq 'wrong-job-source') { $Jobs[0].head_sha = 'b' * 40 }
        if ($global:ReleaseCiFixtureScenario -eq 'incomplete') { $Jobs[0].status = 'in_progress'; $Jobs[0].conclusion = $null }
        if ($global:ReleaseCiFixtureScenario -eq 'new-failure' -and $Endpoint -like '*/runs/2/*') {
            $Jobs[0].conclusion = 'failure'
        }
        if ($global:ReleaseCiFixtureScenario -eq 'linux-cancelled') { $Jobs[1].conclusion = 'cancelled' }
        if ($global:ReleaseCiFixtureScenario -eq 'windows-cancelled') { $Jobs[0].conclusion = 'cancelled' }
        if ($global:ReleaseCiFixtureScenario -eq 'macos-cancelled') { $Jobs[2].conclusion = 'cancelled' }
        if ($global:ReleaseCiFixtureScenario -eq 'supply-chain-cancelled') { $Jobs[3].conclusion = 'cancelled' }
        @{ jobs = @($Jobs) } | ConvertTo-Json -Depth 5 -Compress
        return
    }
    $Run = @{ id = 2; run_attempt = 1; head_sha = $global:ReleaseCiFixtureCommit;
        head_repository = @{ full_name = 'nosistech/nosis-harness' };
        event = 'push'; status = 'completed'; conclusion = 'success';
        html_url = 'https://example.invalid/ci/2' }
    $Runs = @($Run)
    switch ($global:ReleaseCiFixtureScenario) {
        'malformed-runs' { return '{}' }
        'manual-ci' { $Run.event = 'workflow_dispatch' }
        'no-run' { $Runs = @() }
        'wrong-source' { $Run.head_sha = 'b' * 40 }
        'fork' { $Run.head_repository.full_name = 'other/nosis-harness' }
        'pull-request' { $Run.event = 'pull_request' }
        'incomplete' { $Run.status = 'in_progress'; $Run.conclusion = $null }
        'linux-cancelled' { $Run.conclusion = 'cancelled' }
        'new-failure' {
            $Older = $Run.Clone(); $Older.id = 1
            $Run.conclusion = 'failure'; $Runs = @($Older, $Run)
        }
    }
    @{ workflow_runs = $Runs } | ConvertTo-Json -Depth 5 -Compress
}

foreach ($Case in @('pass', 'linux-cancelled', 'manual-ci', 'api-error', 'no-run', 'wrong-source', 'fork',
        'pull-request', 'incomplete', 'new-failure', 'missing-job', 'skipped-job', 'wrong-job-source',
        'empty-jobs', 'duplicate-job', 'malformed-runs', 'windows-cancelled',
        'macos-cancelled', 'supply-chain-cancelled')) {
    $global:ReleaseCiFixtureScenario = $Case
    $Rejected = $false
    try { & "$PSScriptRoot/verify-release-ci.ps1" -SourceCommit $global:ReleaseCiFixtureCommit | Out-Null }
    catch {
        if ($Case -in @('pass', 'linux-cancelled', 'manual-ci')) { throw }
        $Rejected = $true
    }
    if ($Rejected -eq ($Case -in @('pass', 'linux-cancelled', 'manual-ci'))) { throw "CI verification fixture failed: $Case" }
    Write-Output "PASS: $Case"
}
