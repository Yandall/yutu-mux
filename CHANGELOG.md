# Changelog

## Unreleased

### Added

- In-app updates: when a new version is out, a dialog offers Update and
  restart, Omit this version, or Update later. Downloads are checksum
  verified, and Settings shows the update button too (#97).
- Rename playlists from the right-click menu. The new name syncs to
  YouTube Music while signed in (#96).
- Windows media keys and the Windows media flyout now control playback
  and show the current song (#99).

### Changed

- The Now Playing toggle uses a microphone icon that fills in while the
  panel is open.
- Start mix now plays the same curated mix YouTube Music builds for a
  song (personalized when signed in) instead of a same-artist search.
  It fills the queue right away without leaving the current page, and
  the queue shows "Playing from: Mix of Song - Artist", which survives
  restarts. Starting a mix of the song already playing keeps it
  playing; a stopped one starts.
- Autoplay after clicking a song uses that same YouTube Music mix, so
  it keeps going for the whole mix instead of stopping after 10 songs.
- The Radio buttons are now Start mix, sized to match the Play and
  Shuffle buttons next to them.

### Fixed

- Hovering a song that appears in several lists at once highlights
  only the row under the cursor (#98).

## 0.1.0

- yutu-mux released for Windows, Linux, and macOS.
