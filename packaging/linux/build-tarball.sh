#!/bin/sh
# Build the release binary and wrap it as a portable Linux tarball.
#
#   packaging/linux/build-tarball.sh [--output DIR]
#
# Output (under DIR, default dist/): a versioned folder
# yutu-mux-<version>-linux-<arch>/ holding the tarball
# yutu-mux-<version>-linux-<arch>.tar.gz plus its .tar.gz.sha256
# checksum (the two files uploaded to GitHub Releases).
#
# Requirements: Linux, Rust, and the build libraries (ALSA, X11/Wayland,
# SQLite, OpenSSL, libmpv headers). The tarball binary links those
# system libraries, so it is portable across similar desktop distros,
# not static. Recipients need the runtime libraries installed (the
# per-user installer handles that).
set -eu

OUT="dist"

while [ $# -gt 0 ]; do
    case "$1" in
        --output) OUT="$2"; shift ;;
        -h|--help)
            sed -n '2,12p' "$0"
            exit 0
            ;;
        *) echo "unknown flag: $1 (see --help)" >&2; exit 1 ;;
    esac
    shift
done

if [ "$(uname -s)" != "Linux" ]; then
    echo "error: this script wraps a Linux tarball; run it on Linux" >&2
    exit 1
fi

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
VERSION="$(grep -m1 '^version *= *"' "$ROOT/Cargo.toml" | cut -d'"' -f2)"
ARCH="$(uname -m)"
case "$ARCH" in
    x86_64|amd64) ARCH="x86_64" ;;
    *) echo "error: no portable tarball for $ARCH" >&2; exit 1 ;;
esac

echo "==> building release ($VERSION, $ARCH)"
(cd "$ROOT" && cargo build --release --locked)
BIN="$ROOT/target/release/yutu-mux"
[ -x "$BIN" ] || { echo "error: release binary missing at $BIN" >&2; exit 1; }

echo "==> staging tarball"
NAME="yutu-mux-$VERSION-linux-$ARCH"
FOLDER="$OUT/$NAME"
rm -rf "$FOLDER"
mkdir -p "$FOLDER/stage/$NAME"
STAGE="$FOLDER/stage/$NAME"
cp "$BIN" "$STAGE/"
cp "$ROOT/README.md" "$STAGE/"
if test -f "$ROOT/LICENSE"; then cp "$ROOT/LICENSE" "$STAGE/"; fi
cp "$ROOT/flatpak/org.yutu_mux.YutuMux.yaml" "$ROOT/flatpak/org.yutu_mux.YutuMux.desktop" "$STAGE/"
cp -R "$ROOT/packaging/linux/icons" "$STAGE/icons"
tar -czf "$FOLDER/$NAME.tar.gz" -C "$FOLDER/stage" "$NAME"
rm -rf "$FOLDER/stage"
(cd "$FOLDER" && sha256sum "$NAME.tar.gz" > "$NAME.tar.gz.sha256")

echo "done: $FOLDER/$NAME.tar.gz"
echo "checksum: $FOLDER/$NAME.tar.gz.sha256"
