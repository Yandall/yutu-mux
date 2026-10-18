# Changelog

## 0.2.0 (unreleased)

- Memory diet: lists warm one art raster per track (not two),
  rows fetch a small 120px raster (not full-size thumbs),
  in-memory art capped at 128 handles with oldest-half eviction,
  disk-file handles instead of byte copies, shared HTTP clients,
  history/lyrics/URL maps capped, debug cache census in Settings
- macOS Now Playing + media keys (Control Center metadata, headset
  commands) via a souvlaki driver; Linux MPRIS untouched, no UI changes
- EQ curve persists: preset, gains, and bypass survive restarts and
  push silently to mpv on launch (garbage falls back to Flat)
- Seek-bar drags preview while held (pointer cursor, audio pauses) and
  commit on release; advances never fire mid-drag, shuffle starts off
- Gapless-feeling track ends: a backend gone silent after audible
  playback advances like the threshold path (frozen tails and short
  reads no longer wedge the queue on silence)
- Audible, customizable EQ: bands are now 1-octave-wide (the old 1 Hz
  width moved broadband level by 0.0 dB), with a hardware-style
  graphic-EQ panel in Settings (vertical drag sliders, smooth curve,
  bypass toggle, arrow-key nudges); presets are predefined curves for
  those sliders and edits relabel to Custom (or back to a match)
- Cookies-only sign-in: the Google sign-in section is gone and the
  signed-in cookie row no longer carries Import library (Sync now is
  the one refresh); saved OAuth sessions keep syncing untouched
- Song versions win in search: video rows duplicating a song row are
  hidden (streams and art resolve to the song), survivors keep server
  order; libraries, playlists, and radio play as listed
- Mute button beside every volume slider: silences output without
  touching the level (unmuting restores it exactly), survives restarts
- Reliable advances: failed stream fetches retry twice, then skip to
  the next song (or report honestly at the end of the queue); the next
  song starts prefetching 2s before the current one ends
- Lyrics follow: the Now Playing drawer auto-scrolls so the current
  verse stays centered; scrolling manually pauses it until you scroll
  back, seek, or change tracks
- Day-long image cache: covers older than 24 hours refetch on demand
  instead of pinning disk forever (expired files survive reads and are
  swept by the pruner, so offline viewing keeps its bytes)
- Bolder buttons: labels render one weight up (Medium) and icons draw
  a slightly heavier stroke, everywhere a button is (pills, rows,
  menus, tabs, seeks); artwork and decorations are untouched
- Animated loading states: the sidebar sync loader spins (one frame per
  tick), and busy buttons keep their label with the spinner beside it
  instead of collapsing to dots (same width idle and busy)
- Quiet background sync: the top progress banner appears for library
  imports only, while sync pulls surface through a spinning loader
  beside the sidebar title (same slot reserved when idle, so nothing
  shifts)
- Lazy image component: every artwork site renders through one
  cache-first view (memory, then disk, then placeholder), and fetches
  follow the viewport (scroll math per list plus overscan, throttled
  sweeps, on-open warming for popup/drawer/flyout). The disk cache
  holds every fetched cover (LRU-pruned to 150MB) instead of only
  downloads, so scrolled rows stop sticking on placeholders
- Search rows gain hover states and menus everywhere: the Songs filter
  matches the hover-reveal plus art play overlay, album/artist/playlist
  rows highlight on hover with a hover ellipsis and right-click menus
  (Open, Shuffle play, Start mix, Go to artist, Play)
- Search uses YouTube's server relevance order: one raw unfiltered
  search renders the web mixed order with the official top hit first
  (no more client text re-sort burying it), each filter tab runs its
  own raw filtered search with the web params (songs carry artists,
  albums and durations again), signed-in sessions get personalized
  ranking like the web client, and failed categories fall back instead
  of vanishing. The suggestion popup closes on pick and outside click
  (either button) and supports arrow-key navigation with Enter to play
- Silent test suite: the 440Hz rodio test tone is gone, control and
  clock tests emit no audio
