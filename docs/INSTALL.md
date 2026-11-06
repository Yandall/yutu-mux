# Per-user installer (Linux + macOS)

`packaging/install.sh` turns a repository checkout into a per-user
installation. It installs system dependencies, installs Rust through
rustup only when Cargo is missing, builds the release, and installs the
application below `~/.local` by default.

```sh
git clone https://github.com/Yandall/yutu-mux.git
cd yutu-mux
./packaging/install.sh
```

The script prompts before using the system package manager and before
replacing existing application files. Use `--yes` for unattended installs.
No `curl | sh` application installer is offered: running from a checkout
keeps the source and build visible.

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
```

The desktop entry launches `yutu-mux` by name, so `~/.local/bin` must be on
PATH. Many desktop distributions configure this automatically after the
next login. There is currently no application icon asset, so the launcher
uses the desktop environment's generic icon.

## macOS

Dependencies come from Homebrew: `mpv`, `yt-dlp`, and `pkg-config`. The
script reuses `packaging/macos/build-app.sh` to build and ad-hoc sign the
existing native app bundle.

Installed files:

```text
~/.local/Applications/YutuMux.app
~/.local/bin/yutu-mux -> ~/.local/Applications/YutuMux.app/Contents/MacOS/YutuMux
```

Move `YutuMux.app` to `/Applications` manually if preferred. First launch
requires right-click, Open because the bundle is ad-hoc signed, not
notarized. Homebrew remains required for libmpv and yt-dlp; this is not the
self-contained distribution bundle. The macOS installer path is untested
because the maintainer has no Mac.

## Options and removal

```sh
./packaging/install.sh --dry-run       # print actions without changes
./packaging/install.sh --no-deps       # skip system package installation
./packaging/install.sh --deps-only     # install dependencies only
./packaging/install.sh --prefix /absolute/path --yes
./packaging/uninstall.sh               # remove app files, keep user data
```

`YUTU_INSTALL_PREFIX` provides another way to set the installation prefix.
The prefix must be absolute. Uninstall removes only the binary, Linux
launcher, and macOS bundle listed above. It never removes the library,
artwork/lyrics cache, logs, or sign-in session.

Windows is not supported by these scripts. Native Windows packaging is
planned separately; a Windows `.exe` cannot be produced by running these
Unix scripts.
