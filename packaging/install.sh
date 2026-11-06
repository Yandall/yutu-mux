#!/bin/sh
# Install yutu-mux and its dependencies on Linux or macOS.
#
#   packaging/install.sh [options]
#
# Options:
#   --deps-only      Install system dependencies, then stop.
#   --no-deps        Skip system dependency installation.
#   --prefix DIR     Install below DIR (default: ~/.local).
#   --yes            Do not ask before installing or replacing files.
#   --dry-run        Print actions without installing or building anything.
#   -h, --help       Show this help.
#
# Linux installs target/release/yutu-mux to PREFIX/bin and a desktop
# launcher to PREFIX/share/applications. macOS installs YutuMux.app to
# PREFIX/Applications and a yutu-mux command to PREFIX/bin.
#
# Run from a repository checkout. Linux system packages use sudo and
# macOS dependencies use Homebrew. Application files stay below PREFIX.
set -eu

usage() {
    sed -n '2,23p' "$0"
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

while [ "$#" -gt 0 ]; do
    case "$1" in
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

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
[ -f "$ROOT/Cargo.toml" ] || fail "run this script from a yutu-mux checkout"
OS=$(uname -s)
case "$OS" in
    Linux|Darwin) ;;
    *) fail "unsupported operating system: $OS (Linux and macOS only)" ;;
esac

if [ "$DRY_RUN" -eq 1 ]; then
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

install_linux_deps() {
    if have apt-get; then
        run_sudo apt-get update
        run_sudo apt-get install -y \
            build-essential pkg-config curl \
            libasound2-dev libx11-dev libwayland-dev libxkbcommon-dev \
            libsqlite3-dev libssl-dev libmpv-dev yt-dlp
    elif have dnf; then
        run_sudo dnf install -y \
            gcc gcc-c++ make pkgconf-pkg-config curl \
            alsa-lib-devel libX11-devel wayland-devel libxkbcommon-devel \
            sqlite-devel openssl-devel mpv-libs-devel yt-dlp
    elif have pacman; then
        run_sudo pacman -S --needed \
            base-devel pkgconf curl \
            alsa-lib libx11 wayland libxkbcommon sqlite openssl mpv yt-dlp
    elif have zypper; then
        run_sudo zypper install -y \
            gcc gcc-c++ make pkg-config curl \
            alsa-devel libX11-devel wayland-devel libxkbcommon-devel \
            sqlite3-devel libopenssl-devel libmpv-devel yt-dlp
    else
        fail "no supported package manager found; install Rust, ALSA, X11/Wayland, SQLite, OpenSSL, libmpv and yt-dlp, then rerun with --no-deps"
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
    run brew install mpv yt-dlp pkg-config
}

if [ "$DEPS" -eq 1 ]; then
    if [ "$OS" = Linux ]; then
        say "System dependencies: build tools, ALSA, X11/Wayland, SQLite, OpenSSL, libmpv and yt-dlp."
    else
        say "Homebrew dependencies: mpv, yt-dlp and pkg-config."
    fi
    confirm "Install system dependencies?"
    if [ "$DRY_RUN" -eq 0 ]; then
        if [ "$OS" = Linux ]; then
            install_linux_deps
        else
            install_macos_deps
        fi
    else
        say "would install dependencies using the detected package manager"
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

[ "$DEPS_ONLY" -eq 0 ] || {
    say "Dependencies installed. No application files were installed."
    exit 0
}

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

BIN="$ROOT/target/release/yutu-mux"
if [ "$DRY_RUN" -eq 0 ] && [ ! -x "$BIN" ]; then
    fail "release build finished without producing $BIN"
fi

replace_target() {
    if [ -e "$1" ] || [ -L "$1" ]; then
        confirm "Replace existing $1?"
    fi
}

# Install only after the complete release build has succeeded. A failed
# build therefore leaves the previous application untouched.
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
    install_new_file 755 "$BIN_DIR/yutu-mux" "$BIN"
    install_new_file 644 "$APPS_DIR/yutu-mux.desktop" "$ROOT/packaging/linux/yutu-mux.desktop"
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
