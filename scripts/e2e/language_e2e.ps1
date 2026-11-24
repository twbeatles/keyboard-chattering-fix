#Requires -Version 5.1
<#
.SYNOPSIS
    End-to-end check of UI language selection on the real binary.
.DESCRIPTION
    Starts the app once per `sLanguage` value (throw-away APPDATA) and reads
    the header subtitle the window actually shows. Korean text contains
    Hangul and Japanese text contains kana, so the check does not depend
    on wording.
      - sLanguage=ko / en / ja      -> that language, whatever Windows uses
      - sLanguage=auto, key missing,
        unknown value ("klingon")    -> Korean on a Korean Windows display
                                       language, Japanese on a Japanese one,
                                       otherwise English
    The automatic language is read from `--check-language`.
    Finally, a session-end save must write `sLanguage=auto` for a settings
    file that had no language key (a fresh install keeps following Windows).
.PARAMETER Build
    Rebuild the debug binaries first.
#>
param([switch]$Build)

. (Join-Path $PSScriptRoot "common.ps1")

Assert-NoRunningInstance
$bins = Get-DebugBinaries -Build:$Build
$ID_HEADER_SUB = 101

# What the app says "Auto" resolves to here (its detection is unit-tested
# against locale names; this script verifies the wiring to the window).
$cli = Invoke-AppCli -Exe $bins.App -Arguments @("--check-language")
if ($cli.ExitCode -ne 0 -or $cli.Output -notmatch '-> (ko|en|ja)\s*$') { throw "--check-language failed: $($cli.Output)" }
[void]($cli.Output -match '-> (ko|en|ja)\s*$')
$expectedAuto = $Matches[1]
Write-Host $cli.Output

function Get-ScriptOf([string]$Text) {
    $chars = @($Text.ToCharArray() | ForEach-Object { [int]$_ })
    if (($chars | Where-Object { $_ -ge 0xAC00 -and $_ -le 0xD7A3 }).Count -gt 0) { return "ko" }
    # Hiragana + Katakana (full width); Japanese UI text always has kana.
    if (($chars | Where-Object { ($_ -ge 0x3041 -and $_ -le 0x3096) -or ($_ -ge 0x30A1 -and $_ -le 0x30FF) }).Count -gt 0) { return "ja" }
    return "en"
}

function Get-ShownLanguage([string[]]$Lines) {
    $appData = New-TempAppData $Lines
    $proc = $null
    try {
        $proc = Start-AppProcess -Exe $bins.App -AppData $appData.Root
        $win = Wait-MainWindow -ProcessId $proc.Id
        Start-Sleep -Milliseconds 600
        $text = Get-ControlText -Hwnd $win.Hwnd -Id $ID_HEADER_SUB
        $shown = Get-ScriptOf $text
        [PSCustomObject]@{ Language = $shown; Text = $text; Win = $win; Proc = $proc; AppData = $appData }
    } catch {
        Stop-AppProcess $proc
        Remove-Item -Recurse -Force $appData.Root -ErrorAction SilentlyContinue
        throw
    }
}

$cases = @(
    @{ Name = "sLanguage=ko";       Lines = @("sLanguage=ko");      Want = "ko" },
    @{ Name = "sLanguage=en";       Lines = @("sLanguage=en");      Want = "en" },
    @{ Name = "sLanguage=ja";       Lines = @("sLanguage=ja");      Want = "ja" },
    @{ Name = "sLanguage=KO (legacy case)"; Lines = @("sLanguage=KO"); Want = "ko" },
    @{ Name = "sLanguage=auto";     Lines = @("sLanguage=auto");    Want = $expectedAuto },
    @{ Name = "no language key";    Lines = @("iThreshold=50");     Want = $expectedAuto },
    @{ Name = "sLanguage=klingon";  Lines = @("sLanguage=klingon"); Want = $expectedAuto }
)
foreach ($case in $cases) {
    $r = Get-ShownLanguage $case.Lines
    try {
        Assert-Check ($r.Language -eq $case.Want) "$($case.Name): shows $($case.Want) (got $($r.Language): '$($r.Text)')"
    } finally {
        Stop-AppProcess $r.Proc
        Remove-Item -Recurse -Force $r.AppData.Root -ErrorAction SilentlyContinue
    }
}

