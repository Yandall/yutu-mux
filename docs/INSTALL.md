# Per-user installer (Linux + macOS)

No checkout needed on Linux x86_64 or macOS arm64: the installer
downloads the prebuilt release (tarball or disk image), so Rust is
never installed. One command (pass `--yes` because a
pipe has no terminal for confirmation prompts):

```sh
curl -fsSL https://raw.githubusercontent.com/Yandall/yutu-mux/main/packaging/install.sh | sh -s -- --yes
```

Prefer to inspect first? Download the script, read it, then run it:

```sh
curl -fsSL -o install.sh https://raw.githubusercontent.com/Yandall/yutu-mux/main/packaging/install.sh
sh install.sh --help
sh install.sh --yes
```

From a checkout, the same script lives at `packaging/install.sh`:

```sh
git clone https://github.com/Yandall/yutu-mux.git
cd yutu-mux
./packaging/install.sh
```

The script prompts before using the system package manager and before
replacing existing application files. Use `--yes` for unattended installs.

## Prebuilt release vs source build

The default is `--from-release`: download the prebuilt artifact from
the latest GitHub release and install it. Linux x86_64 gets the
`yutu-mux-<version>-linux-x86_64.tar.gz` tarball (runtime libraries
only: ALSA, X11/Wayland, SQLite, OpenSSL, libmpv, yt-dlp); macOS
arm64 gets the self-contained `YutuMux-<version>-macos-arm64.dmg`
disk image (Homebrew mpv and yt-dlp as runtime fallback). No compiler
and no Rust on either platform.

Every release ships a matching `.sha256` file, and the installer refuses
to unpack or install when the checksum is missing or mismatched. TLS
already authenticates the GitHub download; the checksum additionally
guards against truncation, corruption, and CDN mixups. It is an
integrity check, not an authorship signature.

```sh
# Pin a release, or point at a fork:
sh install.sh --yes --version v0.2.0
sh install.sh --yes --repo OWNER/REPO
# Build manually instead (needs Rust plus a checkout; the installer
# never compiles): cargo build --release, or
# packaging/macos/build-app.sh --self-contained on a Mac.
```

Platforms without a prebuilt asset are not covered by the installer;
build them manually (`cargo build --release`, or
`packaging/macos/build-app.sh` on a Mac, see `docs/PACKAGING.md`).

## Linux

Supported package-manager families:

- Debian/Ubuntu: `apt-get`
- Fedora: `dnf`
- Arch: `pacman`
- openSUSE: `zypper`

The default feature set needs ALSA, X11/Wayland, SQLite, OpenSSL, libmpv,
and yt-dlp. Package installation uses `sudo` (or runs directly as root).
On distributions where mpv or yt-dlp is supplied by an additional
repository, enable that repository first or install the missing packages
manually and rerun with `--no-deps`.

Installed files:

```text
~/.local/bin/yutu-mux
~/.local/share/applications/yutu-mux.desktop
~/.local/share/icons/hicolor/*/apps/yutu-mux.png   (16-256px)
~/.local/share/icons/hicolor/scalable/apps/yutu-mux.svg
```

The desktop entry launches `yutu-mux` by name, so `~/.local/bin` must be on
PATH. Many desktop distributions configure this automatically after the
next login. The launcher uses the installed rings + M icon set
(`~/.local/share/icons/hicolor`), refreshed with `gtk-update-icon-cache`
when available.

## macOS

The default release path mounts the downloaded disk image (checksum
verified first), copies `YutuMux.app` out, and installs it below the
prefix. Dependencies come from Homebrew (`mpv` and `yt-dlp`, runtime
fallback). Manual builds reuse `packaging/macos/build-app.sh` to build
and ad-hoc sign the bundle (see `docs/PACKAGING.md`).

Installed files:

```text
~/.local/Applications/YutuMux.app
~/.local/bin/yutu-mux -> ~/.local/Applications/YutuMux.app/Contents/MacOS/YutuMux
```

Move `YutuMux.app` to `/Applications` manually if preferred. First launch
requires right-click, Open because the bundle is ad-hoc signed, not
notarized. The release disk image is self-contained (libmpv plus its
own yt-dlp sidecar); the Homebrew packages stay as fallback. The macOS
installer path is untested
because the maintainer has no Mac.

## Options and removal

```sh
./packaging/install.sh --dry-run       # print actions without changes
./packaging/install.sh --no-deps       # skip system package installation
./packaging/install.sh --deps-only     # install dependencies only
./packaging/install.sh --prefix /absolute/path --yes
./packaging/install.sh --from-release --version v0.2.0
./packaging/uninstall.sh               # remove app files, keep user data
```

`YUTU_INSTALL_PREFIX` provides another way to set the installation prefix.
The prefix must be absolute. Uninstall removes only the binary, Linux
launcher, and macOS bundle listed above. It never removes the library,
artwork/lyrics cache, logs, or sign-in session.

Windows is not supported by these scripts. Native Windows packaging is
planned separately; a Windows `.exe` cannot be produced by running these
Unix scripts.
