#!/bin/sh
# Build the Windows zip on Linux through the cross-build container.
#
#   packaging/windows/build-windows-docker.sh [--output DIR] [CROSS-ARGS...]
#
# Same output as the native script (same folder and zip names, same
# self-contained layout): yutu-mux.exe cross-compiled with MinGW-w64 for
# x86_64-pc-windows-gnu, plus libmpv-2.dll and yt-dlp.exe beside it.
# The container image carries Rust, the GNU Windows target, MinGW, and
# 7-Zip, so the host needs only Docker. A persistent cargo registry cache
# lives below ~/.cache so rebuilds do not re-download crates.
#
# Limits: this produces the portable zip only (what install.ps1 and Scoop
# consume). The Inno Setup.exe still needs Windows: ISCC.exe is a
# Windows-only compiler, so the installer is built by the release
# workflow or by packaging/windows/build-windows.ps1 on a Windows host.
set -eu

ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/../.." 2>/dev/null && pwd)"
IMAGE="${YUTU_WINDOWS_IMAGE:-yutu-mux-windows-cross}"
CACHE_DIR="${YUTU_CARGO_CACHE_DIR:-$HOME/.cache/yutu-mux-cargo}"

if ! command -v docker >/dev/null 2>&1; then
    echo "error: docker is required for the Linux Windows build" >&2
    exit 1
fi

echo "==> building cross image ($IMAGE)"
docker build -f packaging/windows/Dockerfile -t "$IMAGE" "$ROOT"

mkdir -p "$CACHE_DIR"
echo "==> building Windows zip in container"
# Run as the calling user so target/ and dist/ stay owned by them, with
# CARGO_HOME on a cached host dir for the same reason (the image stock
# /usr/local/cargo is root-owned).
docker run --rm \
    -v "$ROOT:/work" -w /work \
    -v "$CACHE_DIR:/cargo" \
    -e CARGO_HOME=/cargo \
    --user "$(id -u):$(id -g)" \
    "$IMAGE" "$@"
