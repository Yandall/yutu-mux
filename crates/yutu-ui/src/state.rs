//! yutu-ui state: split from lib.rs (no behavior change).

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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Route {
    #[default]
    Home,
    Search,
    History,
    Discovery,
    Settings,
    Artist(String),
    Album(String),
    PlaylistDetail(String),
    Lyrics(String),
    Radio(String),
}

/// In-progress right-panel drag: source index + insertion slot.
/// Slots sit between rows plus both edges (0 before the first row, len
/// after the last) so every song always has a committable move. A press
/// first arms the drag invisibly (`live` false, no layout change) so a
/// plain click never shifts rows; the slot lanes only appear once the
/// cursor moves past click slop (`live` true). While live the origin row
/// is held by the floating ghost and the list opens one full-row gap at
/// the commit target, so siblings shift by exactly one row to make room.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DragState {
    pub(crate) from: usize,
    pub(crate) slot: Option<usize>,
    /// Armed (press held) vs live (moved past slop, lanes visible).
    pub(crate) live: bool,
}

/// Press snapshot for click-vs-drag: a release with no committed move
/// over the same unmutated row counts as a click (plays it).
#[derive(Debug, Clone)]
pub(crate) struct PressSnap {
    pub(crate) from: usize,
    pub(crate) pos: Option<iced::Point>,
    pub(crate) id: String,
    pub(crate) len: usize,
}

/// Press within slop of release (px): hand tremor must not kill a click.
pub(crate) const CLICK_SLOP_PX: f32 = 6.0;

/// Press and release positions match for click purposes (exact when the
/// cursor never moved, slop-tolerant otherwise).
pub(crate) fn release_is_click_at(
    press: Option<iced::Point>,
    release: Option<iced::Point>,
) -> bool {
    match (press, release) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            let dx = a.x - b.x;
            let dy = a.y - b.y;
            dx * dx + dy * dy < CLICK_SLOP_PX * CLICK_SLOP_PX
        }
        _ => false,
    }
}

/// Right-panel tab: queue or recently played.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RightTab {
    #[default]
    Queue,
    Recent,
}

/// Home shelf contents, recomputed on navigation and on Tick while
/// the shelves are visible (never per frame: the ranking scans the
/// whole catalog plus a history query, ~ms in debug builds).
#[derive(Debug, Clone, Default)]
pub(crate) struct HomeShelves {
    pub(crate) quick: Vec<Track>,
    pub(crate) albums: Vec<AlbumGroup>,
    pub(crate) more_artist: Option<String>,
    pub(crate) more: Vec<Track>,
    pub(crate) weekly: Vec<Track>,
    pub(crate) forgotten: Vec<Track>,
}

/// One recommended-album shelf group: catalog tracks sharing an
/// (album, artist) name, scored by summed plays. Art comes from the
/// first track; playback queues every track in id order.
#[derive(Debug, Clone, Default)]
pub(crate) struct AlbumGroup {
    pub(crate) title: String,
    pub(crate) artist: String,
    pub(crate) plays: usize,
    pub(crate) tracks: Vec<Track>,
}

/// Click behavior for a numbered song row: plain `PlayNow`, or play
/// from here (clicked song plus the suffix behind it, like
/// playlists) anchored at a list position. The route id re-resolves
/// the displayed list at click time, so live arrivals between render
/// and click still queue what the row showed.
#[derive(Debug, Clone)]
pub(crate) enum RowPlay {
    Now,
    ArtistAt(String),
    AlbumAt(String),
}

impl RowPlay {
    pub(crate) fn message(&self, track_id: &str, n: usize) -> Message {
        // `n` is the 1-based row number; suffixes are 0-based.
        let index = n.saturating_sub(1);
        match self {
            Self::Now => Message::PlayNow(track_id.to_string()),
            Self::ArtistAt(id) => Message::PlayArtistAt {
                artist_id: id.clone(),
                index,
                track_id: track_id.to_string(),
            },
            Self::AlbumAt(id) => Message::PlayAlbumAt {
                album_id: id.clone(),
                index,
                track_id: track_id.to_string(),
            },
        }
    }
}

/// Search page type filter (chip row above the results). The backend
/// returns songs, albums, artists and community playlists, so those
/// are the only chips: no empty categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchFilter {
    #[default]
    All,
    Songs,
    Albums,
    Artists,
    Playlists,
}

