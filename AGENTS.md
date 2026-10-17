# AGENTS.md: yutu-mux contributor guide

> Native Rust YouTube Music desktop app. Iced UI (no webview/Tauri).
> Linux + macOS. Free-tier (no Premium assumptions). Open source (MIT).
> This file is the source of truth for future agents. Follow it.

## 1. What this is

- Workspace `yutu-mux`, binary `yutu-mux` (`src/main.rs`).
- Crates (`Cargo.toml` `[workspace] members`):
  - `crates/yutu-core`: models, `MusicBackend` trait, Innertube client,
    auth, SQLite cache/history/likes/playlists, yt-dlp resolver, lyrics,
    scrobble submit, discovery mixes.
  - `crates/yutu-player`: `Queue` state machine, `AudioBackend` trait,
    rodio backend, mpv backend (`mpv` feature), disk cache, EQ, controller.
  - `crates/yutu-ui`: Iced app shell. Thin views only; all logic lives in
    `yutu-core` / `yutu-player`. Iced 0.14 with `tokio`, `image`, `svg`
    features.
  - `crates/yutu-desktop`: MPRIS2 D-Bus service, tray model, macOS notes.
  - `src/main.rs`: wires MPRIS `serve` + `watch_and_signal` loop, then
    runs `yutu_ui::run`.
- Dependency rule (hard): `yutu-ui -> yutu-player -> yutu-core`.
  `yutu-desktop` depends only on `yutu-core`. No cycles.
  UI never touches HTTP or audio directly: it goes through traits/tasks.

## 2. Locked product decisions (do not relitigate)

- UI is **Iced**, pure Rust. Rejected: Tauri/webview, Electron wrappers
  (e.g. pear-desktop), fastframe (egui-only, Iced-incompatible).
- Playback: **mpv default** (progressive URL streaming, real seek/position,
  EQ via `af` chain), **rodio fallback** (full download path).
  `AudioOut` shields the UI from backend choice. EQ is honestly mpv-only;
  rodio must say so in the UI, never fake it.
- Resolver: **yt-dlp sidecar primary** (`--get-url --quiet --no-warnings`,
  AAC preferred). Raw rustypipe path removed.
- Lyrics: **LRCLIB** (free, no credentials). No Genius/Musixmatch for MVP.
- No mock data in the app. Real search, real click-to-play, real queue.
  `MockBackend` exists only for tests.
- Zero emoji in the app. **Lucide SVG icons only** (ISC, pinned 0.469.0,
  verbatim files in `crates/yutu-ui/assets/icons/`, see its README).
  Icon rule: hollow outline for **inactive** states, **filled** version
  for **active** states (e.g. heart vs heart-filled for liked); with no
  active/inactive distinction, prefer the filled version.
- Never write em dashes (neither in the UI nor in code comments/docs):
  use a hyphen, comma, colon, or the `·` separator instead.
- Search-only UX: no video-ID paste field.
- Theme: vivid red accent `SPOT_GREEN = #FF0033` (user override of the
  former soft `#E62117`; pure-red family allowed from Round 11),
  hover `#FF3366`. Dark Spotify-style surfaces,
  rounded inputs/surfaces. Glyphs on red are always white for contrast.
- Auth paths: Google OAuth **device flow requires a user-registered
  client** (unavoidable: ytmapi-rs embeds none); cookie-file import is
  the no-client login path. Never paste cookie contents; secrets only via
  env vars at test time.

## 3. Behavioral contracts (regressions live here)

- **Same-song no-op**: `PlayNow`/`QueueJump` targeting the live track does
  nothing: never restarts audio. Guard in the `Message` handler, not the
  backend. Covered by `replaying_the_live_song_is_a_noop`.
- **Click-to-play replaces the queue + autoplay tail**; **add-to-queue is
  append-only** and never hijacks playback; **new loads cut the old song
  instantly** (`audio.stop()` before resolving, for feedback).
- **Shuffle = physical reorder**: snapshot original order on enable,
  Fisher-Yates shuffle, restore snapshot on disable. Advance walks with no
  repeats to a real end, then autoplay. Repeat All/One loops the manual
  queue and starves autoplay by design.
- **Queue vs Autoplay split**: manual queue and radio/autoplay tail are
  separate lists. Clear sits with the manual list. Bulk queue adds absorb
  matching autoplay ids (dedup).
