# yutu-mux

Native Rust YouTube Music desktop app. Iced UI. Linux + macOS. See `PLAN.md`.

## Status

Working native app on `MockBackend`: Home/Search/Queue/Library/Settings +
Artist/Album/Playlist/Lyrics/Radio views, Up Next editor, user playlists
(persisted), audible test-tone playback, EQ presets, offline-cache tracker,
local scan. Live Innertube behind the `innertube` feature (anonymous +
cookie, suggestions + stream fallback, PO-token aware).

## Run

```sh
cargo run            # native Iced GUI (needs display + audio)
cargo test           # headless-safe (audio/D-Bus/network tests skip)
cargo check -p yutu-core --features innertube  # live Innertube API
cargo run --features yutu-ui/rp  # full resolution (decipher + PO tokens; GPL, see docs/INNERTUBE.md)
cargo clippy --workspace --all-targets -- -D warnings
```

## Run

```sh
cargo run            # native Iced GUI (needs display + audio)
cargo test           # headless-safe (audio tests skip without device)
cargo clippy --workspace --all-targets -- -D warnings
```

Toolchain: `mise use -g rust@stable` (rust 1.99+). System deps: ALSA,
X11/Wayland, SQLite, OpenSSL (present on most Linux desktops).

Library (likes/history) persists to `$XDG_DATA_HOME/yutu-mux/library.db`.

## Layout

- `crates/yutu-core` — models, backend trait, fixtures
- `crates/yutu-player` — queue + audio backend trait
- `crates/yutu-ui` — Iced shell (views in Phase 3)
- `src/main.rs` — binary wiring
