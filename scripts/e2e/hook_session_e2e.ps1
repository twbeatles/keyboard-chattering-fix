#Requires -Version 5.1
<#
.SYNOPSIS
    End-to-end check of the keyboard hook and session-end persistence on the
    real binary (PROJECT_AUDIT ISSUE-002/004/005).
.DESCRIPTION
    1. Starts the app with a throw-away APPDATA (threshold 200 ms, injected
       input NOT excluded so synthesized F24 presses are filtered).
    2. Sends an F24 chatter burst: press, quick re-press (must be blocked),
       slow press (must pass).
    3. Suspends the app's UI thread and sends the same burst again. The hook
       runs on its own thread, so filtering must keep working while the UI
       is frozen (with the hook on the UI thread these presses would stall
       until Windows unhooks the filter).
    4. Adds `key.41=120` to the settings file behind the app's back.
    5. Sends WM_QUERYENDSESSION / WM_ENDSESSION(TRUE), then kills the process
       the way Windows may after a logoff (no WM_DESTROY).
    6. Asserts the file holds the F24 counters (6 presses, 2 blocked), the
       external `key.41` edit, and the original settings.
    Never touches the real settings file. Do not type during the ~6 s run.
.PARAMETER Build
    Rebuild the debug binaries first.
.PARAMETER AppExe
    Test another build instead (e.g. an older commit, to confirm the checks
    catch the regressions they guard against).
#>
param([switch]$Build, [string]$AppExe)

. (Join-Path $PSScriptRoot "common.ps1")

Assert-NoRunningInstance
$bins = Get-DebugBinaries -Build:$Build
if ($AppExe) { $bins = [PSCustomObject]@{ App = (Resolve-Path $AppExe).Path; Helper = $null } }
$appData = New-TempAppData @(
    "iThreshold=200",
    "bFilterEnabled=1",
    "bExcludeInjected=0",
    "bMinimizeOnLaunch=1",
    "bPlaySound=0",
    "sLanguage=en"
)

function Send-ChatterBurst {
    Send-Key                                # 1st press: passes
    Start-Sleep -Milliseconds 60
    Send-Key                                # re-press 60 ms after release: blocked
    Start-Sleep -Milliseconds 500
    Send-Key                                # 500 ms later: passes
    # Settle past the 200 ms threshold, or the next burst's first press
    # would (correctly) count as chatter of this one.
    Start-Sleep -Milliseconds 500
}

$proc = $null
$threadHandle = [IntPtr]::Zero
try {
    Write-Host "Starting $($bins.App) (APPDATA=$($appData.Root))"
    $proc = Start-AppProcess -Exe $bins.App -AppData $appData.Root
    $win = Wait-MainWindow -ProcessId $proc.Id
    Start-Sleep -Milliseconds 800          # first stats tick; hook is installed before the window

    Write-Host "Burst 1: UI thread running"
    Send-ChatterBurst

    Write-Host "Burst 2: UI thread suspended"
    # THREAD_SUSPEND_RESUME = 0x0002
    $threadHandle = [KcfixE2E.Native]::OpenThread(0x0002, $false, $win.ThreadId)
    Assert-Check ($threadHandle -ne [IntPtr]::Zero) "opened the UI thread for suspension"
    $burstStarted = Get-Date
    [void][KcfixE2E.Native]::SuspendThread($threadHandle)
    try {
        Send-ChatterBurst
    } finally {
        [void][KcfixE2E.Native]::ResumeThread($threadHandle)
    }
    $burstMs = ((Get-Date) - $burstStarted).TotalMilliseconds
    # A hook on the frozen thread would stall every keystroke for the hook
    # timeout (up to ~1 s each); the burst itself takes ~0.8 s.
    Assert-Check ($burstMs -lt 2500) ("input was not stalled by the frozen UI thread ({0:N0} ms)" -f $burstMs)
    Start-Sleep -Milliseconds 300

    # External edit while the app runs (file-only per-key threshold).
    [IO.File]::AppendAllText($appData.Ini, "key.41=120`n")

    $allow = Send-WindowMessage -Hwnd $win.Hwnd -Msg 0x0011 -WParam 0 -LParam 0x80000000   # WM_QUERYENDSESSION, ENDSESSION_LOGOFF
    Assert-Check ($allow -eq 1) "WM_QUERYENDSESSION does not veto the logoff"
    [void](Send-WindowMessage -Hwnd $win.Hwnd -Msg 0x0016 -WParam 1 -LParam 0x80000000)     # WM_ENDSESSION(TRUE)
    Stop-AppProcess $proc                    # terminated like a real logoff: no WM_DESTROY

    $stat = Get-IniValue $appData.Ini "stat.87"
    Assert-Check ($stat -eq "6,2") "F24 counted 6 presses / 2 blocked across both bursts (got '$stat')"
    # Only meaningful once the app rewrote the file (the stat line proves it);
    # an app that never saves would leave the edit in place trivially.
    Assert-Check (($null -ne $stat) -and ((Get-IniValue $appData.Ini "key.41") -eq "120")) "external key.41 edit survived the session-end rewrite"
    Assert-Check ((Get-IniValue $appData.Ini "iThreshold") -eq "200") "UI-owned settings kept"
    Assert-Check ((Get-IniValue $appData.Ini "bFilterEnabled") -eq "1") "protection state persisted"
} finally {
    if ($threadHandle -ne [IntPtr]::Zero) { [void][KcfixE2E.Native]::CloseHandle($threadHandle) }
    Stop-AppProcess $proc
    Remove-Item -Recurse -Force $appData.Root -ErrorAction SilentlyContinue
}

Complete-Run "hook_session_e2e"
