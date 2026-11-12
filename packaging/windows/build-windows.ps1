# Build the release binary on Windows and stage a self-contained folder.
#
#   powershell -ExecutionPolicy Bypass -File packaging/windows/build-windows.ps1 [-Output DIR] [-MpvTag TAG] [-MpvCommit HASH]
#
# The staged folder (under DIR, default dist/) is
# yutu-mux-<version>-windows-x86_64/ holding yutu-mux.exe plus every
# runtime dependency beside it: libmpv-2.dll (fully static build, the one
# file carrying mpv + ffmpeg), the standalone yt-dlp.exe sidecar, and the
# app icon. Nothing is installed on the machine and nothing needs to be
# on PATH: `find_yt_dlp` checks the executable's own directory first, and
# Windows loads DLLs from the application directory first. The same folder
# is zipped as yutu-mux-<version>-windows-x86_64.zip plus its .zip.sha256
# checksum (the two files uploaded to GitHub Releases, next to the Inno
# Setup installer built from packaging/windows/YutuMux.iss).
#
# mpv comes from the shinchiro mpv-winbuild-cmake releases (dev package
# only: headers, the libmpv.dll.a import lib, and libmpv-2.dll). Bump
# -MpvTag/-MpvCommit together when the pin goes stale. yt-dlp intentionally tracks latest at build time: a
# pinned old yt-dlp breaks against YouTube changes within weeks, so
# freshness beats reproducibility here (same reason the macOS script
# fetches the latest yt-dlp_macos).
#
# Requirements: Windows x64, Rust (MSVC), 7-Zip, and Visual Studio Build
# Tools (dumpbin + lib.exe are only needed when the dev package ships no
# mpv.lib; they are located via vswhere or PATH). Extra cargo flags pass
# through via CARGO_FLAGS. Without mpv (no dev package, rodio-only):
# $env:CARGO_FLAGS="--no-default-features --features submit,innertube,lyrics".

param(
    [string]$Output = "dist",
    [string]$MpvTag = "20261108",
    [string]$MpvCommit = "55d3feb45c"
)

$ErrorActionPreference = "Stop"

$root = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$cargoToml = Get-Content (Join-Path $root "Cargo.toml") -Raw
$version = ([regex]::Match($cargoToml, '(?m)^version\s*=\s*"([^"]+)"')).Groups[1].Value
if (-not $version) { throw "could not read version from Cargo.toml" }

if ($env:PROCESSOR_ARCHITECTURE -ne "AMD64") {
    throw "no portable package for $($env:PROCESSOR_ARCHITECTURE) (x86_64 only)"
}
$arch = "x86_64"

if ($env:YUTU_MPV_DEV_URL) { $mpvDevUrl = $env:YUTU_MPV_DEV_URL } `
else { $mpvDevUrl = "https://github.com/shinchiro/mpv-winbuild-cmake/releases/download/$MpvTag/mpv-dev-x86_64-$MpvTag-git-$MpvCommit.7z" }
if ($env:YUTU_YTDLP_URL) { $ytdlpUrl = $env:YUTU_YTDLP_URL } `
else { $ytdlpUrl = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe" }

function Find-SevenZip {
    $candidates = @()
    if ($env:ProgramFiles) { $candidates += (Join-Path $env:ProgramFiles "7-Zip\7z.exe") }
    $candidates += "7z"
    foreach ($c in $candidates) {
        $cmd = Get-Command $c -ErrorAction SilentlyContinue
        if ($cmd) { return $cmd.Source }
    }
    throw "7-Zip is required to unpack mpv (install it or add 7z.exe to PATH)"
}

function Find-VsTool($fileName) {
    $cmd = Get-Command $fileName -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    # Filesystem search first: vswhere's -find glob matches nothing on
    # some images (observed on GitHub's windows-latest), while link.exe
    # proves the MSVC layout below exists. Both Program Files roots:
    # 64-bit VS lives under ProgramFiles, older installs under x86.
    $roots = @()
    if ($env:ProgramFiles) { $roots += $env:ProgramFiles }
    if (${env:ProgramFiles(x86)}) { $roots += ${env:ProgramFiles(x86)} }
    foreach ($pf in $roots) {
        $hit = Get-ChildItem -Path (Join-Path $pf "Microsoft Visual Studio\*\*\VC\Tools\MSVC\*\bin\Hostx64\x64\$fileName") -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($hit) { return $hit.FullName }
    }
    # Last resort: the VS installer locator.
    $vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
    if (Test-Path $vswhere) {
        $found = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools -find "VC\Tools\MSVC\*\bin\Hostx64\x64\$fileName" 2>$null | Select-Object -First 1
        if ($found -and (Test-Path $found)) { return $found }
    }
    return $null
}

function New-MpvImportLib($dllPath, $outDir) {
    $dumpbin = Find-VsTool "dumpbin.exe"
    $libexe = Find-VsTool "lib.exe"
    if (-not $dumpbin -or -not $libexe) {
        throw "mpv dev package has no mpv.lib and dumpbin.exe/lib.exe were not found (install Visual Studio Build Tools)"
    }
    $dllBase = [IO.Path]::GetFileNameWithoutExtension($dllPath)
    $exportsFile = Join-Path $outDir "mpv-exports.txt"
    & $dumpbin /exports $dllPath "/out:$exportsFile" | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "dumpbin failed on $dllPath" }
    $names = @()
    $inTable = $false
    foreach ($line in (Get-Content $exportsFile)) {
        if (-not $inTable) {
            if ($line -match "ordinal\s+hint\s+RVA\s+name") { $inTable = $true }
            continue
        }
        # Blank lines separate the header from the rows: skip them
        # (breaking on them parsed zero exports) and stop at the tail.
        if ($line -match "Summary") { break }
        if ($line -match "^\s*\d+\s+\w+\s+[0-9A-Fa-f]+\s+(\S+)\s*$") { $names += $Matches[1] }
    }
    if ($names.Count -eq 0) { throw "no exports parsed from $dllPath" }
    $defPath = Join-Path $outDir "mpv.def"
    $def = "LIBRARY $dllBase`r`nEXPORTS`r`n" + ($names -join "`r`n") + "`r`n"
    [IO.File]::WriteAllText($defPath, $def, [Text.UTF8Encoding]::new($false))
    & $libexe "/def:$defPath" "/out:$(Join-Path $outDir 'mpv.lib')" /machine:x64 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "lib.exe failed to create mpv.lib" }
    return (Join-Path $outDir "mpv.lib")
}

