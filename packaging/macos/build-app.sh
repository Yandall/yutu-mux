#!/bin/bash
# Build the release binary and wrap it as a macOS YutuMux.app bundle.
#
#   packaging/macos/build-app.sh [--self-contained] [--output DIR]
#
# Output (under DIR, default dist/): stage/YutuMux.app (+ an
# Applications shortcut for the disk image), a versioned folder
# YutuMux-<version>-macos-<arch>/ holding the disk image
# YutuMux-<version>-macos-<arch>.dmg plus its `.dmg.sha256` checksum
# (the two files uploaded to GitHub Releases), and a
# YutuMux-<version>-macos-<arch>.sha256 manifest of every file in the
# bundle (verify with `shasum -a 256 -c` from the stage dir).
#
# --self-contained bundles libmpv (+ its ffmpeg libs) via dylibbundler and
# fetches the standalone yt-dlp macOS binary, so the app runs with nothing
# installed. Without it the bundle links Homebrew's libmpv and finds
# yt-dlp on PATH (both must exist on the machine).
#
# Requirements: Xcode CLT, Rust, and for the default feature set
# `brew install mpv` (libmpv) plus pkg-config. The repo's
# .cargo/config.toml already points the linker at Homebrew's lib dir.
# Extra cargo flags pass through via CARGO_FLAGS (e.g. rodio-only:
# `--no-default-features --features submit,innertube,lyrics`).
#
# One architecture per run (native): repeat per arch and lipo the two
# binaries together for a universal bundle.
# pipefail: the release build tees through a log (for lock-error
# detection), and without it a cargo failure would hide behind tee.
set -euo pipefail

SELF_CONTAINED=0
OUT="dist"

while [ $# -gt 0 ]; do
    case "$1" in
        --self-contained) SELF_CONTAINED=1 ;;
        --output) OUT="$2"; shift ;;
        -h|--help)
            sed -n '2,20p' "$0"
            exit 0
            ;;
        *) echo "unknown flag: $1 (see --help)" >&2; exit 1 ;;
    esac
    shift
done

if [ "$(uname)" != "Darwin" ]; then
    echo "error: this script wraps a macOS .app bundle; run it on a Mac" >&2
    exit 1
fi

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
APP_NAME="YutuMux"
IDENTIFIER="org.yutu_mux.YutuMux"
VERSION="$(grep -m1 '^version *= *"' "$ROOT/Cargo.toml" | cut -d'"' -f2)"
ARCH="$(uname -m)"
# shellcheck disable=SC2086
[ -n "${CARGO_FLAGS:-}" ] && echo "cargo flags: $CARGO_FLAGS"

echo "==> building release ($VERSION, $ARCH)"
# Prefer the pinned lockfile (same deps as the Linux CI tarball). When it
# is missing or stale (partial checkout, local manifest edits), cargo
# refuses --locked: warn and retry unlocked so the build still succeeds,
# then remind about committing the regenerated lock.
LOCK_ARGS="--locked"
if [ ! -f "$ROOT/Cargo.lock" ]; then
    echo "warning: no Cargo.lock in $ROOT; building unlocked" >&2
    echo "prefer a full clone plus tag checkout for release builds" >&2
    LOCK_ARGS=""
fi
BUILD_LOG="$(mktemp "${TMPDIR:-/tmp}/yutu-build.XXXXXX")"
# shellcheck disable=SC2086
if (cd "$ROOT" && cargo build --release $LOCK_ARGS ${CARGO_FLAGS:-} 2>&1 | tee "$BUILD_LOG"); then
    rm -f "$BUILD_LOG"
elif [ -n "$LOCK_ARGS" ] && grep -qi "lock file" "$BUILD_LOG"; then
    echo "warning: --locked build failed; retrying unlocked to regenerate Cargo.lock" >&2
    rm -f "$BUILD_LOG"
    # shellcheck disable=SC2086
    (cd "$ROOT" && cargo build --release ${CARGO_FLAGS:-})
    echo "warning: built with a regenerated lock; commit Cargo.lock if it changed" >&2
else
    rm -f "$BUILD_LOG"
    exit 1
fi
BIN="$ROOT/target/release/yutu-mux"
[ -x "$BIN" ] || { echo "error: release binary missing at $BIN" >&2; exit 1; }

STAGE="$OUT/stage"
APP="$STAGE/$APP_NAME.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/"{MacOS,Resources}

echo "==> staging bundle"
cp "$BIN" "$APP/Contents/MacOS/yutu-mux-bin"
cat > "$APP/Contents/MacOS/$APP_NAME" <<'EOF'
#!/bin/sh
# Finder launches get a skeletal PATH: Homebrew, the bundled yt-dlp
# sidecar, and this directory are added back before starting the app.
HERE="$(dirname "$0")"
export PATH="$HERE:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
# argv[0] names the app in the menu bar (About/Hide/Quit): without
# -a it reads the post-exec binary name (yutu-mux-bin) instead.
exec -a "$APP_NAME" "$HERE/yutu-mux-bin" "$@"
EOF
chmod +x "$APP/Contents/MacOS/$APP_NAME"
cat > "$APP/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>$APP_NAME</string>
  <key>CFBundleIdentifier</key><string>$IDENTIFIER</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleExecutable</key><string>$APP_NAME</string>
  <key>CFBundleIconFile</key><string>$APP_NAME</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
