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
  - `packaging/install.sh`: Linux/macOS per-user installer; dependency
    setup plus prebuilt tarball/dmg download only, never compiles,
    desktop launcher or macOS app bundle.
  - `packaging/uninstall.sh`: removes installed app files, keeps user data.
  - `packaging/macos/build-app.sh`: macOS `.app` bundle builder used by
    the installer; optional self-contained distribution packaging.
  - `src/main.rs`: wires MPRIS `serve` + `watch_and_signal` loop, then
    runs `yutu_ui::run`.
- Dependency rule (hard): `yutu-ui -> yutu-player -> yutu-core`.
  `yutu-desktop` depends only on `yutu-core`. No cycles.
  UI never touches HTTP or audio directly: it goes through traits/tasks.

## 2. Locked product decisions (do not relitigate)

- UI is **Iced**, pure Rust. Rejected: Tauri/webview, Electron wrappers
  (e.g. pear-desktop), fastframe (egui-only, Iced-incompatible).
- Playback: **mpv default** (progressive URL streaming, real seek/position,
  EQ via `af` chain), **rodio fallback** (full fetch path, memory-only).
  `AudioOut` shields the UI from backend choice. EQ is honestly mpv-only;
  rodio must say so in the UI, never fake it. EQ bands are 1-octave-wide
  (`t=o:w=1`): Hertz widths are inaudible 1 Hz slivers on music.
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
- Row click rule (app-wide UI pattern, no exceptions for new lists):
  song rows play on **art click** (hover play overlay via
  `art_play_button`) **and title click** (`row_button` with
  `Message::PlayNow`), with **no separate play button** and no inline
  action buttons; extra actions live in the right-click context menu
  only. Cards behave the same (the whole card is the press target).
- Never write em dashes (neither in the UI nor in code comments/docs):
  use a hyphen, comma, colon, or the `·` separator instead.
- Search-only UX: no video-ID paste field. The one paste field is the
  New playlist dialog's playlist-link import (playlists only).
- Theme: vivid red accent `ACCENT = #FF0033` (user override of the
  former soft `#E62117`; pure-red family allowed from Round 11),
  hover `#FF3366`. Dark Spotify-style surfaces,
  rounded inputs/surfaces. Glyphs on red are always white for contrast.
- Auth: cookies-only UI (the Google sign-in section was removed; the
  OAuth device-flow backend stays for legacy saved sessions only).
  Cookie-file import is the login path. Never paste cookie contents;
  secrets only via env vars at test time. Signed-in cookie rows carry
  no import button: Sync now (Sync section) is the one refresh.

## 3. Behavioral contracts (regressions live here)

- **Same-song no-op**: `PlayNow`/`QueueJump` targeting the live track does
  nothing: never restarts audio. Guard in the `Message` handler, not the
  backend. Covered by `replaying_the_live_song_is_a_noop`.
- **Click-to-play replaces the queue + autoplay tail**; **add-to-queue is
  append-only** and never hijacks playback; **new loads cut the old song
  instantly** (`audio.stop()` before resolving, for feedback).
- **Shuffle = physical reorder**: snapshot original order on enable,
  Fisher-Yates shuffle, restore snapshot on disable. Shuffle always
  starts off at launch (saved order kept as-is). Advance walks with no
  repeats to a real end, then autoplay. Repeat All/One loops the manual
  queue and starves autoplay by design.
- **Queue vs Autoplay split**: manual queue and radio/autoplay tail are
  separate lists. Clear sits with the manual list. Bulk queue adds absorb
  matching autoplay ids (dedup).
- **Seek anti-flicker**: 1.2s manual position clock after a seek
  (`seek_hold_until`); a seek that lands **while paused** freezes the
  manual clock until resume (`paused_seek` flag). New loads clear both.
- **Prev restarts past 3s**: past `PREV_RESTART_SECS` the back button
  seeks to 00:00 (no reload); inside it the previous song plays. At
  the head with no previous song, restart in place instead of
  reloading. Bar, `p` key, and widget share `prev_task`.