Write-Host "==> downloading mpv ($MpvTag)"
$tmp = Join-Path ([IO.Path]::GetTempPath()) ("yutu-mux-win-" + [Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    $sevenZip = Find-SevenZip
    $devArc = Join-Path $tmp "mpv-dev.7z"
    Invoke-WebRequest -Uri $mpvDevUrl -OutFile $devArc -UseBasicParsing
    $devDir = Join-Path $tmp "mpv-dev"
    & $sevenZip x $devArc "-o$devDir" -y | Out-Null
    $mpvDll = Get-ChildItem -Path $devDir -Filter "libmpv-2.dll" -Recurse | Select-Object -First 1
    if (-not $mpvDll) { throw "mpv dev package has no libmpv-2.dll; it changed layout, refusing a broken bundle" }

    Write-Host "==> resolving mpv import library"
    $mpvLib = Get-ChildItem -Path $devDir -Filter "mpv.lib" -Recurse | Select-Object -First 1
    if ($mpvLib) {
        $mpvLibDir = $mpvLib.DirectoryName
    } else {
        $dll = $mpvDll
        $genDir = Join-Path $tmp "mpv-genlib"
        New-Item -ItemType Directory -Path $genDir | Out-Null
        $mpvLib = New-MpvImportLib $dll.FullName $genDir
        $mpvLibDir = $genDir
    }
    Write-Host "mpv import lib: $mpvLibDir"
    $env:LIB = "$mpvLibDir;$env:LIB"

    Write-Host "==> building release ($version, $arch)"
    Push-Location $root
    try {
        if ($env:CARGO_FLAGS) {
            $cargoArgs = @("build", "--release", "--locked") + ($env:CARGO_FLAGS -split "\s+")
        } else {
            $cargoArgs = @("build", "--release", "--locked")
        }
        & cargo $cargoArgs
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
    } finally {
        Pop-Location
    }
    $bin = Join-Path $root "target\release\yutu-mux.exe"
    if (-not (Test-Path $bin)) { throw "release binary missing at $bin" }

    Write-Host "==> downloading yt-dlp"
    $stage = Join-Path $tmp "stage"
    New-Item -ItemType Directory -Path $stage | Out-Null
    Invoke-WebRequest -Uri $ytdlpUrl -OutFile (Join-Path $stage "yt-dlp.exe") -UseBasicParsing

    Write-Host "==> staging package"
    Copy-Item $bin $stage
    Copy-Item $mpvDll.FullName $stage
    $ico = Join-Path $root "packaging\windows\yutu-mux.ico"
    if (Test-Path $ico) { Copy-Item $ico $stage }
    Copy-Item (Join-Path $root "README.md") $stage
    if (Test-Path (Join-Path $root "LICENSE")) { Copy-Item (Join-Path $root "LICENSE") $stage }
    if (-not (Get-ChildItem $stage -Filter "*mpv*.dll")) {
        throw "staged no mpv DLL; the mpv package changed layout, refusing a broken bundle"
    }
    if (-not (Test-Path (Join-Path $stage "yt-dlp.exe"))) { throw "yt-dlp.exe missing from stage" }

    Write-Host "==> writing zip"
    $name = "yutu-mux-$version-windows-$arch"
    $outDir = Join-Path $root "$Output\$name"
    if (Test-Path $outDir) { Remove-Item $outDir -Recurse -Force }
    New-Item -ItemType Directory -Path $outDir | Out-Null
    $folder = Join-Path $outDir $name
    Copy-Item $stage $folder -Recurse
    $zip = "$folder.zip"
    Compress-Archive -Path $folder -DestinationPath $zip -Force
    $hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
    Set-Content "$zip.sha256" "$hash  $(Split-Path $zip -Leaf)`n" -NoNewline

    Write-Host "done: $zip"
    Write-Host "checksum: $zip.sha256"
} finally {
    Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
}
