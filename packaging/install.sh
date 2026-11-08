#!/bin/sh
# Install yutu-mux and its dependencies on Linux or macOS.
#
#   sh packaging/install.sh [options]
#   curl -fsSL https://raw.githubusercontent.com/Yandall/yutu-mux/main/packaging/install.sh | sh -s -- --yes [options]
#
# The installer only fetches prebuilt release artifacts (Linux x86_64
# tarball, macOS arm64 disk image) plus system dependencies. It never
# compiles anything and never installs Rust. To build from source, see
# docs/PACKAGING.md and run cargo or packaging/macos/build-app.sh
# by hand.
#
# Options:
#   --version TAG      Release to download (default: latest).
#   --repo OWNER/REPO  GitHub repository (default: Yandall/yutu-mux).
#   --deps-only        Install system dependencies, then stop.
#   --no-deps          Skip system dependency installation.
#   --prefix DIR       Install below DIR (default: ~/.local).
#   --yes              Do not ask before installing or replacing files.
#   --dry-run          Print actions without installing or building anything.
#   -h, --help         Show this help.
#
# Linux installs the yutu-mux binary to PREFIX/bin and a desktop
# launcher to PREFIX/share/applications. macOS installs YutuMux.app to
# PREFIX/Applications and a yutu-mux command to PREFIX/bin.
#
# Linux system packages use sudo and macOS dependencies use Homebrew.
# Application files stay below PREFIX.
set -eu

usage() {
    sed -n '2,27p' "$0"
}

say() {
    printf '%s\n' "$*"
}

fail() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

have() {
    command -v "$1" >/dev/null 2>&1
}

DEPS=1
DEPS_ONLY=0
ASSUME_YES=0
DRY_RUN=0
PREFIX="${YUTU_INSTALL_PREFIX:-$HOME/.local}"
REPO="${YUTU_REPO:-Yandall/yutu-mux}"
VERSION="${YUTU_VERSION:-latest}"

while [ "$#" -gt 0 ]; do
    case "$1" in
        --from-release) ;;
        --build-from-source|--build) fail "the installer never builds from source; it only installs the prebuilt release (build manually with cargo, see docs/PACKAGING.md)" ;;
        --version)
            [ "$#" -ge 2 ] || fail "--version needs a tag (for example v0.2.0)"
            VERSION=$2
            shift
            ;;
        --repo)
            [ "$#" -ge 2 ] || fail "--repo needs OWNER/REPO"
            REPO=$2
            shift
            ;;
        --deps-only) DEPS_ONLY=1 ;;
        --no-deps) DEPS=0 ;;
        --prefix)
            [ "$#" -ge 2 ] || fail "--prefix needs a directory"
            PREFIX=$2
            shift
            ;;
        --yes) ASSUME_YES=1 ;;
        --dry-run) DRY_RUN=1 ;;
        -h|--help)
            usage
            exit 0
            ;;
        *) fail "unknown option: $1 (use --help)" ;;
    esac
    shift
done

