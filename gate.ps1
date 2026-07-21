# gate.ps1 - the nosis build gate. Run before every commit.
#
# Mechanizes the three checks that define "clean" for this workspace:
#   1. fmt --check   - no formatting drift (the check that would have caught the
#                      37-hunk fmt backlog before it bit a slice mid-flight).
#   2. clippy        - -D warnings, all targets, release.
#   3. test          - the full workspace suite, release.
#
# GATE RULE: each step's real exit code is captured via $LASTEXITCODE and
# aggregated - never piped through `tail` (a pipeline's exit code is the last
# command's, so `| tail` would mask a real failure with tail's 0).
#
# GOTCHA: a running nh.exe locks target\debug\nh.exe and fails the link - this
# script kills it first.
#
# FUTURE (Slice E "LOOP"): add `cargo deny check` once cargo-deny is on PATH,
# and a frozen-surface sensor that flags edits outside the milestone's mutable
# surface. Both slot in as additional Invoke-GateStep calls below.

$ErrorActionPreference = 'Continue'

Write-Host "killing any running nh.exe (locks target\debug\nh.exe)..." -ForegroundColor DarkGray
Get-Process nh -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue

$script:steps = @()
function Invoke-GateStep($name, [scriptblock]$cmd) {
    Write-Host ""
    Write-Host "=== $name ===" -ForegroundColor Cyan
    & $cmd
    $code = $LASTEXITCODE
    $script:steps += [pscustomobject]@{ Name = $name; Exit = $code }
    if ($code -ne 0) { Write-Host "$name FAILED (exit $code)" -ForegroundColor Red }
    else { Write-Host "$name ok" -ForegroundColor Green }
}

Invoke-GateStep 'fmt --check'       { cargo fmt --all --check }
Invoke-GateStep 'clippy -D warnings' { cargo clippy --workspace --all-targets --release -- -D warnings }
Invoke-GateStep 'deny check'        { cargo deny check }
Invoke-GateStep 'test --release'    { cargo test --workspace --release }

Write-Host ""
Write-Host "===== GATE SUMMARY =====" -ForegroundColor Cyan
$script:steps | ForEach-Object { Write-Host ("{0,-20} exit={1}" -f $_.Name, $_.Exit) }

$failed = @($script:steps | Where-Object { $_.Exit -ne 0 })
if ($failed.Count -gt 0) {
    Write-Host "GATE: FAIL" -ForegroundColor Red
    exit 1
}
Write-Host "GATE: PASS" -ForegroundColor Green
exit 0
