$ErrorActionPreference = 'Stop'

# FileCommandSetup.exe is a WiX Burn bundle: uninstalling means re-running
# the cached bundle copy with /uninstall. Burn registers the bundle under
# the Uninstall keys (HKLM for a per-machine install, HKCU for per-user)
# with an UninstallString pointing at FileCommandSetup.exe in the Package
# Cache; plain MSI registrations (MsiExec.exe /X...) are NOT the bundle and
# must not be matched. Burn also persists the original InstallScope, so
# the same scope that was installed is the scope removed.
$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition

# Scope this package installed (written by chocolateyInstall.ps1). A
# machine can carry both scopes at once; only the scope this package
# installed is ours to remove. Fall back to searching every hive if the
# marker is missing (e.g. package files hand-copied).
$scope = Get-Content -Path (Join-Path $toolsDir 'install-scope.txt') -ErrorAction SilentlyContinue
$uninstallKeys = switch ($scope) {
    'perMachine' { @('HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*',
                     'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*') }
    'perUser'    { @('HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*') }
    default      { @('HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*',
                     'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*',
                     'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*') }
}

$bundle = Get-ItemProperty -Path $uninstallKeys -ErrorAction SilentlyContinue |
    Where-Object { $_.DisplayName -eq 'FileCommand' -and $_.UninstallString -match 'FileCommandSetup\.exe' } |
    Select-Object -First 1

if (-not $bundle) {
    Write-Warning "FileCommand bundle registration not found (scope: $($scope -or 'unknown'); already uninstalled?). Nothing to do."
    return
}

# UninstallString looks like:
#   "C:\...\Package Cache\{guid}\FileCommandSetup.exe" /uninstall
if ($bundle.UninstallString -match '^"([^"]+)"') {
    $bundleExe = $Matches[1]
} else {
    $bundleExe = ($bundle.UninstallString -split '\s+')[0]
}

if (-not (Test-Path $bundleExe)) {
    throw "Cached bundle not found at '$bundleExe' -- cannot uninstall cleanly."
}

Write-Host "Uninstalling FileCommand via $bundleExe"
$process = Start-Process -FilePath $bundleExe `
    -ArgumentList '/uninstall', '/quiet', '/norestart' `
    -Wait -PassThru

# 0 = success; 3010/1641 = success, reboot required; 1605 = already removed
if ($process.ExitCode -notin @(0, 3010, 1641, 1605)) {
    throw "FileCommand uninstall failed with exit code $($process.ExitCode)."
}
if ($process.ExitCode -in @(3010, 1641)) {
    Write-Warning 'FileCommand was uninstalled; a reboot is required to finish removing it.'
}
