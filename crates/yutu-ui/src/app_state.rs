//! yutu-ui app_state: split from lib.rs (no behavior change).

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use pointing_slider::pointing_slider;
#[allow(unused_imports)]
use row_button::row_button;
#[allow(unused_imports)]
use std::collections::{HashMap, HashSet};
#[allow(unused_imports)]
use yutu_core::{
    forgotten_favorites, history_section, should_scrobble, weekly_mix, Album, Artist, HistoryEntry,
    HistorySection, LibraryStore, LyricsProvider, LyricsResult, Playlist, PlaylistHit,
    QueueSnapshot, SearchAll, SearchResults, TopKind, Track,
};
#[cfg(feature = "mpv")]
#[allow(unused_imports)]
use yutu_player::MpvBackend;
#[allow(unused_imports)]
use yutu_player::{EqPreset, Queue, RodioBackend, BAND_COUNT, MAX_GAIN_DB, MIN_GAIN_DB};

pub struct AppState {
    pub route: Route,
    pub search_query: String,
    pub results: SearchResults,
    /// Server-ordered All response for the current query (keystroke
    /// narrowed like `results`; the All tab renders it as-is).
    pub all_results: SearchAll,
    pub queue: Queue,
    pub liked: HashSet<String>,
    /// Liked ids in add order (membership stays in `liked`).
    pub liked_order: Vec<String>,
    /// Disliked ("don't recommend") ids: membership plus add order below.
    pub disliked: HashSet<String>,
    /// Disliked ids in add order (membership stays in `disliked`).
    pub disliked_order: Vec<String>,
    pub history: Vec<String>,
    /// Timestamped play log, newest first (drives the History page).
    /// Capped in memory; the database keeps every play ever.
    pub history_full: Vec<HistoryEntry>,
    /// History page search text (in-memory only, never persisted).
    pub history_filter: String,
    /// History page render window (newest first; "Show more" pages in
    /// older plays). Full rows are built on every view, so an uncapped
    /// multi-thousand log stalls frames.
    pub history_limit: usize,
    pub volume: f32,
    /// Muted: the backend plays silence while `volume` (slider, session)
    /// stays untouched, so unmuting restores the exact level.
    pub muted: bool,
    pub status: String,
    pub user_playlists: Vec<Playlist>,
    pub new_playlist_name: String,
    pub playlist_seq: u64,
    /// Selected EQ preset name (persisted pref; restored at startup).
    /// "Custom" when the band gains match no built-in curve.
    pub eq_preset: String,
    /// Live EQ band gains in dB, band order matches `EQ_FREQUENCIES`
    /// (persisted pref as JSON; restored at startup). Preset buttons
    /// overwrite all ten; band sliders edit one and relabel to the
    /// matching preset (or "Custom").
    pub eq_gains: [f32; BAND_COUNT],
    /// EQ apply throttle: slider drags push at most one `af` re-init per
    /// 600 ms (each push rebuilds mpv's filter graph: a per-tick storm
    /// would stutter). `eq_cooldown_until` gates immediate pushes;
    /// `eq_dirty` defers the trailing edge to the Tick flush.
    pub eq_cooldown_until: Option<std::time::Instant>,
    pub eq_dirty: bool,
    /// EQ bypass toggle (persisted pref, default on). Off applies a
    /// flat chain but keeps `eq_gains` + labels, so the curve survives.
    pub eq_enabled: bool,
    /// Last persisted EQ curve (preset name, gains, bypass): the Tick
    /// saver writes prefs on change, so drags settle without storms.
    pub eq_saved: Option<(String, [f32; BAND_COUNT], bool)>,
    /// Seek-bar drag preview in seconds (Some while held): display only
    /// (time label, lyrics, MPRIS follow it). The backend seek and the
    /// auto-advance wait for release; in-memory only, never persisted.
    pub seek_drag: Option<f32>,
    /// Last preview motion (orphaned drags, e.g. a layout switch that
    /// dropped the canvas flag, expire from Tick instead of gating the
    /// advance forever).
    pub seek_drag_at: Option<std::time::Instant>,
    /// The drag paused playback (resume on release only in this case).
    pub seek_paused: bool,
    /// Scrobble toggle (in-memory only; submission needs credentials below).
    pub scrobble_enabled: bool,
    /// Scrobble username (in-memory only; never persisted).
    pub scrobble_user: String,
    /// ListenBrainz user token (in-memory only; never persisted or logged).
    pub scrobble_lb_token: String,
    /// Last.fm API key / shared secret / session key (in-memory only).
    pub scrobble_fm_key: String,
    pub scrobble_fm_secret: String,
    pub scrobble_fm_sk: String,
    pub(crate) paused: bool,
    pub(crate) audio: Option<AudioOut>,
    /// Last Tick-observed playback position (drives the progress bar).
    pub(crate) position_secs: f64,
    /// Track id currently being fetched (stale arrivals are dropped).
    pub(crate) loading_track: Option<String>,
    /// Resolved stream URLs by track id: (url, expire_unix). Fresh hits
    /// play instantly: back/prev/replay never re-resolve.
    pub(crate) url_cache: HashMap<String, (String, u64)>,
    /// Next song's stream, resolved early (played instantly on advance).
    pub(crate) prefetched: Option<(String, String)>,
    /// Lyrics by (track id, provider) + in-flight fetches. Both providers
    /// are fetched up front in parallel when a song starts, so the drawer
    /// `< >` stepper switches instantly with no reload flash; a provider
    /// that arrives first displays while the other is still in flight.
    pub(crate) lyrics_cache: HashMap<(String, LyricsProvider), LyricsResult>,
    pub(crate) lyrics_loading: HashSet<(String, LyricsProvider)>,
    /// Default lyrics provider (Settings, persisted `lyrics_provider`
    /// pref): displayed when its result is cached, with the other
    /// provider as automatic fallback on first load.
    pub(crate) lyrics_provider: LyricsProvider,
    /// Tracks pinned to strict provider display by the drawer/page
    /// stepper: a pinned track shows only the default provider's
    /// result (loading, or the no-lyrics state when it has none) and
    /// never falls back. Fresh song loads are automatic until the user
    /// pins them (capped; a dropped pin falls back to automatic).
    pub(crate) lyrics_manual: HashSet<String>,
    /// Lyrics follow: auto-scroll the drawer so the current verse stays
    /// centered. Manual scrolling disengages (proximity, seek, or track
    /// change re-engages); never persisted.
    pub(crate) lyrics_follow: bool,
    /// Last followed (track id, verse index): detects track/verse
    /// changes so snaps fire once per change, never per tick.
    pub(crate) lyrics_follow_at: Option<(String, usize)>,
    /// Last issued follow target (drawer y): tells manual scrolls apart
    /// from our own snaps (snaps land exactly, so they never trip it).
    pub(crate) lyrics_target: f32,
    /// Last commanded scroll offsets, newest first (NaN = none yet):
    /// glide echoes match one of these exactly, so only real user
    /// scrolls disengage follow. A ring (not just the latest) because
    /// echoes can lag commands under load; a user scroll bit-exactly
    /// equal to a recent command is negligible.
    pub(crate) lyrics_cmds: [f32; 3],
    /// Browser-style back/forward stacks (Route history, capped).
    pub(crate) nav_back: Vec<Route>,
    pub(crate) nav_forward: Vec<Route>,
    /// Right-panel tab (view-only state).
    pub(crate) right_tab: RightTab,
    /// Memoized Home/Discovery shelves (quick picks, albums, more-from,
    /// weekly mix, forgotten): ranking scans the whole catalog plus a
    /// history query, so views read this instead of recomputing per
    /// frame. Refreshed on navigation and on Tick while visible.
    pub(crate) home_shelves: HomeShelves,
    /// Queue id order before shuffle was turned on. Toggling off restores
    /// it (ids added while shuffled append at the end); the playing song
    /// never skips: [`Queue::reorder`] retargets its index by identity.
    pub(crate) pre_shuffle: Option<Vec<String>>,
    /// Current track when shuffle turned on (id + occurrence rank): if it
    /// is still current at restore time, this exact occurrence is
    /// re-seated (`reorder` alone retargets duplicates by first match).
    pub(crate) pre_shuffle_cur: Option<(String, usize)>,
    /// Suggestion popup: texts + generation + visibility.
    pub(crate) suggest: Vec<String>,
    pub(crate) suggest_gen: u64,
    pub(crate) show_suggest: bool,
    /// Keyboard highlight into the open suggest popup (`None` = the
    /// query itself submits on Enter). Indexes text suggestions first,
    /// then the top-track rows; reset on every query change.
    pub(crate) suggest_idx: Option<usize>,
    /// Last persisted queue snapshot (changed-identity check for the Tick
    /// saver: queue writes land within one tick, positions within 30s).
    pub(crate) queue_saved: Option<QueueSnapshot>,
    /// Last queue save time (positions refresh at most every 30s).
    pub(crate) queue_saved_at: Option<std::time::Instant>,
    /// Saved position to seek after a restored queue starts playing
    /// (dropped on any other load or failure).
    pub(crate) pending_resume: Option<f64>,
    /// User seek requested while the track was still resolving
    /// (mpv has no file yet, so an immediate seek fails). Remembered
    /// and applied once the load lands, instead of erroring.
    pub(crate) pending_seek: Option<f64>,
    /// Startup autoload in flight: the restored track buffers paused
    /// at the saved position, so the first play/seek is instant.
    /// Successful landings pause the backend instead of playing;
    /// any explicit user transport clears it (the user took over).
    pub(crate) autoload: bool,
    /// A session restore is waiting for its first play (the play button
    /// loads the saved track instead of resuming an empty backend).
    pub(crate) restored_pending: bool,
    /// Context menu open for this track id (None = closed).
    pub(crate) context_menu: Option<String>,
    /// Playlist menu open for this playlist id (None = closed).
    pub(crate) pl_menu: Option<String>,
    /// Selector menu open (None = closed): sort picks and settings picks
    /// share one dropdown idiom.
    pub(crate) sort_menu: Option<SelectMenu>,
    /// Search collection menu open (None = closed).
    pub(crate) coll_menu: Option<CollectionMenu>,
    /// Playlist flyout open for this track id (side panel, menu stays).
    pub(crate) submenu_playlists_for: Option<String>,
    /// Cursor on the Playlists toggle row (hover-opens the flyout).
    pub(crate) submenu_trigger_hot: bool,
    /// Cursor on the flyout panel itself (hover keeps it open).
    pub(crate) submenu_fly_hot: bool,
    /// Last instant the cursor was seen on the toggle or the panel:
    /// crossing the gap between the two disjoint layers exits one
    /// before entering the other, so the Tick only closes past this
    /// grace instead of flickering mid-transit.
    pub(crate) submenu_hot_at: Option<std::time::Instant>,
    /// Cursor position frozen when the menu opened (the menu must not
    /// chase the mouse while the user aims at it).
    pub(crate) menu_at: Option<iced::Point>,
    /// Last cursor position (window coords: anchors the context menu).
    pub(crate) cursor: Option<iced::Point>,
    /// Last window size (clamps the context menu on screen).
    pub(crate) window: Option<iced::Size>,
    /// Pinned sidebars stay open; unpinned ones reveal on edge hover.
    pub(crate) sidebar_pinned: bool,
    pub(crate) right_pinned: bool,
    /// Latched hover states (updated in `CursorMoved`; see `sidebar_open`).
    pub(crate) sidebar_latched: bool,
    pub(crate) right_latched: bool,
    /// Animated sidebar widths (ease toward the open/closed targets).
    pub(crate) sidebar_w: f32,
    pub(crate) right_w: f32,
    /// Bottom drawer (Now Playing + lyrics): open flag + slide 0..1.
    pub(crate) drawer_open: bool,
    pub(crate) drawer_anim: f32,
    /// Animation prefs (in-memory; never persisted): master toggle plus
    /// a duration in ms for panel glides.
    pub(crate) animations_enabled: bool,
    pub(crate) anim_ms: u64,
    /// Display font pick (persisted pref `font_family`; relaunch to
    /// apply, since the renderer font is fixed at startup).
    pub(crate) font_family: FontFamily,
    /// UI language (persisted pref `language`; system locale on first
    /// launch; applies instantly, views read it every frame).
    pub(crate) language: Language,
    /// UI scale 0.8..=1.5 (persisted pref `ui_scale`; instant via
    /// the application scale factor).
    pub(crate) ui_scale: f32,
    /// Installed families for the font picker (async startup scan;
    /// empty until it lands).
    pub(crate) system_font_rows: Vec<String>,
    /// Picker rows (pinned heads plus installed families): rebuilt
    /// when the scan lands, borrowed by the view (never rebuilt or
    /// cloned per frame).
    pub(crate) font_rows: Vec<String>,
    /// Live artist top songs: server-ranked ids per channel id (play
    /// counts included). Merged into the catalog on arrival; views
    /// prefer these over the alphabetical catalog fallback. Holds the
    /// head 5 until "Show more" swaps in the full shelf.
    pub(crate) artist_top: HashMap<String, Vec<String>>,
    /// Channels whose full ranked shelf has landed (see
    /// [`Self::artist_full_task`]): the expanded view reads all rows,
    /// the collapsed head reads the first 5.
    pub(crate) artist_full: HashSet<String>,
    /// Live album tracklists: `track_no`-ordered ids per album browse
    /// id. Same merge-then-prefer rule as [`Self::artist_top`].
    pub(crate) album_live: HashMap<String, Vec<String>>,
    /// Route-to-browse aliases (name/title routes → channel/album
    /// browse ids): written at fetch launch and arrival, so cached
    /// lists survive later searches replacing `results` (which the
    /// resolvers read first). Plain overwrites (latest wins); tiny
    /// strings, so no cap needed unlike the lists below.
    pub(crate) artist_alias: HashMap<String, String>,
    pub(crate) album_alias: HashMap<String, String>,
    /// Insertion order for the two live-list maps above (FIFO eviction
    /// past the cap; `HashMap` alone cannot age keys).
    pub(crate) artist_top_order: Vec<String>,
    pub(crate) album_live_order: Vec<String>,
    /// In-flight detail fetches (channel / album browse id): arrival
    /// clears only its own marker, so quick page hops never strand it.
    pub(crate) artist_loading: Option<String>,
    pub(crate) album_loading: Option<String>,
    /// In-flight full-shelf fetch (expanded artist page): separate
    /// marker so a late head arrival never cancels or is cancelled by
    /// the expand fetch.
    pub(crate) artist_full_loading: Option<String>,
    /// In-flight bare-name resolves (route id): one scoped search at
    /// a time per kind, chained into the list fetch on success.
    pub(crate) artist_resolving: Option<String>,
    pub(crate) album_resolving: Option<String>,
    /// Full top-songs list expanded (artist page "Show all"): reset
    /// on every page open; the collapsed Popular shows the head.
    pub(crate) artist_expanded: bool,
    /// Drawer lyrics scroll offset (sticky compact header past 200px).
    pub(crate) drawer_scroll: f32,
    /// Scrollbar fade per scrollable key (last motion + opacity 0..1):
    /// scroll motion lights the bar, the ticker fades it after idle.
    /// Never persisted; missing keys read as invisible.
    pub(crate) scroll_hint: HashMap<&'static str, (std::time::Instant, f32)>,
    /// Scroll y offset per scrollable key: drives lazy art visibility
    /// (the component fetches only what the viewport covers).
    pub(crate) scroll_offsets: HashMap<&'static str, f32>,
    /// Last fetched scroll bucket per key (scroll storms collapse).
    pub(crate) art_buckets: HashMap<&'static str, i64>,
    /// Last art sweep per key (minimum gap between sweeps, per list).
    pub(crate) art_sweep_at: HashMap<&'static str, std::time::Instant>,
    /// Last artwork dir prune (bounded on-disk cache).
    pub(crate) art_pruned_at: Option<std::time::Instant>,
    /// Playlist filter text in the sidebar.
    pub(crate) sidebar_filter: String,
    /// Autoplay tail belongs to this seed (stale arrivals drop).
    pub(crate) autoplay_for: Option<String>,
    /// Spotify-style split: manual queue (user picks) plays first, then
    /// this radio-picked tail ("Next up"). Never persisted.
    pub(crate) autoplay: Vec<Track>,
    /// Queue row under the cursor (right panel hover reveals its remove).
    pub(crate) hovered_queue: Option<usize>,
    /// History-page row under the cursor (per-row index, like the
    /// queue: repeat plays share a track id).
    pub(crate) hovered_history: Option<usize>,
    /// Cursor over a row remove button (presses there must never arm a
    /// drag: slipping off delete must not become a reorder).
    pub(crate) hover_remove: Option<usize>,
    /// Track-row hover by id (reveals full-row highlight where lists are
    /// not index-stable: search/library/discovery rows). Recent has its
    /// own hover so the same track in the sidebar and the center never
    /// light up together.
    pub(crate) hovered_track: Option<String>,
    /// Right-panel Recent row under the cursor (id-keyed: the list
    /// dedupes, so one id names one row; separate from hovered_track
    /// so center rows with the same id stay at rest).
    pub(crate) hovered_recent: Option<String>,
    /// Search collection hover by id (album/artist/playlist rows).
    pub(crate) hovered_album: Option<String>,
    pub(crate) hovered_artist: Option<String>,
    pub(crate) hovered_search_playlist: Option<String>,
    /// Autoplay row under the cursor (same reveal, separate index space).
    pub(crate) hovered_auto: Option<usize>,
    /// Right-panel drag in progress (row press to slot hover to release).
    pub(crate) drag: Option<DragState>,
    /// Press snapshot for click detection (cleared on every release).
    pub(crate) press_snap: Option<PressSnap>,
    /// Sidebar playlist under the cursor (hover play affordance).
    pub(crate) hovered_playlist: Option<String>,
    /// Last committing reorder time: a shared press/release with the
    /// drop eats the row click that follows it (150ms window).
    pub(crate) just_reordered_at: Option<std::time::Instant>,
    /// Eased drop-slot gap 0..1 (opens room for the ghost chip).
    pub(crate) drag_gap: f32,
    /// Recent playback/fetch failures (capped, surfaced in Settings).
    pub(crate) error_log: Vec<String>,
    /// Manual position clock deadline after a user seek (anti-flicker).
    pub(crate) seek_hold_until: Option<std::time::Instant>,
    /// A seek landed while paused (backend clock stays stale until resume).
    pub(crate) paused_seek: bool,
    /// Last Tick instant (drives the manual clock during a seek hold).
    pub(crate) last_tick: Option<std::time::Instant>,
    /// Smoothed frames-per-second for the debug readout (0 = unmeasured).
    pub(crate) fps: f32,
    /// Last frame instant the meter saw (EMA needs the interval).
    pub(crate) last_frame: Option<std::time::Instant>,
    /// Last view() build cost in milliseconds (FPS readout, flag only).
    #[cfg(feature = "debug-panel")]
    pub(crate) view_build_ms: std::cell::Cell<f32>,
    /// Cookie-file path for import (Settings).
    pub(crate) cookie_path: String,
    /// Source export awaiting a delete/keep decision after a successful
    /// cookie import (modal). The secure session is already stored, so
    /// this plaintext file can go. `None` hides the modal.
    #[cfg(feature = "innertube")]
    pub(crate) pending_cookie_cleanup: Option<String>,
    /// Fresh-import source captured at dispatch (immune to later edits of
    /// the path field). Moved into `pending_cookie_cleanup` when the
    /// import completes; session re-imports and syncs never set it, so
    /// they never offer to delete a file they did not consume.
    #[cfg(feature = "innertube")]
    pub(crate) pending_cookie_source: Option<String>,
    /// Cleanup-modal entrance 0..1 (slides up from below while the
    /// ticker runs, snaps when animations are off). Shared by every
    /// modal dialog.
    pub(crate) modal_anim: f32,
    /// Playlist awaiting a delete/keep decision (modal). `None` hides
    /// the dialog; the menu row arms it, the dialog confirms it.
    pub(crate) pending_playlist_delete: Option<String>,
    /// Playlist awaiting a rename (modal). `None` hides the dialog;
    /// the menu row arms it with the current title in
    /// `rename_playlist_name`, the dialog confirms it.
    pub(crate) pending_playlist_rename: Option<String>,
    /// Rename dialog input (prefilled on arm, edited per keystroke).
    pub(crate) rename_playlist_name: String,
    /// Sign-out confirm dialog open (Settings row arms it).
    #[cfg(feature = "innertube")]
    pub(crate) pending_signout: bool,
    /// Scrobbling credentials dialog open (Settings button).
    pub(crate) scrobble_modal_open: bool,
    /// Artwork by thumbnail URL (iced image handles) + in-flight set.
    /// In-memory artwork handles with their fetch time: entries older
    /// than the TTL read as missing (see [`art_mem`]); the count cap
    /// still bounds the map, expiry only decides what renders.
    pub(crate) artwork: HashMap<String, (iced::widget::image::Handle, std::time::Instant)>,
    pub(crate) artwork_loading: HashSet<String>,
    /// Failed artwork URLs with the failure instant: sketchy or removed
    /// covers are not refetched for a cooldown (stops retry loops when
    /// completion chaining re-scans the visible screenful).
    pub(crate) artwork_failed: HashMap<String, std::time::Instant>,
    /// Ids whose metadata probe came back empty, with the instant:
    /// unresolvable videos are not re-probed for a cooldown.
    pub(crate) hydrate_cooldown: HashMap<String, std::time::Instant>,
    /// Ids with a metadata probe in flight (dedups lazy batches
    /// across navigations and ticks: no repeat fetch while pending).
    pub(crate) hydrating: HashSet<String>,
    /// First Tick after launch runs the Home lazy batch (startup
    /// itself does zero network so the window appears instantly).
    pub(crate) lazy_init_done: bool,
    /// Failed resolve attempts for the loading track: a few retries,
    /// then skip forward (never silently stuck, never an error loop).
    pub(crate) resolve_attempts: u8,
    /// Backend audio actually heard for the current load (nonzero raw
    /// position): distinguishes a finished track from a load that never
    /// started, so the EOF-idle net never fires on gaps or failures.
    pub(crate) heard_audio: bool,
    /// When the current stream URL was handed to mpv, until audio is
    /// heard: the watchdog for loads the backend accepted but never
    /// opened (see [`STREAM_OPEN_GRACE`]). Cleared wherever
    /// `heard_audio` resets (new loads, both Stops).
    /// Bound to the track id it was armed for: it only ever fires for
    /// that track (queue edits that shift `current()` cannot retarget it).
    pub(crate) stream_opened_at: Option<(std::time::Instant, String)>,
    /// Track id a prefetch was already attempted for (no Tick spam).
    pub(crate) prefetch_attempted: Option<String>,
    /// Google device-flow login in progress (cloned into poll tasks).
    #[cfg(feature = "innertube")]
    pub(crate) google_login: Option<yutu_core::GoogleLogin>,
    /// A saved OAuth token exists on disk (signed in).
    #[cfg(feature = "innertube")]
    pub(crate) logged_in: bool,
    /// A saved cookie session exists on disk (stay signed in).
    #[cfg(feature = "innertube")]
    pub(crate) cookie_logged_in: bool,
    /// A login/library network task is in flight.
    #[cfg(feature = "innertube")]
    pub(crate) login_busy: bool,
    /// Staged import progress: songs (liked) in the base stage, playlists
    /// done/total, plus the run generation (stale playlist arrivals from
    /// an older run are ignored). Powers the per-playlist status line.
    #[cfg(feature = "innertube")]
    pub(crate) import_songs: usize,
    #[cfg(feature = "innertube")]
    pub(crate) import_likes: usize,
    #[cfg(feature = "innertube")]
    pub(crate) import_done: usize,
    #[cfg(feature = "innertube")]
    pub(crate) import_total: usize,
    #[cfg(feature = "innertube")]
    pub(crate) import_gen: u64,
    /// An import is in flight: the top banner shows live progress.
    /// Sync pulls set this too (the pipeline is shared), but the banner
    /// stays down for them via `sync_active`: they surface only through
    /// the sidebar spinner.
    #[cfg(feature = "innertube")]
    pub(crate) import_active: bool,
    /// A sync pull (manual or scheduled) is in flight: no top banner,
    /// the sidebar title carries the spinner instead. Always paired
    /// with `import_active` (the pipeline is shared); cleared on every
    /// path that clears `import_active`.
    #[cfg(feature = "innertube")]
    pub(crate) sync_active: bool,
    /// Spinner frame for loading states: advanced one step per
    /// `Animate` tick while the pipeline runs (the ticker runs then,
    /// see [`Self::animating`]). Wraps around the precomputed frames.
    /// Shared by the sidebar sync glyph, busy buttons, and the artist
    /// Show-more loader.
    pub(crate) spin_frame: u64,
    /// Expected import songs (base songs plus known playlist totals) and
    /// playlist songs already finished: banner bar math. Per-list live
    /// counts arrive via ticks; finished indices quarantine late ticks.
    #[cfg(feature = "innertube")]
    pub(crate) import_expected: usize,
    #[cfg(feature = "innertube")]
    pub(crate) import_live: HashMap<usize, PlaylistLive>,
    #[cfg(feature = "innertube")]
    pub(crate) import_finished: HashSet<usize>,
    /// Banner display count, eased toward [`Self::import_song_target`]
    /// every Tick (see [`Self::ease_import_shown`]).
    #[cfg(feature = "innertube")]
    pub(crate) import_shown: usize,
    /// Scheduled sync: pulls web changes every [`AUTO_SYNC_INTERVAL`]
    /// while signed in (plus a manual Sync button in Settings).
    #[cfg(feature = "innertube")]
    pub(crate) auto_sync: bool,
    /// Last completed pull (any kind): drives the scheduler and the
    /// Settings "Last synced" line.
    #[cfg(feature = "innertube")]
    pub(crate) last_sync: Option<std::time::Instant>,
    /// Same, as unix time, persisted so the label survives restarts.
    #[cfg(feature = "innertube")]
    pub(crate) last_sync_unix: Option<i64>,
    /// Launch instant: the first auto-sync waits for a grace period so
    /// startup stays instant.
    #[cfg(feature = "innertube")]
    pub(crate) started_at: std::time::Instant,
    /// End-of-track auto-advance already fired for the current song.
    pub(crate) end_advanced: bool,
    pub(crate) library_path: String,
    /// Real-track catalog: every track the app has seen (search, radio,
    /// playlists). ID lookups resolve here: never fixtures.
    pub(crate) catalog: HashMap<String, Track>,
    /// Last complete real search (local filtering base for `SearchChanged`).
    pub(crate) search_base: SearchResults,
    /// Last complete unfiltered search (same narrowing base for All).
    pub(crate) search_all_base: SearchAll,
    /// Query cache: normalized query to (arrival time, results). Repeat
    /// searches (backspace/retype, Enter-after-popup) skip the network.
    pub(crate) search_cache: HashMap<String, (std::time::Instant, SearchBundle)>,
    /// A real search is in flight (loading indicator).
    pub(crate) searching: bool,
    /// Keystroke generation: each input bumps it; only the latest
    /// debounced task may fire (no search spam while typing).
    pub(crate) search_gen: u64,
    /// Search page type filter (chip row): kept across queries.
    pub(crate) search_filter: SearchFilter,
    /// Current radio sequence (filled by `RadioReady`).
    pub(crate) radio_tracks: Vec<Track>,
    /// MPRIS handle for live desktop propagation (`None` in tests/headless).
    pub(crate) mpris: Option<yutu_desktop::SharedPlayer>,
    /// Per-playlist sort pick (own playlists): display-only except Manual,
    /// which is the stored order and the only draggable mode. Persisted
    /// via `playlist_sort_<id>` prefs, default Manual.
    pub(crate) playlist_sorts: HashMap<String, PlaylistSort>,
    /// Insertion times per playlist (track id to added_at): loaded once
    /// at startup, kept in step on add/remove/sync/reorder. Manual
    /// reorders move positions but never touch these, so oldest/newest
    /// stay on add time.
    pub(crate) playlist_added: HashMap<String, HashMap<String, i64>>,
    /// Manual playlist reorder drag: same slot model as the queue drag
    /// (grip press, slot hover, release anywhere), but scoped to the
    /// open PlaylistDetail list. Only Manual mode arms it.
    pub(crate) pl_drag: Option<DragState>,
    pub(crate) pl_press_snap: Option<PressSnap>,
    /// Playlist detail row under the cursor (index into the ordered view):
    /// arms the manual drag and renders the hover highlight.
    pub(crate) pl_hover: Option<usize>,
    /// Cursor over a playlist row action button (heart or ellipsis):
    /// presses there must never arm a manual drag (slipping off like or
    /// menu must not become a reorder, same contract as the queue
    /// remove guard). Index into the ordered view, like `pl_hover`.
    pub(crate) pl_action_hover: Option<usize>,
    /// Which playlist the in-flight manual drag belongs to (stale drags
    /// from a navigated-away list never commit).
    pub(crate) pl_drag_id: Option<String>,
}
