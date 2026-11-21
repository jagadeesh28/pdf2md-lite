<#
.SYNOPSIS
    Packages pdfmd-lite into Windows WiX MSI Installers (Slim & Full Offline Flavors).

.DESCRIPTION
    Builds native WiX MSI installers for pdfmd-lite with Program Files deployment,
    Desktop and Start Menu shortcuts, system PATH registration, and Windows File
    Explorer right-click context menu integration.

.PARAMETER Flavor
    Packaging flavor: 'Slim', 'Full', or 'All' (Default: 'All').

.PARAMETER OutputDir
    Target directory for generated MSI files (Default: 'dist\installers').

.PARAMETER BuildRelease
    If set, executes 'cargo build --release --workspace' before packaging.
#>

param(
    [ValidateSet('Slim', 'Full', 'All')]
    [string]$Flavor = 'All',

    [string]$OutputDir = 'dist\installers',

    [string]$Version = '',

    [switch]$BuildRelease
)

$ErrorActionPreference = 'Stop'

Write-Host "=================================================================" -ForegroundColor Cyan
Write-Host "         pdfmd-lite Windows WiX MSI Packaging Automation         " -ForegroundColor Cyan
Write-Host "=================================================================" -ForegroundColor Cyan

# 1. Locate wix.exe
$WixExe = Get-Command wix.exe -ErrorAction SilentlyContinue
if (-not $WixExe) {
    Write-Error "wix.exe not found in PATH. Please install WiX v4/v5 via 'dotnet tool install --global wix'."
}
Write-Host "[OK] WiX Toolset detected: $($WixExe.Source)" -ForegroundColor Green

# 2. Optionally build release binaries
if ($BuildRelease -or (-not (Test-Path "target\release\pdfmd-lite.exe")) -or (-not (Test-Path "target\release\pdfmd-lite-desktop.exe"))) {
    Write-Host "[*] Compiling release binaries with cargo build --release --workspace..." -ForegroundColor Yellow
    cargo build --release --workspace
    if ($LASTEXITCODE -ne 0) {
        Write-Error "Cargo release build failed."
    }
}

if ((-not (Test-Path "target\release\pdfmd-lite.exe")) -or (-not (Test-Path "target\release\pdfmd-lite-desktop.exe"))) {
    Write-Error "Required binaries not found in target\release. Run with -BuildRelease."
}

# 3. Create Output Directory
if (-not (Test-Path $OutputDir)) {
    New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null
}

if ([string]::IsNullOrWhiteSpace($Version)) {
    $cargoToml = Get-Content "Cargo.toml" -Raw
    if ($cargoToml -match 'version\s*=\s*"([^"]+)"') {
        $Version = $Matches[1]
    } else {
        $Version = "0.2.0"
    }
}
Write-Host "[OK] Target installer version: v$Version" -ForegroundColor Green
$Generated = @()

# 4. Package Slim Flavor
if ($Flavor -eq 'Slim' -or $Flavor -eq 'All') {
    $SlimMsi = Join-Path $OutputDir "pdfmd-lite-v$Version-x64-slim.msi"
    Write-Host "`n[*] Building Slim MSI installer: $SlimMsi..." -ForegroundColor Yellow

    & wix.exe build -ext WixToolset.UI.wixext/5.0.2 -d "SourceDir=target\release" -o $SlimMsi wix\main.wxs
    if ($LASTEXITCODE -ne 0) {
        Write-Error "Failed to build Slim MSI installer."
    }

    $SlimHash = (Get-FileHash -Path $SlimMsi -Algorithm SHA256).Hash
    Set-Content -Path "$SlimMsi.sha256" -Value "$SlimHash  $(Split-Path $SlimMsi -Leaf)"

    $SlimSizeMb = [math]::Round((Get-Item $SlimMsi).Length / 1MB, 2)
    Write-Host "[OK] Slim MSI completed: $SlimSizeMb MB (SHA-256: $SlimHash)" -ForegroundColor Green
    $Generated += [PSCustomObject]@{
        Flavor = "Slim"
        File = (Split-Path $SlimMsi -Leaf)
        Size = "$SlimSizeMb MB"
        SHA256 = $SlimHash.Substring(0, 16) + "..."
    }
}

# 5. Package Full Offline Flavor
if ($Flavor -eq 'Full' -or $Flavor -eq 'All') {
    $FullMsi = Join-Path $OutputDir "pdfmd-lite-v$Version-x64-full-offline.msi"
    Write-Host "`n[*] Building Full Offline MSI installer (with models/): $FullMsi..." -ForegroundColor Yellow

    & wix.exe build -ext WixToolset.UI.wixext/5.0.2 -d "SourceDir=target\release" -d "ModelsDir=models" -o $FullMsi wix\full.wxs
    if ($LASTEXITCODE -ne 0) {
        Write-Error "Failed to build Full Offline MSI installer."
    }

    $FullHash = (Get-FileHash -Path $FullMsi -Algorithm SHA256).Hash
    Set-Content -Path "$FullMsi.sha256" -Value "$FullHash  $(Split-Path $FullMsi -Leaf)"

    $FullSizeMb = [math]::Round((Get-Item $FullMsi).Length / 1MB, 2)
    Write-Host "[OK] Full Offline MSI completed: $FullSizeMb MB (SHA-256: $FullHash)" -ForegroundColor Green
    $Generated += [PSCustomObject]@{
        Flavor = "Full Offline"
        File = (Split-Path $FullMsi -Leaf)
        Size = "$FullSizeMb MB"
        SHA256 = $FullHash.Substring(0, 16) + "..."
    }
}

Write-Host "`n=================================================================" -ForegroundColor Cyan
Write-Host "                       Build Summary                             " -ForegroundColor Cyan
Write-Host "=================================================================" -ForegroundColor Cyan
$Generated | Format-Table -AutoSize
Write-Host "Artifacts written to: $(Resolve-Path $OutputDir)`n" -ForegroundColor Green