case "$PREFIX" in
    /*) ;;
    *) fail "install prefix must be an absolute path: $PREFIX" ;;
esac
[ "$PREFIX" != "/" ] || fail "refusing to install directly below /"
case "$PREFIX" in
    *"/.."|*"/../"*|"/.."|*"/./"*|*"/.") fail "install prefix must not contain . or .. components" ;;
esac

OS=$(uname -s)
case "$OS" in
    Linux|Darwin) ;;
    *) fail "unsupported operating system: $OS (Linux and macOS only)" ;;
esac
ARCH=$(uname -m)
case "$ARCH" in
    x86_64|amd64) ARCH="x86_64" ;;
esac

case "$OS-$ARCH" in
    Linux-x86_64|Darwin-arm64|Darwin-x86_64) ;;
    *) fail "no prebuilt release for $OS $ARCH" ;;
esac

if [ "$DRY_RUN" -eq 1 ]; then
    TMP_ROOT="/tmp/yutu-mux-install-dry-run"
    confirm() {
        say "would ask: $1"
    }
    run() {
        printf 'would run:'
        printf ' %s' "$@"
        printf '\n'
    }
    make_dir() {
        say "would create directory: $1"
    }
    copy_file() {
        say "would copy $1 to $2"
    }
    install_file() {
        say "would install $2 to $3 (mode $1)"
    }
else
    TMP_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/yutu-mux-install.XXXXXX")
    trap 'rm -rf -- "$TMP_ROOT"' EXIT INT TERM
    confirm() {
        if [ "$ASSUME_YES" -eq 1 ]; then
            return 0
        fi
        if [ ! -t 0 ]; then
            fail "$1 (rerun with --yes to allow this)"
        fi
        printf '%s [y/N] ' "$1"
        IFS= read -r answer
        case "$answer" in
            y|Y|yes|YES|Yes) return 0 ;;
            *) fail "cancelled" ;;
        esac
    }
    run() {
        "$@"
    }
    make_dir() {
        mkdir -p "$1"
    }
    copy_file() {
        if [ -d "$1" ]; then
            cp -R "$1" "$2"
        else
            cp "$1" "$2"
        fi
    }
    install_file() {
        install -m "$1" "$2" "$3"
    }
fi

install_linux_runtime_deps() {
    if have apt-get; then
        run_sudo apt-get update
        alsa_pkg="libasound2"
        if have apt-cache && apt-cache show libasound2t64 >/dev/null 2>&1; then
            alsa_pkg="libasound2t64"
        fi
        run_sudo apt-get install -y \
            curl ca-certificates tar yt-dlp libmpv2 "$alsa_pkg" \
            libx11-6 libwayland-client0 libxkbcommon0 libsqlite3-0 libssl3
    elif have dnf; then
        run_sudo dnf install -y \
            curl ca-certificates tar yt-dlp mpv-libs \
            alsa-lib libX11 wayland libxkbcommon sqlite openssl-libs
    elif have pacman; then
        run_sudo pacman -S --needed \
            curl ca-certificates tar yt-dlp mpv \
            alsa-lib libx11 wayland libxkbcommon sqlite openssl
    elif have zypper; then
        run_sudo zypper install -y \
            curl ca-certificates tar yt-dlp libmpv2 \
            alsa libX11-6 libwayland-client0 libxkbcommon0 libsqlite3-0 libopenssl3
    else
        fail "no supported package manager found; install ALSA, X11/Wayland, SQLite, OpenSSL, libmpv and yt-dlp, then rerun with --no-deps"
    fi
}

run_sudo() {
    if [ "$(id -u)" -eq 0 ]; then
        run "$@"
    elif have sudo; then
        run sudo "$@"
    else
        fail "system packages require root or sudo; install them manually, then rerun with --no-deps"
    fi
}

install_macos_runtime_deps() {
    have brew || fail "Homebrew is required for automatic macOS dependencies; install it from https://brew.sh or install mpv and yt-dlp manually, then rerun with --no-deps"
    run brew install mpv yt-dlp
}

write_desktop_file() {
    cat > "$1" <<'EOF'
[Desktop Entry]
Type=Application
Name=YutuMux
Comment=Native YouTube Music desktop client
Exec=yutu-mux
Icon=yutu-mux
Terminal=false
Categories=AudioVideo;Audio;Player;Music;
Keywords=YouTube;Music;Player;Streaming;
StartupNotify=true
EOF
}

sha256_of() {
    if have sha256sum; then
        sha256sum "$1" | cut -d' ' -f1
    elif have shasum; then
        shasum -a 256 "$1" | cut -d' ' -f1
    else
        fail "neither sha256sum nor shasum is available; install coreutils or perl, then rerun"
    fi
}

# Verify a release tarball against its .sha256 file. Fails closed: a
# missing or mismatched checksum aborts before anything is unpacked or
# installed. (TLS already authenticates the GitHub download; the
# checksum additionally guards against truncation, corruption, and
# CDN mixups. It is not a signature: see docs/INSTALL.md.)
verify_release() {
    archive=$1
    checksum_file=$2
    [ -f "$checksum_file" ] || fail "checksum file is missing: $checksum_file"
    expected=$(cut -d' ' -f1 < "$checksum_file")
    case "$expected" in
        ""|*[!0-9a-fA-F]*) fail "checksum file is malformed: $checksum_file" ;;
    esac
    actual=$(sha256_of "$archive")
    [ "$expected" = "$actual" ] || fail "checksum mismatch for $(basename "$archive"): download is corrupt or tampered with, refusing to install"
}

# Follow the /releases/latest redirect to its tag (vX.Y.Z). The disk
# image asset name embeds the Cargo version, so `latest` must resolve
# to a tag before the asset name is known.
resolve_latest_tag() {
    effective=$(curl --proto '=https' --tlsv1.2 -fsSL -o /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest") \
        || fail "could not resolve the latest release (check that $REPO has releases)"
    case "$effective" in
        */releases/tag/?*)
            tag=${effective##*/releases/tag/}
            ;;
        *)
            fail "could not resolve the latest release tag from $effective"
            ;;
    esac
    printf '%s\n' "$tag"
}