# Non-Korean systems get English. This PC cannot change its Windows language
# for a test, so KCFIX_LOCALE (documented diagnostic override) stands in.
foreach ($pair in @(@("en-US", "en"), @("de-DE", "en"), @("ja-JP", "ja"), @("zh-Hans-CN", "en"), @("ko-KR", "ko"), @("ko", "ko"))) {
    $cli = Invoke-AppCli -Exe $bins.App -Arguments @("--check-language") -Environment @{ KCFIX_LOCALE = $pair[0] }
    Assert-Check ($cli.Output -eq "system language: $($pair[0]) -> $($pair[1])") "--check-language with KCFIX_LOCALE=$($pair[0]) reports $($pair[1]) ($($cli.Output))"

    $appData = New-TempAppData @("sLanguage=auto")
    $proc = $null
    try {
        $proc = Start-AppProcess -Exe $bins.App -AppData $appData.Root -Environment @{ KCFIX_LOCALE = $pair[0] }
        $win = Wait-MainWindow -ProcessId $proc.Id
        Start-Sleep -Milliseconds 500
        $text = Get-ControlText -Hwnd $win.Hwnd -Id $ID_HEADER_SUB
        $shown = Get-ScriptOf $text
        Assert-Check ($shown -eq $pair[1]) "auto + system $($pair[0]): window shows $($pair[1]) (got $shown)"
    } finally {
        Stop-AppProcess $proc
        Remove-Item -Recurse -Force $appData.Root -ErrorAction SilentlyContinue
    }
}

# Changing the picker retranslates at once and is remembered. Rows: 0 =
# Auto, 1 = Korean, 2 = English, 3 = Japanese
# (CB_SETCURSEL + CBN_SELCHANGE, as a click does).
function Set-LanguageRow($Win, [int]$Row) {
    $combo = [KcfixE2E.Native]::GetDlgItem($Win.Hwnd, 415)   # ID_COMBO_LANG
    [void](Send-WindowMessage -Hwnd $combo -Msg 0x014E -WParam $Row -LParam 0)          # CB_SETCURSEL
    [void](Send-WindowMessage -Hwnd $Win.Hwnd -Msg 0x0111 -WParam ((1 -shl 16) -bor 415) -LParam $combo.ToInt64())   # WM_COMMAND, CBN_SELCHANGE
    Start-Sleep -Milliseconds 400
}
function Get-ShownFromWindow($Win) {
    $text = Get-ControlText -Hwnd $Win.Hwnd -Id $ID_HEADER_SUB
    Get-ScriptOf $text
}
$appData = New-TempAppData @("sLanguage=auto")
$proc = $null
try {
    $proc = Start-AppProcess -Exe $bins.App -AppData $appData.Root -Environment @{ KCFIX_LOCALE = "ko-KR" }
    $win = Wait-MainWindow -ProcessId $proc.Id
    Start-Sleep -Milliseconds 500
    Assert-Check ((Get-ShownFromWindow $win) -eq "ko") "picker test starts in Korean (auto on a Korean system)"
    Set-LanguageRow $win 2
    Assert-Check ((Get-ShownFromWindow $win) -eq "en") "picking English retranslates immediately"
    Assert-Check ((Get-IniValue $appData.Ini "sLanguage") -eq "en") "picking English is saved as sLanguage=en"
    Set-LanguageRow $win 0
    Assert-Check ((Get-ShownFromWindow $win) -eq "ko") "picking Auto follows the (Korean) system again"
    Assert-Check ((Get-IniValue $appData.Ini "sLanguage") -eq "auto") "picking Auto is saved as sLanguage=auto"
    Set-LanguageRow $win 1
    Assert-Check ((Get-IniValue $appData.Ini "sLanguage") -eq "ko") "picking Korean is saved as sLanguage=ko"
    Set-LanguageRow $win 3
    Assert-Check ((Get-ShownFromWindow $win) -eq "ja") "picking Japanese retranslates immediately"
    Assert-Check ((Get-IniValue $appData.Ini "sLanguage") -eq "ja") "picking Japanese is saved as sLanguage=ja"
} finally {
    Stop-AppProcess $proc
    Remove-Item -Recurse -Force $appData.Root -ErrorAction SilentlyContinue
}

# A fresh install (no language key) must keep following Windows after a save.
$r = Get-ShownLanguage @("iThreshold=50")
try {
    [void](Send-WindowMessage -Hwnd $r.Win.Hwnd -Msg 0x0016 -WParam 1 -LParam 0x80000000)   # WM_ENDSESSION(TRUE)
    Stop-AppProcess $r.Proc
    Assert-Check ((Get-IniValue $r.AppData.Ini "sLanguage") -eq "auto") "a save without a stored choice writes sLanguage=auto"
} finally {
    Stop-AppProcess $r.Proc
    Remove-Item -Recurse -Force $r.AppData.Root -ErrorAction SilentlyContinue
}

Complete-Run "language_e2e"
