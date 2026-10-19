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
24. Round 13: drag from anywhere (global press arming over inner buttons) + stable origin chip while held (no collapse, bar-only slot)
25. Round 14: whole-row press path (no inner buttons) + snapshot click + remove-hover guard + 2-line titles + queue art warming (head + ahead window) + review H fixes
26. Round 15: search focus kept while typing (view root always a Stack; layer toggles no longer rebuild widget state)
27. Round 16: library track cache + missing-metadata hydration (Favorites and covers survive restart with no search-first; bare liked/history ids hydrate in background via watch playlist, on-demand fetch on unknown ids, pre-art playlist rows healed)
28. Round 17: lazy visible-screen fetching (instant open with zero startup network/writes; first Tick plus every navigation hydrates only the visible screenful capped at 8 with in-flight dedup; artwork disk-first with small-thumb lazy warm plus sidebar covers; hydrate probes exact-id only)
29. Round 17 fix: Home covers read the 544px hero key so the lazy warmer now warms both URL keys (raw + hero) like every other path; ArtworkReady chains the capped remainder; 120s failure cooldowns for dead art/unresolvable ids stop refetch loops; hydrate probes run concurrently per id
28. Round 17: Downloaded default playlist (built-in alongside Favorites; completed offline downloads join in add order, persisted in SQLite `downloads`, startup prune of missing files, remove deletes the cached audio, add triggers a download, undeletable)
29. Round 18: offline playback (disk-first play on both backends; full download bundle of audio + lyrics + artwork + metadata; disk read-through for covers/lyrics; sidecar heal on online play; 403 retry + direct-download fallback)
30. Round 19: uniform cards/rows (fixed heights + clip + truncation: 64px home cards single-line, 52px wide rows single-line, 58px queue sidebar with 2-line titles, truncated artists everywhere) + compact search popup rows (32px cover, no inline buttons, row-click plays, right-click menu, `Track.plays` play counts from search mapping, memory-only)
31. Round 20: overlap-proof rows (clipped text columns + tighter caps: popup 30/28, queue title 40 so the artist line always survives) + Jump Back In horizontal pick strip (140px cover-over-text fixed cards, click plays, right-click menu)
32. Round 21: slim auto-hiding scrollbars (6px, hover/drag reveal, floating so no layout shift) on all six scrollables + 10px margin under the home strip + vertically centered row content + fixed 2-line title / 1-line artist boxes in the queue (artist can no longer be pushed out)
33. Round 22: real scrollbar fade transition (motion lights per-list opacity, ticker eases out after 700ms idle) + 4px bars app-wide + 14px strip reserve so the bar clears the cards + compact centered queue text block (short rows no longer hug the top)
34. Round 23: cookie import accepts Netscape `cookies.txt` (normalise to header before login, SAPISID fail-fast); stay signed in via a 0600 session file with one-click re-import plus sign-out; full account import (songs + liked + every playlist with tracks, fail-open per list, honest empty-account message); two-way sync for likes/playlist creates/adds/deletes (local-first, failures report, single-track removes local-only for lack of upstream setVideoId)
35. Round 24: import streams every continuation page (single-shot queries stopped at ~100 items) for playlist tracks/library songs/playlists/albums, plus a tolerant raw-JSON fallback for row shapes the typed parser rejects (flex-column durations, verified live: 248-track playlist whole, 100-track odd-shape playlist recovered); per-page song ticks against the details total move the banner bar even for a single playlist
36. Round 25: staged import with progress (base songs merge first, one task per playlist, per-list status) so big imports no longer freeze the window; all import SQLite writes moved into the background tasks, never the GUI thread; top in-flow banner with red bar, % and live counts; liked songs via `VLLM` (bare `LM` 400s, verified live)
41. Round 30: saved playback session (queue ids + index + position + shuffle/repeat/volume in two new SQLite tables, Tick saver for identity changes plus 30s position refresh, restore paused with unknown ids dropped, first play loads the saved track and seeks back, MPRIS Play covered)
42. Round 31: playlist SQLite writes in one transaction each + 5s busy timeout (fixes app freeze on removing a track from a large list: the old per-row renumber ran one fsync per track on the event thread); renumber is a single UPDATE
43. Round 32: web-to-app sync (fixes music.youtube.com edits never arriving): Settings "Sync now" + auto-sync every 15 min while signed in (90 s startup grace, persisted toggle, "Last synced" label); pending pushes persist in SQLite (`pending_ops`, latest intent wins, exact-op clear) and replay before each pull; server-wins reconcile for likes/dislikes (status-aware library read), playlist tracks/titles, and deletions (clean-headers-only prune); unpushed lists are never fetched or replaced; fail-open pages never wipe state
44. Round 33: playlist click-to-play from the clicked song on (song + rest replace the queue) plus a Shuffle pill on playlist pages; bulk queue ops resolve in memory and persist once in one transaction (82ms to 4ms for 250 tracks on tmpfs, HDD fsync storms gone); shuffle play shuffles fully with a random first track and always reshuffles via a fresh snapshot
45. Round 34: shuffle play fetches the shuffled head (permute the source list before queueing through one shared fresh-play path, so the single resolve targets the random first track instead of dropping stale; regression test pins loading id to current id; menu Shuffle play routed to the same path; Shuffle buttons sized like their Play siblings with the shuffle icon)
46. Round 35: silent test suite (removed the 440Hz rodio test tone; control ops test appends no source, clock test drives ClockState directly with no device)
47. Round 36: search collection rows (Songs filter hover-reveal + art overlay; album/artist/playlist hover highlight + ellipsis/right-click menus with Open/Shuffle/Radio/Go-to-artist/Play; stale menus never open and fresh results prune them; adversarial review med+ fixed)
48. Round 37: server-ordered search (raw unfiltered youtubei/v1/search + tolerant flat-layout parser with web mixed order and playable top card, replacing the client text re-sort; raw filtered searches per tab with the web params, shelf-aware subtitle parsing with durations; signed-in sessions personalize ranking via OAuth bearer or cookie SAPISIDHASH with anonymous 401/403 fallback; either half backstops the other; suggestion popup closes on pick/outside click with arrow-key navigation + Enter; adversarial review high+med fixed)
49. Round 38: lazy image component (one cache-first view for all art sites; visibility via scroll-offset math per list plus overscan with throttled sweeps and on-open warming; disk cache holds every cover with LRU prune to 150MB; adversarial review highs fixed)
50. Round 39: quiet background sync (top banner is imports-only; sync pulls surface through a spinning loader beside the sidebar title with a reserved slot so the pin button never hops; sync/error/logout paths all clear the paired flag; adversarial review lows fixed)
51. Round 40: animated loading states (sidebar spinner rotates one frame per Animate tick via runtime-transformed loader frames; busy buttons keep their label with the spinner beside it and identical idle geometry, replacing the dots; adversarial review lows fixed)
52. Round 41: bolder buttons (Medium labels via btn_text plus 2.5-stroke icons via btn_icon across all 138 button spans, including track rows, like states, and lyrics seeks; verbatim assets untouched; adversarial review meds fixed)
53. Round 42: day-long image cache (24h absolute TTL on memory and disk tiers; expired covers miss and refetch, sweeps delete; reads never destroy offline files; single-stat lookups; adversarial review highs fixed)
54. Round 43: lyrics follow (drawer eased glide centers the current verse, 25%/tick on the Animate ticker with exact-echo takeover detection; manual scroll disengages with proximity/seek/track-change resume; adversarial review highs fixed)
55. Round 44: advance retry (3 resolve attempts then skip-or-hold, stale arrivals never orphan a load, 2s prefetch window, 15s/300s resolve timeouts; adversarial review highs fixed)
56. Round 45: mute button (level-preserving toggle beside every volume slider, slider/MPRIS drags unmute, effective volume to MPRIS, flag persisted in the session; adversarial review meds fixed)
57. Round 46: song-version preference (search drops video rows duplicating a song row with duration guard, server order kept, top hit untouched; libraries/playlists/radio out of scope; adversarial review highs fixed)
58. Round 47: cookies-only auth UI (Google sign-in section removed, Import library deleted from the signed-in cookie row leaving Sync now as the one refresh; OAuth backend kept for legacy sessions)
59. Round 48: audible + custom EQ (1-octave bands fix the 1 Hz-width inaudibility, 10 per-band sliders with Custom/builtin relabel, 600 ms drag throttle with Tick trailing flush; adversarial review meds fixed)
60. Round 49: graphic-EQ panel (canvas widget: vertical drag sliders, smooth curve + wash, dB/freq labels, bypass toggle, arrow-key nudges, narrow-window scaling; adversarial review meds fixed)
61. Round 50: EOF-idle advance net (silent backend after heard playback advances like the threshold path; Stop/MPRIS-Stop disarm; adversarial review high fixed)
62. Round 51: seek-drag bar (Pointer canvas, preview-while-held with pause, commit on release, advance gates, orphan watchdog) + shuffle starts off at launch; adversarial review highs/meds fixed
63. Round 52: EQ curve persists (gains JSON + bypass prefs, tick saver, silent restore with mpv-only fallback; adversarial review highs fixed)
64. Round 53: macOS packaging script (release build + .app bundle, launcher PATH fix, opt-in self-contained libmpv/yt-dlp, ad-hoc sign, ditto zip)
65. Round 54: macOS Now Playing + media keys (souvlaki driver over shared state, zero UI changes, mac CI job; adversarial review high fixed)
66. Round 55: memory diet (single-raster art warming, 128-handle LRU, path handles, shared HTTP clients; review-fixed menu/album hero warming + hero cooldown)
67. Round 56: diet follow-up (w120 row rasters everywhere, row/card key split, history/lyrics/URL caps, debug cache census; 3 review rounds, all HIGH fixed)
68. Round 57: mimalloc global allocator (prompt page return for the macOS footprint number)
69. Round 58: debug tooling (debug-panel feature for the in-app debug UI; week-long file logging with panic hook; all prod prints routed through it)
70. Round 59: arrow cursor on content surfaces (RowButton widget: real button semantics, Idle cursor; controls keep the hand)
71. Round 60: empty-space hand root-caused (seek-bar canvas voted Pointer globally; now hover-gated) via cursor sampler + vote tracing; tracer removed
72. Round 61: Library nav button removed from the sidebar (route/page stays for fallbacks + tests)
73. Round 62: bundled Inter font + Appearance prefs (system family picker with async scan, one-click relaunch for the pending pick, instant 80-150% interface scale; both survive restarts)

Each commit: `cargo fmt && cargo clippy && cargo test` before push. Push to `main` until contributors arrive, then PRs.
