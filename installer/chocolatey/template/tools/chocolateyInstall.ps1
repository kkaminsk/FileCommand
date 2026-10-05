$ErrorActionPreference = 'Stop'

# <RELEASE_URL> and <SHA256> are stamped by installer/chocolatey/build.ps1
# at pack time -- never run this copy directly from the source tree.
$releaseUrl = '<RELEASE_URL>'
$sha256     = '<SHA256>'

# Default scope follows Chocolatey convention (machine-wide). The wrapped
# Burn bundle understands InstallScope=perUser|perMachine (see Bundle.wxs).
$scope = 'perMachine'
$pp = Get-PackageParameters
if ($pp.Scope) {
    if ($pp.Scope -notin @('user', 'machine')) {
        throw "Invalid /Scope value '$($pp.Scope)'. Expected 'user' or 'machine'."
    }
    $scope = if ($pp.Scope -eq 'user') { 'perUser' } else { 'perMachine' }
}

$packageArgs = @{
    packageName    = $env:ChocolateyPackageName
    fileType       = 'exe'
    url64bit       = "$releaseUrl/FileCommandSetup.exe"
    checksum64     = $sha256
    checksumType64 = 'sha256'
    silentArgs     = "/quiet /norestart InstallScope=$scope"
    validExitCodes = @(0, 3010, 1641) # 3010/1641 = success, reboot required
}

Install-ChocolateyPackage @packageArgs

# Record the scope for chocolateyUninstall.ps1: on machines where both
# scopes are installed (e.g. a previous manual MSI/bundle install), the
# Uninstall keys hold more than one 'FileCommand' entry, and only the
# scope this package installed is ours to remove.
$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition
Set-Content -Path (Join-Path $toolsDir 'install-scope.txt') -Value $scope -Encoding ASCII