- Playlist menu Shuffle play uses the fixed fresh-play path (same as
  the page pill); playlist Shuffle button matches the Play all size
  with the shuffle icon, and artist/album Shuffle pills use it too
- Shuffle play fetches the shuffled head: the source list shuffles
  before queueing (one fetch for the right track, no stale drop)
- Playlist click-to-play from the clicked song: the song plus the rest
  of the playlist from that point replace the queue; playlist pages
  gain a Shuffle pill; bulk queue ops resolve in memory and persist
  once in one SQLite transaction (no per-track open + fsync freezes);
  shuffle play shuffles fully with a random first track and always
  reshuffles, even when shuffle mode was already on
- Web-to-app sync: Settings "Sync now" plus auto-sync every 15 min while
  signed in (90 s grace after launch, toggle persisted, "Last synced"
  label). Pulls replay still-pending local pushes first, then reconcile
  server-wins: library rows carry their server like status so likes and
  dislikes replace the local sets, playlist tracks/titles refresh in
  place, server-deleted lists are pruned, and lists with unpushed edits
  are never fetched or replaced. Failed pages fail open (no ratings
  replace, no pruning), so a partial pull never wipes local state;
  pending pushes persist in SQLite and survive restarts
- Saved playback session: the queue, current song, position, shuffle,
  repeat and volume survive restarts (restored paused, press play to
  continue from the saved position; unknown ids drop honestly)
- Unified search page: no per-type placeholder sections (loading text
  with an animated ellipsis while results arrive, then a text-match
  relevance-ordered mixed list with the top hit in a hero card); every
  row names its type (Song, Album, Artist) and All/Songs/Albums/Artists
  chips filter the list in place
- Search hero actions: artist/album top results get clickable art and
  title plus Shuffle and Radio buttons over the known catalog tracks
  (hidden when none are known yet)
- Community playlists in search: live playlist search joins the unified
  list with a Playlists chip; heroes get Play + Shuffle and rows play
  on click (tracks fetch live first, verified anonymous: 95 tracks)
- Uniform media cards: album/artist cards share the home pick fixed size
  with single-line truncated labels, and card strips wrap onto the next
  line in narrow windows instead of squashing, so every card keeps one
  height and alignment
- Web-to-app sync: Settings "Sync now" plus auto-sync every 15 min while
  signed in (90 s grace after launch, toggle persisted, "Last synced"
  label). Pulls replay still-pending local pushes first, then reconcile
  server-wins: library rows carry their server like status so likes and
  dislikes replace the local sets, playlist tracks/titles refresh in
  place, server-deleted lists are pruned, and lists with unpushed edits
  are never fetched or replaced. Failed pages fail open (no ratings
  replace, no pruning), so a partial pull never wipes local state;
  pending pushes persist in SQLite and survive restarts
- Staged library import with live progress: songs merge first, then one
  task per playlist streams tracks in with per-list status, so big
  imports no longer freeze the window on a single silent future; all
  SQLite writes run in the background tasks, never on the GUI thread
  (the OS flagged the window frozen when the event loop stalled); a top
  banner pushes content down with a red progress bar, % and live counts
- Playlist/library import streams every continuation page (single-shot
  queries stopped at ~100 items) plus a tolerant raw-JSON fallback for
  row shapes the typed parser rejects, so 300-song playlists import
  whole instead of truncated or failed; per-page song ticks drive the
  banner bar song by song against the details total, even for a single
  playlist; pages report the moment they arrive (not buffered to the
  end) and the displayed count eases toward the truth each Tick so
  bursts sweep visibly instead of teleporting 0 to 100%
- Liked songs import for real: the browse endpoint wants the
  `VL`-prefixed `VLLM` id (bare `LM` 400s), so likes land in Favorites
  instead of silently failing
- Stay signed in with cookies: successful imports stash a 0600 session,  restarts show "Signed in with cookies" with one-click re-import plus
  sign-out; imports now pull songs + liked + every playlist with tracks,
  and likes/dislikes plus playlist creates/adds/removes/deletes mirror
  to YouTube Music in the background (removes resolve the per-membership
  `setVideoId` from raw browse JSON; like/dislike share one server
  status with local mutual exclusion)
