#Requires -Version 5.1
<#
.SYNOPSIS
    Stages and packs the FileCommand Chocolatey package (.nupkg).

.DESCRIPTION
    The Chocolatey package wraps the signed FileCommandSetup.exe produced by
    installer/build.ps1: at install time the package downloads the bootstrapper
    from the release URL, verifies its SHA-256, and runs it silently.

    This script:
        1. Reads the version (and repository URL) from the workspace Cargo.toml.
        2. Computes the SHA-256 of the local FileCommandSetup.exe.
        3. Stages template\ into <OutDir>\pkg, substituting <VERSION>,
           <REPO_URL>, <RELEASE_URL>, and <SHA256> in the nuspec,
           chocolateyInstall.ps1, VERIFICATION.txt, and LICENSE.txt.
        4. Verifies no placeholders remain, then runs `choco pack`, producing
           filecommand.<version>.nupkg in <OutDir>.

.PARAMETER ReleaseUrl
    Base URL the published FileCommandSetup.exe is downloadable from (e.g. a
    GitHub Release asset directory, without the filename). Defaults to
    "<RepoUrl>/releases/download/v<Version>" -- the standard GitHub Releases
    layout.

.PARAMETER Version
    Package version. Defaults to [workspace.package].version in Cargo.toml
    (the same value installer/build.ps1 stamps into the installer).

.PARAMETER SetupExe
    FileCommandSetup.exe to hash. Defaults to installer\out\FileCommandSetup.exe
    (build installer/build.ps1 first, ideally with -Sign).

.PARAMETER RepoUrl
    Project repository URL used for projectUrl/licenseUrl in the nuspec.
    Defaults to [workspace.package].repository in Cargo.toml.

.PARAMETER OutDir
    Output directory for the staged package tree and the .nupkg. Defaults to
    installer\out\chocolatey.

.PARAMETER StageOnly
    Stage the substituted package tree and stop before `choco pack`. Useful
    for reviewing the generated files, or on machines without Chocolatey.

.EXAMPLE
    .\build.ps1
    Stage the package from the current workspace version and pack it with
    choco, using the default GitHub Releases download URL.

.EXAMPLE
    .\build.ps1 -ReleaseUrl https://example.com/releases/0.1.2 -StageOnly
    Stage with an explicit download base URL, without packing.
#>
[CmdletBinding()]
param(
    [string]$ReleaseUrl,
    [string]$Version,
    [string]$SetupExe = (Join-Path (Split-Path -Parent $PSScriptRoot) 'out\FileCommandSetup.exe'),
    [string]$RepoUrl,
    [string]$OutDir = (Join-Path (Split-Path -Parent $PSScriptRoot) 'out\chocolatey'),
    [switch]$StageOnly
)

$ErrorActionPreference = 'Stop'

$RepoRoot    = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$TemplateDir = Join-Path $PSScriptRoot 'template'
$StageDir    = Join-Path $OutDir 'pkg'

function Write-Step {
    param([string]$Message)
    Write-Host "==> $Message" -ForegroundColor Cyan
}

function Get-WorkspacePackageField {
    param([string]$Field)
    $cargoToml = Join-Path $RepoRoot 'Cargo.toml'
    if (-not (Test-Path $cargoToml)) {
        throw "Cannot find workspace Cargo.toml at $cargoToml"
    }
    $text = Get-Content -Raw -Path $cargoToml
    $pattern = '(?ms)^\[workspace\.package\].*?^' + $Field + '\s*=\s*"([^"]+)"'
    if ($text -notmatch $pattern) {
        throw "Could not find [workspace.package] $Field in $cargoToml"
    }
    return $Matches[1]
}

# ---------------------------------------------------------------------------
# 1. Resolve version / repo URL / release URL / checksum
# ---------------------------------------------------------------------------