if [ "$DEPS" -eq 1 ]; then
    if [ "$OS" = Linux ]; then
        say "System dependencies: ALSA, X11/Wayland, SQLite, OpenSSL, libmpv and yt-dlp (runtime only, no compiler or Rust)."
    else
        say "Homebrew dependencies: mpv and yt-dlp (runtime only, no compiler or Rust; the bundle also carries its own copies)."
    fi
    confirm "Install system dependencies?"
    if [ "$DRY_RUN" -eq 0 ]; then
        if [ "$OS" = Linux ]; then
            install_linux_runtime_deps
        else
            install_macos_runtime_deps
        fi
    else
        say "would install dependencies using the detected package manager"
    fi
fi

[ "$DEPS_ONLY" -eq 0 ] || {
    say "Dependencies installed. No application files were installed."
    exit 0
}

have curl || fail "curl is required to download the release; install it, then rerun"
if [ "$OS" = "Linux" ]; then
    have tar || fail "tar is required to unpack the release; install it, then rerun"
    ASSET="yutu-mux-x86_64-unknown-linux-gnu.tar.gz"
    if [ -n "${YUTU_RELEASE_URL:-}" ]; then
        URL="$YUTU_RELEASE_URL"
    elif [ "$VERSION" = "latest" ]; then
        URL="https://github.com/$REPO/releases/latest/download/$ASSET"
    else
        URL="https://github.com/$REPO/releases/download/$VERSION/$ASSET"
    fi
    say "Downloading prebuilt yutu-mux ($VERSION, Linux x86_64)."
    if [ "$DRY_RUN" -eq 1 ]; then
        say "would download $URL and unpack it"
        say "would verify $URL.sha256 before unpacking"
        BIN_SRC="$TMP_ROOT/yutu-mux-x86_64-unknown-linux-gnu/yutu-mux"
    elif [ -n "${YUTU_TEST_RELEASE_FILE:-}" ]; then
        say "Using test release file $YUTU_TEST_RELEASE_FILE."
        cp "$YUTU_TEST_RELEASE_FILE" "$TMP_ROOT/pkg.tar.gz"
        if [ -f "${YUTU_TEST_RELEASE_FILE}.sha256" ]; then
            cp "${YUTU_TEST_RELEASE_FILE}.sha256" "$TMP_ROOT/pkg.tar.gz.sha256"
            verify_release "$TMP_ROOT/pkg.tar.gz" "$TMP_ROOT/pkg.tar.gz.sha256"
        else
            say "No test checksum file found; skipping verification (test hook only)."
        fi
        run tar -xzf "$TMP_ROOT/pkg.tar.gz" -C "$TMP_ROOT"
        BIN_SRC=$(find "$TMP_ROOT" -name yutu-mux -type f | head -n 1)
        [ -n "$BIN_SRC" ] || fail "release archive did not contain a yutu-mux binary"
    else
        run curl --proto '=https' --tlsv1.2 -fsSL -o "$TMP_ROOT/pkg.tar.gz" "$URL" \
            || fail "download failed: $URL (check the release exists)"
        run curl --proto '=https' --tlsv1.2 -fsSL -o "$TMP_ROOT/pkg.tar.gz.sha256" "$URL.sha256" \
            || fail "checksum download failed: $URL.sha256 (the release is missing its .sha256 file)"
        verify_release "$TMP_ROOT/pkg.tar.gz" "$TMP_ROOT/pkg.tar.gz.sha256"
        run tar -xzf "$TMP_ROOT/pkg.tar.gz" -C "$TMP_ROOT"
        BIN_SRC=$(find "$TMP_ROOT" -name yutu-mux -type f | head -n 1)
        [ -n "$BIN_SRC" ] || fail "release archive did not contain a yutu-mux binary"
    fi
    else
        # macOS disk image (YutuMux-<version>-macos-<arch>.dmg). The
        # asset name embeds the Cargo version without a leading v,
        # while --version takes the release tag, so strip one leading
        # v. `latest` resolves through the /releases/latest redirect.
        have hdiutil || fail "hdiutil is required to mount the disk image; it ships with macOS"
        if [ "$VERSION" = "latest" ]; then
            if [ "$DRY_RUN" -eq 1 ]; then
                say "would resolve the latest release tag"
                TAG="vX.Y.Z"
            else
                TAG=$(resolve_latest_tag)
            fi
        else
            TAG="$VERSION"
        fi
        case "$TAG" in
            v*) ASSET_VERSION=${TAG#v} ;;
            *) ASSET_VERSION=$TAG ;;
        esac
        ASSET="YutuMux-$ASSET_VERSION-macos-$ARCH.dmg"
        if [ -n "${YUTU_RELEASE_URL:-}" ]; then
            URL="$YUTU_RELEASE_URL"
        elif [ "$VERSION" = "latest" ]; then
            URL="https://github.com/$REPO/releases/latest/download/$ASSET"
        else
            URL="https://github.com/$REPO/releases/download/$VERSION/$ASSET"
        fi
        say "Downloading prebuilt YutuMux ($TAG, macOS $ARCH)."
        if [ "$DRY_RUN" -eq 1 ]; then
            say "would download $URL and mount it"
            say "would verify $URL.sha256 before mounting"
            APP_SRC="$TMP_ROOT/YutuMux.app"
        else
            run curl --proto '=https' --tlsv1.2 -fsSL -o "$TMP_ROOT/pkg.dmg" "$URL" \
                || fail "download failed: $URL (check the release has a macOS $ARCH asset)"
            run curl --proto '=https' --tlsv1.2 -fsSL -o "$TMP_ROOT/pkg.dmg.sha256" "$URL.sha256" \
                || fail "checksum download failed: $URL.sha256 (the release is missing its .sha256 file)"
            verify_release "$TMP_ROOT/pkg.dmg" "$TMP_ROOT/pkg.dmg.sha256"
            MNT="$TMP_ROOT/mnt"
            make_dir "$MNT"
            trap 'hdiutil detach "$MNT" >/dev/null 2>&1 || true; rm -rf -- "$TMP_ROOT"' EXIT INT TERM
            run hdiutil attach -nobrowse -readonly -mountpoint "$MNT" "$TMP_ROOT/pkg.dmg"
            if [ ! -d "$MNT/YutuMux.app" ]; then
                run hdiutil detach "$MNT" || true
                fail "disk image did not contain YutuMux.app"
            fi
            rm -rf -- "$TMP_ROOT/YutuMux.app"
            copy_file "$MNT/YutuMux.app" "$TMP_ROOT/"
            run hdiutil detach "$MNT"
            APP_SRC="$TMP_ROOT/YutuMux.app"
        fi
    fi

