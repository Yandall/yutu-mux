# Changelog

## 0.2.0 (unreleased)

- Real audio resolution: yt-dlp sidecar first (AAC preferred), 1 MiB
  range-chunked downloads, permanent ignored e2e test
  (`resolve -> fetch -> decode` green in ~7s)
- Scrobble submission: Last.fm + ListenBrainz live POSTs (`submit`
  feature, default-on), Settings credential fields, status reporting
- MPRIS live: UI publishes track/status/volume to the shared handle on
  every state change (`playerctl` shows the real track now)
- `MpvBackend` (`mpv` feature, libmpv2): gapless flag, seek, volume,
  10-band EQ via `af` chain; rodio stays default
- Offline cache is disk-backed: store/read/evict-to-disk, LRU file
  deletion, self-heal on missing files
- Real Innertube search: `search_tracks/albums/artists` on
  `InnertubeBackend` (anonymous works) + redacted live fixture
- Release pipeline: fixed Flatpak manifest, `release.yml` tarball flow,
  `docs/PACKAGING.md`
- Spotify-style dark UI: sidebar / center / Now-playing panel / player bar
- No-mock UI: live search/radio by default, real-track catalog, Jump Back
  In home, queue click-to-jump + shuffle/repeat controls
- Real click-to-play: search result click resolves + streams real audio
  (mpv default with seek/position/clock, rodio fallback); video-ID paste
  UI removed; test tone retired
- Player bar: elapsed/total times, draggable progress, auto-advance at
  track end, artwork placeholder + like, live MPRIS position
- Instant playback: mpv streams URLs progressively (no full download),
  expire-aware URL cache (back/replay instant), next-song prefetch at 75%,
  1s-early auto-advance; rodio keeps the download path
- Synced lyrics view: LRCLIB fetch on open, current-line highlight
  following playback, plain/instrumental fallbacks
- Thumbnails in search rows, queue, right panel and player bar
  (60px YTM art, async cached); durations on rows
- Browser nav (back/forward + history), Home search bar with inline
  results, truly centered player bar, honest empty states
- As-you-type search (global header bar, ~450ms debounce, stale-safe),
  results stay on the current page; synced lyrics click-to-seek +
  auto-follow on track change
- Spotify-style refresh: 2-across quick picks with artwork, sidebar
  Playlists/Albums/Artists chips + recently played, Queue/Recent tabs
  with Playing-from, global search header
- Google sign-in (device flow, Settings): library + liked import into
  catalog, token at 0600, sign-out clears
- Click-to-play replaces the queue + same-artist autoplay tail; play-next
  insert; suggestion popup under the bar (Enter = full Search page)
- Context menu (… button, right-click N/A in Iced): play/next/queue/mix/
  lyrics/like/download/album/artist + add-to-playlist; offline download
  to disk cache; NowPlaying drawer via ▲; YTM-style slim sidebar
  (Home/Search/Library + filter + inline create + collapse rail)
- Honest EQ status (mpv-only filters; rodio reports it); bigger nav
  buttons; cookie-file import (normal browser login, no OAuth client)
- Real right-click context menu at the cursor (mouse_area +
  cursor/window tracking, clamped on screen); left-click/Esc
  dismisses, right-clicking another row moves it in one step
- Add-to-queue is append-only (never hijacks playback); new loads cut
  the old song instantly for immediate feedback
- Spotify queue split: manual Queue + radio Autoplay tail (shifts in at
  the end, survives clear); hover reveals row ✕, menu removes from
  either list; menu no longer dims the UI and lost its ✕
- Desktop controls work: MPRIS Play/Pause/Next/Prev/Seek/SetPosition/
  Volume via a command inbox drained on tick; lyric verse-click
  flicker fixed with a 1.2s manual position clock after seeks
- Playback failures log to stderr + Settings (Recent playback errors)
- Clear queue sits with the manual list (above Autoplay); context menu
  shows the album cover; bulk queues absorb matching autoplay ids
- Hover ✕ truly appears (layout-stable placeholder); autoplay rows
  right-click everywhere; right panel scrolls the full queue + tail
- Widget timeline fixed: PropertiesChanged signals on track/status/
  volume change + Rate property (1Hz watch loop next to the bus)