- **Seek anti-flicker**: 1.2s manual position clock after a seek
  (`seek_hold_until`); a seek that lands **while paused** freezes the
  manual clock until resume (`paused_seek` flag). New loads clear both.
- **Lyrics `line_at`** = index of last line with `start_ms <= position`
  (binary search). Empty-text stanza markers share the next line's
  timestamp and must never steal the highlight (test asserts this).
- **LRC blank lines survive parsing** as stanza separators (empty-text
  markers), rendered as vertical gaps.
- **Status text**: full last-action log in debug builds; **release shows
  failures only**. In-app error panel is debug-only UI (stderr always).
- **MPRIS**: `PlayerCommand` inbox drained on Tick (≤0.5s lag) +
  1Hz `PropertiesChanged` watch loop with `Rate`. Must support
  play/pause/next/prev/seek/volume/shuffle/loop and track the timeline.

## 4. Iced 0.14 notes (gaps we work around)

- Right-click: `mouse_area.on_right_press` + cursor tracking + frozen menu
  anchor; dismiss via scrim left-click or Esc; right-clicking another row
  moves the menu in one step (scrim sets no right-press handler).
- Context menu placement: cursor-anchored, clamped on screen, frozen on
  open.
- Drag-drop queue: grip press + hover target + release-anywhere; ghost row
  + live preview.
- Animations: `ease_toward` converging on a 33ms `Animate` subscription
  that only runs while `animating()` is true. Sidebars/drawer animate;
  keep steps small or motion feels clunky.
- Sliders: custom `themed_slider` style (red rail, white round handle).
  Apply `.style(themed_slider)` explicitly at every slider site.
- `text_input` padding: use the widget's own `.padding(10)` **at the end
  of the chain**. Never insert `.padding` mid-chain inside `row!`/`column!`
  macros: it breaks macro parsing ("no rules expected `.`"). Never let
  scripted edits eat/restore legitimate `,`-terminated chain lines.
- Art placeholders must keep **full fixed dimensions**
  (`Length::Fixed(size)`, centered) so rows don't jump when art lands.
- `scrollable.on_scroll(|v| ... v.absolute_offset().y ...)` is how sticky
  headers read scroll offset. `AbsoluteOffset` lives in
  `iced_core::widget::operation::scrollable`.
- Handle radius literal must be `8.0` (f32), not `8.0.into()` (clippy
  `useless_conversion` + `float_literal_f32_fallback` are deny-level).

## 5. Data model rules

- `Track.thumbnail_url` = small art; `thumbnail_large_url` = hero art
  (drawer). When adding a field to `Track`, update **every** constructor:
  `innertube.rs` (all song/video arms), `backend.rs`, `discovery.rs`,
  `library.rs`, plus UI test fixtures: or `E0063/E0560` will list them.
  `Album`/`Artist` do NOT share `Track` fields; don't blanket-apply.
- `thumbnail_large_url` needs `pick_large_thumb_url` (max width);
  `pick_thumb_url` stays min-width. Both return `None` on empty input.
- Lyrics cache keyed by track id; `lyrics_content(id)` helper is shared by
  the lyrics page and the Now Playing drawer: don't fork it.

## 6. Features & build matrix

- `yutu-ui` default = `["submit", "innertube", "mpv", "lyrics"]`
  (the real app).
- `--no-default-features` = offline/rodio-only compile check. Must stay
  green; gate Innertube-only UI (`cookie_section`, etc.) with
  `#[cfg(feature = "innertube")]`.
- `--all-features` must also stay green.
- `yutu-core` features: `innertube` (ytmapi-rs+tokio+reqwest), `lyrics`,
  `submit`. `yutu-player` feature: `mpv` (libmpv2).

## 7. Verification (run before every commit)

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo clippy -p yutu-ui --all-targets --no-default-features -- -D warnings
cargo test --workspace
cargo test -p yutu-ui --no-default-features
# live (needs network + yt-dlp): YUTU_YTDLP_BIN=/path/to/yt-dlp cargo test -p yutu-ui --lib -- --ignored
```

- Clippy gotchas seen in this repo: `doc_lazy_continuation` (keep doc
  list continuations indented, first line must not look like a lazy list
  item: merge short docs into the existing `/// Rules:` block style);
  `needless_return` (no `return` tail in `error_section`-style fns);
  unused `use super::*;` (keep it at the **top** of `mod tests`, before
  the first `#[test]`, or imports resolve wrong).
