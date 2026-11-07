# yutu-mux

Native Rust YouTube Music desktop app. Iced UI (no webview). Linux + macOS.

## Features

- Real search, artist/album/playlist pages, queue + autoplay tail,
  shuffle/repeat, lyrics (LRCLIB), user playlists, likes/history
- Streaming playback: mpv default (progressive URL streaming, seek,
  EQ), rodio fallback (memory-only fetch)
- 10-band graphic EQ (mpv-only, honest fallback message on rodio)
- Local file scan, scrobble (Last.fm + ListenBrainz), live MPRIS
- Cookie import sign-in with two-way library sync

Honest limits: free-tier bitrate ceiling (128-256kbps AAC/Opus is
YouTube's, not ours), no offline keep (streaming only), PO-token 403s
surface as errors, macOS packaging untested.

## Install

### One-command installer (Linux + macOS)

No checkout needed on Linux x86_64. This downloads the prebuilt release,
so Rust is never installed:

```sh
curl -fsSL https://raw.githubusercontent.com/Yandall/yutu-mux/main/packaging/install.sh | sh -s -- --yes
```

From a checkout, the same script lives at `packaging/install.sh`:

```sh
./packaging/install.sh
```

The script detects Linux `apt`, `dnf`, `pacman`, or `zypper`, or Homebrew
on macOS, installs the needed dependencies, and installs per-user below
`~/.local`. The script asks before using the system package manager and
before replacing an existing installation. Linux installs the `yutu-mux`
command and a desktop launcher. macOS builds and installs
`~/.local/Applications/YutuMux.app` plus a `yutu-mux` command in
`~/.local/bin` (add that directory to PATH if needed). The macOS app
bundle is ad-hoc signed: first launch is right-click, Open.

Useful variants:

```sh
./packaging/install.sh --dry-run       # show actions, change nothing
./packaging/install.sh --no-deps       # dependencies already installed
./packaging/install.sh --deps-only     # prepare a development machine
./packaging/install.sh --prefix /some/absolute/path --yes
./packaging/install.sh --build-from-source   # compile locally instead of the prebuilt release
```

Updating is the same command again (Linux x86_64 re-downloads the latest
release; source builds recompile from an updated checkout). Remove it
later with
`packaging/uninstall.sh` (user library, cache, and login data are kept).
See `docs/INSTALL.md` for supported package managers and install locations.
Windows packaging is planned but not supported yet.

### Manual development build

Rust 1.99+ via `mise use -g rust@stable`. System deps:

```sh
# Arch
sudo pacman -S mpv alsa-lib sqlite openssl yt-dlp
# Debian/Ubuntu
sudo apt install libasound2-dev pkg-config libx11-dev libwayland-dev \
  libxkbcommon-dev libsqlite3-dev libssl-dev libmpv-dev yt-dlp
# macOS
brew install mpv yt-dlp
```

Then:

```sh
cargo run            # native Iced GUI (needs display + audio)
cargo run --features debug-panel   # with the debug UI (bug reports)
cargo test           # headless-safe (audio/D-Bus/network tests skip)
```

`cargo run --no-default-features` builds the rodio-only check (no
Innertube, no mpv, no lyrics). See `docs/PACKAGING.md` for Flatpak and
the portable tarball, `docs/TROUBLESHOOTING.md` for common failures.

## Sign in

Settings imports a Netscape `cookies.txt` (private window, export via a
"Get cookies.txt" extension). The file picker reads it, the app never
shows its contents, the session is stored at 0600. Never paste cookie
contents anywhere. Details and ToS notes in `docs/AUTH.md`.

## Disclaimer

Unofficial, not affiliated with YouTube or Google. For personal and
educational use. YouTube Music has no official API; this client uses
reverse-engineered Innertube calls plus a `yt-dlp` sidecar for stream
resolution, which may violate YouTube's Terms of Service and risks
account action. Streaming only: the app keeps no offline copies.

## Layout

- `crates/yutu-core`: models, backend trait, Innertube client, SQLite
  cache/history/likes/playlists, yt-dlp resolver, lyrics, scrobble
- `crates/yutu-player`: queue state machine, audio backend trait
  (rodio + mpv), controller
- `crates/yutu-ui`: Iced app shell (thin views only)
- `crates/yutu-desktop`: MPRIS2 D-Bus service, tray model
- `src/main.rs`: binary wiring

## Contributing

See `CONTRIBUTING.md`. Branch per feature (`feat/<name>`), PR to
`main`. Run `cargo fmt`, 3-config clippy, and `cargo test --workspace`
before pushing.

## License

MIT, see `LICENSE`. Third-party notices in `NOTICE.md` (Inter OFL-1.1,
Lucide ISC, symphonia MPL-2.0, and others).

Note: the default build links the system `libmpv`
(GPL-2.0-or-later) at runtime, so distributed default binaries are
GPL-affected combined works. The `--no-default-features` build is
GPL-free.
