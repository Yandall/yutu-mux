# Changelog

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