- Test env: `YUTU_TEST_VIDEO_ID` (default `dQw4w9WgXcQ`; player e2e
  defaults `JWW8KK8kd1I`; verified-good `F8qqWkmtCG0`, `JWW8KK8kd1I`),
  `YUTU_YTDLP_BIN` or PATH yt-dlp, `YUTU_OAUTH_CLIENT_ID` /
  `YUTU_OAUTH_CLIENT_SECRET`, `YUTU_TEST_OAUTH_FILE`,
  `YUTU_TEST_COOKIE_FILE`, `YUTU_TEST_LYRICS_VIDEO_ID`.
- System deps: ALSA, X11/Wayland, SQLite, OpenSSL + `libmpv`
  (`sudo pacman -S mpv` / `sudo apt install libmpv-dev` / `brew install mpv`).
- GUI/audio confirmation needs a display+audio machine; headless env
  verifies logic paths only. Never ask the user to paste secrets.

## 8. Workflow rules

- Toolchain via mise (rust 1.99+). Prefer dedicated file tools over
  shell for edits; use `shell` for cargo commands.
- **Parallelize with subagents**: fan out independent workstreams to
  background subagents (read-only specs or isolated tasks), integrate
  sequentially when they share files (especially the single-file UI).
  Then run adversarial reviewer agents over the final diff before
  pushing; fix every med+ finding, cheap lows too.
- Keep `PLAN.md` execution tracker and `CHANGELOG.md` (0.2.0 unreleased
  section) updated with each round.
- Only committer. New features go on a branch per feature
  (`feat/<name>`, `fix/<name>`), pushed for review as a PR; merge to
  `main` only once the feature is complete (3-config clippy + fmt +
  workspace tests green, adversarial review med+ findings fixed).
  NEVER merge a PR (via gh or otherwise) unless the user explicitly
  asked for that merge: the user tests every feature on their machine
  before it lands on `main`. Opening the PR is fine; merging is not.
  When working on a feature/fix, create a new branch and open a PR
  using the gh CLI (e.g. `gh pr create --base main --head <branch>
  --title ... --body ...`).
  Trivial direct-to-`main` pushes are for docs/changelog-only or
  seconds-old broken commits (`force-with-lease` only there).
- Run `cargo run` = real app. User verifies GUI/audio on their machine.
- Commit messages: `feat|fix|docs(scope): ...` with behavior + test
  summary. Verify 3-config clippy + fmt + workspace tests before push.

## 9. Key files

- `PLAN.md`: phased plan + execution tracker (append rounds, don't rewrite).
- `CHANGELOG.md`: release notes, 0.2.0 unreleased section on top.
- `crates/yutu-ui/src/lib.rs`: Iced app, playback flow, all views
  (large; grep for `fn drawer_view`, `fn lyrics_content`,
  `fn sidebar_target`, `themed_slider`, `status_text`).
- `crates/yutu-player/src/queue.rs`: queue state machine (`reorder`,
  `move_to`, `remove_id`, `insert_next`, `cycle_repeat`).
- `crates/yutu-player/src/mpv.rs`: mpv backend (stream URLs, seek, EQ).
- `crates/yutu-desktop/src/mpris.rs`: Player inbox, properties, signal
  watch loop.
- `crates/yutu-core/src/innertube.rs`: search, OAuth variant, library,
  suggestions, recommendations, thumb pickers.
- `crates/yutu-core/src/auth.rs`: OAuth device flow + `OAuthPersist` +
  cookie auth (redacted Debug).
- `crates/yutu-core/src/models.rs`: Track/Album/Artist.
- `crates/yutu-core/src/ytdlp.rs`: yt-dlp resolver flags.
- `crates/yutu-core/src/lyrics_providers.rs`: `parse_lrc`, `line_at`,
  LRCLIB fetch.
- `crates/yutu-ui/assets/icons/`: Lucide SVGs + attribution README.
- `src/main.rs`: MPRIS serve + watch loop wiring.

## 10. Known judgment calls / quirks (don't "fix" blindly)

- Unpinned sidebars shift layout instead of overlaying.
- Shuffle with current-last visibly does little (tail already ordered).
- Widget seek-while-paused applies silently until resume.
- Repeat All/One starves autoplay by design.
- `README.md` has a duplicated `## Run` section (historical).
