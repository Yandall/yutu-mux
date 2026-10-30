# Contributing to yutu-mux

Thanks for stopping by. Small project, single maintainer, PRs welcome.

## Ground rules

- New features go on a branch (`feat/<name>`, `fix/<name>`) with a PR to
  `main`. The maintainer merges once the feature is verified on real
  hardware. Docs/changelog-only tweaks may land directly.
- Dependency rule: `yutu-ui -> yutu-player -> yutu-core`.
  `yutu-desktop` depends only on `yutu-core`. No cycles. UI never touches
  HTTP or audio directly.
- No mock data in the app. `MockBackend` is tests-only.
- UI: Iced only, Lucide SVG icons only (no emoji), row click rule
  (art + title play, extras in the right-click menu).

## Before you push

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo clippy -p yutu-ui --all-targets --no-default-features -- -D warnings
cargo test --workspace
cargo test -p yutu-ui --no-default-features
```

System deps: ALSA, X11/Wayland, SQLite, OpenSSL + `libmpv`
(`sudo pacman -S mpv` / `sudo apt install libmpv-dev` / `brew install mpv`),
plus a `yt-dlp` binary on PATH for stream resolution.

## Secrets

Never commit credentials. Cookies and OAuth files are gitignored
(`cookies.txt`, `*oauth*.json`, `.env`). Tests use env vars
(`YUTU_TEST_*`) and redacted fixtures. If you touch auth, keep Debug
output redacted.

## Bug reports

Run with the debug panel in any build:

```sh
cargo run --features debug-panel
```

Attach the relevant lines from `<data>/logs/` (7-day rotation). Redact
cookies and tokens before pasting.