/// Own-playlist sort pick (PlaylistDetail header): display-only except
/// Manual, which is the stored position order and the only draggable
/// mode. Oldest/newest read insertion time (`added_at`, not position),
/// so manual reorders never move them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlaylistSort {
    #[default]
    Manual,
    NewestFirst,
    OldestFirst,
    Title,
    Artist,
    Album,
    LikedFirst,
}

impl PlaylistSort {
    /// All picks in picker order (matches the request list).
    pub const ALL: [Self; 7] = [
        Self::NewestFirst,
        Self::OldestFirst,
        Self::Title,
        Self::Artist,
        Self::Album,
        Self::LikedFirst,
        Self::Manual,
    ];

    /// Picker label (user-facing, stays English like other glue labels).
    pub fn label(self) -> &'static str {
        match self {
            Self::NewestFirst => "Newest first",
            Self::OldestFirst => "Oldest first",
            Self::Title => "Title",
            Self::Artist => "Artist",
            Self::Album => "Album",
            Self::LikedFirst => "Liked first",
            Self::Manual => "Manual",
        }
    }

    /// Stored pref value.
    pub fn stored(self) -> &'static str {
        match self {
            Self::NewestFirst => "newest",
            Self::OldestFirst => "oldest",
            Self::Title => "title",
            Self::Artist => "artist",
            Self::Album => "album",
            Self::LikedFirst => "liked",
            Self::Manual => "manual",
        }
    }

    /// Parse a stored value (unknown falls back to Manual, so prefs never
    /// break the view).
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_lowercase().as_str() {
            "newest" | "newest-first" | "newest first" => Self::NewestFirst,
            "oldest" | "oldest-first" | "oldest first" => Self::OldestFirst,
            "title" => Self::Title,
            "artist" => Self::Artist,
            "album" => Self::Album,
            "liked" | "liked-first" | "liked first" => Self::LikedFirst,
            _ => Self::Manual,
        }
    }
}

/// Which selector menu is open (None = closed): at most one at a time.
/// The open card floats from its button (input-anchored dropdown
/// overlay); dismissal stays top-level (scrim + Esc) like the menus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectMenu {
    /// Sort picks for this playlist id (detail header).
    Sort(String),
    /// Settings language pick.
    Language,
    /// Settings lyrics-provider pick.
    LyricsProvider,
    /// Settings display-font pick.
    FontFamily,
}

/// Which sign-in started an import (status wording only). `Sync` is a
/// manual or scheduled refresh (same pipeline, "Sync" wording).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(feature = "innertube")]
pub enum ImportKind {
    Cookie,
    Library,
    Sync,
}

/// One in-flight playlist fetch for the banner bar: cumulative arrived
/// songs plus the best-effort total (unknown when details disagrees).
#[derive(Debug, Clone, Default)]
#[cfg(feature = "innertube")]
pub(crate) struct PlaylistLive {
    pub(crate) arrived: usize,
    pub(crate) count: Option<usize>,
}

#[cfg(feature = "innertube")]
impl ImportKind {
    /// Short source name for failure lines ("Cookie import failed").
    pub(crate) fn label(self) -> &'static str {
        match self {
            ImportKind::Cookie => "Cookie",
            ImportKind::Library => "Library",
            ImportKind::Sync => "Sync",
        }
    }
}

/// Base pull payload: the staged import plus the flags sync needs to
/// reconcile safely (see [`StagedImport`]).
#[derive(Debug, Clone, Default)]
#[cfg(feature = "innertube")]
pub(crate) struct BasePull {
    pub(crate) tracks: Vec<Track>,
    pub(crate) liked_ids: Vec<String>,
    pub(crate) disliked_ids: Vec<String>,
    pub(crate) headers: Vec<yutu_core::ServerPlaylist>,
    pub(crate) headers_clean: bool,
    pub(crate) library_ok: bool,
}