replace_target() {
    if [ -e "$1" ] || [ -L "$1" ]; then
        confirm "Replace existing $1?"
    fi
}

# Install only after the complete release download has succeeded. A
# failed download therefore leaves the previous application untouched.
install_new_file() {
    target=$2
    replace_target "$target"
    if [ "$DRY_RUN" -eq 1 ]; then
        say "would install $3 to $2 (mode $1)"
    else
        temporary="$target.install-new.$$"
        rm -f -- "$temporary"
        install_file "$1" "$3" "$temporary"
        mv -f -- "$temporary" "$target"
    fi
}

if [ "$OS" = Linux ]; then
    BIN_DIR="$PREFIX/bin"
    APPS_DIR="$PREFIX/share/applications"
    ICONS_DIR="$PREFIX/share/icons"
    make_dir "$BIN_DIR"
    make_dir "$APPS_DIR"
    make_dir "$ICONS_DIR"
    STAGED_DESKTOP="$TMP_ROOT/yutu-mux.desktop"
    if [ "$DRY_RUN" -eq 0 ]; then
        write_desktop_file "$STAGED_DESKTOP"
    fi
    install_new_file 755 "$BIN_DIR/yutu-mux" "$BIN_SRC"
    install_new_file 644 "$APPS_DIR/yutu-mux.desktop" "$STAGED_DESKTOP"
    # Icons ship inside the tarball; a checkout run prefers its own
    # icon tree (same files).
    SCRIPT_ICONS=""
    case "$0" in
        *install.sh)
            candidate=$(CDPATH= cd -- "$(dirname -- "$0")/.." 2>/dev/null && pwd) || candidate=""
            if [ -n "$candidate" ] && [ -d "$candidate/packaging/linux/icons/hicolor" ]; then
                SCRIPT_ICONS="$candidate/packaging/linux/icons/hicolor"
            fi
            ;;
    esac
    if [ -n "$SCRIPT_ICONS" ]; then
        ICON_SRC="$SCRIPT_ICONS"
    else
        ICON_SRC=$(find "$TMP_ROOT" -type d -name hicolor | head -n 1)
    fi
    if [ -n "${ICON_SRC:-}" ] && [ -d "$ICON_SRC" ]; then
        if [ "$DRY_RUN" -eq 1 ]; then
            say "would install icon theme entries from $ICON_SRC to $ICONS_DIR"
        else
            copy_file "$ICON_SRC" "$ICONS_DIR/"
            if have gtk-update-icon-cache; then
                run gtk-update-icon-cache -f -t "$ICONS_DIR/hicolor" || true
            fi
        fi
    else
        say "No icon set found; skipping icon installation."
    fi
    say "Installed yutu-mux to $BIN_DIR/yutu-mux"
    say "Desktop launcher: $APPS_DIR/yutu-mux.desktop"
