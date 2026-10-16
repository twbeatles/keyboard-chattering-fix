param(
    [string]$ExeName = "KeyboardChatteringFix.exe",
    [string]$HelperName = "kcfix-updater.exe",
    [string]$DistDir = "dist",
    [switch]$NoSign,
    [switch]$SkipSmokeCheck
)

$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")

function Get-SigningConfig {
    $signtool = Get-Command signtool.exe -ErrorAction SilentlyContinue
    if (-not $signtool) {
        throw "signtool.exe not found. Install Windows SDK Signing Tools, or run with -NoSign."
    }

    $signtoolArgs = @("sign", "/fd", "SHA256")

    if ($env:SIGN_PFX_PATH) {
        if (-not (Test-Path $env:SIGN_PFX_PATH)) {
            throw "SIGN_PFX_PATH not found: $($env:SIGN_PFX_PATH)"
        }
        $signtoolArgs += @("/f", $env:SIGN_PFX_PATH)
        if ($env:SIGN_PFX_PASSWORD) {
            $signtoolArgs += @("/p", $env:SIGN_PFX_PASSWORD)
        }
    } elseif ($env:SIGN_CERT_SHA1) {
        $signtoolArgs += @("/sha1", $env:SIGN_CERT_SHA1)
        if ($env:SIGN_CERT_STORE) {
            $signtoolArgs += @("/s", $env:SIGN_CERT_STORE)
        }
        if ($env:SIGN_CERT_SUBJECT) {
            $signtoolArgs += @("/n", $env:SIGN_CERT_SUBJECT)
        }
    } else {
        throw "Signing configuration missing. Set SIGN_PFX_PATH (and optional SIGN_PFX_PASSWORD) or SIGN_CERT_SHA1."
    }

    $timestampUrl = if ($env:SIGN_TIMESTAMP_URL) { $env:SIGN_TIMESTAMP_URL } else { "http://timestamp.digicert.com" }
    $signtoolArgs += @("/tr", $timestampUrl, "/td", "SHA256")
    return [PSCustomObject]@{
        SignTool = $signtool.Source
        SignArgs = $signtoolArgs
    }
}

function Invoke-Sign {
    param(
        [Parameter(Mandatory = $true)]
        [string]$ExePath,
        [Parameter(Mandatory = $true)]
        [PSCustomObject]$SigningConfig
    )
    Write-Host "Signing $ExePath"
    $signArgs = @($SigningConfig.SignArgs + $ExePath)
    & $SigningConfig.SignTool @signArgs
    if ($LASTEXITCODE -ne 0) {
        throw "signtool sign failed with exit code $LASTEXITCODE"
    }

    & $SigningConfig.SignTool verify /pa /v $ExePath
    if ($LASTEXITCODE -ne 0) {
        throw "signtool verify failed with exit code $LASTEXITCODE"
    }
}

function Test-VersionMetadata {
    $configFile = Join-Path $repoRoot "src\config.rs"
    $cargoFile = Join-Path $repoRoot "Cargo.toml"
    $configText = Get-Content $configFile -Raw
    $cargoText = Get-Content $cargoFile -Raw

    $configMatch = [regex]::Match($configText, 'pub const VERSION: &str = "([^"]+)"')
    if (-not $configMatch.Success) {
        throw "VERSION not found in $configFile"
    }
    $version = $configMatch.Groups[1].Value
    $parts = @($version.Split(".") | ForEach-Object { [int]$_ })
    if ($parts.Count -ne 3) {
        throw "VERSION must be major.minor.patch: $version"
    }
    if ($cargoText -notmatch [regex]::Escape("version = `"$version`"")) {
        throw "Cargo.toml [package] version must be $version"
    }
    Write-Host "Version metadata OK: $version"
    return $version
}

function Invoke-PackagedSmokeCheck {
    param(
        [Parameter(Mandatory = $true)]
        [string]$ExePath,
        [Parameter(Mandatory = $true)]
        [string]$Version
    )

    # The GUI build detaches stdio, so only the exit code is asserted here.
    Write-Host "Running packaged smoke check (--version): $ExePath"
    $proc = Start-Process `
        -FilePath $ExePath `
        -ArgumentList @("--version") `
        -PassThru `
        -WindowStyle Hidden
    if (-not $proc.WaitForExit(60000)) {
        try {
            $proc.Kill()
        } catch {
        }
        throw "packaged --version timed out"
    }
    if ($proc.ExitCode -ne 0) {
        throw "packaged --version failed with exit code $($proc.ExitCode)"
    }
    Write-Host "Smoke check passed (exit 0)."
}

Push-Location $repoRoot
try {
    $version = Test-VersionMetadata

    $signingConfig = $null
    if (-not $NoSign) {
        # Fail fast before build when signing prerequisites are missing.
        $signingConfig = Get-SigningConfig
    }

    Write-Host "Building Rust release binaries (keyboard-chattering-fix, kcfix-updater)"
    cargo build --release -p keyboard-chattering-fix -p kcfix-updater
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build --release failed with exit code $LASTEXITCODE"
    }

    $builtExe = Join-Path $repoRoot "target\release\keyboard-chattering-fix.exe"
    if (-not (Test-Path $builtExe)) {
        throw "Built EXE not found: $builtExe"
    }
    $builtUpdater = Join-Path $repoRoot "target\release\kcfix-updater.exe"
    if (-not (Test-Path $builtUpdater)) {
        throw "Built updater not found: $builtUpdater"
    }
    $distPath = Join-Path $repoRoot $DistDir
    New-Item -ItemType Directory -Force -Path $distPath | Out-Null
    $exePath = Join-Path $distPath $ExeName
    Copy-Item -LiteralPath $builtExe -Destination $exePath -Force
    Write-Host "Copied $builtExe -> $exePath"
    $updaterDist = Join-Path $distPath $HelperName
    Copy-Item -LiteralPath $builtUpdater -Destination $updaterDist -Force
    Write-Host "Copied $builtUpdater -> $updaterDist"

    if ($SkipSmokeCheck) {
        Write-Host "Skipping packaged smoke check (-SkipSmokeCheck)."
    } else {
        Invoke-PackagedSmokeCheck -ExePath $exePath -Version $version
    }

    if ($NoSign) {
        Write-Host "Skipping code signing (-NoSign)."
    } else {
        Invoke-Sign -ExePath $exePath -SigningConfig $signingConfig
        Invoke-Sign -ExePath $updaterDist -SigningConfig $signingConfig
    }

    $zipPath = Join-Path $distPath "KeyboardChatteringFix.zip"
    Compress-Archive -Force -Path $exePath, $updaterDist -DestinationPath $zipPath
    Write-Host "Packed $zipPath"

    Write-Host "Done: $exePath (v$version)"
} finally {
    Pop-Location
}
