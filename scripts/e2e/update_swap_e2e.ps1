#Requires -Version 5.1
<#
.SYNOPSIS
    End-to-end check of the update swap with the real helper, twice in a row
    (PROJECT_AUDIT ISSUE-001/003).
.DESCRIPTION
    Sets up a throw-away install folder (app + kcfix-updater.exe) and, for two
    consecutive rounds, does what the app does after a verified download:
    stages a helper copy in %TEMP%, runs it with the app's command line
    (`--timeout-secs`, `--sha256`), and lets it swap in a replacement, relaunch
    the app, and wait for its readiness handshake. Each replacement carries a
    distinct marker appended to the PE file, so the check knows which bytes
    were installed.
    Per round it asserts: helper exit 0, the marker is installed, no
    `.exe.old` backup or staged replacement is left, the relaunched app
    acknowledged and runs, and `--check-helper` still accepts the helper next
    to the updated app (the helper itself is never replaced by an update).
    The network/signature half of the update is covered by unit tests.
.PARAMETER Build
    Rebuild the debug binaries first.
.PARAMETER HelperExe
    Use this helper instead of the freshly built one, e.g. a helper from an
    already published release (`gh release download v0.2.0 -p kcfix-updater.exe`)
    to prove the current app still drives helpers that installs already have.
#>
param([switch]$Build, [string]$HelperExe)

. (Join-Path $PSScriptRoot "common.ps1")

Assert-NoRunningInstance
$bins = Get-DebugBinaries -Build:$Build
$install = New-TempDir "install"
$appData = New-TempAppData @("bMinimizeOnLaunch=1", "bPlaySound=0", "sLanguage=en")
$app = Join-Path $install "KeyboardChatteringFix.exe"
Copy-Item $bins.App $app
$helperSource = if ($HelperExe) { (Resolve-Path $HelperExe).Path } else { $bins.Helper }
Write-Host "Helper under test: $helperSource"
Copy-Item $helperSource (Join-Path $install "kcfix-updater.exe")

function Get-Tail([string]$Path, [int]$Count) {
    $bytes = [IO.File]::ReadAllBytes($Path)
    [Text.Encoding]::ASCII.GetString($bytes, $bytes.Length - $Count, $Count)
}

function Stop-InstalledApp {
    Get-Process -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -eq $app } |
        ForEach-Object { $_.Kill(); [void]$_.WaitForExit(10000) }
}

$staged = @()
try {
    & $app --check-helper | Out-Null
    Assert-Check ($LASTEXITCODE -eq 0) "fresh install: --check-helper accepts the helper"

    foreach ($round in 1..2) {
        Write-Host "Round $round"
        $marker = "KCFIX-E2E-ROUND-$round"
        $replacement = Join-Path $env:TEMP ("KeyboardChatteringFix_update_e2e_{0}_{1}.exe" -f $round, [Guid]::NewGuid().ToString("N").Substring(0, 8))
        Copy-Item $bins.App $replacement
        # Bytes appended after the PE image are ignored by the loader.
        $fs = [IO.File]::Open($replacement, "Append")
        try { $m = [Text.Encoding]::ASCII.GetBytes($marker); $fs.Write($m, 0, $m.Length) } finally { $fs.Dispose() }
        $sha = (Get-FileHash -Algorithm SHA256 $replacement).Hash.ToLowerInvariant()

        # The app runs a staged copy of the helper, never the one in place.
        $helperCopy = Join-Path $env:TEMP ("kcfix-updater_helper_e2e_{0}.exe" -f [Guid]::NewGuid().ToString("N").Substring(0, 8))
        Copy-Item (Join-Path $install "kcfix-updater.exe") $helperCopy
        $staged += $helperCopy, $replacement

        $args = @("--pid", "0", "--current", "`"$app`"", "--replacement", "`"$replacement`"",
                  "--timeout-secs", "120", "--sha256", $sha)
        $helper = Start-AppProcess -Exe $helperCopy -AppData $appData.Root -Arguments $args -Environment @{ KCFIX_LANG = "en" }
        $finished = $helper.WaitForExit(120000)
        if (-not $finished) { $helper.Kill() }
        Assert-Check ($finished -and $helper.ExitCode -eq 0) "helper finished with exit 0 (exit $($helper.ExitCode))"

        Assert-Check ((Get-Tail $app $marker.Length) -eq $marker) "round $round replacement is installed"
        Assert-Check (-not (Test-Path "$app.old") -and -not (Test-Path ($app -replace '\.exe$', '.exe.old'))) "no .exe.old backup left"
        Assert-Check (-not (Test-Path $replacement)) "staged replacement was moved, not copied"
        $running = @(Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $app })
        Assert-Check ($running.Count -eq 1) "updated app relaunched and acknowledged readiness"

        Stop-InstalledApp
        & $app --check-helper | Out-Null
        Assert-Check ($LASTEXITCODE -eq 0) "after round $round : --check-helper still accepts the helper in place"
    }
} finally {
    Stop-InstalledApp
    foreach ($f in $staged) { Remove-Item -Force $f -ErrorAction SilentlyContinue }
    Remove-Item -Recurse -Force $install, $appData.Root -ErrorAction SilentlyContinue
}

Complete-Run "update_swap_e2e"