- Cookie import accepts Netscape `cookies.txt` (the "Get cookies.txt"
  export shape): rows convert to a header before login, missing SAPISID
  fails fast; previously every Netscape file failed as bad/expired
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
  lyrics/like/don't-recommend/download/album/artist + add-to-playlist;
  offline download to disk cache; NowPlaying drawer via ▲; YTM-style
  slim sidebar
  (Home/Search/Library + filter + inline create + collapse rail)
- Write ops audited end to end: like (`like/like`), don't-recommend
  (`like/dislike`), clear (`like/removelike`), playlist create
  (`playlist/create`, private), add (`browse/edit_playlist`,
  `ACTION_ADD_VIDEO`), remove (`browse/edit_playlist`,
  `ACTION_REMOVE_VIDEO` via raw-JSON `setVideoId`), delete
  (`playlist/delete`), rename (`ACTION_SET_PLAYLIST_NAME`), artist
  subscribe (`subscription/subscribe` + `unsubscribe`); disliked ids
  persist locally and stay out of autoplay/radio/mixes
- Playlist id forms fixed per endpoint: `browse` reads send the
  `VL`-prefixed id, edits/deletes/playlist-likes send the raw id (a
  `VL...` id to `browse/edit_playlist` 400s `invalid argument`);
  imports store canonical ids with a one-time `VL...` migration
- Playlist SQLite writes run in one transaction each (plus a 5s busy
  timeout): removing a track from a large list no longer stalls the
  event thread with one fsync per row; renumber is a single UPDATE
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
- Repeat One replays only at track end (manual Next always advances,
  incl. MPRIS/controller); drag slots live only between songs with no
  origin hole; hairline queue dividers; empty rail reserves nothing
- Drawer sheet sits below the header with no bottom gap; drawer fetches
  lyrics on open; 544px hi-res hero art; filled hearts when liked;
  favorites labels; playlist side-flyout with covers; capped search
  bar; search query cache + 200ms debounce; stacked settings;
  animation speed in ms; full em-dash sweep
- Drag ghost stays in place until a slot arms; whole-row drag with
  reorder-click suppression; duration-based animation (1px floor,
  20-400ms); airy dividers; transparent inner row buttons
- Drawer replaces the center when open (nothing behind interacts);
  navigation closes it; search popup floats above with autodismiss;
  flyout aligns to its toggle row; playlist menus, play-all clears,
  queue-append action, sidebar hover play
- Startup catalog seeding plus art persistence migration; queue art
  merge; lyrics on every track start; exact-ms lyric seeks with
  marker-safe highlight; Favorites playlist leads everywhere
- 20-item hardening: autoplay absorb on queue ops; edge-slot DnD (middle
  of 3 moves), no grip icon, true-margin dividers, bar-only slot;
  "Loading lyrics"; drawer Fill + persistent header; Y-gated sidebar
  expand; aligned/shrinking search; modal hovers; add-order Favorites
  with liked_at; player-bar menu + artist link; MPRIS artUrl; #FF0033
  with white glyphs; catalog hero fallback; album ax- resolution;
  playlist shuffle play + trailing hover play
- Art policy: hi-res art everywhere except small song rows (64px
  threshold + focal heroes + album/artist hires + warmed keys); bolder
  icons (16px) + bold pill labels; wide center cap (1150 at 1600px);
  right-docked settings; responsive player (compact < 700, stacked
  minimal < 500, taller hover zone); mouse side-button back/forward
  with drag/menu/popup guards
- Drag from anywhere: global left-press arms via hovered row (inner
  buttons swallow area presses); origin keeps a full-size held chip so
  the song never disappears and siblings never shift
- Whole-row press: inner buttons removed (press reaches the row from
  text/art/gaps); snapshot click with slop/modal/mutation guards;
  remove-hover never arms; titles cap at 2 lines; queue art warms the
  head plus the ahead window on every track start