- **Seek drag**: the bar previews while held (display only, Pointer
  cursor); backend seek + both advance paths wait for release. First
  preview pauses, release resumes only then. New loads, Stop, and a
  10s-motionless watchdog clear the drag; all position intents become
  preview while held.
- **EOF-idle advance**: at end-of-file the backend goes idle (raw 0/0)
  while metadata duration survives, so the threshold path can miss;
  a silent backend after heard playback (`heard_audio`, any nonzero
  raw position) advances like the threshold path. The `idle-active`
  property is the second witness (the duration property can outlive
  the file, so 0/0 alone can miss). New loads and both
  Stops clear `heard_audio`; paused/loading/never-heard never qualify.
- **Lyrics `line_at`** = index of last line with `start_ms <= position`
  (binary search). Empty-text stanza markers share the next line's
  timestamp and must never steal the highlight (test asserts this).
- **LRC blank lines survive parsing** as stanza separators (empty-text
  markers), rendered as vertical gaps.
- **Status text**: full last-action log only with the `debug-panel`
  feature (e.g. `cargo run --features debug-panel`); without the flag
  only failures surface (human labels, never provider internals), in
  dev builds just like release. In-app error panel, cache census, and
  FPS meter are flag-only UI too (stderr/log file always).
- **Layout modes** (`layout_mode`, window width): Narrow < 800 <
  Medium < 1100 <= Normal < 1600 <= Wide. Narrow: no rails, library
  and queue are drawers (header menu/queue buttons, scrim, Esc and
  navigation close them), Settings lives in the library drawer.
  Medium: both panels are rails that hover-expand **over** the
  content (pins ignored, restored when wide again). Normal/Wide push
  the content like before. All center geometry goes through
  `center_width()` / `content_width()`: never re-derive it from
  `sidebar_w` / `right_w`. Minimum window 360 x 480.
  Content follows `content_width()`: rows drop the heart < 480,
  headers stack < 560, quick picks single column < 600, Now Playing
  two-column >= 900 (hero fixed, lyrics scroll alone; follow math
  uses `LYRIC_STEPPER_H` there), Wide caps the body at 1400. Row
  titles ellipsize to a per-frame char budget (`row_title_chars`).
- **Link import prefers songs**: every music video row (OMV/UGC) of an
  imported playlist is swapped for its song version via one filtered
  song search (`song_match::best_song_match`, `SWAP_MIN_SCORE`); live,
  remix, cover etc. never replace a plain video, weak matches keep the
  video. The server create sends the resolved ids (batched), never a
  `sourcePlaylistId` copy (that would bring the videos back).
- **Foreign playlist imports** (`playlist_sources.rs`): one link field
  routes by host (`detect_source`). Deezer uses its public API; Spotify
  (embed page, max 100 rows) and Apple Music (page JSON) read only the
  anonymous public page on a user-pasted link: no login, no token
  extraction, no cookies, no background fetches. CSV/text exports are
  the documented fallback when a page format breaks. Rows match via
  `song_match::resolve_foreign` (songs first, video fallback); misses
  and capped reads show in the dialog summary (release status lines
  hide info).
