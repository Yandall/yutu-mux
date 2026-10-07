# yutu-mux

Native Rust YouTube Music desktop app. Iced UI. Linux + macOS. See `PLAN.md`.

## Status

Phase 0 scaffold. `MockBackend` only — no network, no audio yet.

## Run

```sh
cargo run
cargo test
```

Toolchain: `mise use -g rust@stable` (rust 1.99+).

## Layout

- `crates/yutu-core` — models, backend trait, fixtures
- `crates/yutu-player` — queue + audio backend trait
- `crates/yutu-ui` — Iced shell (views in Phase 3)
- `src/main.rs` — binary wiring
