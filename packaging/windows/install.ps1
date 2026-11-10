# Install YutuMux on Windows without admin rights.
#
#   powershell -ExecutionPolicy Bypass -File install.ps1 [-Version TAG] [-Yes]
#
# Downloads the self-contained release zip (app plus mpv DLLs and the
# yt-dlp sidecar, all beside the exe) and extracts it below
# $env:LOCALAPPDATA\Programs\YutuMux with a Start Menu shortcut. No
# dependencies are installed and nothing needs to be on PATH. To inspect
# first, download this script, read it, then run it.
#
#   irm -o install.ps1 https://raw.githubusercontent.com/Yandall/yutu-mux/main/packaging/windows/install.ps1
#   powershell -ExecutionPolicy Bypass -File install.ps1 -Yes
#
# Options:
#   -Version TAG   Release tag to download (default: latest).
#   -Repo OWNER/REPO  GitHub repository (default: Yandall/yutu-mux).
#   -InstallDir DIR   Where to install (default: %LOCALAPPDATA%\Programs\YutuMux).
#   -Yes           Do not ask before downloading or replacing files.
#   -NoShortcut    Skip the Start Menu shortcut.
#   -DesktopShortcut  Also add a desktop shortcut.

param(
    [string]$Version = "latest",
    [string]$Repo = "Yandall/yutu-mux",
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA "Programs\YutuMux"),
    [switch]$Yes,
    [switch]$NoShortcut,
    [switch]$DesktopShortcut
)

$ErrorActionPreference = "Stop"
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

function Confirm-Step($prompt) {
    if ($Yes) { return }
    if (-not [Environment]::UserInteractive -or [Console]::IsInputRedirected) {
        throw "$prompt (rerun with -Yes to allow this)"
    }
    $answer = Read-Host "$prompt [y/N]"
    if ($answer -notin @("y", "Y", "yes", "YES", "Yes")) { throw "cancelled" }
}

if ($Version -eq "latest") {
    try {
        $rel = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" -UseBasicParsing
        $tag = $rel.tag_name
    } catch {
        throw "could not resolve the latest release (check that $Repo has releases, or pass -Version vX.Y.Z)"
    }
} else {
    $tag = $Version
}
$assetVersion = $tag.TrimStart("v")
$zipName = "yutu-mux-$assetVersion-windows-x86_64.zip"
if ($env:YUTU_RELEASE_URL) { $url = $env:YUTU_RELEASE_URL } `
elseif ($Version -eq "latest") { $url = "https://github.com/$Repo/releases/latest/download/$zipName" } `
else { $url = "https://github.com/$Repo/releases/download/$Version/$zipName" }

Write-Host "Downloading self-contained YutuMux ($tag, Windows x86_64)."
Write-Host "No dependencies are installed: mpv DLLs and yt-dlp ship inside the zip."
Confirm-Step "Download and install?"

$tmp = Join-Path ([IO.Path]::GetTempPath()) ("yutu-mux-install-" + [Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    $zip = Join-Path $tmp $zipName
    Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing -ErrorAction Stop
    Invoke-WebRequest -Uri "$url.sha256" -OutFile "$zip.sha256" -UseBasicParsing -ErrorAction Stop
    $expected = ((Get-Content "$zip.sha256" -Raw) -split "\s+" | Where-Object { $_ -match "^[0-9a-fA-F]{64}$" } | Select-Object -First 1)
    if (-not $expected) { throw "checksum file is malformed: $url.sha256" }
    $actual = (Get-FileHash $zip -Algorithm SHA256).Hash
    if ($actual.ToLower() -ne $expected.ToLower()) {
        throw "checksum mismatch for $zipName : download is corrupt or tampered with, refusing to install"
    }
    $extract = Join-Path $tmp "pkg"
    Expand-Archive -Path $zip -DestinationPath $extract -Force
    $inner = Get-ChildItem $extract -Directory | Select-Object -First 1
    if ($inner) { $src = $inner.FullName } else { $src = $extract }
    $exe = Join-Path $src "yutu-mux.exe"
    if (-not (Test-Path $exe)) { throw "release archive did not contain yutu-mux.exe" }

    if ((Test-Path $InstallDir) -and (Get-ChildItem $InstallDir -Force | Select-Object -First 1)) {
        Confirm-Step "Replace existing $InstallDir?"
    }
    if (-not (Test-Path $InstallDir)) { New-Item -ItemType Directory -Path $InstallDir | Out-Null }
    Copy-Item (Join-Path $src "*") $InstallDir -Recurse -Force

    if (-not $NoShortcut) {
        $shell = New-Object -ComObject WScript.Shell
        $programs = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs"
        $lnk = $shell.CreateShortcut((Join-Path $programs "YutuMux.lnk"))
        $lnk.TargetPath = (Join-Path $InstallDir "yutu-mux.exe")
        $lnk.WorkingDirectory = $InstallDir
        $icon = Join-Path $InstallDir "yutu-mux.ico"
        if (Test-Path $icon) { $lnk.IconLocation = $icon }
        $lnk.Save()
        if ($DesktopShortcut) {
            $desk = $shell.CreateShortcut((Join-Path ([Environment]::GetFolderPath("Desktop")) "YutuMux.lnk"))
            $desk.TargetPath = (Join-Path $InstallDir "yutu-mux.exe")
            $desk.WorkingDirectory = $InstallDir
            if (Test-Path $icon) { $desk.IconLocation = $icon }
            $desk.Save()
        }
    }
} finally {
    Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host "Installed YutuMux to $InstallDir"
Write-Host "Launch YutuMux from the Start Menu and sign in through Settings with a cookies.txt file."
Write-Host "User library, artwork/lyrics cache, and login data live under $env:APPDATA\yutu-mux and survive reinstalls."
