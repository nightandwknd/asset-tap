# Asset Tap CLI installer for Windows -- https://assettap.dev/install.ps1
#
#   irm https://assettap.dev/install.ps1 | iex
#
# Pin a version (any release tag) via environment variable:
#
#   $env:ASSET_TAP_VERSION = "v26.8.12"; irm https://assettap.dev/install.ps1 | iex
#
# Installs asset-tap.exe and the atap alias into %LOCALAPPDATA%\AssetTap\bin
# and adds that directory to your user PATH. Override the destination:
#
#   $env:ASSET_TAP_INSTALL_DIR = "C:\tools\asset-tap"; irm https://assettap.dev/install.ps1 | iex
#
# What it does, in order: download the CLI zip for the requested release from
# GitHub Releases -> verify it against the release's SHA256SUMS -> extract ->
# copy into the install dir -> register the dir on your user PATH -> print
# next steps. Runs on Windows PowerShell 5.1 (preinstalled) and PowerShell 7+.
#
# The desktop app has its own installer:
# https://github.com/nightandwknd/asset-tap/releases/latest

$ErrorActionPreference = "Stop"

function Install-AssetTap {
    if ($env:OS -ne "Windows_NT") {
        Write-Host "This installer is for Windows. On macOS/Linux use:" -ForegroundColor Red
        Write-Host "  curl -fsSL https://assettap.dev/install | bash"
        exit 1
    }

    # Windows PowerShell 5.1 defaults to TLS 1.0 -- GitHub requires 1.2+.
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

    $Repo = "nightandwknd/asset-tap"
    $Asset = "asset-tap-cli-windows.zip"
    $Tag = if ($env:ASSET_TAP_VERSION) { $env:ASSET_TAP_VERSION } else { "latest" }
    $InstallDir = if ($env:ASSET_TAP_INSTALL_DIR) { $env:ASSET_TAP_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA "AssetTap\bin" }

    # releases/latest/download redirects without the GitHub API -- no rate limits.
    if ($Tag -eq "latest") {
        $Base = "https://github.com/$Repo/releases/latest/download"
    } else {
        $Base = "https://github.com/$Repo/releases/download/$Tag"
    }

    $Tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("asset-tap-install-" + [System.IO.Path]::GetRandomFileName())
    New-Item -ItemType Directory -Path $Tmp -Force | Out-Null

    try {
        # -- Download + verify ------------------------------------------------
        Write-Host "  downloading $Asset ($Tag)" -ForegroundColor Green
        try {
            Invoke-WebRequest -UseBasicParsing -Uri "$Base/$Asset" -OutFile (Join-Path $Tmp $Asset)
            Invoke-WebRequest -UseBasicParsing -Uri "$Base/SHA256SUMS.txt" -OutFile (Join-Path $Tmp "SHA256SUMS.txt")
        } catch {
            Write-Host "  download failed: $Base/$Asset (does the release exist?)" -ForegroundColor Red
            exit 1
        }

        $SumLine = Select-String -Path (Join-Path $Tmp "SHA256SUMS.txt") -Pattern ([regex]::Escape($Asset)) | Select-Object -First 1
        if (-not $SumLine) {
            Write-Host "  checksum manifest has no entry for $Asset -- refusing to install" -ForegroundColor Red
            exit 1
        }
        $Expected = ($SumLine.Line -split "\s+")[0].ToLower()
        $Actual = (Get-FileHash -Algorithm SHA256 -Path (Join-Path $Tmp $Asset)).Hash.ToLower()
        if ($Expected -ne $Actual) {
            Write-Host "  checksum verification FAILED for $Asset -- refusing to install" -ForegroundColor Red
            exit 1
        }
        Write-Host "  checksum verified" -ForegroundColor Green

        # -- Install ----------------------------------------------------------
        Expand-Archive -Path (Join-Path $Tmp $Asset) -DestinationPath (Join-Path $Tmp "extracted") -Force
        New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
        # The zip carries asset-tap.exe and atap.cmd (a short alias).
        Copy-Item -Path (Join-Path $Tmp "extracted\*") -Destination $InstallDir -Recurse -Force

        $Exe = Join-Path $InstallDir "asset-tap.exe"
        $Version = & $Exe --version
        if ($LASTEXITCODE -ne 0) {
            Write-Host "  installed binary failed to run -- please report: https://github.com/$Repo/issues" -ForegroundColor Red
            exit 1
        }
        Write-Host "  installed $Version -> $InstallDir (asset-tap + atap)" -ForegroundColor Green

        # -- User PATH registration (the Windows convention) ------------------
        $UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
        if (($UserPath -split ";") -notcontains $InstallDir) {
            [Environment]::SetEnvironmentVariable("Path", "$UserPath;$InstallDir", "User")
            # Make it work in THIS session too, not just new terminals.
            $env:Path = "$env:Path;$InstallDir"
            Write-Host "  added $InstallDir to your user PATH (new terminals pick it up automatically)"
        }

        Write-Host ""
        Write-Host "  Next: store a provider key and generate your first asset"
        Write-Host ""
        Write-Host "    asset-tap auth set fal.ai      # or: asset-tap auth set meshy"
        Write-Host "    asset-tap `"a stylized sci-fi crate`""
        Write-Host ""
        Write-Host "  Docs: https://assettap.dev/docs/getting-started/"
    } finally {
        Remove-Item -Path $Tmp -Recurse -Force -ErrorAction SilentlyContinue
    }
}

Install-AssetTap