- Lazy artwork both-keys + chaining: the visible warmer now fetches raw and 544px hero keys (Home cards read hero, rows read raw) so covers load without playing first; each arrival chains the capped remainder; 120s cooldowns for failed art/empty probes stop retry loops; metadata probes run concurrently per id
- Lazy visible fetching: startup does zero network and zero SQLite writes (memory-only seed, instant window with cached titles plus disk covers); first Tick and every navigation fetch only the visible screenful (metadata capped at 8 with in-flight dedup, artwork disk-first small-thumb warm plus sidebar covers); hydrate probes keep exact-id matches only so small batches never explode into 100+ rows
- Library track cache + hydration: every seen Track is cached in SQLite so Favorites/history/covers render on launch with no search; bare liked/history ids from older installs hydrate in the background (watch playlist probes, capped) plus on-demand fetch whenever an unknown id is opened/played/queued; pre-art playlist rows heal their covers; unknown actions now say Fetching info instead of Track not found
- Downloaded default playlist: built-in list alongside Favorites for offline songs; completed downloads join in add order and persist across restarts (SQLite `downloads` plus track cache), startup prunes files that are gone, removing deletes the cached audio, adding starts a download, play/queue/shuffle work from the detail page
- Download 403 resilience: chunked fetches now send browser-like headers, a 403 re-resolves once for a fresh URL, then falls back to a direct sidecar download (`yt-dlp -o -`) before the raw probes
- Offline playback of downloads: play paths try the on-disk bytes first (either backend, no network), and downloads now store the full bundle (audio + lyrics + artwork on disk with metadata in SQLite); covers/lyrics render from disk, online plays heal missing sidecars for downloaded songs
- Offline artwork fix: saved covers now carry a sniffed image extension (the decoder keys off it, so extensionless files never rendered); early `.bin` files migrate on first lookup
- Download removal: the context menu toggles Download/Remove download per song, and removal deletes the whole bundle (audio + lyrics + covers) instead of just the audio file
- Search focus: view root is always a Stack, so popup layers appearing
  no longer rebuild widget state; typing keeps focus and cursor until
  the user clicks elsewhere (plus a stable search input id)
- Playback clock: wall-clock position on rodio, real `time-pos` on mpv
- Lyrics providers: LRCLIB (no login) + LRC parse/sync helpers
- Google login: OAuth device flow, library/liked/albums, YTM lyrics,
  watch-based recommendations
- Uniform cards and rows: Home quick picks are a fixed 64px with
  single-line truncated title/artist; search/library/radio/queue-page
  rows are a fixed 52px single-line; artists truncate everywhere so
  long names clip instead of stretching the list; queue sidebar rows
  are a fixed 58px with the title capped at 2 lines
- Compact search popup rows: 32px cover, no inline play/like/menu
  buttons (row click plays, right-click opens the menu), play count
  plus duration on the trailing edge; play counts come from the
  search mapping (`Track.plays`, memory-only, never persisted)
- Overlap-proof rows: every text column clips, so narrow windows cut
  text instead of painting over durations/controls; tighter char
  caps (popup 30/28, queue title 40 for a real 2-line cap with the
  artist always visible)
- Jump Back In is a horizontal pick strip: 140px cover on top with
  single-line title/artist below in fixed 156x200 cards (click plays,
  right-click opens the menu)
- Slim auto-hiding scrollbars everywhere (6px, transparent until
  hover/drag; floating overlay, so hiding takes no layout space):
  queue panel, center body, drawer lyrics, home strip (with 10px
  bottom breathing room), playlist flyout
- Queue rows center vertically in their fixed height; queue text is a
  fixed 2-line title box plus a fixed 1-line artist box with explicit
  line heights, so long names clip in-box and the artist always shows
- Scrollbar fades with a real transition: scroll motion lights the bar
  at full strength, the Animate ticker eases it out after 700ms idle
  (~8 frames); 4px slim everywhere; home strip reserves 14px below
  the cards so the bar clears them, plus 10px below the strip
- Queue text is a compact auto-height block (title up to 2 lines +
  artist line) centered in the fixed row, so short rows no longer hug
  the top and long names truncate instead of crowding the artist

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
