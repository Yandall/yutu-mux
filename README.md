# yutu-mux

Native Rust YouTube Music desktop app. Iced UI. Linux + macOS. See `PLAN.md`.

## Status

Phase 0 scaffold. `MockBackend` only — no network, no audio yet.

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
