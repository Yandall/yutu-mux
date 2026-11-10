#!/bin/sh
# Cross-build the self-contained Windows zip on Linux (runs natively on a
# MinGW host, normally via packaging/windows/build-windows-docker.sh).
#
#   packaging/windows/build-windows-cross.sh [--output DIR]
#     [--mpv-tag TAG] [--mpv-commit HASH]
#
# Same output as packaging/windows/build-windows.ps1: a staged folder
# yutu-mux-<version>-windows-x86_64/ (yutu-mux.exe plus libmpv-2.dll and
# yt-dlp.exe beside it, nothing on PATH needed) zipped with a .sha256
# checksum. The exe is compiled for x86_64-pc-windows-gnu with the
# MinGW-w64 linker against the import lib shipped in the mpv dev package.
# Env overrides mirror the PowerShell script: YUTU_MPV_DEV_URL,
# YUTU_YTDLP_URL, CARGO_FLAGS.
set -eu

OUT="dist"
MPV_TAG="20261108"
MPV_COMMIT="55d3feb45c"

while [ $# -gt 0 ]; do
    case "$1" in
        --output) OUT="$2"; shift ;;
        --mpv-tag) MPV_TAG="$2"; shift ;;
        --mpv-commit) MPV_COMMIT="$2"; shift ;;
        -h|--help)
            sed -n '2,10p' "$0"
            exit 0
            ;;
        *) echo "unknown flag: $1" >&2; exit 1 ;;
    esac
    shift
done

ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/../.." 2>/dev/null && pwd)"
VERSION="$(grep -m1 '^version *= *"' "$ROOT/Cargo.toml" | cut -d'"' -f2)"
ARCH="x86_64"

if [ "$(uname -m)" != "x86_64" ]; then
    echo "error: the cross build runs on x86_64 Linux only" >&2
    exit 1
fi
for tool in x86_64-w64-mingw32-gcc x86_64-w64-mingw32-objdump 7z curl python3; do
    command -v "$tool" >/dev/null 2>&1 || {
        echo "error: missing $tool (use the Dockerfile image)" >&2
        exit 1
    }
done

if [ -n "${YUTU_MPV_DEV_URL:-}" ]; then
    MPV_DEV_URL="$YUTU_MPV_DEV_URL"
else
    MPV_DEV_URL="https://github.com/shinchiro/mpv-winbuild-cmake/releases/download/$MPV_TAG/mpv-dev-x86_64-$MPV_TAG-git-$MPV_COMMIT.7z"
fi
if [ -n "${YUTU_YTDLP_URL:-}" ]; then
    YTDLP_URL="$YUTU_YTDLP_URL"
else
    YTDLP_URL="https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe"
fi

TMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/yutu-win-cross.XXXXXX")"
trap 'rm -rf -- "$TMP_ROOT"' EXIT INT TERM

echo "==> downloading mpv dev package ($MPV_TAG)"
mkdir -p "$TMP_ROOT/mpv-dev"
curl --proto '=https' --tlsv1.2 -fsSL -o "$TMP_ROOT/mpv-dev.7z" "$MPV_DEV_URL"
7z x "$TMP_ROOT/mpv-dev.7z" "-o$TMP_ROOT/mpv-dev" -y >/dev/null
LIBMPV_DLL="$(find "$TMP_ROOT/mpv-dev" -name 'libmpv-2.dll' | head -n 1)"
[ -n "$LIBMPV_DLL" ] || {
    echo "error: mpv dev package has no libmpv-2.dll; it changed layout, refusing a broken bundle" >&2
    exit 1
}
MPV_LIB_DIR="$(dirname -- "$LIBMPV_DLL")"

echo "==> building release ($VERSION, windows-$ARCH)"
export RUSTFLAGS="${RUSTFLAGS:-} -L $MPV_LIB_DIR"
# shellcheck disable=SC2086
(cd "$ROOT" && cargo build --release --locked --target x86_64-pc-windows-gnu \
    --config 'target.x86_64-pc-windows-gnu.linker="x86_64-w64-mingw32-gcc"' \
    ${CARGO_FLAGS:-})
BIN="$ROOT/target/x86_64-pc-windows-gnu/release/yutu-mux.exe"
[ -x "$BIN" ] || {
    echo "error: release binary missing at $BIN" >&2
    exit 1
}

echo "==> downloading yt-dlp"
STAGE="$TMP_ROOT/stage"
mkdir -p "$STAGE"
curl --proto '=https' --tlsv1.2 -fsSL -o "$STAGE/yt-dlp.exe" "$YTDLP_URL"

echo "==> staging bundle"
cp "$BIN" "$STAGE/"
cp "$LIBMPV_DLL" "$STAGE/"
ICO="$ROOT/packaging/windows/yutu-mux.ico"
[ -f "$ICO" ] && cp "$ICO" "$STAGE/"
cp "$ROOT/README.md" "$STAGE/"
[ -f "$ROOT/LICENSE" ] && cp "$ROOT/LICENSE" "$STAGE/"

# MinGW runtime check: the exe and libmpv may need libgcc/libstdc++/winpthread
# DLLs when the toolchain links them shared. Copy any such need from the
# MinGW sysroot into the bundle; anything else is a Windows system DLL and
# stays out. A missing sysroot DLL fails loudly instead of shipping a
# bundle that dies on launch.
SYSROOT="$(x86_64-w64-mingw32-gcc -print-sysroot)"
for target in "$STAGE/yutu-mux.exe" "$STAGE/libmpv-2.dll"; do
    needed="$(x86_64-w64-mingw32-objdump -p "$target" | awk '/DLL Name:/{print tolower($3)}')"
    for dll in $needed; do
        if [ -f "$STAGE/$dll" ]; then
            continue
        fi
        case "$dll" in
            libgcc_s_seh-1.dll|libstdc++-6.dll|libwinpthread-1.dll)
                src="$SYSROOT/lib/$dll"
                [ -f "$src" ] || {
                    echo "error: $target needs $dll, not found in MinGW sysroot" >&2
                    exit 1
                }
                cp "$src" "$STAGE/"
                echo "bundled MinGW runtime: $dll"
                ;;
            *)
                echo "uses system DLL: $dll"
                ;;
        esac
    done
done

if command -v file >/dev/null 2>&1; then
    file "$STAGE/yutu-mux.exe"
fi

echo "==> writing zip"
NAME="yutu-mux-$VERSION-windows-$ARCH"
FOLDER="$ROOT/$OUT/$NAME"
rm -rf "$FOLDER"
mkdir -p "$FOLDER"
cp -R "$STAGE" "$FOLDER/$NAME"
python3 - "$FOLDER" "$NAME" <<'EOF'
import shutil
import sys
shutil.make_archive(f"{sys.argv[1]}/{sys.argv[2]}", "zip", sys.argv[1], sys.argv[2])
EOF
(cd "$FOLDER" && sha256sum "$NAME.zip" > "$NAME.zip.sha256")

echo "done: $FOLDER/$NAME.zip"
echo "checksum: $FOLDER/$NAME.zip.sha256"
