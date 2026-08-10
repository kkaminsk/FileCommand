#Requires -Version 5.1
<#
.SYNOPSIS
    Builds the FileCommand release binary and packages it into the WiX MSI
    and Burn bundle (FileCommandSetup.exe).

.DESCRIPTION
    Reproducible installer build (design.md decision D6):
        1. Checks prerequisites: .NET SDK and the WiX v4/v5 CLI tool.
        2. Runs `cargo build --release`.
        3. Reads the version from the workspace Cargo.toml and stamps it
           into both MSIs (ProductVersion) and the bundle (Version).
        4. Runs `wix build` for PackagePerUser.wxs and
           PackagePerMachine.wxs, then for Bundle.wxs.

    This script AUTHORS the build steps; it does not itself require the
    WiX toolset to be present to exist correctly, but running it does
    require WiX v4/v5 and the .NET SDK to be installed (see README.md).

.PARAMETER OutDir
    Output directory for the two MSIs and FileCommandSetup.exe. Defaults
    to installer\out next to this script.

.PARAMETER SkipCargoBuild
    Skip `cargo build --release` and package whatever is already at
    target\release\filecommand.exe. Useful for iterating on WiX authoring
    without rebuilding Rust every time.

.EXAMPLE
    .\build.ps1
    Full build: cargo build --release, then both MSIs, then
    FileCommandSetup.exe.
#>
[CmdletBinding()]
param(
    [string]$OutDir = (Join-Path $PSScriptRoot 'out'),
    [switch]$SkipCargoBuild
)

$ErrorActionPreference = 'Stop'

$RepoRoot      = Split-Path -Parent $PSScriptRoot
$InstallerDir  = $PSScriptRoot
$ReleaseDir    = Join-Path $RepoRoot 'target\release'
$PerUserWxs    = Join-Path $InstallerDir 'PackagePerUser.wxs'
$PerMachineWxs = Join-Path $InstallerDir 'PackagePerMachine.wxs'
$BundleWxs     = Join-Path $InstallerDir 'Bundle.wxs'
$PerUserMsi    = Join-Path $OutDir 'PackagePerUser.msi'
$PerMachineMsi = Join-Path $OutDir 'PackagePerMachine.msi'
$BundleExe     = Join-Path $OutDir 'FileCommandSetup.exe'

function Write-Step {
    param([string]$Message)
    Write-Host "==> $Message" -ForegroundColor Cyan
}

function Assert-Prerequisite {
    param(
        [string]$Name,
        [scriptblock]$Check,
        [string]$InstallHint
    )
    Write-Step "Checking prerequisite: $Name"
    $ok = $false
    try {
        $ok = & $Check
    } catch {
        $ok = $false
    }
    if (-not $ok) {
        Write-Host "Missing prerequisite: $Name" -ForegroundColor Red
        Write-Host "  $InstallHint" -ForegroundColor Yellow
        throw "Prerequisite check failed: $Name"
    }
    Write-Host "  OK" -ForegroundColor Green
}

function Get-WorkspaceVersion {
    $cargoToml = Join-Path $RepoRoot 'Cargo.toml'
    if (-not (Test-Path $cargoToml)) {
        throw "Cannot find workspace Cargo.toml at $cargoToml"
    }
    $text = Get-Content -Raw -Path $cargoToml
    # Match `version = "x.y.z"` inside the [workspace.package] table.
    if ($text -notmatch '(?ms)^\[workspace\.package\].*?^version\s*=\s*"([^"]+)"') {
        throw "Could not find [workspace.package] version in $cargoToml"
    }
    return $Matches[1]
}

# ---------------------------------------------------------------------------
# 1. Prerequisite checks
# ---------------------------------------------------------------------------

Assert-Prerequisite -Name '.NET SDK' -Check {
    $null = & dotnet --version 2>$null
    $LASTEXITCODE -eq 0
} -InstallHint 'Install the .NET SDK from https://dotnet.microsoft.com/download'

Assert-Prerequisite -Name 'WiX Toolset CLI (v4/v5)' -Check {
    $null = & wix --version 2>$null
    $LASTEXITCODE -eq 0
} -InstallHint 'Install with: dotnet tool install --global wix'

Assert-Prerequisite -Name 'cargo' -Check {
    $null = & cargo --version 2>$null
    $LASTEXITCODE -eq 0
} -InstallHint 'Install the Rust toolchain from https://rustup.rs'

# ---------------------------------------------------------------------------
# 2. Build the release binary
# ---------------------------------------------------------------------------

if ($SkipCargoBuild) {
    Write-Step 'Skipping cargo build --release (per -SkipCargoBuild)'
} else {
    Write-Step 'cargo build --release'
    Push-Location $RepoRoot
    try {
        & cargo build --release --workspace
        if ($LASTEXITCODE -ne 0) {
            throw "cargo build --release failed with exit code $LASTEXITCODE"
        }
    } finally {
        Pop-Location
    }
}

$ExePath = Join-Path $ReleaseDir 'filecommand.exe'
if (-not (Test-Path $ExePath)) {
    throw "Expected release binary not found at $ExePath. Run without -SkipCargoBuild first."
}

# ---------------------------------------------------------------------------
# 3. Version stamping
# ---------------------------------------------------------------------------

$Version = Get-WorkspaceVersion
Write-Step "Workspace version: $Version"

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

# ---------------------------------------------------------------------------
# 4. wix build: MSI, 5. wix build: Burn bundle
# ---------------------------------------------------------------------------
# `wix extension add` (see README.md prerequisites) restores extensions into
# a `.wix\extensions` cache relative to the current directory, so `wix
# build` must run from $InstallerDir regardless of where this script was
# invoked from, or it won't find WixToolset.BootstrapperApplications.wixext.

Push-Location $InstallerDir
try {
    Write-Step "wix build PackagePerUser.wxs -> $PerUserMsi"
    & wix build $PerUserWxs `
        -d "ProductVersion=$Version" `
        -d "SourceDir=$ReleaseDir" `
        -arch x64 `
        -o $PerUserMsi
    if ($LASTEXITCODE -ne 0) {
        throw "wix build failed for PackagePerUser.wxs with exit code $LASTEXITCODE"
    }

    Write-Step "wix build PackagePerMachine.wxs -> $PerMachineMsi"
    & wix build $PerMachineWxs `
        -d "ProductVersion=$Version" `
        -d "SourceDir=$ReleaseDir" `
        -arch x64 `
        -o $PerMachineMsi
    if ($LASTEXITCODE -ne 0) {
        throw "wix build failed for PackagePerMachine.wxs with exit code $LASTEXITCODE"
    }

    Write-Step "wix build Bundle.wxs -> $BundleExe"
    & wix build $BundleWxs `
        -d "ProductVersion=$Version" `
        -d "PerUserMsiPath=$PerUserMsi" `
        -d "PerMachineMsiPath=$PerMachineMsi" `
        -ext WixToolset.BootstrapperApplications.wixext `
        -arch x64 `
        -o $BundleExe
    if ($LASTEXITCODE -ne 0) {
        throw "wix build failed for Bundle.wxs with exit code $LASTEXITCODE"
    }
} finally {
    Pop-Location
}

Write-Host ""
Write-Host "Build complete:" -ForegroundColor Green
Write-Host "  Per-user MSI:    $PerUserMsi"
Write-Host "  Per-machine MSI: $PerMachineMsi"
Write-Host "  Bundle:          $BundleExe"
Write-Host ""
Write-Host "These are UNSIGNED development builds. See README.md for the production signing steps." -ForegroundColor Yellow