- Zero emoji: 33 Lucide SVGs (ISC, tinted) for every button/placeholder
- True shuffle: current-first Fisher-Yates order, walked once to a real
  end, then autoplay; repeat-one replays mid-queue; widget shuffle/loop
  buttons work end to end (LoopStatus/Shuffle properties + setters)
- Snappier search: 300/150ms debounce, instant catalog pre-results,
  live progress row; right-panel drag reorder via grip handles
- Spotify-style search/artist/album: top-result hero, numbered popular
  tracks, cover-art album cards, photo artist cards, PLAY/SHUFFLE
  hero pills, scrollbar clears the durations
- Visible shuffle (physical reorder + snapshot restore), filled transport
  icons, YouTube-red theme, rounded inputs/surfaces, hover feedback on
  every clickable, centered icon rows, drag ghost + live preview,
  tooltip volume, whole-row clicks, autoplay hover remove, bold lyrics
  with note marker, pin/unpin sidebars with edge-hover reveal
- Now Playing drawer: hi-res art, centered info, lyrics moved in with
  sticky compact header, bottom-sheet slide, stateful caret, lyrics
  strophe gaps, paused-seek flicker fix; same-song clicks are no-ops
- Menu rows fill the hitbox; padded inputs; softer red; narrow sidebar
  hover zones; transparent nav arrows; placeholder art keeps dimensions;
  red sliders; wider timeline; dev-only status text
- Shuffle-everything: current leads the shuffled order, exact restore
  on toggle-off (duplicate-safe); slot-model drag-drop with every
  between-position reachable, compact ghost chip, eased gap
- Center-column Now Playing drawer (sidebars/player stay visible),
  centered hi-res hero + narrow lyrics column, drawer-open art warm
- Two-line track rows everywhere + full-row hover; loaded art shares
  the placeholder card box; Play-next list-plus icon; playlist menu
  shows all lists with live Add/Remove + scroll; Lyrics/Radio leave
  the sidebar (menu + drawer own them); icon-inside search pill
- Sidebar hover hysteresis (panels linger while inside), 56px right
  thumbnail rail, smoother animation steps + Settings toggle/speed
- Playback clock: wall-clock position on rodio, real `time-pos` on mpv
- Lyrics providers: LRCLIB (no login) + LRC parse/sync helpers
- Google login: OAuth device flow, library/liked/albums, YTM lyrics,
  watch-based recommendations

## 0.1.0

First working native app (mock library + live Innertube core):

- Iced views: Home/Search/Queue/Library/Discovery/Settings +
  Artist/Album/Playlist/Lyrics/Radio; Up Next editor; shortcuts
- Persistent likes/history/user playlists (SQLite)
- Audible rodio pipeline; EQ presets, offline-cache tracker, local scan
- Scrobble rules + ListenBrainz payloads (submit stubbed); Weekly Mix /
  Forgotten Favorites
- MPRIS2 advertisement from the binary (track propagation follow-up)
- `innertube` feature: anonymous + cookie connect, suggestions, stream
  fallback, PO-token policy, breakage reports (live-verified)

Known gaps: real stream URLs in the player, EQ audible effect (needs mpv
backend), scrobble submission, Flatpak/dmg artifacts.

- Native Iced shell: Home/Search/Queue/Library/Settings + Artist/Album/
  Playlist/Lyrics/Radio/Discovery views, Up Next editor, keyboard shortcuts
- MockBackend library; persistent likes/history/user playlists (SQLite)
- Audible rodio pipeline (test tone; real streams need Innertube wiring)
- EQ presets, offline-cache tracker, local scan, Player controller
- Scrobble rules + ListenBrainz payloads (submit stubbed), Weekly Mix /
  Forgotten Favorites from local history
- MPRIS2 D-Bus service + tray model (Linux); macOS notes
- Live Innertube behind `innertube` feature (anonymous + cookie,
  suggestions, stream fallback, PO-token aware, breakage reports)

## 0.1.0 goals (remaining)

- Binary wiring for `innertube` feature + MPRIS live updates from UI
- Flatpak + macOS dmg artifacts, `cargo dist`, release tag
