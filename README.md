# yutu-mux

Native Rust YouTube Music desktop app. Iced UI. Linux + macOS. See `PLAN.md`.

## Status

Working native app on real YouTube Music data (no mock fixtures in the UI):
Home/Search/Queue/Library/Settings + Artist/Album/Playlist/Lyrics/Radio
views, Up Next editor with click-to-jump + shuffle/repeat, user playlists
(persisted), real search + streaming playback (yt-dlp + Innertube, on by
default), EQ presets, disk-backed offline cache, local scan, scrobble
submission (Last.fm + ListenBrainz), live MPRIS. OAuth device-flow login,
library fetch, and LRCLIB lyrics live in `yutu-core` (UI wiring in progress).

## Run

```sh
cargo run            # native Iced GUI (needs display + audio)
cargo test           # headless-safe (audio/D-Bus/network tests skip)
cargo check -p yutu-core --features innertube  # live Innertube API
cargo run --features submit  # scrobble submission (same as default)
cargo check -p yutu-player --features mpv  # gapless/EQ backend (needs libmpv)
cargo clippy --workspace --all-targets -- -D warnings
```

## Run

```sh
cargo run            # native Iced GUI (needs display + audio)
cargo test           # headless-safe (audio tests skip without device)
cargo clippy --workspace --all-targets -- -D warnings
```

Toolchain: `mise use -g rust@stable` (rust 1.99+). System deps: ALSA,
X11/Wayland, SQLite, OpenSSL (present on most Linux desktops), plus
`libmpv` for the default playback backend (`sudo pacman -S mpv` /
`sudo apt install libmpv-dev` / `brew install mpv`).
`cargo run --no-default-features` builds the rodio-only offline check.

Library (likes/history) persists to `$XDG_DATA_HOME/yutu-mux/library.db`.

## Layout

- `crates/yutu-core` — models, backend trait, fixtures
- `crates/yutu-player` — queue + audio backend trait
- `crates/yutu-ui` — Iced shell (views in Phase 3)
- `src/main.rs` — binary wiring
