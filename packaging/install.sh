#!/bin/sh
# Install yutu-mux and its dependencies on Linux or macOS.
#
#   sh packaging/install.sh [options]
#   curl -fsSL https://raw.githubusercontent.com/Yandall/yutu-mux/main/packaging/install.sh | sh -s -- --yes [options]
#
# Options:
#   --from-release     Download the prebuilt release (Linux x86_64 default).
#   --build-from-source
#                      Compile locally with Cargo (needs Rust and a checkout).
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
# Prebuilt releases cover Linux x86_64. Anything else compiles from
# source, which needs a checkout (cloned automatically when git is
# available) plus Rust. Linux system packages use sudo and macOS
# dependencies use Homebrew. Application files stay below PREFIX.
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
MODE="auto"
PREFIX="${YUTU_INSTALL_PREFIX:-$HOME/.local}"
REPO="${YUTU_REPO:-Yandall/yutu-mux}"
VERSION="${YUTU_VERSION:-latest}"

while [ "$#" -gt 0 ]; do
    case "$1" in
        --from-release) MODE="release" ;;
        --build-from-source|--build) MODE="source" ;;
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

# A checkout is only required for source builds. Remote runs
# (curl ... | sh) have no checkout; the release path never needs one.
ROOT=""
if [ -n "${0:-}" ]; then
    case "$0" in
        *install.sh)
            candidate=$(CDPATH= cd -- "$(dirname -- "$0")/.." 2>/dev/null && pwd) || candidate=""
            if [ -n "$candidate" ] && [ -f "$candidate/Cargo.toml" ]; then
                ROOT="$candidate"
            fi
            ;;
    esac
fi

OS=$(uname -s)
case "$OS" in
    Linux|Darwin) ;;
    *) fail "unsupported operating system: $OS (Linux and macOS only)" ;;
esac
ARCH=$(uname -m)
case "$ARCH" in
    x86_64|amd64) ARCH="x86_64" ;;
esac

if [ "$MODE" = "auto" ]; then
    if [ "$OS" = "Linux" ] && [ "$ARCH" = "x86_64" ]; then
        MODE="release"
    else
        MODE="source"
    fi
fi

if [ "$MODE" = "release" ]; then
    [ "$OS" = "Linux" ] || fail "prebuilt releases cover Linux x86_64 only; rerun with --build-from-source"
    [ "$ARCH" = "x86_64" ] || fail "no prebuilt release for $ARCH; rerun with --build-from-source"
fi

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

install_linux_build_deps() {
    if have apt-get; then
        run_sudo apt-get update
        run_sudo apt-get install -y \
            build-essential pkg-config curl ca-certificates tar \
            libasound2-dev libx11-dev libwayland-dev libxkbcommon-dev \
            libsqlite3-dev libssl-dev libmpv-dev yt-dlp git
    elif have dnf; then
        run_sudo dnf install -y \
            gcc gcc-c++ make pkgconf-pkg-config curl ca-certificates tar \
            alsa-lib-devel libX11-devel wayland-devel libxkbcommon-devel \
            sqlite-devel openssl-devel mpv-libs-devel yt-dlp git
    elif have pacman; then
        run_sudo pacman -S --needed \
            base-devel pkgconf curl ca-certificates tar \
            alsa-lib libx11 wayland libxkbcommon sqlite openssl mpv yt-dlp git
    elif have zypper; then
        run_sudo zypper install -y \
            gcc gcc-c++ make pkg-config curl ca-certificates tar \
            alsa-devel libX11-devel wayland-devel libxkbcommon-devel \
            sqlite3-devel libopenssl-devel libmpv-devel yt-dlp git
    else
        fail "no supported package manager found; install build tools, ALSA, X11/Wayland, SQLite, OpenSSL, libmpv, yt-dlp and git, then rerun with --no-deps"
    fi
}

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

