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
