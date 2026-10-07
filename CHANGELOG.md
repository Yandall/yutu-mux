# Changelog

## Unreleased (toward 0.1.0)

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
