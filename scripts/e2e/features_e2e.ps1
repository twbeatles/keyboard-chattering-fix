#Requires -Version 5.1
<#
.SYNOPSIS
    End-to-end check of the v0.3 input features on the real binary: release
    delay, mouse-button filtering, wheel-reversal filtering, global hotkeys,
    CLI control of the running instance, per-app block windows, and the
    automatic pause for a listed app.
.DESCRIPTION
    Runs the app with a throw-away APPDATA (threshold 200 ms, injected input
    filtered, release delay 60 ms, mouse and wheel filters on, hotkey
    Ctrl+Alt+F23, auto-pause for charmap.exe) and checks:
    1. Release delay: F24 held, released, and bounced back 10 ms later stays
       down for Windows (async key state), and the final release is sent
       again once the delay ran out.
    2. Mouse: an X2 (forward) button chatter over the app's own window is
       blocked like a key.
    3. Wheel: a chatter-fast direction reversal (vertical and horizontal) is
       blocked; same-direction scrolling always passes (stat.0A 6/2).
    4. Hotkey: Ctrl+Alt+F23 turns protection off and on (status pill).
    5. CLI: --toggle, --pause-15, and --resume drive the running instance
       without starting a second copy.
    6. Auto-pause: with Character Map in front the pill shows "Paused" and
       F24 chatter is not counted; it resumes when the app is gone.
    7. Counters persisted on session end: F24 2/1, X2 2/1, wheel 6/2,
       protection on.
    8. Per-app window (second isolated run): with charmap.exe=100 and a
       20 ms global window, 50 ms re-presses are blocked only because the
       profile applies (stat.87 3/2).
    Never touches the real settings file. Synthesized input: F23/F24,
    Ctrl/Alt, X2 clicks, and wheel ticks. Do not type or move
    the mouse during the ~25 s run.
.PARAMETER Build
    Rebuild the debug binaries first.
.PARAMETER AppExe
    Test another build instead.
#>
param([switch]$Build, [string]$AppExe)

. (Join-Path $PSScriptRoot "common.ps1")

