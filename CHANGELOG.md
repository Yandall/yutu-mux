# Changelog

## Unreleased

### Added

- In-app updates with Update and restart, Omit this version, or Update
  later (#97).
- Rename playlists from the right-click menu, synced while signed in (#96).
- Windows media keys and media flyout support (#99).
- The computer no longer goes to sleep while music is playing; the
  screen can still turn off (#104).
- Search inside a playlist; clicking a result still plays the whole
  playlist from that song on (#105).
- The top search bar and playlist search start compact and grow when
  you click into them (#105).
- Narrow windows (under 800px) open the library and queue as drawers
  from new header buttons; Settings moves into the library drawer (#107).

### Changed

- Now Playing toggle uses a microphone icon that fills while open.
- Start mix plays YouTube Music's own mix in the queue, labeled
  "Playing from" (#100).
- Autoplay uses the same YouTube Music mix and plays all of it (#100).
- Radio buttons are now Start mix, matching the Play/Shuffle size (#100).
- The Search tab and the / key focus the top search bar and select its
  text, so typing replaces the old query (#106).
- Medium windows (800 to 1100px) show the library and queue as slim
  rails that expand over the content on hover (#107).
- The back and forward arrows hide when the window is too narrow (#107).
- The window can no longer be shrunk below 360 x 480 (#107).
- On narrow windows, song titles end in "…", quick picks use one column,
  and album and artist headers stack the cover above the title (#108).
- Playlist, album and artist buttons wrap onto a second line instead of
  being cut off on narrow windows (#108).
- Song rows hide the heart button on narrow windows; Add to favorites
  stays in the right-click menu (#108).
- Very wide windows use more of the screen, and Now Playing shows the
  cover beside the lyrics (#108).
- Now Playing shows a soft blur of the cover's colors behind it (#108).
- Playlist pages show the playlist cover next to the title (#108).

### Fixed

- Hovering a song shown in several lists highlights only that row (#98).
- The library playlist list scrolls when it is longer than the window (#108).
- On narrow windows the volume slider lines up with the seek bar (#108).
- Typing in the search bar no longer freezes the app (#102).
- Search waits until you pause typing for 300ms (#102).
- Songs no longer show a 0:00 length in playlists, history, and search (#103).
- Videos without a known length learn it the first time they play (#103).

### Removed

- The Recent tab in the queue sidebar; use the History page instead.

## 0.1.0

- yutu-mux released for Windows, Linux, and macOS.
