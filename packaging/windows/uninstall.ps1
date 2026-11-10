# Remove files installed by packaging/windows/install.ps1. Keeps user data.
#
#   powershell -ExecutionPolicy Bypass -File uninstall.ps1 [-InstallDir DIR] [-Yes]

param(
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA "Programs\YutuMux"),
    [switch]$Yes
)

$ErrorActionPreference = "Stop"

if (-not $Yes) {
    if (-not [Environment]::UserInteractive -or [Console]::IsInputRedirected) {
        throw "rerun with -Yes to uninstall"
    }
    $answer = Read-Host "Remove YutuMux files below $InstallDir ? User data is kept. [y/N]"
    if ($answer -notin @("y", "Y", "yes", "YES", "Yes")) { throw "cancelled" }
}

if (Test-Path $InstallDir) {
    Remove-Item $InstallDir -Recurse -Force
    Write-Host "removed $InstallDir"
}
$programs = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs"
$startLink = Join-Path $programs "YutuMux.lnk"
if (Test-Path $startLink) {
    Remove-Item $startLink -Force
    Write-Host "removed $startLink"
}
$deskLink = Join-Path ([Environment]::GetFolderPath("Desktop")) "YutuMux.lnk"
if (Test-Path $deskLink) {
    Remove-Item $deskLink -Force
    Write-Host "removed $deskLink"
}
Write-Host "User library, cache, and login data were not removed ($env:APPDATA\yutu-mux)."