if (-not ("KcfixE2E.Features" -as [type])) {
    Add-Type -Namespace KcfixE2E -Name Features -MemberDefinition @'
[DllImport("user32.dll")] public static extern short GetAsyncKeyState(int vk);
[DllImport("user32.dll")] public static extern void mouse_event(uint flags, int dx, int dy, uint data, UIntPtr extra);
[DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
[DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
[DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
[DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
public struct POINT { public int X, Y; }
public struct RECT { public int Left, Top, Right, Bottom; }
'@
}

$VK_F23 = 0x86
$VK_CONTROL = 0x11
$VK_MENU = 0x12
$VK_XBUTTON2 = 0x06
$ID_PILL = 102
$ID_HOME_NOTE = 216

function Test-Down([int]$Vk) {
    ([KcfixE2E.Features]::GetAsyncKeyState($Vk) -band 0x8000) -ne 0
}

function Send-X2([bool]$Down) {
    # MOUSEEVENTF_XDOWN 0x80 / XUP 0x100, XBUTTON2 = 2
    [KcfixE2E.Features]::mouse_event($(if ($Down) { 0x80 } else { 0x100 }), 0, 0, 2, [UIntPtr]::Zero)
}

function Send-Wheel([int]$Delta, [uint32]$Flags = 0x800) {
    # MOUSEEVENTF_WHEEL 0x800 (vertical) / HWHEEL 0x1000 (horizontal).
    # dwData carries the signed tick count (±120 per notch); the low word
    # reaches the low-level hook as the signed high word, like real hardware.
    if ($Delta -lt 0) { $data = [uint32](4294967296 + $Delta) } else { $data = [uint32]$Delta }
    [KcfixE2E.Features]::mouse_event($Flags, 0, 0, $data, [UIntPtr]::Zero)
}

function Send-Hotkey {
    [KcfixE2E.Native]::keybd_event($VK_CONTROL, 0, 0, [UIntPtr]::Zero)
    [KcfixE2E.Native]::keybd_event($VK_MENU, 0, 0, [UIntPtr]::Zero)
    Send-Key -Vk $VK_F23
    [KcfixE2E.Native]::keybd_event($VK_MENU, 0, 2, [UIntPtr]::Zero)
    [KcfixE2E.Native]::keybd_event($VK_CONTROL, 0, 2, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 900   # release delay + one stats tick
}

# Matched on the ASCII end of the text: Windows PowerShell reads this file
# in the ANSI code page, so the status symbols cannot be compared.
function Wait-Pill([IntPtr]$Hwnd, [string]$Suffix, [int]$TimeoutMs = 3000) {
    $deadline = (Get-Date).AddMilliseconds($TimeoutMs)
    do {
        $got = Get-ControlText -Hwnd $Hwnd -Id $ID_PILL
        if ($got.EndsWith($Suffix)) { return $got }
        Start-Sleep -Milliseconds 100
    } while ((Get-Date) -lt $deadline)
    $got
}

Assert-NoRunningInstance
$bins = Get-DebugBinaries -Build:$Build
if ($AppExe) { $bins = [PSCustomObject]@{ App = (Resolve-Path $AppExe).Path; Helper = $null } }
$appData = New-TempAppData @(
    "iThreshold=200",
    "bFilterEnabled=1",
    "bExcludeInjected=0",
    "bMinimizeOnLaunch=0",
    "bPlaySound=0",
    "sLanguage=en",
    "iReleaseDeferMs=60",
    "bFilterMouse=1",
    "bFilterWheel=1",
    "sHotkeyToggle=Ctrl+Alt+F23",
    "sAutoDisableApps=charmap.exe"
)

$proc = $null
$charmap = $null
$cursor = New-Object KcfixE2E.Features+POINT
[void][KcfixE2E.Features]::GetCursorPos([ref]$cursor)
try {
    Write-Host "Starting $($bins.App) (APPDATA=$($appData.Root))"
    $proc = Start-AppProcess -Exe $bins.App -AppData $appData.Root
    $win = Wait-MainWindow -ProcessId $proc.Id
    [void][KcfixE2E.Features]::ShowWindow($win.Hwnd, 5)
    [void][KcfixE2E.Features]::SetForegroundWindow($win.Hwnd)
    Start-Sleep -Milliseconds 1200          # mouse hook installed, first ticks done

    Write-Host "1. Release delay"
    [KcfixE2E.Native]::keybd_event($script:VK_F24, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 80
    [KcfixE2E.Native]::keybd_event($script:VK_F24, 0, 2, [UIntPtr]::Zero)   # held back
    Start-Sleep -Milliseconds 10
    [KcfixE2E.Native]::keybd_event($script:VK_F24, 0, 0, [UIntPtr]::Zero)   # bounce: swallowed
    Start-Sleep -Milliseconds 150
    Assert-Check (Test-Down $script:VK_F24) "a bounce while held never shows the key released"
    [KcfixE2E.Native]::keybd_event($script:VK_F24, 0, 2, [UIntPtr]::Zero)   # real release
    Start-Sleep -Milliseconds 300
    Assert-Check (-not (Test-Down $script:VK_F24)) "the real release is sent after the delay"

    Write-Host "2. Mouse X2 chatter over the app window"
    $rect = New-Object KcfixE2E.Features+RECT
    [void][KcfixE2E.Features]::GetWindowRect($win.Hwnd, [ref]$rect)
    # Lower part of the Home page: empty window background.
    [void][KcfixE2E.Features]::SetCursorPos([int](($rect.Left + $rect.Right) / 2), $rect.Bottom - 120)
    Start-Sleep -Milliseconds 100
    Send-X2 $true; Start-Sleep -Milliseconds 40
    Send-X2 $false; Start-Sleep -Milliseconds 15
    Send-X2 $true; Start-Sleep -Milliseconds 40   # bounce while held: blocked
    Send-X2 $false
    Start-Sleep -Milliseconds 400
    Assert-Check (-not (Test-Down $VK_XBUTTON2)) "X2 ends up released"

    Write-Host "3. Wheel reversal filtering (vertical + horizontal)"
    Send-Wheel 120; Start-Sleep -Milliseconds 400
    Send-Wheel 120; Start-Sleep -Milliseconds 15
    Send-Wheel -120; Start-Sleep -Milliseconds 400   # chatter-fast reversal: blocked
    Send-Wheel 120; Start-Sleep -Milliseconds 400
    Send-Wheel 120 0x1000; Start-Sleep -Milliseconds 15   # horizontal axis is independent
    Send-Wheel -120 0x1000; Start-Sleep -Milliseconds 400 # horizontal reversal: blocked

    Write-Host "4. Hotkey"
    Send-Hotkey
    $pill = Wait-Pill $win.Hwnd "Protection off"
    Assert-Check ($pill.EndsWith("Protection off")) "Ctrl+Alt+F23 turns protection off (pill '$pill')"
    Send-Hotkey
    $pill = Wait-Pill $win.Hwnd "Protection on"
    Assert-Check ($pill.EndsWith("Protection on")) "Ctrl+Alt+F23 turns it back on (pill '$pill')"

    Write-Host "5. CLI control of the running instance"
    $cli = Invoke-AppCli -Exe $bins.App -Arguments @("--toggle")
    Assert-Check ($cli.ExitCode -eq 0 -and $cli.Output -match "sent --toggle") "second copy forwards --toggle instead of starting (exit $($cli.ExitCode), '$($cli.Output)')"
    $pill = Wait-Pill $win.Hwnd "Protection off"
    Assert-Check ($pill.EndsWith("Protection off")) "--toggle turns protection off (pill '$pill')"
    $cli = Invoke-AppCli -Exe $bins.App -Arguments @("--toggle")
    $pill = Wait-Pill $win.Hwnd "Protection on"
    Assert-Check ($pill.EndsWith("Protection on")) "--toggle turns it back on (pill '$pill')"
    $cli = Invoke-AppCli -Exe $bins.App -Arguments @("--pause-15")
    $pill = Wait-Pill $win.Hwnd "Paused"
    Assert-Check ($pill.EndsWith("Paused")) "--pause-15 pauses protection (pill '$pill')"
    $cli = Invoke-AppCli -Exe $bins.App -Arguments @("--resume")
    $pill = Wait-Pill $win.Hwnd "Protection on"
    Assert-Check ($pill.EndsWith("Protection on")) "--resume turns protection back on (pill '$pill')"

    Write-Host "6. Auto-pause for a listed app"
    if (-not (Get-Command "charmap.exe" -ErrorAction SilentlyContinue)) {
        Write-Host "  SKIP  charmap.exe is not available on this system"
    } else {
    $charmap = Start-Process -FilePath "charmap.exe" -PassThru
    Start-Sleep -Milliseconds 1500
    $pill = Wait-Pill $win.Hwnd "Paused"
    Assert-Check ($pill.EndsWith("Paused")) "Character Map in front pauses protection (pill '$pill')"
    $note = Get-ControlText -Hwnd $win.Hwnd -Id $ID_HOME_NOTE
    Assert-Check ($note -match "charmap") "the Home note names the app ('$note')"
    Send-Key; Start-Sleep -Milliseconds 30; Send-Key   # not counted while paused
    Start-Sleep -Milliseconds 300
    Stop-Process -Id $charmap.Id -ErrorAction SilentlyContinue
    $charmap = $null
    [void][KcfixE2E.Features]::SetForegroundWindow($win.Hwnd)
    $pill = Wait-Pill $win.Hwnd "Protection on"
    Assert-Check ($pill.EndsWith("Protection on")) "protection resumes when the app is gone (pill '$pill')"
    }

    Write-Host "7. Counters on session end"
    [void](Send-WindowMessage -Hwnd $win.Hwnd -Msg 0x0011 -WParam 0 -LParam 0x80000000)
    [void](Send-WindowMessage -Hwnd $win.Hwnd -Msg 0x0016 -WParam 1 -LParam 0x80000000)
    Stop-AppProcess $proc
    $f24 = Get-IniValue $appData.Ini "stat.87"
    Assert-Check ($f24 -eq "2,1") "F24: 2 presses / 1 blocked bounce, paused presses not counted (got '$f24')"
    $x2 = Get-IniValue $appData.Ini "stat.06"
    Assert-Check ($x2 -eq "2,1") "X2: 2 presses / 1 blocked (got '$x2')"
    $wheel = Get-IniValue $appData.Ini "stat.0A"
    Assert-Check ($wheel -eq "6,2") "Wheel: 6 ticks / 2 blocked reversals (got '$wheel')"
    Assert-Check ((Get-IniValue $appData.Ini "bFilterEnabled") -eq "1") "protection persisted as on"

    Write-Host "8. Per-app block window (isolated run)"
    if (-not (Get-Command "charmap.exe" -ErrorAction SilentlyContinue)) {
        Write-Host "  SKIP  charmap.exe is not available on this system"
    } else {
    $appData2 = New-TempAppData @(
        "iThreshold=20",
        "bFilterEnabled=1",
        "bFilterMouse=0",
        "bFilterWheel=0",
        "sAppProfiles=charmap.exe=100",
        "iMouseThreshold=30",
        "sMeasureFrom=release",
        "iMinChatterMs=0",
        "bExcludeInjected=0"
    )
    $proc2 = Start-AppProcess -Exe $bins.App -AppData $appData2.Root
    $win2 = Wait-MainWindow -ProcessId $proc2.Id
    $charmap2 = Start-Process -FilePath "charmap.exe" -PassThru
    Start-Sleep -Milliseconds 1500
    # 50 ms gaps: the global 20 ms window would let these through, but the
    # charmap profile (100 ms) blocks the quick re-press.
    Send-Key; Start-Sleep -Milliseconds 50
    Send-Key; Start-Sleep -Milliseconds 50
    Send-Key; Start-Sleep -Milliseconds 300
    Stop-Process -Id $charmap2.Id -ErrorAction SilentlyContinue
    $charmap2 = $null
    [void](Send-WindowMessage -Hwnd $win2.Hwnd -Msg 0x0011 -WParam 0 -LParam 0x80000000)
    [void](Send-WindowMessage -Hwnd $win2.Hwnd -Msg 0x0016 -WParam 1 -LParam 0x80000000)
    Stop-AppProcess $proc2
    $f24p = Get-IniValue $appData2.Ini "stat.87"
    Assert-Check ($f24p -eq "3,2") "charmap profile (100 ms) blocks 50 ms re-presses the global 20 ms lets through (got '$f24p')"
    Remove-Item -Recurse -Force $appData2.Root -ErrorAction SilentlyContinue
    }
} finally {
    if ($charmap) { Stop-Process -Id $charmap.Id -ErrorAction SilentlyContinue }
    if ($charmap2) { Stop-Process -Id $charmap2.Id -ErrorAction SilentlyContinue }
    [void][KcfixE2E.Features]::SetCursorPos($cursor.X, $cursor.Y)
    Stop-AppProcess $proc
    if ($proc2) { Stop-AppProcess $proc2 }
    Remove-Item -Recurse -Force $appData.Root -ErrorAction SilentlyContinue
    if ($appData2 -and $appData2.Root -ne $appData.Root) { Remove-Item -Recurse -Force $appData2.Root -ErrorAction SilentlyContinue }
}

Complete-Run "features_e2e"