install_macos_deps() {
    have brew || fail "Homebrew is required for automatic macOS dependencies; install it from https://brew.sh or install mpv, yt-dlp and pkg-config manually, then rerun with --no-deps"
    run brew install mpv yt-dlp pkg-config git
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

if [ "$DEPS" -eq 1 ]; then
    if [ "$OS" = Linux ] && [ "$MODE" = "release" ]; then
        say "System dependencies: ALSA, X11/Wayland, SQLite, OpenSSL, libmpv and yt-dlp (runtime only, no compiler or Rust)."
    elif [ "$OS" = Linux ]; then
        say "System dependencies: build tools, ALSA, X11/Wayland, SQLite, OpenSSL, libmpv and yt-dlp."
    else
        say "Homebrew dependencies: mpv, yt-dlp, pkg-config and git."
    fi
    confirm "Install system dependencies?"
    if [ "$DRY_RUN" -eq 0 ]; then
        if [ "$OS" = Linux ] && [ "$MODE" = "release" ]; then
            install_linux_runtime_deps
        elif [ "$OS" = Linux ]; then
            install_linux_build_deps
        else
            install_macos_deps
        fi
    else
        say "would install dependencies using the detected package manager"
    fi
fi

[ "$DEPS_ONLY" -eq 0 ] || {
    say "Dependencies installed. No application files were installed."
    exit 0
}

if [ "$MODE" = "release" ]; then
    have curl || fail "curl is required to download the release; install it, then rerun"
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
    if [ -z "$ROOT" ]; then
        have git || fail "building from source needs a checkout: git clone https://github.com/$REPO.git, then run packaging/install.sh there"
        confirm "No checkout found. Clone $REPO now?"
        if [ "$DRY_RUN" -eq 1 ]; then
            say "would clone https://github.com/$REPO.git (tag: $VERSION)"
            ROOT="$TMP_ROOT/yutu-mux-src"
        else
            if [ "$VERSION" = "latest" ]; then
                run git clone --depth 1 "https://github.com/$REPO.git" "$TMP_ROOT/yutu-mux-src"
            else
                run git clone --depth 1 --branch "$VERSION" "https://github.com/$REPO.git" "$TMP_ROOT/yutu-mux-src"
            fi
            ROOT="$TMP_ROOT/yutu-mux-src"
            [ -f "$ROOT/Cargo.toml" ] || fail "clone did not produce a yutu-mux checkout"
        fi
    fi
    if ! have cargo; then
        if [ "$DEPS" -eq 0 ]; then
            fail "Rust/Cargo is missing; install it from https://rustup.rs, then rerun"
        fi
        confirm "Rust is missing. Install it with rustup now?"
        have curl || fail "curl is required to install Rust"
        if [ "$DRY_RUN" -eq 1 ]; then
            say "would download and run rustup-init with -y --profile minimal --default-toolchain stable"
        else
            curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs \
                | sh -s -- -y --profile minimal --default-toolchain stable
            # rustup installs here unless the user configured CARGO_HOME.
            # Do not replace an existing CARGO_HOME.
            CARGO_BIN="${CARGO_HOME:-$HOME/.cargo}/bin"
            PATH="$CARGO_BIN:$PATH"
            export PATH
        fi
    fi
    if [ "$DRY_RUN" -eq 0 ] && ! have cargo; then
        fail "Cargo is still unavailable; restart the shell or add ~/.cargo/bin to PATH"
    fi
    say "Building yutu-mux in release mode (this can take several minutes)."
    if [ "$OS" = Linux ]; then
        run cargo build --release --locked --manifest-path "$ROOT/Cargo.toml"
    else
        # build-app.sh performs the release build and ad-hoc signs the bundle.
        run env CARGO_FLAGS= "$ROOT/packaging/macos/build-app.sh" \
            --output "$ROOT/target/install-macos"
    fi
    BIN_SRC="$ROOT/target/release/yutu-mux"
    if [ "$DRY_RUN" -eq 0 ] && [ ! -x "$BIN_SRC" ]; then
        fail "release build finished without producing $BIN_SRC"
    fi
fi

replace_target() {
    if [ -e "$1" ] || [ -L "$1" ]; then
        confirm "Replace existing $1?"
    fi
}

# Install only after the complete release download or build has
# succeeded. A failed download or build therefore leaves the previous
# application untouched.
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
    make_dir "$BIN_DIR"
    make_dir "$APPS_DIR"
    STAGED_DESKTOP="$TMP_ROOT/yutu-mux.desktop"
    if [ "$DRY_RUN" -eq 0 ]; then
        write_desktop_file "$STAGED_DESKTOP"
    fi
    install_new_file 755 "$BIN_DIR/yutu-mux" "$BIN_SRC"
    install_new_file 644 "$APPS_DIR/yutu-mux.desktop" "$STAGED_DESKTOP"
    say "Installed yutu-mux to $BIN_DIR/yutu-mux"
    say "Desktop launcher: $APPS_DIR/yutu-mux.desktop"
else
    APP_DIR="$PREFIX/Applications/YutuMux.app"
    BIN_DIR="$PREFIX/bin"
    BUILD_OUT="$ROOT/target/install-macos"
    replace_target "$APP_DIR"
    replace_target "$BIN_DIR/yutu-mux"
    make_dir "$PREFIX/Applications"
    make_dir "$BIN_DIR"
    if [ "$DRY_RUN" -eq 1 ]; then
        say "would copy $BUILD_OUT/stage/YutuMux.app to $PREFIX/Applications/"
    else
        temporary="$PREFIX/Applications/.YutuMux.app.install-new.$$"
        rm -rf -- "$temporary"
        copy_file "$BUILD_OUT/stage/YutuMux.app" "$temporary"
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
