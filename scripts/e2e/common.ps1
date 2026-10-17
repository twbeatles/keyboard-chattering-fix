#Requires -Version 5.1
# Shared helpers for the end-to-end checks in this folder. Dot-source it.
#
# Safety rules every script follows:
# - The app runs with APPDATA pointed at a throw-away folder, so the real
#   settings file is never read or written.
# - The only synthesized key is F24 (VK 0x87), which no application binds.
# - A running real instance aborts the run instead of being touched.

$ErrorActionPreference = "Stop"

$script:RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..\..")

if (-not ("KcfixE2E.Native" -as [type])) {
    Add-Type -Namespace KcfixE2E -Name Native -MemberDefinition @'
[DllImport("user32.dll", CharSet = CharSet.Unicode)]
public static extern IntPtr FindWindowExW(IntPtr parent, IntPtr after, string cls, string title);
[DllImport("user32.dll")]
public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
[DllImport("user32.dll")]
public static extern IntPtr SendMessageTimeoutW(IntPtr hwnd, uint msg, UIntPtr wParam, IntPtr lParam, uint flags, uint timeoutMs, out UIntPtr result);
[DllImport("user32.dll")]
public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
[DllImport("user32.dll")]
public static extern IntPtr GetDlgItem(IntPtr hwnd, int id);
[DllImport("user32.dll", CharSet = CharSet.Unicode)]
public static extern IntPtr SendMessageW(IntPtr hwnd, uint msg, IntPtr wParam, System.Text.StringBuilder lParam);
[DllImport("kernel32.dll")]
public static extern IntPtr OpenThread(uint access, bool inherit, uint threadId);
[DllImport("kernel32.dll")]
public static extern uint SuspendThread(IntPtr thread);
[DllImport("kernel32.dll")]
public static extern uint ResumeThread(IntPtr thread);
[DllImport("kernel32.dll")]
public static extern bool CloseHandle(IntPtr handle);
'@
}

$script:WindowClass = "KeyboardChatteringFixMain"
$script:VK_F24 = 0x87

function Assert-NoRunningInstance {
    $running = Get-Process -Name "KeyboardChatteringFix", "keyboard-chattering-fix" -ErrorAction SilentlyContinue
    if ($running) {
        throw "A Keyboard Chatter Fix instance is running (pid $($running.Id -join ', ')). Exit it first; the E2E run needs the single-instance slot."
    }
}

function Get-DebugBinaries {
    param([switch]$Build)
    $app = Join-Path $script:RepoRoot "target\debug\keyboard-chattering-fix.exe"
    $helper = Join-Path $script:RepoRoot "target\debug\kcfix-updater.exe"
    if ($Build -or -not (Test-Path $app) -or -not (Test-Path $helper)) {
        Push-Location $script:RepoRoot
        try {
            cargo build -p keyboard-chattering-fix -p kcfix-updater
            if ($LASTEXITCODE -ne 0) { throw "cargo build failed ($LASTEXITCODE)" }
        } finally {
            Pop-Location
        }
    }
    [PSCustomObject]@{ App = $app; Helper = $helper }
}

function New-TempDir([string]$Tag) {
    $dir = Join-Path $env:TEMP ("kcfix_e2e_{0}_{1}" -f $Tag, [Guid]::NewGuid().ToString("N").Substring(0, 8))
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    $dir
}

function New-TempAppData([string[]]$SettingsLines) {
    $appData = New-TempDir "appdata"
    $cfgDir = Join-Path $appData "KeyboardChatteringFix"
    New-Item -ItemType Directory -Force -Path $cfgDir | Out-Null
    $ini = Join-Path $cfgDir "settings.ini"
    $text = "[KeyboardChatteringFix]`n" + (($SettingsLines | ForEach-Object { "$_`n" }) -join "")
    [IO.File]::WriteAllText($ini, $text, (New-Object Text.UTF8Encoding($false)))
    [PSCustomObject]@{ Root = $appData; Ini = $ini }
}

function Start-AppProcess {
    param(
        [Parameter(Mandatory = $true)][string]$Exe,
        [Parameter(Mandatory = $true)][string]$AppData,
        [string[]]$Arguments = @(),
        [hashtable]$Environment = @{}
    )
    $psi = New-Object Diagnostics.ProcessStartInfo
    $psi.FileName = $Exe
    $psi.Arguments = ($Arguments -join " ")
    $psi.UseShellExecute = $false
    $psi.EnvironmentVariables["APPDATA"] = $AppData
    foreach ($k in $Environment.Keys) { $psi.EnvironmentVariables[$k] = $Environment[$k] }
    [Diagnostics.Process]::Start($psi)
}

