# yutu-mux — Build Plan

> Native Rust YouTube Music desktop app. Iced UI. Linux + macOS. Free-tier. Open source.
> Audio: pure Rust first (`rodio` + `symphonia` + `cpal`), `libmpv` later for gapless/HLS.
> API: reverse-engineered Innertube via `ytmapi-rs` / `rustypipe` (no official API exists).

## 0. Decisions locked

- UI: **Iced** (pure Rust, wgpu, custom Spotify-like UI). No Tauri/webview.
- Audio v1: **rodio + symphonia**. v2: optional `libmpv2` backend behind a trait.
- Targets: **Linux-first, macOS-compatible**. No Premium assumptions (128–256kbps AAC/Opus ceiling is YouTube's, not ours).
- Auth: cookie import (`cookies.txt`) + unauthenticated search. OAuth TV-client later.
- License: TBD (recommend MIT OR Apache-2.0 to match Iced/rodio; GPL-3.0 only if Slint ever adopted).
- PO-token reality: `web_music` GVS streams need per-video PO tokens (BotGuard, post-Aug-2024). Strategy: try Android/web clients that don't need tokens, fall back to `rustypipe-botguard` sidecar pattern, document Premium exemption. Never promise account safety.

## 1. Architecture

Cargo workspace:

```
yutu-mux/
  Cargo.toml            # workspace
  PLAN.md               # this file
  crates/
    yutu-core/          # models, Innertube client trait, auth, SQLite cache, history
    yutu-player/        # queue state machine + audio backend trait (rodio v1, mpv v2)
    yutu-ui/            # Iced app (views, components) — thin, all logic in core/player
  src/main.rs           # binary wiring everything
```

Dependency rules: `yutu-ui -> yutu-player -> yutu-core`. No cycles. UI never touches HTTP/audio directly.

Key traits (stability anchors):

```rust
// yutu-core
trait MusicBackend {
  search(), suggestions(), home(), charts(), moods(),
  artist(), album(), playlist(), radio(), lyrics(), library()
}
// yutu-player
trait AudioBackend { load_url(), play(), pause(), seek(), volume() }
struct Queue { items, index, shuffle, repeat_mode }
```

Backends: `MockBackend` (deterministic fixtures, no network — UI always works) → `InnertubeBackend` (real).

## 2. Phase 0 — Foundation (DONE when `cargo check` + `cargo test` green)

- [x] Toolchain via mise (rust 1.99), system deps verified (alsa, x11, wayland, sqlite, openssl)
- [ ] Workspace scaffold + crate skeletons + binary
- [ ] `cargo fmt`, `clippy -D warnings` clean
- [ ] README (run instructions), LICENSE, .gitignore, CI (`cargo check/test/fmt/clippy`)
- [ ] Commit + push

## 3. Phase 1 — Core models + backend abstraction + cache

Goal: UI can render everything without network.

- Models: `Track, Album, Artist, Playlist, Lyrics, SearchResults, HomeFeed, QueueItem`
- `MockBackend` with 10-track fixture covering all views
- SQLite cache (`rusqlite` + `sqlx`? choose `rusqlite` for zero-async pain): search cache, play history, likes
- Auth store: cookie file load/save (never log), `AuthState::{Anonymous, Cookie}`
- Unit tests: model serde, queue transitions, cache round-trip
- Done: `cargo test -p yutu-core` green, no network in tests

## 4. Phase 2 — Playback pipeline (audible ASAP)

- `yutu-player`: `Queue` (next/prev/shuffle/repeat-one/all, Up Next) + `RodioBackend` (cpal stream, symphonia decode from HTTPS progressive URL)
- Stream resolution: `Innertube player` → pick Opus/AAC URL → hand to rodio. Document PO-token fallback chain:
  1. try `android_vr`/unauthenticated client without PO requirement
  2. try `web_music` + cached PO token
  3. surface clear error linking to `rustypipe-botguard` install
- Player UI bar: play/pause/next/prev/seek/volume/progress. No gapless yet (document as Phase 5).
- Integration test: mock URL (local wav) plays 1s without panic. Real-network test behind `#[ignore]`.
- Done: press play on mock track → audio out on Linux.

## 5. Phase 3 — Basic YouTube Music parity (match web client)

Each item = Iced view + core query + cache + loading/error states:

1. Search + suggestions (debounced, cancellable)
2. Home (new releases, quick picks, moods/genres, charts)
3. Artist page (top songs, albums/singles, similar, radio start)
4. Album page (tracklist, play-all, add-to-queue/playlist)
5. Playlist detail + user playlist CRUD (create/rename/add/remove/delete)
6. Library (songs/albums/artists/playlists/uploads/history)
7. Up Next / queue editor (reorder, remove, clear, shuffle)
8. Lyrics view (sync-less first, timed later)
9. Radio / autoplay (seed from track/artist, infinite continue)
10. History + Liked songs (persisted, survives restart)
11. Settings: auth (cookie import/clear), audio output picker, cache clear
12. Keyboard: space, ←/→ seek, j/k or arrows nav, `/` search, `q` queue

Done: anonymous user can search → open artist/album → play → queue → like → reopen app with history intact. All against `MockBackend`; `InnertubeBackend` wired behind feature flag.

## 6. Phase 4 — Real Innertube wiring (behind `innertube` feature)

- Vendor `ytmapi-rs` (preferred, async tokio, typed) with `rustypipe` as fallback reference for player decipher
- Implement `InnertubeBackend` for every Phase-3 query; snapshot JSON fixtures for tests (no live calls in CI)
- Breakage harness: `reports/` dir like rustypipe (save failing payload + request meta)
- Cookie auth: SAPISIDHASH signing, rotation warning, logout clears cache
- Done: `cargo run --features innertube` browses real YTM anonymously; logged-in via cookie file

## 7. Phase 5 — Spotify parity YTM lacks (the differentiators)

Ordered by value/effort:

1. Gapless + crossfade (needs mpv backend or custom symphonia double-buffer; rodio alone insufficient — implement `MpvBackend` behind feature)
2. 10-band EQ + loudness normalize + bitrate/codec badge
3. Offline cache: smart-download liked + queue, LRU cap, airplane toggle (streams only — no DRM circumvention claims)
4. Local files import (folder watch, sidecar playlist)
5. Discovery: Weekly Mix + Forgotten Favorites generated from local history (no server ML)
6. Scrobble: Last.fm + ListenBrainz
7. Desktop integration: MPRIS (Linux) + tray, macOS Now Playing, media keys, Discord RPC
8. Command palette + global shortcuts + vim-nav option
9. Smart filters (downloaded/liked/unplayed) + saved sorts
10. LAN handoff v1 (phone-as-remote via tiny HTTP API on loopback — simplified Connect)

Done: user can plausibly replace Spotify for daily listening.

## 8. Phase 6 — Packaging + release

- Linux: Flatpak manifest + AppImage + system tray icons; macOS: `.dmg` + notarize-ready bundle
- `cargo dist` or `tauri-bundler`-free `cargo-bundle` for artifacts
- Docs: `docs/AUTH.md` (cookie safety), `docs/PO_TOKEN.md`, `docs/TROUBLESHOOTING.md`
- 0.1.0 tag, GitHub release, changelog

## 9. Risks (do not hand-wave)

| Risk | Mitigation |
|---|---|
| Innertube JSON changes weekly | snapshot fixtures + reports dir + pin client version per release |
| PO-token 403s | multi-client fallback + sidecar instructions + honest error UI |
| Audio quality ceiling | display codec/bitrate, never claim 320kbps |
| ToS (no official API) | OSS/personal-use positioning, no ads-stripping marketing, account-safety warnings |
| Iced missing widgets (virtual lists) | build minimal virtualized list early; cap lists at 200 + pagination |
| No sudo on build hosts | static/user-space only; document system deps for Flatpak/brew |

## 10. Execution order (this session → onwards)

1. Phase 0 scaffold + green check (done, `2c2b3e0`)
2. Phase 1 models + mock + cache (done, `749b701`)
3. Phase 2 audible playback (done, `746443f`)
4. Phase 3 views: shell + queue editor (done, `99cbdaf` + `b04fa8e`); detail views/radio/lyrics/playlists/shortcuts (done, `87a75a0`); persistent playlists (done, `a68646e`); EQ/scrobble/discovery panels (in progress)
5. Phase 4 innertube: policy + reports (done, `843639b`); live `ytmapi-rs` wiring (done, `ae7418d`, live-verified)
6. Phase 5 audio adv: EQ presets + offline cache + local scan + controller (done, `b1951ed`); scrobble + discovery core (done, `692b51c`); MPRIS/tray (done, `e762ae9`)
7. Phase 6 packaging: flatpak draft + troubleshooting (done, `15ddf69`); artifacts + 0.1.0 tag (queued)
8. yt-dlp resolution path: sidecar resolve + range-chunked fetch + e2e proof (done, `b73d84b`); Spotify-style UI redress (done, `e940063`)
9. Phase 5/6 parallel push: scrobble submit + MPRIS live + mpv backend + disk cache + release pipeline (this session); Innertube real search backend (done, pending UI wiring)
10. MVP push: no-mock UI (live search/radio default, catalog, queue/player controls) + playback clock + LRCLIB lyrics + OAuth login/library/YTM lyrics (this session; UI wiring for lyrics/login next)
11. Real playback: click-to-play real audio (mpv default, rodio fallback), progress + draggable seek, auto-advance, video-ID paste removed, tone retired
12. Instant round: progressive streaming, URL cache + prefetch, synced-lyrics view, thumbnails everywhere, browser nav, Home search, centered bar (drag-drop queue deferred: no Iced primitive)
13. Round 2: as-you-type debounced search (stays on page, stale-safe), clickable + following lyrics, Spotify-style refresh (quick-pick grid, library chips, Queue/Recent tabs, global header), Google device-flow login + library/liked import
14. Round 3: suggestion popup + Enter-to-Search, play-now queue replace + autoplay tail, play-next insert, context menu (… button), offline download, NowPlaying drawer, YTM slim sidebar + collapse, honest EQ, cookie import (no OAuth client), bigger nav
15. Round 4: queue/autoplay split views, hover ✕ + menu remove, undimmed frozen menu, MPRIS command inbox (play/pause/next/prev/seek/volume), seek-hold anti-flicker, in-app error log
16. Round 5: Lucide icons (zero emoji), Fisher-Yates shuffle + repeat-one fix + widget shuffle/loop, snappier search (pre-results), panel drag reorder, scrollbar clearance, Spotify search/artist/album views
17. Round 6: physical shuffle w/ restore, filled icons, red theme, rounded UI, hover states, icon alignment, drag ghost/preview, volume tooltip, whole-row clicks, autoplay hover remove, lyric styling, pin/unpin sidebars
18. Round 7: drawer redesign (lyrics home, sticky header, slide), no-op replays, full-row menus, input padding, red tone, hover zones, nav arrows, stable placeholders, red sliders, wide timeline, dev status
19. Round 8: shuffle-everything + restore, slot DnD, center drawer, two-line rows, playlist Add/Remove, right rail, hysteresis, animation prefs, search pill (4 spec agents + 2 adversarial reviewers)
20. Round 9: repeat-one Next fix, between-only DnD, dividers, rail zone/empty-hide, drawer top-space + lyrics fetch + hires hero, filled hearts, playlist flyout, header/settings/search/anim upgrades, em-dash sweep (3 spec agents + 2 adversarial reviewers)
21. Round 10: drag ghost/row/anim/row-click pass, drawer modality + popup overlay + flyout align + playlist menus, startup/art/lyrics/lyric-click/Favorites data layer (3 spec agents + 1 adversarial reviewer)
22. Round 11: 20-item hardening (autoplay absorb, edge-slot DnD, grip removal, true-margin dividers, bar-only slot, lyrics text, drawer Fill + persistent header, Y-gated expand, aligned/shrinking search, modal hovers, add-order Favorites + liked_at, player-bar menu, MPRIS artUrl, #FF0033 + white glyphs, hero fallback, album ax- fix, shuffle play, trailing hover play) + review F fixes (3 spec agents + 1 adversarial reviewer)
23. Round 12: art resolution policy (hi-res everywhere but song rows), bolder icons/pills, wide center cap + right-docked settings, responsive player (wide/compact/minimal) + mouse back/forward, review G key-split fixes (3 spec agents + 1 adversarial reviewer)

Each commit: `cargo fmt && cargo clippy && cargo test` before push. Push to `main` until contributors arrive, then PRs.