EOF

if [ "$SELF_CONTAINED" -eq 1 ]; then
    command -v dylibbundler >/dev/null 2>&1 \
        || { echo "error: need dylibbundler (brew install dylibbundler)" >&2; exit 1; }
    echo "==> bundling libmpv (+ ffmpeg libs)"
    # dylibbundler refuses a missing dest dir (newer versions error
    # out instead of creating it), so make it unconditionally.
    mkdir -p "$APP/Contents/libs"
    dylibbundler -b \
        -x "$APP/Contents/MacOS/yutu-mux-bin" \
        -d "$APP/Contents/libs" \
        -p @executable_path/../libs \
        -of
    echo "==> fetching standalone yt-dlp"
    curl -fsSL -o "$APP/Contents/MacOS/yt-dlp" \
        https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_macos
    chmod +x "$APP/Contents/MacOS/yt-dlp"
fi

echo "==> building app icon (.icns from the grey master)"
ICONSET_TMP="$(mktemp -d "${TMPDIR:-/tmp}/yutu-iconset.XXXXXX")"
ICONSET="$ICONSET_TMP/$APP_NAME.iconset"
mkdir -p "$ICONSET"
# Near-black rounded-square master (assets/logo): every slot downscales
# from it, so no upscaling anywhere. sips and iconutil are macOS stock.
ICONSRC="$ROOT/assets/logo/yutu-mux-background.png"
while read -r px name; do
    sips -z "$px" "$px" "$ICONSRC" --out "$ICONSET/$name.png" >/dev/null
done <<'SIZES'
16 icon_16x16
32 icon_16x16@2x
32 icon_32x32
64 icon_32x32@2x
128 icon_128x128
256 icon_128x128@2x
256 icon_256x256
512 icon_256x256@2x
512 icon_512x512
1024 icon_512x512@2x
SIZES
iconutil -c icns -o "$APP/Contents/Resources/$APP_NAME.icns" "$ICONSET"
rm -rf "$ICONSET_TMP"
[ -s "$APP/Contents/Resources/$APP_NAME.icns" ] \
    || { echo "error: .icns build failed (see iconutil output above)" >&2; exit 1; }

echo "==> writing About panel credits"
cat > "$APP/Contents/Resources/Credits.rtf" <<EOF
{\\rtf1\\ansi
YutuMux $VERSION\\par
Native YouTube Music desktop app (Iced, no webview).\\par
\\par
MIT License: free software, see LICENSE.\\par
Source: https://github.com/Yandall/yutu-mux\\par
\\par
Bundled type: Adwaita Sans (OFL-1.1). Icons: Lucide (ISC).\\par
Playback links the system libmpv (GPL-2.0); the rodio-only build is GPL-free.\\par
}
EOF

echo "==> ad-hoc signing (personal use; distribution needs a Developer ID)"
codesign -s - --deep --force "$APP" >/dev/null
# Nudge Finder past its icon cache for rebuilt bundles at the same path.
touch "$APP"

# Checksum manifest of the signed bundle (deterministic order, paths
# relative to the stage dir: verify with
# `cd stage && shasum -a 256 -c ../YutuMux-<version>-macos-<arch>.sha256`).
# shasum is macOS stock, and this script only runs on macOS.
echo "==> writing bundle checksum manifest"
SHAFILE="$OUT/$APP_NAME-$VERSION-macos-$ARCH.sha256"
(cd "$STAGE" && find "$APP_NAME.app" \( -type f -o -type l \) -print0 \
    | LC_ALL=C sort -z | xargs -0 shasum -a 256) > "$SHAFILE"
echo "==> checksum: $SHAFILE"

# Single-file distributable (GitHub Releases cannot take a directory):
# UDZO disk image with the signed bundle plus an Applications shortcut
# for the standard drag-install, collected with its checksum in a
# versioned folder. hdiutil is macOS stock, like everything else this
# script needs.
echo "==> staging disk image (app + Applications shortcut)"
ln -sfn /Applications "$STAGE/Applications"
FOLDER="$OUT/$APP_NAME-$VERSION-macos-$ARCH"
rm -rf "$FOLDER"
mkdir -p "$FOLDER"
DMG="$FOLDER/$APP_NAME-$VERSION-macos-$ARCH.dmg"
hdiutil create -volname "$APP_NAME $VERSION" -srcfolder "$STAGE" -ov -format UDZO "$DMG"
echo "==> disk image: $DMG"
(cd "$FOLDER" && shasum -a 256 "$(basename "$DMG")") > "$DMG.sha256"
echo "==> checksum: $DMG.sha256"

echo "done: $APP"
echo "first launch: right-click -> Open (unsigned bundle), then sign in via cookies.txt in Settings"