function Wait-MainWindow {
    param([int]$ProcessId, [int]$TimeoutSec = 15)
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    while ((Get-Date) -lt $deadline) {
        $hwnd = [IntPtr]::Zero
        while ($true) {
            # [NullString]: PowerShell would marshal $null as "" (title must be "")
            $hwnd = [KcfixE2E.Native]::FindWindowExW([IntPtr]::Zero, $hwnd, $script:WindowClass, [NullString]::Value)
            if ($hwnd -eq [IntPtr]::Zero) { break }
            $owner = 0
            $thread = [KcfixE2E.Native]::GetWindowThreadProcessId($hwnd, [ref]$owner)
            if ($owner -eq $ProcessId) {
                return [PSCustomObject]@{ Hwnd = $hwnd; ThreadId = $thread }
            }
        }
        Start-Sleep -Milliseconds 100
    }
    throw "Main window of pid $ProcessId did not appear within $TimeoutSec s"
}

function Send-WindowMessage {
    param([IntPtr]$Hwnd, [uint32]$Msg, [uint64]$WParam, [int64]$LParam, [int]$TimeoutMs = 5000)
    $result = [UIntPtr]::Zero
    # SMTO_ABORTIFHUNG (2): a hung window fails the check instead of the run.
    $ok = [KcfixE2E.Native]::SendMessageTimeoutW($Hwnd, $Msg, [UIntPtr]$WParam, [IntPtr]$LParam, 2, $TimeoutMs, [ref]$result)
    if ($ok -eq [IntPtr]::Zero) { throw "SendMessageTimeout(0x{0:X}) failed or timed out" -f $Msg }
    $result.ToUInt64()
}

function Send-Key {
    param([byte]$Vk = $script:VK_F24, [int]$HoldMs = 30)
    [KcfixE2E.Native]::keybd_event($Vk, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds $HoldMs
    [KcfixE2E.Native]::keybd_event($Vk, 0, 2, [UIntPtr]::Zero)   # KEYEVENTF_KEYUP
}

# Text of a child control of the app's main window (WM_GETTEXT, cross-process safe).
function Get-ControlText {
    param([IntPtr]$Hwnd, [int]$Id)
    $child = [KcfixE2E.Native]::GetDlgItem($Hwnd, $Id)
    if ($child -eq [IntPtr]::Zero) { throw "control $Id not found" }
    $sb = New-Object Text.StringBuilder 512
    [void][KcfixE2E.Native]::SendMessageW($child, 0x000D, [IntPtr]512, $sb)   # WM_GETTEXT
    $sb.ToString()
}

# Run the app's console flags (--version, --check-language, ...) and capture
# stdout. The app is a GUI-subsystem exe, so `& exe` would neither wait nor capture.
function Invoke-AppCli {
    param([string]$Exe, [string[]]$Arguments, [hashtable]$Environment = @{})
    $psi = New-Object Diagnostics.ProcessStartInfo
    foreach ($k in $Environment.Keys) { $psi.EnvironmentVariables[$k] = $Environment[$k] }
    $psi.FileName = $Exe
    $psi.Arguments = ($Arguments -join " ")
    $psi.UseShellExecute = $false
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $p = [Diagnostics.Process]::Start($psi)
    $out = $p.StandardOutput.ReadToEnd()
    [void]$p.WaitForExit(30000)
    [PSCustomObject]@{ ExitCode = $p.ExitCode; Output = $out.Trim() }
}

function Stop-AppProcess([Diagnostics.Process]$Process) {
    if ($Process -and -not $Process.HasExited) {
        $Process.Kill()
        [void]$Process.WaitForExit(10000)
    }
}

function Get-IniValue([string]$Ini, [string]$Key) {
    foreach ($line in [IO.File]::ReadAllLines($Ini)) {
        if ($line -match "^\s*$([regex]::Escape($Key))\s*=\s*(.*)$") { return $Matches[1].Trim() }
    }
    $null
}

$script:Failures = New-Object Collections.Generic.List[string]

function Assert-Check([bool]$Condition, [string]$Message) {
    if ($Condition) {
        Write-Host "  PASS  $Message" -ForegroundColor Green
    } else {
        Write-Host "  FAIL  $Message" -ForegroundColor Red
        $script:Failures.Add($Message)
    }
}

function Complete-Run([string]$Name) {
    if ($script:Failures.Count -gt 0) {
        Write-Host "$Name : $($script:Failures.Count) check(s) failed" -ForegroundColor Red
        exit 1
    }
    Write-Host "$Name : all checks passed" -ForegroundColor Green
    exit 0
}