- **Mini player** (`mini_player.rs`, design "Pill · Glow"): the app is
  an `iced::daemon`; the main window opens at boot and closing it
  quits (`Message::WindowClosed`). The mini player is a second
  frameless, transparent, always-on-top window and never minimizes
  or hides the main one (Wayland cannot restore it). Window events
  are filtered to `main_window`, so the mini window never moves the
  app's cursor, layout mode or drag arming. Views/titles/themes go
  through `view_for` / `title_for` / `theme_for`; `view()` stays the
  main window. Lyrics toggle resizes downward, kept in `mini_lyrics`.
  Own app id `yutu-mux-mini` (README has the Hyprland rule). Position
  saved in `mini_pos`: window `Moved` events (X11/macOS/Windows), or
  `hyprctl` under Hyprland (read on close, moved back on open, top
  edge re-pinned after a lyrics resize since Hyprland keeps centers).
  Hyprland dispatchers address windows by `address:`, never `pid:`
  (two windows). A drag ignores only the leave that arrives within
  `MINI_GRAB_LEAVE_MS` of it (the Wayland move grab's); macOS sends no
  such leave, so any later leave hides the chips. Ticker lines are fitted (`ticker_lines`):
  neighbors 1 line, current verse at most 2 at a smaller size.
- **Control socket** (`docs/CONTROL.md`): opt-in only (`--control` /
  `YUTU_CONTROL=1`), Unix socket `0600` in a `0700` dir, no TCP.
  Commands parse in `yutu_core::control` and execute in
  `yutu-ui/src/control.rs` strictly by dispatching existing `Message`s
  (no side doors around UI contracts). New user actions should get a
  `ctl` command too. Agents: drive the app with `yutu-mux ctl` (state,
  play, search, nav, screenshot) rather than synthetic input.
- **MPRIS**: `PlayerCommand` inbox drained on Tick (≤0.5s lag) +
  1Hz `PropertiesChanged` watch loop with `Rate`. Must support
  play/pause/next/prev/seek/volume/shuffle/loop and track the timeline.

## 4. Iced 0.14 notes (gaps we work around)

- Right-click: `mouse_area.on_right_press` + cursor tracking + frozen menu
  anchor; dismiss via scrim left-click or Esc; right-clicking another row
  moves the menu in one step (scrim sets no right-press handler).
- Context menu placement: cursor-anchored, clamped on screen, frozen on
  open.
- Playlist flyout: toggle hover opens it (no click), Tick grace closes
  it once both toggle and panel are cold (gap transit exits one layer
  first); top-aligned with the toggle row by shared `MENU_ROW_H`
  (every menu row is that fixed height, so the anchor cannot drift).
- Drag-drop queue: grip press + hover target + release-anywhere; ghost row
  + live preview.
- Animations: `ease_toward` converging on a 33ms `Animate` subscription
  that only runs while `animating()` is true. Sidebars/drawer animate;
  keep steps small or motion feels clunky.
- Sliders: custom `themed_slider` style (red rail, white round handle).
  Apply `.style(themed_slider)` explicitly at every slider site.
  Stock sliders hardcode Grab/Grabbing cursors (no style knob), so all
  slider sites use `pointing_slider` (maps grab votes to Pointer).
- Icon-only buttons use `icon_hover` (round hover, `ctrl_box` keeps
  it a circle) or `accent_button` when active; never `hover_row`,
  which is the rounded-square hover for rows and menu items.
- Cursor: every clickable component shows the pointing hand
  (`RowButton` votes Pointer when pressable, plain buttons/toggles/
  checkboxes/pick lists already do, `PointingSlider` maps the stock
  grab votes, seek bar and EQ graph vote Pointer, queue sidebar rows
  vote Pointer via their whole-row `mouse_area` interaction). Text fields keep
  the I-beam, disabled rows keep the arrow, invisible dismiss scrims
  keep the arrow.
- `text_input` padding: use the widget's own `.padding(10)` **at the end
  of the chain**. Never insert `.padding` mid-chain inside `row!`/`column!`
  macros: it breaks macro parsing ("no rules expected `.`"). Never let
  scripted edits eat/restore legitimate `,`-terminated chain lines.
- Art placeholders must keep **full fixed dimensions**
  (`Length::Fixed(size)`, centered) so rows don't jump when art lands.
- `scrollable.on_scroll(|v| ... v.absolute_offset().y ...)` is how sticky
  headers read scroll offset. `AbsoluteOffset` lives in
  `iced_core::widget::operation::scrollable`.
- Text fields have no native right-click menu: wrap every input with
  `edit_menu(input, value, Some(on_input), secure)` (`input_menu.rs`:
  Cut / Copy / Paste / Select all on the input's own state and the
  system clipboard; secure fields never copy). New inputs must use it.
- Row budgets that sit beside an `icon_pill_button` reserve
  `icon_pill_width(label)` (13px label + icon + padding), never a
  `text_line_width + N` guess (that undercount wrapped the playlist
  menu button on Medium windows). The playlist action row never wraps
  from Medium up; Narrow gives the "…" menu its own line on purpose
  (`pl_menu_own_line`), so the search keeps its width.
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
  (the real app). Opt-in `debug-panel` shows the debug UI (error panel,
  cache census, FPS meter, full status text with failure detail) in any
  build, including dev runs: `cargo run` without it behaves like
  release. Pass the flag for user bug reports.
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
# headless layout screenshots at every width (no display needed),
# written to target/layout-snapshots (YUTU_SNAPSHOT_ONLY=home,queue):
cargo test -p yutu-ui --lib layout_snapshots -- --ignored
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
- **Changelog on every change (hard rule)**: every user-visible change
  (feature, fix, behavior change, removal) adds a line to `CHANGELOG.md`
  under the `## Unreleased` section at the top, in the **same commit/PR**
  as the change itself. Group lines under `### Added`, `### Changed`,
  `### Fixed`, `### Removed`; one plain-language line per change, written
  for users (what changed for them, not how), with the PR number when
  known. Keep each entry short: **1 or 2 lines maximum** (wrapped at
  ~72 cols), no detail on edge cases or internals; split a change in
  two entries rather than writing a paragraph. Create the `## Unreleased` section if it is missing. Pure
  refactors, tests, and CI-only changes may skip it. At release, rename
  `## Unreleased` to the version heading so the release notes are ready.
- Only committer. New features go on a branch per feature
  (`feat/<name>`, `fix/<name>`), pushed for review as a PR; merge to
  `main` only once the feature is complete (3-config clippy + fmt +
  workspace tests green).
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
  summary. Verify 3-config clippy + fmt + workspace tests and the
  `CHANGELOG.md` entry before push.

## 9. Key files

- `CHANGELOG.md`: release notes, `## Unreleased` section on top (see
  the changelog rule in section 8).
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
- `crates/yutu-core/src/app_log.rs`: file logging (`<data>/logs/`,
  7-day retention) + stderr mirror; every prod `eprintln!` routes here.
- `crates/yutu-core/src/models.rs`: Track/Album/Artist.
- `crates/yutu-core/src/ytdlp.rs`: yt-dlp resolver flags.
- `crates/yutu-core/src/lyrics_providers.rs`: `parse_lrc`, `line_at`,
  LRCLIB fetch.
- `crates/yutu-ui/src/mini_player.rs`: mini player window (settings,
  pill view, progress ring, lyrics ticker, per-window dispatch).
- `crates/yutu-ui/src/i18n.rs`: app `Language`, Fluent lookup with
  English fallback, per-language failure markers.
- `crates/yutu-ui/assets/locales/<lang>/app.ftl`: UI strings per
  language (translators edit these, never Rust; tests pin parity).
- `crates/yutu-ui/assets/icons/`: Lucide SVGs + attribution README.
- `src/main.rs`: MPRIS serve + watch loop wiring, log init, panic hook,
  `ctl` subcommand dispatch (`src/ctl.rs` client).
- `crates/yutu-core/src/control.rs`: control protocol (command parser,
  socket path, wire format); `crates/yutu-ui/src/control.rs` executes.

## 10. Known judgment calls / quirks (don't "fix" blindly)

- Unpinned sidebars shift layout instead of overlaying in Normal and
  Wide; Medium and Narrow overlay on purpose (no room to shift).
- Shuffle with current-last visibly does little (tail already ordered).
- Widget seek-while-paused applies silently until resume.
- Repeat All/One starves autoplay by design.
- Favorites' Manual order is local only (`favorites_manual_order` pref):
  YouTube Music cannot reorder liked songs. New likes show after it.
- Favorites' cover is generated (`favorites_cover.rs`): a random gradient
  saved once in the `favorites_cover` pref, rendered lazily on first
  display; its glow is the same 8x8 Triangle downscale as image covers.
- `README.md` has a duplicated `## Run` section (historical).