/// First import stage: songs + liked + playlist headers, plus the
/// connected backend so per-playlist fetches reuse it (no reconnect per
/// list). Playlists then stream in next, one task each, keeping the
/// window alive with progress instead of freezing on one long future.
/// `headers_clean` is false when header pages failed (sync must not
/// prune then: absence may be a failed page, not a web deletion);
/// `library_ok` is false when the library fetch failed open (sync must
/// not replace likes/dislikes then).
#[derive(Debug, Clone)]
#[cfg(feature = "innertube")]
pub struct StagedImport {
    pub backend: yutu_core::InnertubeBackend,
    pub tracks: Vec<Track>,
    pub liked_ids: Vec<String>,
    pub disliked_ids: Vec<String>,
    pub headers: Vec<yutu_core::ServerPlaylist>,
    pub headers_clean: bool,
    pub library_ok: bool,
}

/// Collection menu target: which search album/artist hit,
/// community playlist, or home recommended-album group the open menu
/// belongs to (track menus and user playlist menus keep their own
/// id-keyed state).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CollectionMenu {
    Album(String),
    Artist(String),
    Playlist(String),
    HomeAlbum { album: String, artist: String },
}

#[derive(Debug, Clone)]
pub enum Message {
    Navigate(Route),
    /// Open a URL in the system browser (About page links).
    OpenUrl(String),
    SearchChanged(String),
    Search,
    PlayTrack(usize),
    Pause,
    TogglePause,
    EnqueueTrack(String),
    /// Append to the end of the queue without touching playback.
    QueueTrack(String),
    NewPlaylistNameChanged(String),
    Stop,
    Next,
    Prev,
    Like(String),
    /// Dislike ("don't recommend"): toggles `like/dislike` server-side.
    Dislike(String),
    SetVolume(f32),
    ToggleMute,
    QueueClear,
    OpenArtist(String),
    OpenAlbum(String),
    OpenPlaylist(String),
    OpenLyrics(String),
    /// Default lyrics provider pick (Settings + drawer stepper).
    SetLyricsProvider(LyricsProvider),
    /// Step the lyrics provider (`+1` next, `-1` previous): the drawer
    /// and lyrics page `< >` stepper. Pins the viewed track to strict
    /// display (no automatic fallback) and tops up missing fetches.
    CycleLyricsProvider(i8),
    StartRadio(String),
    PlayAll,
    CreatePlaylist(String),
    AddToPlaylist {
        playlist_id: String,
        track_id: String,
    },
    RemoveFromPlaylist {
        playlist_id: String,
        track_id: String,
    },
    /// Delete the playlist, from its confirm dialog (the menu row only
    /// arms the dialog via [`Message::RequestDeletePlaylist`]).
    DeletePlaylist(String),
    /// Arm the playlist-delete confirm dialog (menu row).
    RequestDeletePlaylist(String),
    /// Dismiss the playlist-delete confirm (scrim/Esc/keep: safe default).
    CancelDeletePlaylist,
    SelectEqPreset(String),
    SetEqBand(usize, f32),
    EqToggled(bool),
    SeekPreview(f32),
    SeekRelease,
    ScrobbleToggled(bool),
    ScrobbleUserChanged(String),
    ScrobbleLbTokenChanged(String),
    ScrobbleFmKeyChanged(String),
    ScrobbleFmSecretChanged(String),
    ScrobbleFmSkChanged(String),
    /// Open the scrobbling credentials dialog (Settings).
    OpenScrobbleModal,
    /// Close the scrobbling credentials dialog (scrim/Esc keeps edits).
    CloseScrobbleModal,
    ScrobbleSubmitted {
        target: String,
        result: Result<(), String>,
    },
    PlayWeeklyMix,
    PlayForgotten,
    /// Quick picks shelf: queue the heavy-rotation head.
    PlayQuickPicks,
    /// Recommended-album card: queue that album's tracks in order.
    PlayHomeAlbum {
        album: String,
        artist: String,
    },
    /// Recommended-album card right-click menu.
    OpenHomeAlbumMenu {
        album: String,
        artist: String,
    },
    /// Recommended-album menu: append that album's tracks to the queue.
    QueueHomeAlbum {
        album: String,
        artist: String,
    },
    /// Recommended-album menu: shuffle-play that album's tracks.
    ShuffleHomeAlbum {
        album: String,
        artist: String,
    },
    SearchCompleted {
        query: String,
        result: Result<Box<SearchBundle>, String>,
    },
    RadioReady {
        seed: String,
        result: Result<Vec<Track>, String>,
    },
    QueueJump(usize),
    ToggleShuffle,
    CycleRepeat,
    Tick(std::time::Instant),
    /// Frame redraw tick (drives the debug FPS meter only; the
    /// subscription exists solely with `debug-panel`).
    Frame(std::time::Instant),
    SeekTo(f32),
    /// Exact-ms seek (lyric verse clicks; no f32 truncation).
    SeekToMs(u64),
    NavBack,
    NavForward,
    SetRightTab(RightTab),
    SetSearchFilter(SearchFilter),
    SuggestCompleted {
        gen: u64,
        result: Result<Vec<String>, String>,
    },
    /// Click-to-play: replace the queue with this track + autoplay tail.
    PlayNow(String),
    AutoplayReady {
        seed: String,
        result: Result<Vec<Track>, String>,
    },
    /// Insert after the current item (context menu: play next).
    PlayNext(String),
    /// Play an autoplay-tail item now (it joins the manual queue).
    PlayAutoplay(usize),
    /// Play-all with shuffle turned on first (hero shuffle buttons).
    PlayAllShuffled,
    /// Play one playlist from the clicked row on: the song plus the
    /// rest of the playlist from that point replace the queue. The
    /// clicked id anchors the suffix (ordered-view inserts between
    /// render and click must not shift the head); the index is the
    /// fallback when the id left the list.
    PlayPlaylistAt {
        playlist_id: String,
        index: usize,
        track_id: String,
    },
    /// Shuffle-play one search hero artist (known catalog tracks).
    PlayArtistShuffled(String),
    /// Shuffle-play one search hero album (known catalog tracks).
    PlayAlbumShuffled(String),
    /// Play one search-result playlist now (tracks fetch live first).
    PlaySearchPlaylist(String),
    /// Shuffle-play one search-result playlist (tracks fetch live first).
    PlaySearchPlaylistShuffled(String),
    /// A search-result playlist arrived (or failed): queues on success,
    /// honest status on failure.
    SearchPlaylistReady {
        id: String,
        label: String,
        shuffle: bool,
        result: Result<Vec<Track>, String>,
    },
    /// Remove one occurrence from the manual queue (menu / hover remove).
    RemoveFromQueue(String),
    /// Drop an autoplay-tail item by index.
    RemoveAutoplay(usize),
    /// Row-leave: clears the hover only when it still names this row.
    /// Iced publishes enter/exit in tree order per event, so a fast
    /// multi-row jump emits enter(new) before exit(old): a blind clear
    /// would wipe the fresh hover and leave the cursor on a dead row.
    QueueExit(usize),
    /// History-page hover tracking (per-row index: repeat plays share
    /// a track id, so id-keyed hover would light them all up at once).
    HistoryHover(Option<usize>),
    /// History row-leave (same enter-before-exit ordering; see
    /// [`Message::QueueExit`]).
    HistoryExit(usize),
    /// Id-keyed row hover (full-row highlight on non-index lists).
    TrackHover(Option<String>),
    /// Track row-leave (same enter-before-exit ordering; see
    /// [`Message::QueueExit`]).
    TrackExit(String),
    /// Right-panel Recent hover (same id-keyed contract as track rows
    /// but its own state, so a Recent row and a center row for the
    /// same track never highlight together).
    RecentHover(Option<String>),
    /// Recent row-leave (same enter-before-exit ordering; see
    /// [`Message::QueueExit`]).
    RecentExit(String),
    /// Sidebar playlist hover (reveals the row play button).
    PlaylistHover(Option<String>),
    /// Sidebar playlist row-leave (same enter-before-exit ordering;
    /// see [`Message::QueueExit`]).
    PlaylistExit(String),
    /// Autoplay hover tracking (reveals the row remove).
    AutoHover(Option<usize>),
    /// Autoplay row-leave (same enter-before-exit ordering; see
    /// [`Message::QueueExit`]).
    AutoExit(usize),
    /// Search collection hover (reveals the row highlight + menu).
    AlbumHover(Option<String>),
    ArtistHover(Option<String>),
    SearchPlaylistHover(Option<String>),
    /// Search collection row-leaves (same enter-before-exit ordering;
    /// see [`Message::QueueExit`]).
    AlbumExit(String),
    ArtistExit(String),
    SearchPlaylistExit(String),
    /// Right-panel drag reorder (row press, slot hover, release anywhere).
    DragStart(usize),
    /// Global left press: arms a drag when the cursor is over a queue
    /// row (inner buttons swallow the row area press, so the row alone
    /// cannot arm from text/art; this covers the whole row).
    GlobalPress,
    /// Insertion-slot hover while dragging (0..=len, edges included).
    DragSlot(usize),
    /// Manual playlist reorder (own playlists, Manual sort only): grip
    /// press arms, slot hover goes live, release commits. Scoped by
    /// playlist id so stale drags never commit after navigation.
    PlDragStart {
        playlist_id: String,
        index: usize,
    },
    PlDragSlot(usize),
    PlDragEnd,
    /// Playlist detail row hover (index into the ordered view): highlight
    /// plus manual-drag arming.
    PlRowHover(usize),
    PlRowExit(usize),
    /// Cursor over a playlist row action button (heart or ellipsis):
    /// presses there must never arm a manual drag.
    PlActionHover(Option<usize>),
    /// Action-button leave: clears only when it still names this row,
    /// so a fast jump onto another row's action never disarms it (a
    /// press there must delete, never start a drag).
    PlActionExit(usize),
    /// Own-playlist sort pick (PlaylistDetail header picker).
    PlaylistSortSelected {
        playlist_id: String,
        sort: PlaylistSort,
    },
    /// Animation frame (sidebar widths + drawer slide converge).
    Animate,
    /// Bottom drawer open/close.
    ToggleDrawer,
    /// Drawer lyrics scroll offset (sticky header past 200px).
    DrawerScrolled(f32),
    /// A scrollable moved: lights its scrollbar fade, which the ticker
    /// eases out after idle (the key matches a `scroll_hint` entry), and
    /// carries the y offset so art fetches follow the viewport.
    Scrolled(&'static str, f32),
    DragEnd,
    /// Right-panel row hover: highlight + drag-target tracking.
    PanelHover(usize),
    /// Remove-button hover (presses there never arm a drag).
    RemoveHover(Option<usize>),
    /// Remove-button leave: clears only when it still names this row,
    /// so a fast jump onto another row's remove never disarms it (a
    /// press there must delete, never start a drag).
    RemoveExit(usize),
    /// Context menu open/close: right-click (per-row `mouse_area`) or the
    /// per-row `…` button opens it at the last cursor position.
    OpenContext(String),
    CloseContext,
    /// Dismiss the search popup only (scrim outside the input).
    CloseSuggest,
    /// Clear every row hover (scrims swallow hovers so nothing behind
    /// a popup or menu highlights).
    ClearHovers,
    /// Playlist context menu open/close (sidebar rows, detail header).
    OpenPlaylistMenu(String),
    /// Sort menu open for this playlist id (detail header sort button):
    /// same frozen anchor as the other menus, picks send
    /// [`PlaylistSortSelected`](Message::PlaylistSortSelected).
    /// The open card floats from the button itself (input-anchored
    /// dropdown overlay), never from the cursor.
    OpenSelectMenu(SelectMenu),
    /// Search collection menu open (album/artist/community-playlist
    /// rows): right-click or the hover `…` button, same frozen anchor
    /// as the track and playlist menus.
    OpenAlbumMenu(String),
    OpenArtistMenu(String),
    OpenSearchPlaylistMenu(String),
    /// Play a playlist from the top (clears the queue first).
    PlayPlaylist(String),
    /// Append a whole playlist to the queue (never interrupts).
    QueuePlaylist(String),
    /// Shuffle a playlist then play from the top.
    ShufflePlaylist(String),
    /// Play an artist page list from the clicked song on (clicked song
    /// plus the suffix behind it, like playlists). The clicked id
    /// anchors the suffix: catalog inserts between render and click
    /// must never shift the head to a different song. Index is the
    /// fallback when the id left the list.
    PlayArtistAt {
        artist_id: String,
        index: usize,
        track_id: String,
    },
    /// Play an album tracklist from the clicked song on (same
    /// suffix rule as playlists).
    PlayAlbumAt {
        album_id: String,
        index: usize,
        track_id: String,
    },
    /// Toggle the playlist flyout for a track (menu stays open).
    TogglePlaylistSubmenu(String),
    /// Hover on the Playlists toggle row: enter opens the flyout (no
    /// click needed), exit only clears the hot flag (the Tick grace
    /// closes it once the panel is cold too).
    SubmenuTriggerHot(Option<String>),
    /// Hover on the flyout panel itself: feeds the same dismiss grace.
    SubmenuFlyHot(bool),
    CursorMoved(iced::Point),
    WindowResized(iced::Size),
    /// Context menu: jump to this track's album/artist detail.
    OpenTrackAlbum(String),
    OpenTrackArtist(String),
    ToggleSidebarPin,
    ToggleRightPin,
    /// Animation master toggle (Settings).
    AnimationsToggled(bool),
    /// Animation duration in ms 20..=400 (Settings slider).
    AnimMsChanged(f32),
    /// Display font pick (Settings Appearance): persisted, takes
    /// effect on relaunch (the renderer font is fixed at startup).
    FontFamilyChanged(FontFamily),
    /// UI scale 0.8..=1.5 (Settings Appearance slider): persisted,
    /// applies instantly through the application scale factor.
    UiScaleChanged(f32),
    /// UI language (Settings Language picker): persisted, applies
    /// instantly (views read it every frame).
    LanguageChanged(Language),
    /// Installed font families arrived (async startup scan for the
    /// Settings picker; the window never waits for it).
    SystemFontsLoaded(Vec<String>),
    /// One-click restart for a pending font pick (Appearance shows
    /// the button only while the pick differs from the live font).
    RelaunchNow,
    /// Live artist top songs arrived (server rank with play counts).
    ArtistTopLoaded {
        channel_id: String,
        result: Result<Vec<Track>, String>,
    },
    /// Full ranked shelf arrived (raw browse): replaces the head when
    /// the page is expanded.
    ArtistFullLoaded {
        channel_id: String,
        result: Result<Vec<Track>, String>,
    },
    /// Live album tracklist arrived (`track_no` order with counts).
    AlbumLiveLoaded {
        album_id: String,
        result: Result<Vec<Track>, String>,
    },
    /// Bare artist name resolved to a channel id (scoped search);
    /// chains into the top-songs fetch.
    ArtistChannelResolved {
        route_id: String,
        name: String,
        result: Result<String, String>,
    },
    /// Bare album title resolved to a browse id (scoped search);
    /// chains into the tracklist fetch.
    AlbumBrowseResolved {
        route_id: String,
        title: String,
        result: Result<String, String>,
    },
    /// Artist page "Show all": expand Popular to the full server-ranked
    /// list (one-way; navigating away and back collapses).
    ArtistTopExpand,
    SidebarFilterChanged(String),
    HistoryFilterChanged(String),
    /// History page: render the next window of older plays.
    HistoryMore,
    CookiePathChanged(String),
    CookieImport,
    /// Open the system file picker for the cookies.txt export.
    #[cfg(feature = "innertube")]
    CookieBrowse,
    /// File picker result (`None` = cancelled, path unchanged).
    #[cfg(feature = "innertube")]
    CookieFilePicked(Option<String>),
    /// First import stage done (songs + liked + playlist headers).
    #[cfg(feature = "innertube")]
    ImportStageReady {
        kind: ImportKind,
        result: Result<StagedImport, String>,
    },
    /// One playlist's tracks arrived (or failed); import continues.
    /// `gen` quarantines stragglers from an older import run.
    #[cfg(feature = "innertube")]
    PlaylistImported {
        kind: ImportKind,
        gen: u64,
        title: String,
        index: usize,
        total: usize,
        count: Option<usize>,
        result: Result<Playlist, String>,
    },
    /// One playlist page mapped (cumulative song count plus the
    /// best-effort total): moves the banner bar song by song. Data
    /// still merges only on [`Message::PlaylistImported`].
    #[cfg(feature = "innertube")]
    PlaylistTick {
        gen: u64,
        index: usize,
        arrived: usize,
        count: Option<usize>,
    },
    HomeSearchChanged(String),
    /// Suggestion picked (click or keyboard): same flow as a keystroke
    /// but the popup closes instead of staying open.
    PickSuggestion(String),
    /// Arrow-key navigation over the open suggest popup (wraps).
    SuggestMoveUp,
    SuggestMoveDown,
    /// Enter with the popup open: plays/picks the highlighted row, or
    /// submits the query when nothing is highlighted.
    SuggestPick,
    SearchStay,
    SearchDebounced {
        gen: u64,
        query: String,
    },
    UrlResolved {
        track_id: String,
        result: Result<String, String>,
    },
    UrlPrefetched {
        track_id: String,
        result: Result<String, String>,
    },
    LyricsReady {
        track_id: String,
        provider: LyricsProvider,
        result: Result<LyricsResult, String>,
    },
    ArtworkReady {
        url: String,
        result: Result<Vec<u8>, String>,
    },
    #[cfg(feature = "innertube")]
    GoogleLoginStart,
    #[cfg(feature = "innertube")]
    GoogleLoginBegan {
        result: Result<yutu_core::GoogleLogin, String>,
    },
    #[cfg(feature = "innertube")]
    GoogleLoginPoll,
    #[cfg(feature = "innertube")]
    GoogleLoginPolled {
        result: Result<Option<yutu_core::OAuthPersist>, String>,
    },
    #[cfg(feature = "innertube")]
    GoogleLogout,
    #[cfg(feature = "innertube")]
    LibraryImport,
    /// Re-import via the saved cookie session (no file picker needed).
    #[cfg(feature = "innertube")]
    CookieSessionImport,
    /// Delete the saved cookie session (stays out of the account).
    /// Fires from the sign-out confirm dialog (the Settings row only
    /// arms it via [`Message::RequestSignOut`]).
    #[cfg(feature = "innertube")]
    CookieLogout,
    /// Arm the sign-out confirm dialog (Settings row).
    #[cfg(feature = "innertube")]
    RequestSignOut,
    /// Dismiss the sign-out confirm (scrim/Esc/stay: safe default).
    #[cfg(feature = "innertube")]
    CancelSignOut,
    /// Delete the just-imported `cookies.txt` export (secure session kept).
    #[cfg(feature = "innertube")]
    CookieCleanupDelete,
    /// Keep the just-imported `cookies.txt` export on disk.
    #[cfg(feature = "innertube")]
    CookieCleanupKeep,
    /// Manual refresh from Settings: replays pending pushes, then pulls
    /// web changes (likes, playlist songs/titles, deletions) via the
    /// saved session. Same pipeline as imports, "Sync" wording.
    #[cfg(feature = "innertube")]
    SyncNow,
    /// Scheduled-sync master toggle (Settings).
    #[cfg(feature = "innertube")]
    AutoSyncToggled(bool),
    /// Best-effort server like/unlike result (local state is already
    /// applied: only failures report, so the row never lies).
    #[cfg(feature = "innertube")]
    LikeSynced {
        track_id: String,
        liked: bool,
        result: Result<(), String>,
    },
    /// Best-effort server dislike/clear result (same local-first contract
    /// as [`Message::Like`]).
    #[cfg(feature = "innertube")]
    DislikeSynced {
        track_id: String,
        disliked: bool,
        result: Result<(), String>,
    },
    /// Server playlist creation result: carries the local shell id so the
    /// handler can migrate it to the real server id.
    #[cfg(feature = "innertube")]
    ServerPlaylistCreated {
        local_id: String,
        title: String,
        result: Result<String, String>,
    },
    /// Best-effort server playlist add/remove/delete result (local state
    /// is already applied: only failures report). `op` identifies the
    /// push so the handler can clear its pending row on success.
    #[cfg(feature = "innertube")]
    ServerPlaylistSynced {
        detail: String,
        op: PlaylistSyncOp,
        result: Result<(), String>,
    },
    StreamResolved {
        video_id: String,
        result: Result<Vec<u8>, String>,
    },
    /// Background hydration for ids the catalog lacks (liked/history
    /// rows from older installs, or playlist rows missing art): the
    /// payload registers into the catalog + track cache + artwork.
    TracksHydrated {
        requested: Vec<String>,
        tracks: Vec<Track>,
    },
}

/// Which playlist push a [`Message::ServerPlaylistSynced`] reports, so
/// the handler clears exactly that pending row (a stale in-flight task
/// must never clear a newer intent).
#[derive(Debug, Clone)]
#[cfg(feature = "innertube")]
pub enum PlaylistSyncOp {
    Add {
        playlist_id: String,
        track_id: String,
    },
    Remove {
        playlist_id: String,
        track_id: String,
    },
    Delete {
        playlist_id: String,
    },
}
