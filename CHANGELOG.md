# Changelog

## Unreleased

### Added

- Mini player: a small floating window with the current song, playback
  controls and a progress ring, tinted with the cover's colors.
- The mini player's lyrics button grows it into a five-line synced
  lyrics ticker; it remembers whether lyrics were open.
- The mini player reopens where you left it and its lyrics grow
  downward only (X11, macOS, Windows, Hyprland).
- The mini player always shows the current lyric line in full: long
  lines get smaller text and fewer surrounding lines instead of a cut.
- Unsynced lyrics in the mini player show in full, wrapped, and scroll.
- The mini player has its own window id (`yutu-mux-mini`); the README
  has a Hyprland rule that removes the frame around it.
- The player bar's mini player button also closes it when it is open.
- The mini player's Open button brings the main window forward on
  Hyprland too.
- The mini player's cover is sharp on high-resolution screens.
- macOS: opening or closing the mini player's lyrics no longer flickers
  with stretched content.
- `yutu-mux ctl mini` opens, closes and screenshots the mini player.
- Scriptable control: start with `--control` and drive the running app
  (play, search, navigate, screenshots) via `yutu-mux ctl`.
- `yutu-mux ctl quit` closes the app, and `ctl float` toggles tiling
  on Hyprland so `ctl resize` works there.
- Favorites has its own cover: a heart on a color gradient picked once
  for your library, with a matching glow on its page.
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
- New playlist dialog: set title, description and visibility (public,
  unlisted, private) when creating a playlist.
- Import a YouTube Music playlist from its link into a new playlist.
- Playlist imports use the song version of music videos when one exists.
- Import playlists from Spotify, Apple Music and Deezer links, or from a
  CSV export; songs not found on YouTube Music are listed at the end.
- Imports skip repeated songs and match titles regardless of accents.
- Right-click any text field to cut, copy, paste or select all.
- Export any playlist to a CSV file from its menu; the file imports back
  into Yutu Mux or other services.
- Sort Favorites by newest, oldest, title, artist or album, or drag
  songs into your own order (Manual).

### Changed

- On narrow windows the playlist menu button sits on its own line, so
  the playlist search keeps its full width.
- The library's New playlist button now sits at the top of the
  playlists; the playlist filter box was removed.

- Clicking a recommended album on Home opens its album page instead of
  playing it right away (#112).
- Clicking a playlist in search results opens a playlist page with its
  songs instead of playing it right away (#112).
- The collapsed library rail now shows the app logo at the top.
- The player on narrow windows is more compact: one-line title, and the
  volume sits next to the playback buttons (#109).
- The History search matches the playlist search: a compact pill that
  grows while you type (#109).
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
- Now Playing shows a soft blur of the cover's colors behind it, and
  playlist, album and artist pages glow with their cover at the top (#108).
- All text fields, search bars and pickers share one rounded style (#108).
- Play buttons are one round red button everywhere; Shuffle and Start
  mix are matching grey pills with icons, and all pills share one height (#108).
- Icon buttons get a round hover, and Settings uses switches instead of
  checkboxes (#108).
- Playlist pages get a new header: cover, bold title, song count and
  total length, a round play button, and Shuffle and search as pills;
  sorting moves above the song list (#108).

### Fixed

- The playlist menu button no longer wraps under the search box on
  medium-width windows.
- Clicking an empty spot inside a dialog no longer closes it.
- The volume slider keeps its normal size on wide windows instead of
  stretching across the player bar (#113).
- Lyrics auto-scroll keeps the highlighted verse centered in short or
  narrow windows, even when lines wrap (#114).
- Albums with the same name by different artists (like two "Greatest
  Hits") no longer open each other's page or mix their songs (#112).
- An album page now shows the same cover as its card, instead of a
  music video frame (#112).
- Icon buttons (repeat, mute, like, panel pins) hover with a round
  highlight like the other player buttons; the narrow-window menu
  and queue buttons now show a hover too.
- Narrow windows no longer dim and stop responding to clicks when no
  drawer is open (#109).
- The Now Playing button in the player no longer shrinks on tight
  windows; the volume slider gets shorter instead (#109).
- On very small windows the library drawer scrolls again, search top
  results stack, and song rows keep room for the title (#109).
- The dimmed area behind a narrow-window drawer has rounded corners like
  the panels around it (#109).
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
