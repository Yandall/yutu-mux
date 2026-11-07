# Yutu Mux

> This project was 100% vibecoded. I made it because I wanted to have a
> better YouTube Music client that I could use in Linux and also macOS. I
> cannot guarantee the quality of the code but I'll try to fix any issue
> during my spare time, contributions are welcomed.

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
surface as errors.

## Install

### One-command installer (Linux + macOS)

```sh
curl -fsSL https://raw.githubusercontent.com/Yandall/yutu-mux/main/packaging/install.sh | sh -s -- --yes
```

This installs the app per-user below `~/.local`: the `yutu-mux` command
plus a desktop launcher on Linux, or `YutuMux.app` on macOS (first launch
is right-click, Open). Until the first release is published, pass
`--build-from-source` instead (needs Rust). Run
`packaging/uninstall.sh` to remove it later. Details, options, and manual
build steps: `docs/INSTALL.md`.

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

## Disclaimer of use

Unofficial, not affiliated with YouTube or Google. For personal and
educational use. YouTube Music has no official API; this client uses
reverse-engineered Innertube calls plus a `yt-dlp` sidecar for stream
resolution, which may violate YouTube's Terms of Service and risks
account action, including suspension or termination. Use at your own
risk: the software is provided as is, without warranty of any kind, and
the author accepts no liability for account actions, data loss, or any
other damages. Streaming only: the app keeps no offline copies. Cookie
sessions are stored locally at `0600`; never share your `cookies.txt`
or paste its contents anywhere. System package installation via the
installer script uses `sudo` on Linux and Homebrew on macOS; review
`packaging/install.sh` (or run with `--dry-run`) before piping it to a
shell.

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

MIT, see `LICENSE`. By using or contributing to this project you agree to
the MIT license terms. Third-party notices in `NOTICE.md` (Inter OFL-1.1,
Lucide ISC, symphonia MPL-2.0, and others).

Note: the default build links the system `libmpv`
(GPL-2.0-or-later) at runtime, so distributed default binaries are
GPL-affected combined works. The `--no-default-features` build is
GPL-free.