else
    APP_DIR="$PREFIX/Applications/YutuMux.app"
    BIN_DIR="$PREFIX/bin"
    # APP_SRC came from the mounted disk image above.
    replace_target "$APP_DIR"
    replace_target "$BIN_DIR/yutu-mux"
    make_dir "$PREFIX/Applications"
    make_dir "$BIN_DIR"
    if [ "$DRY_RUN" -eq 1 ]; then
        say "would copy $APP_SRC to $PREFIX/Applications/"
    else
        temporary="$PREFIX/Applications/.YutuMux.app.install-new.$$"
        rm -rf -- "$temporary"
        copy_file "$APP_SRC" "$temporary"
        rm -rf -- "$APP_DIR"
        mv -- "$temporary" "$APP_DIR"
    fi
    # The bundle launcher supplies Homebrew paths for yt-dlp and libmpv.
    # Link the command-line entry point to that same launcher.
    if [ "$DRY_RUN" -eq 1 ]; then
        say "would link $BIN_DIR/yutu-mux to $APP_DIR/Contents/MacOS/YutuMux"
    else
        temporary="$BIN_DIR/.yutu-mux.install-new.$$"
        ln -s "$APP_DIR/Contents/MacOS/YutuMux" "$temporary"
        mv -f -- "$temporary" "$BIN_DIR/yutu-mux"
    fi
    say "Installed $APP_DIR"
    say "First launch: right-click YutuMux and choose Open (ad-hoc signed)."
fi

case ":$PATH:" in
    *":$PREFIX/bin:"*) ;;
    *) say "Add $PREFIX/bin to PATH to run yutu-mux from a terminal." ;;
esac
say "Launch yutu-mux and sign in through Settings with a cookies.txt file."
