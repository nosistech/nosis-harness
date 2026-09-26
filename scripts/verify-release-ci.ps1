#requires -Version 5.1
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[0-9a-f]{40}$')]
    [string]$SourceCommit
)

Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'
$Repository = 'nosistech/nosis-harness'

# Select the newest trusted CI run for these exact bytes. A newer failing or
# incomplete run must not be hidden by an older successful run.
# Manual CI is eligible because a push event can be lost during a platform incident.
$Response = @(& gh api "repos/$Repository/actions/workflows/ci.yml/runs?head_sha=$SourceCommit&per_page=100")
if ($LASTEXITCODE -ne 0) { throw 'Could not read CI evidence; release candidate refused.' }
$Runs = @((($Response -join "`n") | ConvertFrom-Json).workflow_runs | Where-Object {
    $_.head_sha -ceq $SourceCommit -and
    $_.head_repository.full_name -ceq $Repository -and
    $_.event -in @('push', 'workflow_dispatch')
} | Sort-Object -Property id -Descending)
if ($Runs.Count -eq 0) { throw 'No trusted CI run exists for this source commit.' }
$Run = $Runs[0]
# Linux remains non-blocking under the project's platform policy. Therefore use
# explicit required-job conclusions, never the workflow's aggregate conclusion.

$Response = @(& gh api "repos/$Repository/actions/runs/$($Run.id)/attempts/$($Run.run_attempt)/jobs?per_page=100")
if ($LASTEXITCODE -ne 0) { throw 'Could not read CI job evidence; release candidate refused.' }
$Jobs = (($Response -join "`n") | ConvertFrom-Json).jobs
foreach ($RequiredName in @('Checks (windows-latest)', 'Checks (macos-latest)', 'Supply chain', 'Public tree and secrets')) {
    $MatchingJobs = @($Jobs | Where-Object { $_.name -ceq $RequiredName })
    if ($MatchingJobs.Count -ne 1 -or $MatchingJobs[0].status -ne 'completed' -or
        $MatchingJobs[0].conclusion -ne 'success' -or $MatchingJobs[0].head_sha -cne $SourceCommit) {
        throw "Required CI job '$RequiredName' did not pass on this commit."
    }
}
Write-Output "Verified exact-source CI: $($Run.html_url)"
