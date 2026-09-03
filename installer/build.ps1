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

.PARAMETER Sign
    Sign filecommand.exe, both MSIs, and the Burn bundle using Azure
    Trusted Signing (see the Big Hat Group CodeSigning tooling). Requires
    an authenticated `az login` session with the Code Signing Certificate
    Profile Signer role on the BHGPublic/private-mcp profile, plus a local
    checkout of the CodeSigning repo (for the signing dlib + metadata).

.PARAMETER CodeSigningDlib
    Path to Azure.CodeSigning.Dlib.dll. Defaults to the CodeSigning repo
    checkout at C:\GitHub\CodeSigning. Only used with -Sign.

.PARAMETER CodeSigningMetadata
    Path to the Trusted Signing metadata JSON (endpoint, account, cert
    profile). Defaults to metadata-packager-mcp-cli.json in the
    CodeSigning repo checkout. Only used with -Sign.

.PARAMETER SignToolPath
    Path to signtool.exe. Defaults to auto-detecting the newest Windows
    SDK signtool on the machine. Only used with -Sign.

.EXAMPLE
    .\build.ps1
    Full build: cargo build --release, then both MSIs, then
    FileCommandSetup.exe. Unsigned.

.EXAMPLE
    .\build.ps1 -Sign
    Full build, signing filecommand.exe, both MSIs, and the bundle with
    Azure Trusted Signing along the way.
#>
[CmdletBinding()]
param(
    [string]$OutDir = (Join-Path $PSScriptRoot 'out'),
    [switch]$SkipCargoBuild,
    [switch]$Sign,
    [string]$CodeSigningDlib = 'C:\GitHub\CodeSigning\Microsoft.Trusted.Signing.Client.1.0.95\bin\x64\Azure.CodeSigning.Dlib.dll',
    [string]$CodeSigningMetadata = 'C:\GitHub\CodeSigning\metadata-packager-mcp-cli.json',
    [string]$SignToolPath,
    [string]$TimestampServer = 'http://timestamp.acs.microsoft.com'
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

function Find-SignTool {
    $candidates = Get-ChildItem -Path "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" -ErrorAction SilentlyContinue |
        Sort-Object { try { [version]$_.Directory.Parent.Name } catch { [version]'0.0' } } -Descending
    if ($candidates) {
        return $candidates[0].FullName
    }
    return $null
}

function Invoke-FileSigning {
    param([string]$Path)

    if (-not (Test-Path $Path)) {
        throw "Cannot sign missing file: $Path"
    }

    Write-Step "Signing: $Path"
    & $script:SignToolPath sign /fd SHA256 /tr $TimestampServer /td SHA256 `
        /dlib $CodeSigningDlib /dmdf $CodeSigningMetadata $Path
    if ($LASTEXITCODE -ne 0) {
        throw "signtool sign failed for $Path with exit code $LASTEXITCODE"
    }

    & $script:SignToolPath verify /pa $Path
    if ($LASTEXITCODE -ne 0) {
        throw "signtool verify failed for $Path with exit code $LASTEXITCODE"
    }
    Write-Host "  Signed and verified: $Path" -ForegroundColor Green
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

if ($Sign) {
    if (-not $SignToolPath) {
        $SignToolPath = Find-SignTool
    }
    Assert-Prerequisite -Name 'signtool.exe (Windows SDK)' -Check {
        $SignToolPath -and (Test-Path $SignToolPath)
    } -InstallHint 'Install a Windows SDK (10.0.26100.0+) or pass -SignToolPath explicitly'
    $script:SignToolPath = $SignToolPath

    Assert-Prerequisite -Name 'Azure.CodeSigning.Dlib.dll' -Check {
        Test-Path $CodeSigningDlib
    } -InstallHint "Expected at $CodeSigningDlib -- checkout the CodeSigning repo or pass -CodeSigningDlib"

    Assert-Prerequisite -Name 'Trusted Signing metadata' -Check {
        Test-Path $CodeSigningMetadata
    } -InstallHint "Expected at $CodeSigningMetadata -- checkout the CodeSigning repo or pass -CodeSigningMetadata"

    Assert-Prerequisite -Name 'Azure CLI authentication' -Check {
        $null = & az account show 2>$null
        $LASTEXITCODE -eq 0
    } -InstallHint 'Run: az login (needs Code Signing Certificate Profile Signer role on BHGPublic/private-mcp)'
}

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

if ($Sign) {
    Invoke-FileSigning -Path $ExePath
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

    if ($Sign) {
        # Sign both MSIs *before* the bundle build so it embeds the signed
        # copies as-is.
        Invoke-FileSigning -Path $PerUserMsi
        Invoke-FileSigning -Path $PerMachineMsi
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

    if ($Sign) {
        # Burn bundles need a two-pass sign: detach the stub engine, sign
        # it, reattach it, then sign the whole bundle. `wix burn
        # detach`/`reattach` are the WiX v4/v5 replacements for the old
        # `insignia` tool. This must run from $InstallerDir too, same as
        # the builds above.
        $EnginePath = Join-Path $OutDir 'engine.exe'
        Write-Step "wix burn detach -> $EnginePath"
        & wix burn detach $BundleExe -engine $EnginePath
        if ($LASTEXITCODE -ne 0) {
            throw "wix burn detach failed with exit code $LASTEXITCODE"
        }

        Invoke-FileSigning -Path $EnginePath

        Write-Step "wix burn reattach -> $BundleExe"
        & wix burn reattach $BundleExe -engine $EnginePath -o $BundleExe
        if ($LASTEXITCODE -ne 0) {
            throw "wix burn reattach failed with exit code $LASTEXITCODE"
        }
        Remove-Item -Path $EnginePath -Force -ErrorAction SilentlyContinue

        Invoke-FileSigning -Path $BundleExe
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
if ($Sign) {
    Write-Host "All artifacts signed and verified with Azure Trusted Signing." -ForegroundColor Green
} else {
    Write-Host "These are UNSIGNED development builds. Pass -Sign to sign with Azure Trusted Signing, or see README.md for manual production signing steps." -ForegroundColor Yellow
}
