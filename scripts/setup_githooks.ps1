#Requires -Version 5.1
<#
.SYNOPSIS
    저장소의 Git 훅(.githooks)을 활성화합니다. 한 번만 실행하면 됩니다.
.DESCRIPTION
    git config core.hooksPath 를 <저장소>/.githooks 절대경로로 지정해
    pre-push 게이트를 켭니다. (상대경로는 일부 Windows Git에서 해석 실패)
    이미 설정되어 있으면 아무 것도 바꾸지 않습니다.
#>
$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Push-Location $repoRoot
try {
    $top = (git rev-parse --show-toplevel 2>$null)
    if ($LASTEXITCODE -ne 0 -or -not $top) {
        throw "Git 저장소가 아닙니다. 먼저 'git init' 후 원격 저장소를 연결하세요."
    }
    $hooksDir = (Join-Path $top ".githooks").Replace("\", "/")
    $current = (git config --local --get core.hooksPath 2>$null)
    if ($current -eq $hooksDir) {
        Write-Host "이미 활성화되어 있습니다 (core.hooksPath=$hooksDir)."
        return
    }
    git config --local core.hooksPath $hooksDir
    Write-Host "Git 훅 활성화 완료: pre-push 게이트가 푸시마다 fmt/clippy/check를 검사합니다."
} finally {
    Pop-Location
}