if (-not $Version) {
    $Version = Get-WorkspacePackageField -Field 'version'
}
if (-not $RepoUrl) {
    $RepoUrl = Get-WorkspacePackageField -Field 'repository'
}
if (-not $ReleaseUrl) {
    $ReleaseUrl = "$RepoUrl/releases/download/v$Version"
}
Write-Step "Version: $Version  Repo: $RepoUrl"
Write-Step "Release URL: $ReleaseUrl"

if (-not (Test-Path $SetupExe)) {
    throw "FileCommandSetup.exe not found at $SetupExe. Run installer\build.ps1 first (pass -SetupExe to point elsewhere)."
}
$Sha256 = (Get-FileHash -Path $SetupExe -Algorithm SHA256).Hash.ToLowerInvariant()
Write-Step "SHA-256 of $(Split-Path -Leaf $SetupExe): $Sha256"

if (-not $StageOnly) {
    Write-Step 'Checking prerequisite: Chocolatey CLI (choco)'
    $null = & choco --version 2>$null
    if ($LASTEXITCODE -ne 0) {
        throw "Chocolatey CLI not found on PATH. Install from https://chocolatey.org/install or pass -StageOnly to only stage the package."
    }
}

# ---------------------------------------------------------------------------
# 2. Stage the template with substitutions
# ---------------------------------------------------------------------------

if (Test-Path $StageDir) {
    Remove-Item -Recurse -Force $StageDir
}
Write-Step "Staging template -> $StageDir"
Copy-Item -Recurse -Path $TemplateDir -Destination $StageDir

$replacements = @{
    '<VERSION>'     = $Version
    '<REPO_URL>'    = $RepoUrl
    '<RELEASE_URL>' = $ReleaseUrl
    '<SHA256>'      = $Sha256
}
$substitutedFiles = @(
    (Join-Path $StageDir 'filecommand.nuspec'),
    (Join-Path $StageDir 'tools\chocolateyInstall.ps1'),
    (Join-Path $StageDir 'tools\VERIFICATION.txt'),
    (Join-Path $StageDir 'tools\LICENSE.txt')
)
foreach ($file in $substitutedFiles) {
    $text = Get-Content -Raw -Path $file
    foreach ($key in $replacements.Keys) {
        $text = $text.Replace($key, $replacements[$key])
    }
    Set-Content -Path $file -Value $text -NoNewline -Encoding UTF8
}

# ---------------------------------------------------------------------------
# 3. Guard: no placeholders may survive staging
# ---------------------------------------------------------------------------

$leftovers = Get-ChildItem -Recurse -File $StageDir |
    Select-String -Pattern '<[A-Z_]{3,}>' -CaseSensitive |
    ForEach-Object { "$($_.Path): $($_.Line.Trim())" }
if ($leftovers) {
    $leftovers | ForEach-Object { Write-Host "  $_" -ForegroundColor Red }
    throw "Unsubstituted placeholders remain in the staged package (see above)."
}
Write-Host '  All placeholders substituted.' -ForegroundColor Green

# ---------------------------------------------------------------------------
# 4. choco pack
# ---------------------------------------------------------------------------

$NuspecPath = Join-Path $StageDir 'filecommand.nuspec'
if ($StageOnly) {
    Write-Host ''
    Write-Host "Staged package tree at $StageDir (-StageOnly: skipping choco pack)." -ForegroundColor Yellow
    Write-Host "To pack: choco pack '$NuspecPath' --outputdirectory '$OutDir'"
    return
}

Write-Step "choco pack -> $OutDir"
& choco pack $NuspecPath --outputdirectory $OutDir
if ($LASTEXITCODE -ne 0) {
    throw "choco pack failed with exit code $LASTEXITCODE"
}

$Nupkg = Join-Path $OutDir "filecommand.$Version.nupkg"
Write-Host ''
Write-Host "Build complete: $Nupkg" -ForegroundColor Green
Write-Host 'Test locally before pushing (elevated shell):' -ForegroundColor Cyan
Write-Host "  choco install filecommand --source `'$OutDir;https://community.chocolatey.org/api/v2/`'"
Write-Host '  choco uninstall filecommand'
