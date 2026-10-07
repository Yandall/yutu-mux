//! yutu-ui: native Iced app — real YouTube Music data only, no mock fixtures.

use std::collections::{HashMap, HashSet};
use yutu_core::{
    forgotten_favorites, should_scrobble, weekly_mix, Album, HistoryEntry, LibraryStore,
    LyricsResult, Playlist, SearchResults, Track,
};
#[cfg(feature = "mpv")]
use yutu_player::MpvBackend;
use yutu_player::{EqPreset, Queue, RodioBackend};

// ---------------------------------------------------------------------------
// Spotify-style theme (dark, green accent). Keeps every view on one visual
// language: near-black app, slightly lighter sidebar/cards, single green
// primary action, muted secondary text, text-buttons for row actions.
// ---------------------------------------------------------------------------

const SPOT_GREEN: iced::Color = iced::Color::from_rgb8(0x1D, 0xB9, 0x54);
const SPOT_GREEN_HOVER: iced::Color = iced::Color::from_rgb8(0x1F, 0xDF, 0x64);
const MUTED: iced::Color = iced::Color::from_rgb8(0xB3, 0xB3, 0xB3);
const CARD_BG: iced::Color = iced::Color::from_rgb8(0x18, 0x18, 0x18);
const CARD_BG_HOVER: iced::Color = iced::Color::from_rgb8(0x28, 0x28, 0x28);
const SIDEBAR_BG: iced::Color = iced::Color::from_rgb8(0x12, 0x12, 0x12);
const APP_BG: iced::Color = iced::Color::from_rgb8(0x0A, 0x0A, 0x0A);
const NAV_ACTIVE_BG: iced::Color = iced::Color::from_rgb8(0x2A, 0x2A, 0x2A);

fn app_bg(_: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(iced::Background::Color(APP_BG)),
        ..Default::default()
    }
}

fn sidebar_bg(_: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(iced::Background::Color(SIDEBAR_BG)),
        border: iced::Border::default().rounded(8),
        ..Default::default()
    }
}

fn card_bg(_: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(iced::Background::Color(CARD_BG)),
        border: iced::Border::default().rounded(8),
        ..Default::default()
    }
}

fn player_bg(_: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(iced::Background::Color(SIDEBAR_BG)),
        border: iced::Border::default().rounded(8),
        ..Default::default()
    }
}

/// Green circular primary action (Spotify play button).
fn green_play(
    _: &iced::Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let bg = match status {
        iced::widget::button::Status::Hovered => SPOT_GREEN_HOVER,
        _ => SPOT_GREEN,
    };
    iced::widget::button::Style {
        background: Some(iced::Background::Color(bg)),
        text_color: iced::Color::BLACK,
        border: iced::Border::default().rounded(20),
        ..Default::default()
    }
}

/// Sidebar nav: active gets a pill background, inactive is transparent.
fn nav_style(
    active: bool,
) -> impl Fn(&iced::Theme, iced::widget::button::Status) -> iced::widget::button::Style {
    move |_: &iced::Theme, status: iced::widget::button::Status| {
        let hovered = matches!(
            status,
            iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed
        );
        let background = if active || hovered {
            Some(iced::Background::Color(NAV_ACTIVE_BG))
        } else {
            None
        };
        let text_color = if active || hovered {
            iced::Color::WHITE
        } else {
            MUTED
        };
        iced::widget::button::Style {
            background,
            text_color,
            border: iced::Border::default().rounded(6),
            ..Default::default()
        }
    }
}

/// Card-as-button (playlist / mix tiles).
fn card_button(
    _: &iced::Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let bg = match status {
        iced::widget::button::Status::Hovered => CARD_BG_HOVER,
        _ => CARD_BG,
    };
    iced::widget::button::Style {
        background: Some(iced::Background::Color(bg)),
        text_color: iced::Color::WHITE,
        border: iced::Border::default().rounded(8),
        ..Default::default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Route {
    #[default]
    Home,
    Search,
    Queue,
    Library,
    Discovery,
    Settings,
    Artist(String),
    Album(String),
    PlaylistDetail(String),
    Lyrics(String),
    Radio(String),
    NowPlaying,
}

/// Right-panel tab: queue or recently played.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RightTab {
    #[default]
    Queue,
    Recent,
}

#[derive(Debug, Clone)]
pub enum Message {
    Navigate(Route),
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
    SetVolume(f32),
    QueueRemove(usize),
    QueueUp(usize),
    QueueDown(usize),
    QueueClear,
    OpenArtist(String),
    OpenAlbum(String),
    OpenPlaylist(String),
    OpenLyrics(String),
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
    DeletePlaylist(String),
    SelectEqPreset(String),
    ScrobbleToggled(bool),
    ScrobbleUserChanged(String),
    ScrobbleLbTokenChanged(String),
    ScrobbleFmKeyChanged(String),
    ScrobbleFmSecretChanged(String),
    ScrobbleFmSkChanged(String),
    ScrobbleSubmitted {
        target: String,
        result: Result<(), String>,
    },
    PlayWeeklyMix,
    PlayForgotten,
    SearchCompleted {
        query: String,
        result: Result<SearchResults, String>,
    },
    RadioReady {
        seed: String,
        result: Result<Vec<Track>, String>,
    },
    QueueJump(usize),
    ToggleShuffle,
    CycleRepeat,
    Tick(std::time::Instant),
    SeekTo(f32),
    NavBack,
    NavForward,
    SetRightTab(RightTab),
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
    /// Remove one occurrence from the manual queue (menu / hover ✕).
    RemoveFromQueue(String),
    /// Drop an autoplay-tail item by index.
    RemoveAutoplay(usize),
    /// Queue-page hover tracking (reveals the row ✕).
    QueueHover(Option<usize>),
    /// Context menu open/close: right-click (per-row `mouse_area`) or the
    /// per-row `…` button opens it at the last cursor position.
    OpenContext(String),
    CloseContext,
    CursorMoved(iced::Point),
    WindowResized(iced::Size),
    /// Context menu: jump to this track's album/artist detail.
    OpenTrackAlbum(String),
    OpenTrackArtist(String),
    /// Cache the resolved audio bytes for offline listening.
    DownloadTrack(String),
    Downloaded {
        track_id: String,
        result: Result<(), String>,
    },
    ToggleSidebar,
    SidebarFilterChanged(String),
    CookiePathChanged(String),
    CookieImport,
    CookieImported {
        result: Result<(Vec<Track>, Vec<String>), String>,
    },
    HomeSearchChanged(String),
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
    #[cfg(feature = "innertube")]
    LibraryImported {
        result: Result<(Vec<Track>, Vec<String>), String>,
    },
    StreamResolved {
        video_id: String,
        result: Result<Vec<u8>, String>,
    },
}

/// Playback sink: mpv when the `mpv` feature is on and libmpv initializes
/// (real seek + position + audible EQ), otherwise rodio (pure Rust).
/// The UI only talks to this enum — never to a concrete backend.
enum AudioOut {
    Rodio(RodioBackend),
    #[cfg(feature = "mpv")]
    Mpv(MpvBackend),
}

impl AudioOut {
    fn try_new() -> Option<Self> {
        #[cfg(feature = "mpv")]
        if let Ok(mpv) = MpvBackend::new() {
            return Some(Self::Mpv(mpv));
        }
        RodioBackend::try_new().ok().map(Self::Rodio)
    }

    /// True when this backend plays a URL progressively (mpv). Rodio needs
    /// full bytes, so the UI downloads first on that path.
    fn wants_stream(&self) -> bool {
        match self {
            Self::Rodio(_) => false,
            #[cfg(feature = "mpv")]
            Self::Mpv(_) => true,
        }
    }

    /// Hand mpv a stream URL (no download). Rodio cannot do this.
    fn play_stream_url(&self, url: &str) -> Result<(), String> {
        let _ = url;
        match self {
            Self::Rodio(_) => Err("progressive streaming needs the mpv backend".into()),
            #[cfg(feature = "mpv")]
            Self::Mpv(a) => a
                .load_url(url)
                .and_then(|()| a.play())
                .map_err(|e| e.to_string()),
        }
    }

    fn backend_name(&self) -> &'static str {
        match self {
            Self::Rodio(_) => "rodio",
            #[cfg(feature = "mpv")]
            Self::Mpv(_) => "mpv",
        }
    }

    /// Play fully-downloaded bytes (resolve + fetch run in Iced tasks).
    fn play_track_bytes(&self, bytes: Vec<u8>) -> Result<(), String> {
        match self {
            Self::Rodio(a) => a.play_bytes(bytes).map_err(|e| e.to_string()),
            #[cfg(feature = "mpv")]
            Self::Mpv(a) => a
                .load_bytes(&bytes)
                .and_then(|()| a.play())
                .map_err(|e| e.to_string()),
        }
    }

    fn pause(&self) {
        match self {
            Self::Rodio(a) => a.pause(),
            #[cfg(feature = "mpv")]
            Self::Mpv(a) => {
                let _ = a.pause();
            }
        }
    }

    fn resume(&self) {
        match self {
            Self::Rodio(a) => a.resume(),
            #[cfg(feature = "mpv")]
            Self::Mpv(a) => {
                let _ = a.play();
            }
        }
    }

    fn stop(&self) {
        match self {
            Self::Rodio(a) => a.stop(),
            #[cfg(feature = "mpv")]
            Self::Mpv(a) => {
                let _ = a.stop();
            }
        }
    }

    fn set_volume(&self, volume: f32) {
        match self {
            Self::Rodio(a) => a.set_volume(volume),
            #[cfg(feature = "mpv")]
            Self::Mpv(a) => {
                let _ = a.set_volume(volume);
            }
        }
    }

    fn position_secs(&self) -> f64 {
        match self {
            Self::Rodio(a) => a.position_secs(),
            #[cfg(feature = "mpv")]
            Self::Mpv(a) => a.position_secs(),
        }
    }

    /// Backend duration when it knows better (mpv), else `fallback`.
    fn duration_secs(&self, fallback: f64) -> f64 {
        #[cfg(feature = "mpv")]
        if let Self::Mpv(a) = self {
            let d = a.duration_secs();
            if d > 1.0 {
                return d;
            }
        }
        fallback
    }

    /// Draggable seek: real on mpv, unsupported on rodio.
    fn seek_secs(&self, secs: f64) -> Result<(), String> {
        match self {
            Self::Rodio(a) => a.seek_secs(secs).map_err(|e| e.to_string()),
            #[cfg(feature = "mpv")]
            Self::Mpv(a) => a.seek_secs(secs).map_err(|e| e.to_string()),
        }
    }

    /// Rodio decodes to raw PCM with no filter stage: EQ is mpv-only.
    /// Callers report this honestly instead of pretending it applied.
    fn supports_eq(&self) -> bool {
        match self {
            Self::Rodio(_) => false,
            #[cfg(feature = "mpv")]
            Self::Mpv(_) => true,
        }
    }

    fn apply_eq(&self, preset: &EqPreset) -> Result<(), String> {
        #[cfg(not(feature = "mpv"))]
        let _ = preset;
        match self {
            Self::Rodio(_) => Err("EQ needs the mpv backend (libmpv)".into()),
            #[cfg(feature = "mpv")]
            Self::Mpv(a) => a.apply_eq_preset(preset).map_err(|e| e.to_string()),
        }
    }
}

/// Parse a googlevideo `expire` query param (`…&expire=1791411186&…`) into a
/// unix timestamp. Unknown shapes → None (caller re-resolves every time).
fn url_expiry(url: &str) -> Option<u64> {
    url.split(['?', '&'])
        .find_map(|p| p.strip_prefix("expire=").and_then(|v| v.parse().ok()))
}

/// Context-menu origin from the last cursor position, clamped so the
/// 300px menu never runs off screen. Falls back to a sane default when
/// the cursor/window are still unknown (e.g. keyboard-opened).
fn menu_pos(cursor: Option<iced::Point>, window: Option<iced::Size>) -> (f32, f32) {
    const MENU_W: f32 = 300.0;
    const MENU_H: f32 = 470.0;
    let (cx, cy) = cursor.map(|p| (p.x, p.y)).unwrap_or((200.0, 150.0));
    let (ww, hh) = window
        .map(|s| (s.width, s.height))
        .unwrap_or((1280.0, 800.0));
    (
        cx.min((ww - MENU_W).max(0.0)).max(0.0),
        cy.min((hh - MENU_H).max(0.0)).max(0.0),
    )
}

/// Format seconds as `m:ss` for the player bar.
fn fmt_time(secs: f64) -> String {
    let total = secs.max(0.0) as u64;
    format!("{}:{:02}", total / 60, total % 60)
}

pub struct AppState {
    pub route: Route,
    pub search_query: String,
    pub results: SearchResults,
    pub queue: Queue,
    pub liked: HashSet<String>,
    pub history: Vec<String>,
    pub volume: f32,
    pub status: String,
    pub user_playlists: Vec<Playlist>,
    pub new_playlist_name: String,
    pub playlist_seq: u64,
    /// Selected EQ preset name (in-memory only; never persisted).
    pub eq_preset: String,
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
    paused: bool,
    audio: Option<AudioOut>,
    /// Last Tick-observed playback position (drives the progress bar).
    position_secs: f64,
    /// Track id currently being fetched (stale arrivals are dropped).
    loading_track: Option<String>,
    /// Resolved stream URLs by track id: (url, expire_unix). Fresh hits
    /// play instantly — back/prev/replay never re-resolve.
    url_cache: HashMap<String, (String, u64)>,
    /// Next song's stream, resolved early (played instantly on advance).
    prefetched: Option<(String, String)>,
    /// Lyrics by track id (LRCLIB, no login) + in-flight fetch.
    lyrics_cache: HashMap<String, LyricsResult>,
    lyrics_loading: Option<String>,
    /// Browser-style back/forward stacks (Route history, capped).
    nav_back: Vec<Route>,
    nav_forward: Vec<Route>,
    /// Right-panel tab (view-only state).
    right_tab: RightTab,
    /// Suggestion popup: texts + generation + visibility.
    suggest: Vec<String>,
    suggest_gen: u64,
    show_suggest: bool,
    /// Context menu open for this track id (None = closed).
    context_menu: Option<String>,
    /// Cursor position frozen when the menu opened (the menu must not
    /// chase the mouse while the user aims at it).
    menu_at: Option<iced::Point>,
    /// Last cursor position (window coords — anchors the context menu).
    cursor: Option<iced::Point>,
    /// Last window size (clamps the context menu on screen).
    window: Option<iced::Size>,
    /// Sidebar expanded (false = icon rail).
    sidebar_expanded: bool,
    /// Playlist filter text in the sidebar.
    sidebar_filter: String,
    /// Autoplay tail belongs to this seed (stale arrivals drop).
    autoplay_for: Option<String>,
    /// Spotify-style split: manual queue (user picks) plays first, then
    /// this radio-picked tail ("Next up"). Never persisted.
    autoplay: Vec<Track>,
    /// Queue-page row under the cursor (hover reveals its ✕).
    hovered_queue: Option<usize>,
    /// Recent playback/fetch failures (capped, surfaced in Settings).
    error_log: Vec<String>,
    /// Manual position clock deadline after a user seek (anti-flicker).
    seek_hold_until: Option<std::time::Instant>,
    /// Last Tick instant (drives the manual clock during a seek hold).
    last_tick: Option<std::time::Instant>,
    /// Cookie-file path for import (Settings).
    cookie_path: String,
    /// Where the queue is playing from (playlist/artist/radio label).
    playing_from: String,
    /// Artwork by thumbnail URL (iced image handles) + in-flight set.
    artwork: HashMap<String, iced::widget::image::Handle>,
    artwork_loading: HashSet<String>,
    /// Track id a prefetch was already attempted for (no Tick spam).
    prefetch_attempted: Option<String>,
    /// Google device-flow login in progress (cloned into poll tasks).
    #[cfg(feature = "innertube")]
    google_login: Option<yutu_core::GoogleLogin>,
    /// A saved OAuth token exists on disk (signed in).
    #[cfg(feature = "innertube")]
    logged_in: bool,
    /// A login/library network task is in flight.
    #[cfg(feature = "innertube")]
    login_busy: bool,
    /// End-of-track auto-advance already fired for the current song.
    end_advanced: bool,
    library_path: String,
    /// Real-track catalog: every track the app has seen (search, radio,
    /// playlists). ID lookups resolve here — never fixtures.
    catalog: HashMap<String, Track>,
    /// Last complete real search (local filtering base for `SearchChanged`).
    search_base: SearchResults,
    /// A real search is in flight (loading indicator).
    searching: bool,
    /// Keystroke generation: each input bumps it; only the latest
    /// debounced task may fire (no search spam while typing).
    search_gen: u64,
    /// Current radio sequence (filled by `RadioReady`).
    radio_tracks: Vec<Track>,
    /// MPRIS handle for live desktop propagation (`None` in tests/headless).
    mpris: Option<yutu_desktop::SharedPlayer>,
}

/// Deterministic radio ordering: seed first, then same-artist tracks, then
/// the rest. Pure helper over caller-supplied (real) tracks.
pub fn order_radio(seed_id: &str, tracks: &[Track]) -> Vec<Track> {
    order_radio_impl(seed_id, tracks)
}

fn order_radio_impl(seed_id: &str, tracks: &[Track]) -> Vec<Track> {
    let start = tracks.iter().position(|t| t.id == seed_id).unwrap_or(0);
    let seed_artist = tracks
        .get(start)
        .map(|t| t.artist.clone())
        .unwrap_or_default();
    let mut seq = Vec::with_capacity(tracks.len());
    let seed_actual = tracks.get(start).map(|t| t.id.clone()).unwrap_or_default();
    if let Some(t) = tracks.get(start) {
        seq.push(t.clone());
    }
    for t in tracks {
        if t.id != seed_actual && t.artist == seed_artist {
            seq.push(t.clone());
        }
    }
    for t in tracks {
        if t.id != seed_actual && t.artist != seed_artist {
            seq.push(t.clone());
        }
    }
    seq
}

/// Fixture-backed radio for unit tests (never user-visible).
#[cfg(test)]
fn radio_for(seed_id: &str) -> Vec<Track> {
    order_radio(seed_id, &yutu_core::MockBackend::fixture_tracks())
}

impl AppState {
    /// Resolve a track id from the real catalog (search/radio/playlist data).
    fn find_track(&self, id: &str) -> Option<Track> {
        self.catalog.get(id).cloned()
    }

    /// Register freshly fetched tracks; returns them for chaining.
    fn register_tracks(&mut self, tracks: Vec<Track>) -> Vec<Track> {
        for t in &tracks {
            self.catalog.insert(t.id.clone(), t.clone());
        }
        tracks
    }

    /// Merge an imported library: catalog + likes (memory + SQLite) +
    /// artwork tasks for the new tracks.
    fn merge_imported(
        &mut self,
        tracks: Vec<Track>,
        liked_ids: Vec<String>,
    ) -> Vec<iced::Task<Message>> {
        let tracks = self.register_tracks(tracks);
        for id in &liked_ids {
            self.liked.insert(id.clone());
        }
        if let Ok(store) = LibraryStore::open(&self.library_path) {
            for id in &liked_ids {
                let _ = store.like(id);
            }
        }
        self.artwork_tasks(&tracks)
    }

    /// Everything the app has really seen (discovery candidate pool).
    fn catalog_tracks(&self) -> Vec<Track> {
        let mut v: Vec<Track> = self.catalog.values().cloned().collect();
        v.sort_by(|a, b| a.title.cmp(&b.title));
        v
    }
}

/// Case-insensitive substring filter over a full search result set.
fn filter_results(base: &SearchResults, query: &str) -> SearchResults {
    let q = query.to_lowercase();
    if q.trim().is_empty() {
        return SearchResults::default();
    }
    SearchResults {
        tracks: base
            .tracks
            .iter()
            .filter(|t| t.title.to_lowercase().contains(&q) || t.artist.to_lowercase().contains(&q))
            .cloned()
            .collect(),
        albums: base
            .albums
            .iter()
            .filter(|a| a.title.to_lowercase().contains(&q) || a.artist.to_lowercase().contains(&q))
            .cloned()
            .collect(),
        artists: base
            .artists
            .iter()
            .filter(|a| a.name.to_lowercase().contains(&q))
            .cloned()
            .collect(),
    }
}

/// Offline builds: only the yt-dlp path exists; without the binary installed
/// there is nothing else to try.
#[cfg(not(feature = "innertube"))]
async fn resolve_via_raw(video_id: &str) -> Result<Vec<u8>, String> {
    Err(format!(
        "yt-dlp not found and this is an offline build ({video_id}). Install yt-dlp or rebuild with `--features innertube`."
    ))
}

/// yt-dlp `--get-url` only (no download): the fast path behind instant
/// playback. Failures name the track; the caller decides bytes-vs-stream.
async fn resolve_stream_url(video_id: &str) -> Result<String, String> {
    let bin = yutu_core::find_yt_dlp()
        .ok_or_else(|| "yt-dlp not installed — install it to play music.".to_string())?;
    yutu_core::resolve_audio_url(&bin, video_id).map_err(|e| e.to_string())
}

/// Real search over the live Innertube API: tracks, albums and artists.
/// Each category fails open (empty vec) so one bad query cannot sink the
/// other two; only a failed *connection* or a triple failure errors.
#[cfg(feature = "innertube")]
async fn live_search(query: &str) -> Result<SearchResults, String> {
    use yutu_core::InnertubeBackend;
    let backend = InnertubeBackend::connect_anonymous().await?;
    let tracks = backend.search_tracks(query).await;
    let albums = backend.search_albums(query).await;
    let artists = backend.search_artists(query).await;
    match (tracks, albums, artists) {
        (Err(e), Err(_), Err(_)) => Err(e),
        (tracks, albums, artists) => Ok(SearchResults {
            tracks: tracks.unwrap_or_default(),
            albums: albums.unwrap_or_default(),
            artists: artists.unwrap_or_default(),
        }),
    }
}

/// Radio sequence for a seed: the seed first, same-artist tracks next,
/// everything else after.
#[cfg(feature = "innertube")]
async fn live_radio(seed: &str, artist: &str) -> Result<Vec<Track>, String> {
    use yutu_core::InnertubeBackend;
    let backend = InnertubeBackend::connect_anonymous().await?;
    let found = backend.search_tracks(artist).await?;
    let mut seq: Vec<Track> = Vec::with_capacity(found.len());
    if let Some(t) = found.iter().find(|t| t.id == seed).cloned() {
        seq.push(t);
    }
    for t in &found {
        if t.id != seed && t.artist == artist && !seq.iter().any(|x| x.id == t.id) {
            seq.push(t.clone());
        }
    }
    for t in &found {
        if t.id != seed && !seq.iter().any(|x| x.id == t.id) {
            seq.push(t.clone());
        }
    }
    Ok(seq)
}

/// Resolve a video ID to bytes. yt-dlp goes first (maintained decipher, no
/// feature gate, license-clean); then the raw WEB_REMIX probes with sidecar
/// mint + retry on PO-gating. A bare-URL 403 retries once with the session
/// token as `&pot=`. The yt-dlp leg needs no feature; the raw probes do.
async fn resolve_and_fetch(video_id: &str) -> Result<Vec<u8>, String> {
    if let Some(bin) = yutu_core::find_yt_dlp() {
        match yutu_core::resolve_audio_url(&bin, video_id) {
            Ok(url) => {
                return yutu_player::fetch_bytes(&url)
                    .await
                    .map_err(|e| e.to_string());
            }
            Err(e) => {
                eprintln!("[yutu-stream] yt-dlp path failed ({e}); trying raw probes");
            }
        }
    }
    resolve_via_raw(video_id).await
}

/// Raw-probe path (needs the `innertube` feature): anonymous connect,
/// fallback chain, sidecar mint + `poToken` retry on PO-gating.
#[cfg(feature = "innertube")]
async fn resolve_via_raw(video_id: &str) -> Result<Vec<u8>, String> {
    use yutu_core::{InnertubeBackend, StreamError, StreamRequest};
    let backend = InnertubeBackend::connect_anonymous().await?;
    let mut req = StreamRequest::new(video_id);
    match backend.resolve_stream(&mut req).await {
        Ok(url) => yutu_player::fetch_bytes(&url)
            .await
            .map_err(|e| e.to_string()),
        Err(StreamError::PoTokenRequired { .. }) => {
            let Some(bin) = yutu_core::find_botguard_bin() else {
                return Err("PO token required and rustypipe-botguard is not installed.".into());
            };
            let mut retry = StreamRequest::new(video_id);
            let resolved = backend
                .resolve_stream_with_sidecar(&mut retry, &bin)
                .await
                .map_err(|e| format!("{e} — {}", e.hint()))?;
            match yutu_player::fetch_bytes(&resolved.url).await {
                Ok(bytes) => Ok(bytes),
                Err(first) => match resolved.pot_token {
                    Some(pot) => {
                        let sep = if resolved.url.contains('?') { '&' } else { '?' };
                        let url = format!("{}{}pot={}", resolved.url, sep, pot);
                        yutu_player::fetch_bytes(&url)
                            .await
                            .map_err(|e| e.to_string())
                    }
                    None => Err(first.to_string()),
                },
            }
        }
        Err(e) => Err(format!("{e} — {}", e.hint())),
    }
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// XDG data dir without extra deps: $XDG_DATA_HOME or ~/.local/share.
pub fn data_dir() -> String {
    let base = std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        format!("{home}/.local/share")
    });
    format!("{base}/yutu-mux")
}

pub fn default_library_path() -> String {
    format!("{}/library.db", data_dir())
}

/// Saved Google OAuth token bundle (0600, written by the login flow).
#[cfg(feature = "innertube")]
pub fn oauth_path() -> String {
    format!("{}/youtube_oauth.json", data_dir())
}

#[cfg(feature = "innertube")]
fn oauth_path_exists() -> bool {
    #[cfg(feature = "innertube")]
    {
        std::path::Path::new(&oauth_path()).exists()
    }
    #[cfg(not(feature = "innertube"))]
    {
        false
    }
}

fn load_persisted(path: &str) -> (HashSet<String>, Vec<String>, Vec<Playlist>, u64) {
    let empty = (HashSet::new(), Vec::new(), Vec::new(), 0);
    let store = match LibraryStore::open(path) {
        Ok(s) => s,
        Err(_) => return empty,
    };
    // Likes: every id in SQLite — no catalog probe, no fixtures.
    let liked: HashSet<String> = store.liked_ids().unwrap_or_default().into_iter().collect();
    let history = store
        .history(50)
        .unwrap_or_default()
        .into_iter()
        .map(|e| e.track_id)
        .collect();
    let user_playlists = store.user_playlists().unwrap_or_default();
    let seq = user_playlists
        .iter()
        .filter_map(|p| p.id.strip_prefix('u')?.parse::<u64>().ok())
        .max()
        .unwrap_or(0);
    (liked, history, user_playlists, seq)
}

impl AppState {
    pub fn new() -> (Self, iced::Task<Message>) {
        Self::with_library_path(default_library_path())
    }

    pub fn with_library_path(library_path: String) -> (Self, iced::Task<Message>) {
        let audio = match AudioOut::try_new() {
            Some(a) => {
                eprintln!("audio backend: {}", a.backend_name());
                Some(a)
            }
            None => {
                eprintln!("audio unavailable: no output device");
                None
            }
        };
        if let Some(parent) = std::path::Path::new(&library_path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let (liked, history, user_playlists, playlist_seq) = load_persisted(&library_path);
        (
            Self {
                route: Route::Home,
                search_query: String::new(),
                results: SearchResults::default(),
                queue: Queue::default(),
                liked,
                history,
                volume: 0.8,
                status: "Ready — search for music.".into(),
                user_playlists,
                new_playlist_name: String::new(),
                playlist_seq,
                eq_preset: "Flat".into(),
                scrobble_enabled: false,
                scrobble_user: String::new(),
                scrobble_lb_token: String::new(),
                scrobble_fm_key: String::new(),
                scrobble_fm_secret: String::new(),
                scrobble_fm_sk: String::new(),
                paused: false,
                audio,
                position_secs: 0.0,
                loading_track: None,
                url_cache: HashMap::new(),
                prefetched: None,
                lyrics_cache: HashMap::new(),
                lyrics_loading: None,
                nav_back: Vec::new(),
                nav_forward: Vec::new(),
                right_tab: RightTab::Queue,
                suggest: Vec::new(),
                suggest_gen: 0,
                show_suggest: false,
                context_menu: None,
                menu_at: None,
                cursor: None,
                window: None,
                sidebar_expanded: true,
                sidebar_filter: String::new(),
                autoplay_for: None,
                autoplay: Vec::new(),
                hovered_queue: None,
                error_log: Vec::new(),
                seek_hold_until: None,
                last_tick: None,
                cookie_path: String::new(),
                playing_from: String::new(),
                artwork: HashMap::new(),
                artwork_loading: HashSet::new(),
                prefetch_attempted: None,
                #[cfg(feature = "innertube")]
                google_login: None,
                #[cfg(feature = "innertube")]
                logged_in: oauth_path_exists(),
                #[cfg(feature = "innertube")]
                login_busy: false,
                end_advanced: false,
                library_path,
                catalog: HashMap::new(),
                search_base: SearchResults::default(),
                searching: false,
                search_gen: 0,
                radio_tracks: Vec::new(),
                mpris: None,
            },
            iced::Task::none(),
        )
    }

    /// Browser-style navigation: records back history, clears forward.
    pub fn navigate(&mut self, route: Route) {
        self.go(route);
    }

    fn go(&mut self, route: Route) {
        if route != self.route {
            self.nav_back.push(self.route.clone());
            if self.nav_back.len() > 50 {
                self.nav_back.remove(0);
            }
            self.nav_forward.clear();
        }
        self.route = route;
    }

    fn go_back(&mut self) {
        if let Some(prev) = self.nav_back.pop() {
            self.nav_forward.push(self.route.clone());
            self.route = prev;
        }
    }

    fn go_forward(&mut self) {
        if let Some(next) = self.nav_forward.pop() {
            self.nav_back.push(self.route.clone());
            self.route = next;
        }
    }

    pub fn subscription(&self) -> iced::Subscription<Message> {
        let keys = iced::keyboard::listen().filter_map(|event| match event {
            iced::keyboard::Event::KeyPressed { key, .. } => match key.as_ref() {
                iced::keyboard::Key::Named(iced::keyboard::key::Named::Space) => {
                    Some(Message::TogglePause)
                }
                iced::keyboard::Key::Character(" ") => Some(Message::TogglePause),
                iced::keyboard::Key::Character("/") => Some(Message::Navigate(Route::Search)),
                iced::keyboard::Key::Character("n") => Some(Message::Next),
                iced::keyboard::Key::Character("p") => Some(Message::Prev),
                iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape) => {
                    Some(Message::CloseContext)
                }
                _ => None,
            },
            _ => None,
        });
        let tick = iced::time::every(std::time::Duration::from_millis(500)).map(Message::Tick);
        let events = iced::event::listen().filter_map(|event| match event {
            iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                Some(Message::CursorMoved(position))
            }
            iced::Event::Window(iced::window::Event::Resized(size)) => {
                Some(Message::WindowResized(size))
            }
            _ => None,
        });
        iced::Subscription::batch([keys, tick, events])
    }

    fn persist_like(&self, id: &str, liked: bool) {
        let Ok(store) = LibraryStore::open(&self.library_path) else {
            return;
        };
        let _ = if liked {
            store.like(id)
        } else {
            store.unlike(id)
        };
    }

    fn persist_play(&self, id: &str) {
        let Ok(store) = LibraryStore::open(&self.library_path) else {
            return;
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let _ = store.record_play(id, now);
    }

    fn persist_playlist_created(&self, id: &str, title: &str) {
        let Ok(store) = LibraryStore::open(&self.library_path) else {
            return;
        };
        let _ = store.create_playlist(id, title);
    }

    fn persist_playlist_deleted(&self, id: &str) {
        let Ok(store) = LibraryStore::open(&self.library_path) else {
            return;
        };
        let _ = store.delete_playlist(id);
    }

    fn persist_playlist_added(&self, playlist_id: &str, track: &Track) {
        let Ok(store) = LibraryStore::open(&self.library_path) else {
            return;
        };
        let _ = store.add_to_playlist(playlist_id, track);
    }

    fn persist_playlist_removed(&self, playlist_id: &str, track_id: &str) {
        let Ok(store) = LibraryStore::open(&self.library_path) else {
            return;
        };
        let _ = store.remove_from_playlist(playlist_id, track_id);
    }

    /// Click-to-play: queue the track, jump to it, and fetch real audio.
    /// The fetch task completes in `StreamResolved`; the tone is gone.
    fn enqueue(&mut self, track: Track) -> iced::Task<Message> {
        self.playing_from = "Single".into();
        self.history.push(track.id.clone());
        self.persist_play(&track.id);
        self.queue.push(track);
        self.queue.jump_to(self.queue.len().saturating_sub(1));
        self.apply_volume();
        let play = self.play_current_task();
        let just_queued = self.queue.current().cloned();
        let scrobble = just_queued
            .map(|t| self.maybe_scrobble_task(&t))
            .unwrap_or_else(iced::Task::none);
        if self.audio.is_none() {
            self.status.push_str(" (no audio device)");
            return scrobble;
        }
        iced::Task::batch([scrobble, play])
    }

    /// Play whatever is current (no-op on an empty queue). Fast paths first:
    /// a prefetched URL or a fresh cached URL plays instantly with no
    /// download; otherwise a resolve task runs (mpv streams the URL, rodio
    /// downloads the bytes). Sets `loading_track` so stale arrivals drop.
    fn play_current_task(&mut self) -> iced::Task<Message> {
        let Some(t) = self.queue.current().cloned() else {
            return iced::Task::none();
        };
        // Immediate feedback: silence the old song the instant a new load
        // starts (resolving takes ~1s; the old track must not play over it).
        if let Some(audio) = &self.audio {
            audio.stop();
        }
        if self.prefetched.as_ref().is_some_and(|(id, _)| id != &t.id) {
            self.prefetched = None;
        }
        self.prefetch_attempted = None;
        self.loading_track = Some(t.id.clone());
        self.end_advanced = false;
        self.paused = false;
        self.position_secs = 0.0;
        // Lyrics page follows playback: swap the id in place (no back-stack
        // spam) and fetch the new song's lyrics unless cached.
        let mut follow_tasks: Vec<iced::Task<Message>> = Vec::new();
        if matches!(self.route, Route::Lyrics(_)) && self.route != Route::Lyrics(t.id.clone()) {
            self.route = Route::Lyrics(t.id.clone());
            #[cfg(feature = "lyrics")]
            {
                if !self.lyrics_cache.contains_key(&t.id)
                    && self.lyrics_loading.as_deref() != Some(t.id.as_str())
                {
                    self.lyrics_loading = Some(t.id.clone());
                    follow_tasks.push(Self::lyrics_task(t.clone()));
                }
            }
        }
        // Prefetched URL: instant.
        if let Some((_, url)) = self.prefetched.take() {
            if self.try_play_url(&url, &t) {
                return iced::Task::batch(follow_tasks);
            }
        }
        // Fresh cached URL: instant (back/prev/replay never re-resolve).
        if let Some((url, expires)) = self.url_cache.get(&t.id).cloned() {
            if expires > now_unix() as u64 + 60 && self.try_play_url(&url, &t) {
                return iced::Task::batch(follow_tasks);
            }
        }
        self.status = format!("Loading {} — {}…", t.title, t.artist);
        let audio_task = match &self.audio {
            Some(a) if a.wants_stream() => Self::resolve_url_task(t.id),
            _ => Self::resolve_task(t.id),
        };
        follow_tasks.push(audio_task);
        iced::Task::batch(follow_tasks)
    }

    /// Best-effort instant play of a stream URL. True on success.
    fn try_play_url(&mut self, url: &str, t: &Track) -> bool {
        match &self.audio {
            Some(audio) => match audio.play_stream_url(url) {
                Ok(()) => {
                    self.loading_track = None;
                    self.status = format!("♪ {} — {}.", t.title, t.artist);
                    true
                }
                Err(e) => {
                    let msg = format!("Playback failed for {}: {e}", t.id);
                    self.log_error(msg.clone());
                    self.status = msg;
                    false
                }
            },
            None => {
                self.status = "Stream ready, but no audio device.".into();
                false
            }
        }
    }

    /// Id of the likely next track (repeat-all wraps), for prefetching.
    fn peek_next_id(&self) -> Option<String> {
        let items = self.queue.items();
        if items.is_empty() {
            return None;
        }
        let pos = self.queue.position().unwrap_or(0);
        if pos + 1 < items.len() {
            Some(items[pos + 1].id.clone())
        } else if self.queue.repeat == yutu_player::RepeatMode::All {
            items.first().map(|t| t.id.clone())
        } else {
            self.autoplay.first().map(|t| t.id.clone())
        }
    }

    /// Same fetch without touching the status line (bulk actions keep theirs).
    fn fetch_current_task(&mut self) -> iced::Task<Message> {
        match self.queue.current().cloned() {
            None => iced::Task::none(),
            Some(t) => {
                // Same cut-the-old-song feedback as the streaming path.
                if let Some(audio) = &self.audio {
                    audio.stop();
                }
                self.loading_track = Some(t.id.clone());
                self.end_advanced = false;
                self.paused = false;
                self.position_secs = 0.0;
                Self::resolve_task(t.id)
            }
        }
    }

    /// Manual queue first (shuffle/repeat apply there); when it truly ends
    /// and repeat is Off, the autoplay tail shifts in head-first so the
    /// single current-pointer (progress/lyrics/MPRIS) keeps working.
    /// True when current changed.
    fn advance_queue(&mut self) -> bool {
        if self.queue.shuffle && self.queue.len() > 1 {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos() as usize)
                .unwrap_or(0);
            let mut i = now % self.queue.len();
            if Some(i) == self.queue.position() {
                i = (i + 1) % self.queue.len();
            }
            return self.queue.jump_to(i);
        }
        if self.queue.next_track().is_some() {
            return true;
        }
        // Manual queue truly ended: autoplay tail (repeat All/One loop the
        // manual queue by design, like Spotify's repeat).
        if self.queue.repeat == yutu_player::RepeatMode::Off && !self.autoplay.is_empty() {
            let t = self.autoplay.remove(0);
            self.history.push(t.id.clone());
            self.persist_play(&t.id);
            self.queue.push(t);
            self.queue.jump_to(self.queue.len().saturating_sub(1));
            return true;
        }
        false
    }

    fn apply_volume(&self) {
        if let Some(a) = &self.audio {
            a.set_volume(self.volume);
        }
    }

    /// Backend clock (drives the progress bar + MPRIS position).
    fn audio_position(&self) -> f64 {
        self.audio
            .as_ref()
            .map(|a| a.position_secs())
            .unwrap_or(0.0)
            .max(0.0)
    }

    /// Backend duration when it knows better, else the track's metadata.
    fn audio_duration(&self) -> f64 {
        let fallback = self
            .queue
            .current()
            .map(|t| t.duration_secs as f64)
            .unwrap_or(0.0);
        self.audio
            .as_ref()
            .map(|a| a.duration_secs(fallback))
            .unwrap_or(fallback)
            .max(0.0)
    }

    /// Build the scrobble-submit task for a just-queued full play, if
    /// scrobbling is enabled, the play is eligible, and at least one target
    /// has credentials. Without the `submit` feature this is always none.
    fn maybe_scrobble_task(&mut self, track: &Track) -> iced::Task<Message> {
        if !(self.scrobble_enabled && should_scrobble(track.duration_secs, track.duration_secs)) {
            return iced::Task::none();
        }
        #[cfg(not(feature = "submit"))]
        {
            let _ = track;
            self.status_hint_no_scrobble_creds();
            iced::Task::none()
        }
        #[cfg(feature = "submit")]
        {
            use yutu_core::{build_listen_payload, build_scrobble_params, Scrobble};
            let now = now_unix();
            let mut tasks = Vec::new();
            if !self.scrobble_lb_token.trim().is_empty() {
                let payload =
                    build_listen_payload(&track.artist, &track.title, track.album.as_deref(), now);
                let token = self.scrobble_lb_token.clone();
                tasks.push(iced::Task::perform(
                    async move { yutu_core::submit_listenbrainz_listen(&payload, &token).await },
                    |result| Message::ScrobbleSubmitted {
                        target: "ListenBrainz".into(),
                        result,
                    },
                ));
            }
            if !(self.scrobble_fm_key.trim().is_empty()
                || self.scrobble_fm_secret.trim().is_empty()
                || self.scrobble_fm_sk.trim().is_empty())
            {
                let scrobble =
                    Scrobble::new(&track.artist, &track.title, now, track.album.as_deref());
                let params = build_scrobble_params(
                    &scrobble,
                    &self.scrobble_fm_key,
                    &self.scrobble_fm_sk,
                    &self.scrobble_fm_secret,
                );
                tasks.push(iced::Task::perform(
                    async move { yutu_core::submit_lastfm_scrobble(&params).await },
                    |result| Message::ScrobbleSubmitted {
                        target: "Last.fm".into(),
                        result,
                    },
                ));
            }
            if tasks.is_empty() {
                self.status_hint_no_scrobble_creds();
                iced::Task::none()
            } else {
                iced::Task::batch(tasks)
            }
        }
    }

    #[cfg(feature = "submit")]
    fn status_hint_no_scrobble_creds(&mut self) {
        self.status
            .push_str(" | Scrobble-ready (add a ListenBrainz token or Last.fm keys in Settings)");
    }

    #[cfg(not(feature = "submit"))]
    fn status_hint_no_scrobble_creds(&mut self) {
        self.status
            .push_str(" | Scrobble-ready (rebuild with `--features submit`)");
    }

    /// Bulk-append without spamming history (one entry-free push per track),
    /// then play the first new track for real.
    fn queue_many(&mut self, tracks: Vec<Track>, label: &str) -> iced::Task<Message> {
        let n = tracks.len();
        if n == 0 {
            self.status = format!("{label}: nothing to queue.");
            return iced::Task::none();
        }
        let first_new = self.queue.len();
        for t in tracks {
            self.queue.push(t);
        }
        self.queue.jump_to(first_new);
        self.playing_from = label.to_string();
        self.status = format!("{label}: queued {n} track(s).");
        self.apply_volume();
        self.fetch_current_task()
    }

    fn play_index(&mut self, i: usize) -> iced::Task<Message> {
        match self.results.tracks.get(i).cloned() {
            Some(t) => self.enqueue(t),
            None => {
                self.status = format!("No track at index {i}.");
                iced::Task::none()
            }
        }
    }

    fn enqueue_track_task(&mut self, id: &str) -> iced::Task<Message> {
        match self.find_track(id) {
            Some(t) => self.enqueue(t),
            None => {
                self.status = format!("Track not found: {id}.");
                iced::Task::none()
            }
        }
    }

    /// Fetch missing artwork for freshly seen tracks (cap: 12 per payload,
    /// deduped by URL, never refetched once attempted).
    fn artwork_tasks(&mut self, tracks: &[Track]) -> Vec<iced::Task<Message>> {
        let mut tasks = Vec::new();
        for t in tracks {
            if let Some(url) = t.thumbnail_url.as_deref() {
                if !self.artwork.contains_key(url)
                    && !self.artwork_loading.contains(url)
                    && tasks.len() < 12
                {
                    self.artwork_loading.insert(url.to_string());
                    let url_owned = url.to_string();
                    let url_for_msg = url_owned.clone();
                    tasks.push(iced::Task::perform(
                        async move { yutu_player::fetch_bytes(&url_owned).await },
                        move |result| Message::ArtworkReady {
                            url: url_for_msg.clone(),
                            result: result.map_err(|e| e.to_string()),
                        },
                    ));
                }
            }
        }
        tasks
    }

    /// 40px artwork for rows/panel, ♪ placeholder until loaded.
    fn thumb(&self, track: &Track) -> iced::Element<'_, Message> {
        self.thumb_sized(track, 40.0)
    }

    /// Settings login panel: states are signed-in, code-pending, idle.
    /// The URL is also printed to stderr (clickable in the terminal).
    #[cfg(feature = "innertube")]
    fn google_section(&self) -> iced::Element<'_, Message> {
        use iced::widget::{button, column, row, text};
        use iced::Fill;
        if self.logged_in {
            return row![
                text("Signed in ✓").size(13).color(SPOT_GREEN).width(Fill),
                button(
                    text(if self.login_busy {
                        "…"
                    } else {
                        "Import library"
                    })
                    .size(12)
                )
                .style(green_play)
                .on_press(Message::LibraryImport)
                .padding(7),
                button(text("Sign out").size(12))
                    .style(button::text)
                    .on_press(Message::GoogleLogout)
                    .padding(7),
            ]
            .spacing(8)
            .align_y(iced::alignment::Vertical::Center)
            .into();
        }
        if let Some(login) = self.google_login.clone() {
            return column![
                text("1. Open this URL in your browser:")
                    .size(12)
                    .color(MUTED),
                text(login.verification_url.clone()).size(12),
                text(format!("2. Enter the code: {}", login.user_code)).size(13),
                row![
                    button(
                        text(if self.login_busy {
                            "Checking…"
                        } else {
                            "I’ve approved — check"
                        })
                        .size(13)
                    )
                    .style(green_play)
                    .on_press(Message::GoogleLoginPoll)
                    .padding(8),
                    button(text("Cancel").size(12))
                        .style(button::text)
                        .on_press(Message::GoogleLogout)
                        .padding(7),
                ]
                .spacing(8)
                .align_y(iced::alignment::Vertical::Center),
            ]
            .spacing(4)
            .into();
        }
        column![
            text("Your liked songs + library, imported from YouTube Music.")
                .size(12)
                .color(MUTED),
            text("Needs a free Google OAuth client: set YUTU_OAUTH_CLIENT_ID + YUTU_OAUTH_CLIENT_SECRET (same as youtui — register a desktop client).")
                .size(11)
                .color(MUTED),
            button(
                text(if self.login_busy {
                    "Contacting Google…"
                } else {
                    "Sign in with Google"
                })
                .size(13)
            )
            .style(green_play)
            .on_press(Message::GoogleLoginStart)
            .padding(8),
        ]
        .spacing(4)
        .into()
    }

    /// Cookie import panel: normal browser login, no OAuth client needed.
    #[cfg(feature = "innertube")]
    fn cookie_section(&self) -> iced::Element<'_, Message> {
        use iced::widget::{button, column, row, text, text_input};
        use iced::Fill;
        column![
            text("Log into music.youtube.com in your browser, export cookies (e.g. the “Get cookies.txt” extension), then paste the file path — no Google Cloud client required.")
                .size(11)
                .color(MUTED),
            row![
                text_input("/path/to/cookies.txt", &self.cookie_path)
                    .on_input(Message::CookiePathChanged)
                    .width(Fill),
                button(
                    text(if self.login_busy {
                        "…"
                    } else {
                        "Import"
                    })
                    .size(12)
                )
                .style(green_play)
                .on_press(Message::CookieImport)
                .padding(7),
            ]
            .spacing(8)
            .align_y(iced::alignment::Vertical::Center),
        ]
        .spacing(4)
        .into()
    }

    /// In-app error log: the same lines that go to stderr (capped at 30).
    fn error_section(&self) -> iced::Element<'_, Message> {
        use iced::widget::{column, text};
        if self.error_log.is_empty() {
            return text("No errors recorded.").size(12).color(MUTED).into();
        }
        column(
            self.error_log
                .iter()
                .rev()
                .take(10)
                .map(|e| text(e.clone()).size(11).color(MUTED).into())
                .collect::<Vec<_>>(),
        )
        .spacing(2)
        .into()
    }

    /// Offline builds: cookie import needs the innertube feature.
    #[cfg(not(feature = "innertube"))]
    fn cookie_section(&self) -> iced::Element<'_, Message> {
        use iced::widget::text;
        text("Cookie import needs `--features innertube`.")
            .size(12)
            .color(MUTED)
            .into()
    }

    /// Offline builds: login needs the innertube feature.
    #[cfg(not(feature = "innertube"))]
    fn google_section(&self) -> iced::Element<'_, Message> {
        use iced::widget::text;
        text("Google login needs `--features innertube`.")
            .size(12)
            .color(MUTED)
            .into()
    }

    /// Open the context menu, freezing the cursor so the menu stays put.
    fn open_menu(&mut self, id: String) {
        self.menu_at = self.cursor;
        self.context_menu = Some(id);
    }

    /// Close the context menu and drop its anchor.
    fn close_menu(&mut self) {
        self.context_menu = None;
        self.menu_at = None;
    }

    /// Artwork at any square size (home cards use 52, rows use 40).
    fn thumb_sized(&self, track: &Track, size: f32) -> iced::Element<'_, Message> {
        use iced::widget::{container, image, text};
        match track
            .thumbnail_url
            .as_deref()
            .and_then(|u| self.artwork.get(u))
        {
            Some(handle) => image(handle.clone()).width(size).height(size).into(),
            None => container(text("♪").size(16).color(MUTED))
                .style(card_bg)
                .padding(8)
                .into(),
        }
    }

    /// Record a playback/fetch failure for the Settings error log (capped)
    /// and stderr (same line, so journald and the in-app log agree).
    fn log_error(&mut self, msg: String) {
        eprintln!("[yutu-play] {msg}");
        if self.error_log.len() >= 30 {
            self.error_log.remove(0);
        }
        self.error_log.push(msg);
    }

    /// Seek to an absolute position: backend first, manual clock +
    /// anti-flicker hold on success, logged honest error otherwise.
    fn seek_to_secs(&mut self, pos: f64) {
        let pos = pos.max(0.0);
        match &self.audio {
            None => {
                self.status = "No audio device.".into();
            }
            Some(audio) => match audio.seek_secs(pos) {
                Ok(()) => {
                    self.position_secs = pos;
                    // Hold the manual clock ~1.2s: the backend still
                    // reports the pre-seek position for a tick or two.
                    self.seek_hold_until =
                        Some(std::time::Instant::now() + std::time::Duration::from_millis(1200));
                }
                Err(e) => {
                    let msg = format!("Seek failed ({e}).");
                    self.log_error(msg.clone());
                    self.status = msg;
                }
            },
        }
    }

    /// Apply one desktop transport command (MPRIS widget / media keys).
    /// Next/Previous return their real play tasks (dropping them would
    /// move the pointer without ever fetching audio).
    fn apply_player_command(&mut self, cmd: yutu_desktop::PlayerCommand) -> iced::Task<Message> {
        use yutu_desktop::PlayerCommand as C;
        match cmd {
            C::Play => {
                if self.paused {
                    if let Some(a) = &self.audio {
                        a.resume();
                    }
                    self.paused = false;
                    self.status = "Resumed (desktop).".into();
                }
            }
            C::Pause => {
                if let Some(a) = &self.audio {
                    a.pause();
                }
                self.paused = true;
                self.status = "Paused (desktop).".into();
            }
            C::PlayPause => {
                if self.paused {
                    if let Some(a) = &self.audio {
                        a.resume();
                    }
                    self.paused = false;
                    self.status = "Resumed (desktop).".into();
                } else {
                    if let Some(a) = &self.audio {
                        a.pause();
                    }
                    self.paused = true;
                    self.status = "Paused (desktop).".into();
                }
            }
            C::Stop => {
                if let Some(a) = &self.audio {
                    a.stop();
                }
                self.paused = true;
                self.status = "Stopped (desktop).".into();
            }
            C::Next => {
                if self.advance_queue() {
                    self.apply_volume();
                    return self.play_current_task();
                }
            }
            C::Previous => {
                if self.queue.prev_track().is_some() {
                    self.apply_volume();
                    return self.play_current_task();
                }
            }
            C::Seek(offset) => {
                self.seek_to_secs(self.position_secs + offset);
            }
            C::SetPosition(pos) => {
                self.seek_to_secs(pos);
            }
            C::SetVolume(v) => {
                self.volume = (v as f32).clamp(0.0, 1.0);
                if let Some(a) = &self.audio {
                    a.set_volume(self.volume);
                }
            }
        }
        iced::Task::none()
    }

    /// Drain desktop transport commands queued since the last tick.
    fn drain_player_commands(&mut self) -> iced::Task<Message> {
        let cmds: Vec<yutu_desktop::PlayerCommand> = self
            .mpris
            .as_ref()
            .map(|h| {
                h.lock()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .drain_commands()
            })
            .unwrap_or_default();
        if cmds.is_empty() {
            return iced::Task::none();
        }
        let mut tasks = Vec::new();
        for cmd in cmds {
            tasks.push(self.apply_player_command(cmd));
        }
        iced::Task::batch(tasks)
    }

    /// Publish current track/status/volume to the MPRIS handle (if any) so
    /// desktop clients observe live state on property read. No-op headless.
    /// Poison-tolerant: a poisoned mutex still publishes via the inner guard.
    fn publish_mpris(&self) {
        let Some(handle) = self.mpris.as_ref() else {
            return;
        };
        let mut player = handle.lock().unwrap_or_else(|poison| poison.into_inner());
        if let Some(t) = self.queue.current() {
            player.update_track(yutu_desktop::NowPlaying {
                title: t.title.clone(),
                artist: t.artist.clone(),
                album: t.album.clone().unwrap_or_default(),
                track_id: t.id.clone(),
                length_secs: t.duration_secs,
            });
        }
        player.set_status(if self.queue.current().is_none() {
            yutu_desktop::PlaybackStatus::Stopped
        } else if self.paused {
            yutu_desktop::PlaybackStatus::Paused
        } else {
            yutu_desktop::PlaybackStatus::Playing
        });
        player.set_volume(f64::from(self.volume));
        player.set_position(self.position_secs);
    }

    /// Validate + select an EQ preset (in-memory only; never persisted).
    /// Honest about backends: rodio has no filter stage, so selecting a
    /// preset there records the choice and says so (it takes effect on mpv).
    fn select_eq_preset(&mut self, name: &str) {
        match EqPreset::preset_by_name(name) {
            Some(p) => match p.validate() {
                Ok(()) => {
                    self.eq_preset = p.name.clone();
                    match &self.audio {
                        None => {
                            self.status = format!("EQ: {} (no audio device)", p.name);
                        }
                        Some(audio) => match audio.apply_eq(&p) {
                            Ok(()) => {
                                self.status =
                                    format!("EQ: {} (live on {})", p.name, audio.backend_name());
                            }
                            Err(e) => {
                                self.status = format!("EQ: {} (saved; {e})", p.name);
                            }
                        },
                    }
                }
                Err(e) => {
                    self.status = format!("EQ preset invalid: {e}.");
                }
            },
            None => {
                self.status = format!("Unknown EQ preset: {name}.");
            }
        }
    }

    /// One-line EQ backend note for Settings (honest on rodio builds).
    fn eq_note(&self) -> String {
        match &self.audio {
            Some(a) if a.supports_eq() => "EQ is live on mpv.".into(),
            Some(_) => {
                "EQ needs the mpv backend — install libmpv / rebuild with --features mpv.".into()
            }
            None => "EQ: no audio device.".into(),
        }
    }

    /// Persisted play history for discovery (up to 200 entries).
    fn discovery_history(&self) -> Vec<HistoryEntry> {
        LibraryStore::open(&self.library_path)
            .and_then(|s| s.history(200))
            .unwrap_or_default()
    }

    /// Weekly Mix from persisted history + in-memory likes + real catalog.
    pub fn weekly_mix_tracks(&self) -> Vec<Track> {
        weekly_mix(
            &self.discovery_history(),
            &self.liked,
            &self.catalog_tracks(),
            10,
            now_unix(),
        )
    }

    /// Forgotten Favorites from in-memory likes + persisted history + catalog.
    pub fn forgotten_tracks(&self) -> Vec<Track> {
        forgotten_favorites(
            &self.liked,
            &self.discovery_history(),
            &self.catalog_tracks(),
            now_unix(),
        )
    }

    fn all_playlists(&self) -> Vec<Playlist> {
        self.user_playlists.clone()
    }

    fn find_playlist(&self, id: &str) -> Option<Playlist> {
        self.all_playlists().into_iter().find(|p| p.id == id)
    }

    /// Accept an artist id, a catalog track id, or a plain name.
    fn resolve_artist_name(&self, id_or_name: &str) -> String {
        if let Some(a) = self.results.artists.iter().find(|a| a.id == id_or_name) {
            return a.name.clone();
        }
        if let Some(t) = self.catalog.get(id_or_name) {
            return t.artist.clone();
        }
        if self.results.artists.iter().any(|a| a.name == id_or_name) {
            return id_or_name.to_string();
        }
        if self.catalog.values().any(|t| t.artist == id_or_name) {
            return id_or_name.to_string();
        }
        self.results
            .artists
            .first()
            .map(|a| a.name.clone())
            .unwrap_or_else(|| id_or_name.to_string())
    }

    fn artist_tracks(&self, artist: &str) -> Vec<Track> {
        let mut v: Vec<Track> = self
            .catalog
            .values()
            .filter(|t| t.artist == artist)
            .cloned()
            .collect();
        v.sort_by(|a, b| a.title.cmp(&b.title));
        v
    }

    fn artist_albums(&self, artist: &str) -> Vec<Album> {
        let mut albums: Vec<Album> = self
            .results
            .albums
            .iter()
            .filter(|a| a.artist == artist)
            .cloned()
            .collect();
        for t in self.artist_tracks(artist) {
            if let Some(title) = t.album.clone() {
                if !albums.iter().any(|a| a.title == title) {
                    albums.push(Album {
                        id: format!("ax-{title}"),
                        title,
                        artist: artist.into(),
                        year: None,
                    });
                }
            }
        }
        albums
    }

    /// Accept an album id, a catalog track id, or a plain title.
    fn resolve_album_title(&self, id_or_title: &str) -> String {
        if let Some(a) = self.results.albums.iter().find(|a| a.id == id_or_title) {
            return a.title.clone();
        }
        if let Some(t) = self.catalog.get(id_or_title) {
            if let Some(album) = t.album.as_deref() {
                return album.to_string();
            }
        }
        id_or_title.to_string()
    }

    fn album_tracks(&self, id_or_title: &str) -> Vec<Track> {
        let title = self.resolve_album_title(id_or_title);
        let mut v: Vec<Track> = self
            .catalog
            .values()
            .filter(|t| t.album.as_deref() == Some(title.as_str()))
            .cloned()
            .collect();
        v.sort_by(|a, b| a.title.cmp(&b.title));
        v
    }

    /// Dispatcher: actions with async follow-ups (scrobble submits) return
    /// their task directly; everything else goes through `update_sync`.
    /// MPRIS state is published on every path that returns here.
    pub fn update(&mut self, message: Message) -> iced::Task<Message> {
        match message {
            Message::PlayTrack(i) => {
                self.show_suggest = false;
                let task = self.play_index(i);
                self.publish_mpris();
                task
            }
            Message::EnqueueTrack(id) => {
                self.show_suggest = false;
                self.close_menu();
                let task = self.enqueue_track_task(&id);
                self.publish_mpris();
                task
            }
            Message::ScrobbleSubmitted { target, result } => {
                self.status = match &result {
                    Ok(()) => format!("Scrobbled to {target}."),
                    Err(e) => format!("Scrobble to {target} failed: {e}"),
                };
                self.publish_mpris();
                iced::Task::none()
            }
            message => self.update_sync(message),
        }
    }

    /// Synchronous state updates (plus stream-resolve tasks, which are
    /// Iced-side async, not scrobble submits). Publishes MPRIS before
    /// returning; the `PlayStreamId` early return publishes explicitly.
    fn update_sync(&mut self, message: Message) -> iced::Task<Message> {
        match message {
            Message::PlayTrack(_) | Message::EnqueueTrack(_) => {
                unreachable!("play actions are dispatched in update()")
            }
            Message::ScrobbleSubmitted { .. } => {
                unreachable!("scrobble results are dispatched in update()")
            }
            Message::Navigate(r) => self.navigate(r),
            Message::NavBack => self.go_back(),
            Message::NavForward => self.go_forward(),
            Message::SetRightTab(t) => {
                self.right_tab = t;
            }
            Message::HomeSearchChanged(q) => {
                self.search_query = q.clone();
                let trimmed = q.trim().to_string();
                self.search_gen = self.search_gen.wrapping_add(1);
                self.suggest_gen = self.suggest_gen.wrapping_add(1);
                if trimmed.is_empty() {
                    self.results = SearchResults::default();
                    self.suggest.clear();
                    self.show_suggest = false;
                    self.searching = false;
                } else if trimmed.len() >= 2 {
                    self.show_suggest = true;
                    #[cfg(feature = "innertube")]
                    {
                        let gen = self.search_gen;
                        let sgen = self.suggest_gen;
                        let q2 = trimmed.clone();
                        self.searching = true;
                        self.publish_mpris();
                        return iced::Task::batch([
                            Self::search_debounce_task(gen, trimmed),
                            Self::suggest_debounce_task(sgen, q2),
                        ]);
                    }
                }
            }
            Message::SuggestCompleted { gen, result } => {
                if gen != self.suggest_gen {
                    return iced::Task::none();
                }
                match result {
                    Ok(texts) => {
                        self.suggest = texts.into_iter().take(6).collect();
                    }
                    Err(_) => {
                        self.suggest.clear();
                    }
                }
            }
            Message::SearchDebounced { gen, query } => {
                // Stale keystroke (user kept typing): drop silently.
                if gen != self.search_gen {
                    return iced::Task::none();
                }
                let q = query.trim().to_string();
                if q.is_empty() || q.len() < 2 {
                    return iced::Task::none();
                }
                // Query changed while debouncing (Enter fired first): skip.
                if q != self.search_query.trim() {
                    return iced::Task::none();
                }
                #[cfg(feature = "innertube")]
                {
                    self.status = format!("Searching YouTube Music for “{q}”…");
                    self.searching = true;
                    self.publish_mpris();
                    return Self::search_task(q);
                }
                #[cfg(not(feature = "innertube"))]
                {
                    let _ = q;
                    self.status =
                        "Real search needs `--features innertube` (offline build).".into();
                }
            }
            Message::SearchStay => {
                self.show_suggest = false;
                self.go(Route::Search);
                let q = self.search_query.trim().to_string();
                #[cfg(feature = "innertube")]
                {
                    if q.is_empty() {
                        self.status = "Type something to search YouTube Music.".into();
                    } else {
                        self.status = format!("Searching YouTube Music for “{q}”…");
                        self.searching = true;
                        self.publish_mpris();
                        return Self::search_task(q);
                    }
                }
                #[cfg(not(feature = "innertube"))]
                {
                    let _ = q;
                    self.status =
                        "Real search needs `--features innertube` (offline build).".into();
                }
            }
            Message::SearchChanged(q) => {
                self.search_query = q.clone();
                self.results = filter_results(&self.search_base, &q);
            }
            Message::Search => {
                let q = self.search_query.trim().to_string();
                #[cfg(feature = "innertube")]
                {
                    if q.is_empty() {
                        self.status = "Type something to search YouTube Music.".into();
                    } else {
                        self.status = format!("Searching YouTube Music for “{q}”…");
                        self.searching = true;
                        self.publish_mpris();
                        return Self::search_task(q);
                    }
                }
                #[cfg(not(feature = "innertube"))]
                {
                    let _ = q;
                    self.status =
                        "Real search needs `--features innertube` (offline build).".into();
                    self.go(Route::Search);
                }
            }
            Message::SearchCompleted { query, result } => {
                self.searching = false;
                match result {
                    Ok(search) => {
                        let n = search.tracks.len();
                        let tracks = self.register_tracks(search.tracks);
                        let search = SearchResults { tracks, ..search };
                        let tasks = self.artwork_tasks(&search.tracks);
                        // Out-of-order arrival (user typed on): keep the
                        // tracks in the catalog, but never overwrite the
                        // newer query's results.
                        if query.trim() == self.search_query.trim() {
                            self.search_base = search.clone();
                            self.results = filter_results(&search, &self.search_query);
                            // Empty filter (e.g. capped query) shows all.
                            if self.results.tracks.is_empty() && !search.tracks.is_empty() {
                                self.results = search;
                            }
                            self.status = format!("{n} songs for “{query}”.");
                        }
                        self.publish_mpris();
                        return iced::Task::batch(tasks);
                    }
                    Err(e) => {
                        // Stale-query errors stay silent; current ones report.
                        if query.trim() == self.search_query.trim() {
                            self.status = format!("Search failed: {e}");
                        }
                    }
                }
            }
            Message::NewPlaylistNameChanged(n) => {
                self.new_playlist_name = n;
            }
            Message::Pause => {
                if let Some(a) = &self.audio {
                    a.pause();
                    self.paused = true;
                    self.status = "Paused.".into();
                }
            }
            Message::TogglePause => {
                if let Some(a) = &self.audio {
                    if self.paused {
                        a.resume();
                        self.paused = false;
                        self.status = "Resumed.".into();
                    } else {
                        a.pause();
                        self.paused = true;
                        self.status = "Paused.".into();
                    }
                } else {
                    self.paused = !self.paused;
                    self.status = "No audio device.".into();
                }
            }
            Message::Stop => {
                if let Some(a) = &self.audio {
                    a.stop();
                    self.paused = false;
                    self.status = "Stopped.".into();
                }
            }
            Message::Next => {
                if self.advance_queue() {
                    self.apply_volume();
                    self.publish_mpris();
                    return self.play_current_task();
                }
            }
            Message::Prev => {
                if self.queue.prev_track().is_some() {
                    self.apply_volume();
                    self.publish_mpris();
                    return self.play_current_task();
                }
            }
            Message::Like(id) => {
                if self.liked.remove(&id) {
                    self.persist_like(&id, false);
                    self.status = format!("Unliked {id}.");
                } else {
                    self.liked.insert(id.clone());
                    self.persist_like(&id, true);
                    self.status = format!("Liked {id}.");
                }
            }
            Message::SetVolume(v) => {
                self.volume = v.clamp(0.0, 1.0);
                if let Some(a) = &self.audio {
                    a.set_volume(self.volume);
                }
            }
            Message::QueueRemove(i) => {
                self.queue.remove_at(i);
            }
            Message::QueueUp(i) => {
                self.queue.move_up(i);
            }
            Message::QueueDown(i) => {
                self.queue.move_down(i);
            }
            Message::QueueClear => {
                self.queue.clear();
                self.status = if self.autoplay.is_empty() {
                    "Queue cleared.".into()
                } else {
                    format!("Queue cleared ({} autoplay kept).", self.autoplay.len())
                };
            }
            Message::OpenArtist(id) => self.go(Route::Artist(id)),
            Message::OpenAlbum(id) => self.go(Route::Album(id)),
            Message::OpenPlaylist(id) => self.go(Route::PlaylistDetail(id)),
            Message::OpenLyrics(id) => {
                self.close_menu();
                self.go(Route::Lyrics(id.clone()));
                #[cfg(feature = "lyrics")]
                {
                    let needs = !self.lyrics_cache.contains_key(&id)
                        && self.lyrics_loading.as_deref() != Some(id.as_str());
                    if needs {
                        match self.find_track(&id) {
                            None => {
                                self.status =
                                    "Lyrics need a known track — search for it first.".to_string();
                            }
                            Some(t) => {
                                self.lyrics_loading = Some(id);
                                self.publish_mpris();
                                return Self::lyrics_task(t);
                            }
                        }
                    }
                }
            }
            Message::LyricsReady { track_id, result } => {
                self.lyrics_loading = None;
                match result {
                    Ok(lyrics) => {
                        self.lyrics_cache.insert(track_id, lyrics);
                    }
                    Err(e) => {
                        self.status = format!("Lyrics failed: {e}");
                    }
                }
            }
            Message::ArtworkReady { url, result } => {
                self.artwork_loading.remove(&url);
                if let Ok(bytes) = result {
                    if bytes.len() > 512 {
                        self.artwork
                            .insert(url, iced::widget::image::Handle::from_bytes(bytes));
                    }
                }
            }
            Message::StartRadio(seed) => {
                self.close_menu();
                match self.find_track(&seed) {
                    None => {
                        self.status =
                            format!("Radio needs a known track — search for it first ({seed}).");
                    }
                    Some(t) => {
                        let artist = t.artist.clone();
                        #[cfg(feature = "innertube")]
                        {
                            self.status = format!("Starting radio for {} — {}…", t.title, t.artist);
                            self.go(Route::Radio(seed.clone()));
                            self.publish_mpris();
                            return Self::radio_task(seed, artist);
                        }
                        #[cfg(not(feature = "innertube"))]
                        {
                            let _ = artist;
                            self.status = "Radio needs `--features innertube`.".into();
                        }
                    }
                }
            }
            Message::RadioReady { seed, result } => match result {
                Ok(tracks) => {
                    let n = tracks.len();
                    let tracks = self.register_tracks(tracks);
                    self.radio_tracks = tracks.clone();
                    self.go(Route::Radio(seed.clone()));
                    let mut tasks = self.artwork_tasks(&tracks);
                    tasks.push(self.queue_many(tracks, &format!("Radio (seed {seed})")));
                    if n == 0 {
                        self.status = format!("Radio (seed {seed}): nothing found.");
                    }
                    self.publish_mpris();
                    return iced::Task::batch(tasks);
                }
                Err(e) => {
                    self.status = format!("Radio failed: {e}");
                }
            },
            Message::QueueTrack(id) => {
                self.show_suggest = false;
                self.close_menu();
                match self.find_track(&id) {
                    None => {
                        self.status = format!("Track not found: {id}.");
                    }
                    Some(t) => {
                        let was_empty = self.queue.is_empty();
                        self.queue.push(t.clone());
                        self.status = if was_empty {
                            format!(
                                "Added to queue: {} — {} (press ▶ to play).",
                                t.title, t.artist
                            )
                        } else {
                            format!("Added to queue: {} — {}.", t.title, t.artist)
                        };
                    }
                }
            }
            Message::QueueJump(i) => {
                if self.queue.jump_to(i) {
                    self.apply_volume();
                    self.publish_mpris();
                    return self.play_current_task();
                }
            }
            Message::ToggleShuffle => {
                self.queue.shuffle = !self.queue.shuffle;
                self.status = if self.queue.shuffle {
                    "Shuffle on.".into()
                } else {
                    "Shuffle off.".into()
                };
            }
            Message::CycleRepeat => {
                let mode = self.queue.cycle_repeat();
                self.status = format!("Repeat: {mode:?}.");
            }
            Message::PlayAll => {
                let (tracks, label) = match &self.route {
                    Route::Artist(id) => {
                        let name = self.resolve_artist_name(id);
                        (self.artist_tracks(&name), format!("Artist {name}"))
                    }
                    Route::Album(id) => {
                        let title = self.resolve_album_title(id);
                        (self.album_tracks(id), format!("Album {title}"))
                    }
                    Route::PlaylistDetail(id) => match self.find_playlist(id) {
                        Some(p) => (p.tracks.clone(), format!("Playlist {}", p.title)),
                        None => (Vec::new(), "Playlist not found".into()),
                    },
                    Route::Radio(_) => (self.radio_tracks.clone(), "Radio".to_string()),
                    Route::Discovery => (self.weekly_mix_tracks(), "Weekly Mix".to_string()),
                    _ => (self.results.tracks.clone(), "Search results".to_string()),
                };
                if tracks.is_empty() {
                    self.status = format!("{label}: nothing to queue.");
                } else {
                    let task = self.queue_many(tracks, &label);
                    self.publish_mpris();
                    return task;
                }
            }
            Message::CreatePlaylist(title) => {
                let title = title.trim().to_string();
                if title.is_empty() {
                    self.status = "Playlist name is empty.".into();
                } else {
                    self.playlist_seq += 1;
                    let id = format!("u{}", self.playlist_seq);
                    self.user_playlists.push(Playlist {
                        id: id.clone(),
                        title: title.clone(),
                        tracks: Vec::new(),
                    });
                    self.persist_playlist_created(&id, &title);
                    self.new_playlist_name.clear();
                    self.status = format!("Created playlist {title} ({id}).");
                }
            }
            Message::AddToPlaylist {
                playlist_id,
                track_id,
            } => match self.find_track(&track_id) {
                None => self.status = format!("Track not found: {track_id}."),
                Some(track) => match self.user_playlists.iter().position(|p| p.id == playlist_id) {
                    None => {
                        self.status =
                            format!("Playlist {playlist_id} is built-in (read-only) or missing.");
                    }
                    Some(i) => {
                        if self.user_playlists[i]
                            .tracks
                            .iter()
                            .any(|t| t.id == track.id)
                        {
                            self.status = format!(
                                "Already in {}: {}.",
                                self.user_playlists[i].title, track.title
                            );
                        } else {
                            let title = self.user_playlists[i].title.clone();
                            self.user_playlists[i].tracks.push(track.clone());
                            self.persist_playlist_added(&playlist_id, &track);
                            self.status = format!("Added {} to {}.", track.title, title);
                        }
                    }
                },
            },
            Message::RemoveFromPlaylist {
                playlist_id,
                track_id,
            } => match self.user_playlists.iter().position(|p| p.id == playlist_id) {
                None => {
                    self.status =
                        format!("Playlist {playlist_id} is built-in (read-only) or missing.");
                }
                Some(i) => {
                    let title = self.user_playlists[i].title.clone();
                    self.user_playlists[i].tracks.retain(|t| t.id != track_id);
                    self.persist_playlist_removed(&playlist_id, &track_id);
                    self.status = format!("Removed {track_id} from {title}.");
                }
            },
            Message::DeletePlaylist(id) => {
                let before = self.user_playlists.len();
                self.user_playlists.retain(|p| p.id != id);
                if self.user_playlists.len() < before {
                    self.persist_playlist_deleted(&id);
                    if self.route == Route::PlaylistDetail(id.clone()) {
                        self.go(Route::Library);
                    }
                    self.status = format!("Deleted playlist {id}.");
                } else {
                    self.status = format!("Playlist {id} is built-in and can't be deleted.");
                }
            }
            Message::SelectEqPreset(name) => self.select_eq_preset(&name),
            Message::ScrobbleToggled(on) => {
                self.scrobble_enabled = on;
                self.status = if on {
                    "Scrobbling enabled (submits when credentials are set).".into()
                } else {
                    "Scrobbling disabled.".into()
                };
            }
            Message::ScrobbleUserChanged(u) => {
                self.scrobble_user = u;
            }
            Message::ScrobbleLbTokenChanged(t) => {
                self.scrobble_lb_token = t;
            }
            Message::ScrobbleFmKeyChanged(k) => {
                self.scrobble_fm_key = k;
            }
            Message::ScrobbleFmSecretChanged(s) => {
                self.scrobble_fm_secret = s;
            }
            Message::ScrobbleFmSkChanged(sk) => {
                self.scrobble_fm_sk = sk;
            }
            Message::PlayWeeklyMix => {
                let tracks = self.weekly_mix_tracks();
                if tracks.is_empty() {
                    self.status = "Weekly Mix: nothing to queue.".into();
                } else {
                    let task = self.queue_many(tracks, "Weekly Mix");
                    self.publish_mpris();
                    return task;
                }
            }
            Message::PlayForgotten => {
                let tracks = self.forgotten_tracks();
                if tracks.is_empty() {
                    self.status = "Forgotten Favorites: nothing to queue.".into();
                } else {
                    let task = self.queue_many(tracks, "Forgotten Favorites");
                    self.publish_mpris();
                    return task;
                }
            }
            Message::PlayNow(id) => {
                self.close_menu();
                self.show_suggest = false;
                match self.find_track(&id) {
                    None => {
                        self.status = format!("Track not found: {id}.");
                    }
                    Some(t) => {
                        let artist = t.artist.clone();
                        self.queue.clear();
                        self.queue.push(t);
                        self.queue.jump_to(0);
                        self.playing_from = "Single + autoplay".into();
                        self.autoplay_for = Some(id.clone());
                        self.autoplay.clear();
                        self.history.push(id.clone());
                        self.persist_play(&id);
                        self.apply_volume();
                        self.publish_mpris();
                        let play = self.play_current_task();
                        #[cfg(feature = "innertube")]
                        {
                            let auto = Self::autoplay_task(id, artist);
                            return iced::Task::batch([play, auto]);
                        }
                        #[cfg(not(feature = "innertube"))]
                        {
                            let _ = artist;
                            return play;
                        }
                    }
                }
            }
            Message::AutoplayReady { seed, result } => {
                match result {
                    Err(e) => {
                        // Quiet: the tail is best-effort, but keep the log.
                        self.log_error(format!("Autoplay for {seed} failed: {e}"));
                    }
                    Ok(tracks) => {
                        let tracks = self.register_tracks(tracks);
                        // Stale (user moved on): catalog only, no append.
                        if self.autoplay_for.as_deref() != Some(seed.as_str()) {
                            return iced::Task::none();
                        }
                        let known: std::collections::HashSet<String> = self
                            .queue
                            .items()
                            .iter()
                            .chain(self.autoplay.iter())
                            .map(|t| t.id.clone())
                            .collect();
                        let fresh: Vec<Track> = tracks
                            .into_iter()
                            .filter(|t| t.id != seed && !known.contains(&t.id))
                            .take(10)
                            .collect();
                        if fresh.is_empty() {
                            return iced::Task::none();
                        }
                        let art = self.artwork_tasks(&fresh);
                        self.autoplay.extend(fresh);
                        self.publish_mpris();
                        return iced::Task::batch(art);
                    }
                }
            }
            Message::PlayNext(id) => {
                self.close_menu();
                self.show_suggest = false;
                match self.find_track(&id) {
                    None => {
                        self.status = format!("Track not found: {id}.");
                    }
                    Some(t) => {
                        if self.queue.is_empty() {
                            self.queue.push(t);
                            self.queue.jump_to(0);
                            self.apply_volume();
                            self.publish_mpris();
                            return self.play_current_task();
                        }
                        let at = self.queue.insert_next(t.clone());
                        self.status = format!("Queued next: {} — {}.", t.title, t.artist);
                        let _ = at;
                    }
                }
            }
            Message::PlayAutoplay(i) => {
                self.close_menu();
                match self.autoplay.get(i).cloned() {
                    None => {
                        self.status = format!("Nothing at autoplay index {i}.");
                    }
                    Some(t) => {
                        self.autoplay.remove(i);
                        self.queue.push(t);
                        self.queue.jump_to(self.queue.len().saturating_sub(1));
                        self.apply_volume();
                        self.publish_mpris();
                        return self.play_current_task();
                    }
                }
            }
            Message::RemoveFromQueue(id) => {
                self.close_menu();
                let was_current = self.queue.current().is_some_and(|t| t.id == id);
                if self.queue.remove_id(&id) {
                    self.status = format!("Removed {id} from the queue.");
                    if was_current {
                        // The ghost must not keep playing: move on, or stop.
                        if self.advance_queue() {
                            self.apply_volume();
                            self.publish_mpris();
                            return self.play_current_task();
                        }
                        if let Some(a) = &self.audio {
                            a.stop();
                        }
                        self.position_secs = 0.0;
                        self.loading_track = None;
                    }
                } else {
                    self.status = format!("Not in queue: {id}.");
                }
            }
            Message::RemoveAutoplay(i) => {
                self.close_menu();
                if i < self.autoplay.len() {
                    let t = self.autoplay.remove(i);
                    self.status = format!("Removed {} from autoplay.", t.title);
                } else {
                    self.status = format!("Nothing at autoplay index {i}.");
                }
            }
            Message::QueueHover(h) => {
                self.hovered_queue = h;
            }
            Message::OpenContext(id) => {
                self.open_menu(id);
            }
            Message::CursorMoved(p) => {
                self.cursor = Some(p);
                return iced::Task::none();
            }
            Message::WindowResized(sz) => {
                self.window = Some(sz);
                return iced::Task::none();
            }
            Message::CloseContext => {
                self.close_menu();
            }
            Message::OpenTrackAlbum(id) => {
                self.close_menu();
                match self.find_track(&id) {
                    Some(t) => match t.album.clone() {
                        Some(album) => self.go(Route::Album(album)),
                        None => {
                            self.status = format!("No album for {} — {}.", t.title, t.artist);
                        }
                    },
                    None => {
                        self.status = format!("Track not found: {id}.");
                    }
                }
            }
            Message::OpenTrackArtist(id) => {
                self.close_menu();
                match self.find_track(&id) {
                    Some(t) => self.go(Route::Artist(t.artist.clone())),
                    None => {
                        self.status = format!("Track not found: {id}.");
                    }
                }
            }
            Message::DownloadTrack(id) => {
                self.close_menu();
                match self.find_track(&id) {
                    None => {
                        self.status = format!("Track not found: {id}.");
                    }
                    Some(t) => {
                        self.status = format!("Downloading {} — {}…", t.title, t.artist);
                        self.publish_mpris();
                        return Self::download_task(id);
                    }
                }
            }
            Message::Downloaded { track_id, result } => match result {
                Ok(()) => {
                    self.status = format!("Downloaded {track_id} for offline listening.");
                }
                Err(e) => {
                    let msg = format!("Download failed for {track_id}: {e}");
                    self.log_error(msg.clone());
                    self.status = msg;
                }
            },
            Message::ToggleSidebar => {
                self.sidebar_expanded = !self.sidebar_expanded;
            }
            Message::SidebarFilterChanged(f) => {
                self.sidebar_filter = f;
            }
            Message::CookiePathChanged(p) => {
                self.cookie_path = p;
            }
            Message::CookieImport => {
                let path = self.cookie_path.trim().to_string();
                #[cfg(feature = "innertube")]
                {
                    if path.is_empty() {
                        self.status =
                            "Pick your exported cookies file first (see the note below).".into();
                    } else {
                        self.login_busy = true;
                        self.status = "Reading cookies + contacting YouTube Music…".into();
                        self.publish_mpris();
                        return Self::cookie_import_task(path);
                    }
                }
                #[cfg(not(feature = "innertube"))]
                {
                    let _ = path;
                    self.status =
                        "Cookie import needs `--features innertube` (offline build).".into();
                }
            }
            Message::CookieImported { result } => {
                #[cfg(feature = "innertube")]
                {
                    self.login_busy = false;
                }
                match result {
                    Ok((tracks, liked_ids)) => {
                        let n = tracks.len();
                        let m = liked_ids.len();
                        let tasks = self.merge_imported(tracks, liked_ids);
                        self.status = format!("Cookie import: {n} library tracks ({m} liked).");
                        self.publish_mpris();
                        return iced::Task::batch(tasks);
                    }
                    Err(e) => {
                        self.status = format!("Cookie import failed: {e}");
                    }
                }
            }
            #[cfg(feature = "innertube")]
            Message::GoogleLoginStart => {
                self.login_busy = true;
                self.status = "Contacting Google for a device code…".into();
                self.publish_mpris();
                return Self::google_begin_task();
            }
            #[cfg(feature = "innertube")]
            Message::GoogleLoginBegan { result } => {
                self.login_busy = false;
                match result {
                    Ok(login) => {
                        // Terminal copy: the URL is clickable there; the
                        // Settings panel shows it as text (Iced copy TBA).
                        eprintln!(
                            "[yutu-login] open {} and enter {}",
                            login.verification_url, login.user_code
                        );
                        self.google_login = Some(login);
                        self.status =
                            "Approve in your browser, then press “I’ve approved — check”.".into();
                    }
                    Err(e) => {
                        self.status = format!("Sign-in failed: {e}");
                    }
                }
            }
            #[cfg(feature = "innertube")]
            Message::GoogleLoginPoll => match self.google_login.clone() {
                None => {
                    self.status = "Press “Sign in with Google” first.".into();
                }
                Some(login) => {
                    self.login_busy = true;
                    self.status = "Checking approval…".into();
                    self.publish_mpris();
                    return Self::google_poll_task(login);
                }
            },
            #[cfg(feature = "innertube")]
            Message::GoogleLoginPolled { result } => {
                self.login_busy = false;
                match result {
                    Ok(Some(persist)) => {
                        match persist.save_to_file(std::path::Path::new(&oauth_path())) {
                            Ok(()) => {
                                self.logged_in = true;
                                self.google_login = None;
                                self.status = "Signed in — importing your library…".into();
                                self.publish_mpris();
                                return Self::library_import_task();
                            }
                            Err(e) => {
                                self.status = format!("Sign-in failed: {e}");
                            }
                        }
                    }
                    Ok(None) => {
                        self.status =
                            "Still waiting — approve in your browser, then press Check again."
                                .into();
                    }
                    Err(e) => {
                        if e.contains("expired") {
                            self.google_login = None;
                            self.status =
                                "Code expired — press “Sign in with Google” to restart.".into();
                        } else {
                            self.status = format!("Check failed: {e}");
                        }
                    }
                }
            }
            #[cfg(feature = "innertube")]
            Message::GoogleLogout => {
                let _ = std::fs::remove_file(oauth_path());
                self.logged_in = false;
                self.google_login = None;
                self.status = "Signed out (token file removed).".into();
            }
            #[cfg(feature = "innertube")]
            Message::LibraryImport => {
                if !self.logged_in {
                    self.status = "Sign in with Google first.".into();
                } else {
                    self.login_busy = true;
                    self.status = "Importing your library…".into();
                    self.publish_mpris();
                    return Self::library_import_task();
                }
            }
            #[cfg(feature = "innertube")]
            Message::LibraryImported { result } => {
                self.login_busy = false;
                match result {
                    Ok((tracks, liked_ids)) => {
                        let n = tracks.len();
                        let m = liked_ids.len();
                        let tasks = self.merge_imported(tracks, liked_ids);
                        self.status = format!("Imported {n} library tracks ({m} liked).");
                        self.publish_mpris();
                        return iced::Task::batch(tasks);
                    }
                    Err(e) => {
                        self.status = format!("Library import failed: {e}");
                    }
                }
            }
            Message::Tick(now) => {
                // Desktop transport commands first (widget / media keys).
                let mut tasks = vec![self.drain_player_commands()];
                // Position clock: manual advance while a user seek settles
                // (the backend reports the stale pre-seek position for a
                // tick or two — that stale tick is the lyrics flicker).
                let dt = self
                    .last_tick
                    .map(|last| now.duration_since(last).as_secs_f64())
                    .unwrap_or(0.0)
                    .clamp(0.0, 2.0);
                self.last_tick = Some(now);
                if self.seek_hold_until.is_some_and(|until| now < until) {
                    if !self.paused {
                        self.position_secs = (self.position_secs + dt).max(0.0);
                    }
                } else {
                    self.seek_hold_until = None;
                    self.position_secs = self.audio_position();
                }
                let duration = self.audio_duration();
                // Prefetch the next song while this one still plays (>75%).
                // One attempt per track — failures stay silent until advance.
                if !self.paused
                    && duration > 30.0
                    && self.prefetched.is_none()
                    && self.loading_track.is_none()
                    && self.position_secs > duration * 0.75
                {
                    if let Some(id) = self.peek_next_id() {
                        if self.prefetch_attempted.as_deref() != Some(id.as_str()) {
                            self.prefetch_attempted = Some(id.clone());
                            self.publish_mpris();
                            tasks.push(Self::prefetch_task(id));
                        }
                    }
                }
                // Auto-advance once per track end. The threshold sits 1s
                // before the reported duration: mpv's time-pos can hold just
                // short of it, and the rodio wall-clock can overshoot —
                // both cross (duration - 1) exactly once per song.
                if !self.paused
                    && !self.end_advanced
                    && duration > 2.0
                    && self.position_secs >= duration - 1.0
                    && self.advance_queue()
                {
                    self.end_advanced = true;
                    self.publish_mpris();
                    tasks.push(self.play_current_task());
                }
                self.publish_mpris();
                return iced::Task::batch(tasks);
            }
            Message::SeekTo(pos) => {
                self.seek_to_secs(pos as f64);
            }
            Message::UrlResolved { track_id, result } => {
                // Only the in-flight track may report; anything else is stale.
                if self.loading_track.as_deref() != Some(track_id.as_str()) {
                    return iced::Task::none();
                }
                self.loading_track = None;
                match result {
                    Err(e) => {
                        let msg = format!("Playback failed for {track_id}: {e}");
                        self.log_error(msg.clone());
                        self.status = msg;
                    }
                    Ok(url) => {
                        if let Some(expires) = url_expiry(&url) {
                            self.url_cache
                                .insert(track_id.clone(), (url.clone(), expires));
                        }
                        if let Some(t) = self.queue.current().cloned() {
                            if t.id == track_id && !self.try_play_url(&url, &t) {
                                // Stream play failed: fall back to bytes.
                                self.loading_track = Some(t.id.clone());
                                self.publish_mpris();
                                return Self::resolve_task(t.id);
                            }
                        }
                    }
                }
            }
            Message::UrlPrefetched { track_id, result } => {
                // Keep it only if it is still the upcoming track.
                if self.peek_next_id().as_deref() == Some(track_id.as_str()) {
                    if let Ok(url) = result {
                        self.prefetched = Some((track_id, url));
                    }
                }
            }
            Message::StreamResolved { video_id, result } => {
                self.loading_track = None;
                match result {
                    Err(e) => {
                        let msg = format!("Playback failed for {video_id}: {e}");
                        self.log_error(msg.clone());
                        self.status = msg;
                    }
                    Ok(bytes) => {
                        // Stale arrival (user moved on): drop, keep playing.
                        let current_id = self
                            .queue
                            .current()
                            .map(|t| t.id.clone())
                            .unwrap_or_default();
                        if video_id != current_id {
                            return iced::Task::none();
                        }
                        match &self.audio {
                            None => {
                                self.status =
                                    format!("Stream {video_id} fetched, but no audio device.");
                            }
                            Some(audio) => match audio.play_track_bytes(bytes) {
                                Ok(()) => {
                                    self.paused = false;
                                    self.end_advanced = false;
                                    self.position_secs = 0.0;
                                    if let Some(t) = self.queue.current() {
                                        self.status = format!("♪ {} — {}.", t.title, t.artist);
                                    }
                                }
                                Err(e) => {
                                    let msg = format!("Playback failed for {video_id}: {e}");
                                    self.log_error(msg.clone());
                                    self.status = msg;
                                }
                            },
                        }
                    }
                }
            }
        }
        self.publish_mpris();
        iced::Task::none()
    }

    /// Resolve + download task for a video ID. Runs on iced's tokio executor.
    fn resolve_task(video_id: String) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let result = resolve_and_fetch(&video_id).await;
                (video_id, result)
            },
            |(video_id, result)| Message::StreamResolved { video_id, result },
        )
    }

    /// Debounced keystroke: wait ~450ms for the user to keep typing, then
    /// report back — only the latest generation fires a real search.
    #[cfg(feature = "innertube")]
    fn search_debounce_task(gen: u64, query: String) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                tokio::time::sleep(std::time::Duration::from_millis(450)).await;
                (gen, query)
            },
            |(gen, query)| Message::SearchDebounced { gen, query },
        )
    }

    /// Real YouTube Music search (needs the `innertube` feature): one
    /// anonymous connection, three category queries run in sequence.
    /// Partial success is kept — only a total failure errors.
    #[cfg(feature = "innertube")]
    fn search_task(query: String) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let result = live_search(&query).await;
                (query, result)
            },
            |(query, result)| Message::SearchCompleted { query, result },
        )
    }

    /// Resolve a track to a stream URL fast (yt-dlp only, no download).
    /// mpv plays the URL progressively; the rodio path uses `resolve_task`
    /// (full bytes) instead.
    fn resolve_url_task(track_id: String) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let result = resolve_stream_url(&track_id).await;
                (track_id, result)
            },
            |(track_id, result)| Message::UrlResolved { track_id, result },
        )
    }

    /// Lyrics fetch for one catalog track (LRCLIB, no login needed).
    #[cfg(feature = "lyrics")]
    fn lyrics_task(track: Track) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let result = yutu_core::fetch_lrclib(
                    &track.artist,
                    &track.title,
                    track.album.as_deref(),
                    track.duration_secs,
                )
                .await;
                (track.id, result)
            },
            |(track_id, result)| Message::LyricsReady { track_id, result },
        )
    }

    /// Background prefetch of the next song's URL (never auto-plays).
    fn prefetch_task(track_id: String) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let result = resolve_stream_url(&track_id).await;
                (track_id, result)
            },
            |(track_id, result)| Message::UrlPrefetched { track_id, result },
        )
    }

    /// Google device-flow: request the code pair (network I/O).
    #[cfg(feature = "innertube")]
    fn google_begin_task() -> iced::Task<Message> {
        iced::Task::perform(
            async move { yutu_core::begin_google_login().await },
            |result| Message::GoogleLoginBegan { result },
        )
    }

    /// Google device-flow: one approval poll (caller retries on pending).
    #[cfg(feature = "innertube")]
    fn google_poll_task(login: yutu_core::GoogleLogin) -> iced::Task<Message> {
        iced::Task::perform(
            async move { yutu_core::poll_google_login(&login).await },
            |result| Message::GoogleLoginPolled { result },
        )
    }

    /// Signed-in library import: library songs + liked ids (catalog merge).
    #[cfg(feature = "innertube")]
    fn library_import_task() -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let persist =
                    yutu_core::OAuthPersist::load_from_file(std::path::Path::new(&oauth_path()))?;
                let backend = yutu_core::InnertubeBackend::connect_with_oauth(&persist)?;
                let lib = backend.library_songs().await.unwrap_or_default();
                let liked = backend.liked_songs().await.unwrap_or_default();
                if lib.is_empty() && liked.is_empty() {
                    return Err("empty library — is this the right Google account?".to_string());
                }
                let liked_ids: Vec<String> = liked.iter().map(|t| t.id.clone()).collect();
                let mut all = lib;
                for t in liked {
                    if !all.iter().any(|x| x.id == t.id) {
                        all.push(t);
                    }
                }
                Ok((all, liked_ids))
            },
            |result| Message::LibraryImported { result },
        )
    }

    /// Suggestion popup: one light query (texts only, no tracks).
    #[cfg(feature = "innertube")]
    async fn live_suggest(query: &str) -> Result<Vec<String>, String> {
        use yutu_core::InnertubeBackend;
        let backend = InnertubeBackend::connect_anonymous().await?;
        backend.search_suggestions(query).await
    }

    /// Suggestion debounce (shorter than search: the popup must feel instant).
    #[cfg(feature = "innertube")]
    fn suggest_debounce_task(gen: u64, query: String) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                let result = Self::live_suggest(&query).await;
                (gen, result)
            },
            |(gen, result)| Message::SuggestCompleted { gen, result },
        )
    }

    /// Autoplay tail for [`Message::PlayNow`]: same-artist radio, seed first
    /// (the caller strips the seed and anything already queued).
    #[cfg(feature = "innertube")]
    fn autoplay_task(seed: String, artist: String) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let result = live_radio(&seed, &artist).await;
                (seed, result)
            },
            |(seed, result)| Message::AutoplayReady { seed, result },
        )
    }

    /// Offline download: resolve + fetch bytes, persist to the disk cache.
    fn download_task(track_id: String) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let result: Result<(), String> = async {
                    let bytes = resolve_and_fetch(&track_id).await?;
                    let mut cache = yutu_player::OfflineCache::new(500 * 1024 * 1024);
                    cache.set_dir(format!("{}/offline_cache", data_dir()));
                    cache
                        .store_bytes(&track_id, &bytes)
                        .map_err(|e| e.to_string())?;
                    Ok(())
                }
                .await;
                (track_id, result)
            },
            |(track_id, result)| Message::Downloaded { track_id, result },
        )
    }

    /// Cookie import: normal browser login, no OAuth client needed. The user
    /// logs into music.youtube.com in their own browser, exports cookies to
    /// a file, and points the app at it — same library/liked merge as OAuth.
    #[cfg(feature = "innertube")]
    fn cookie_import_task(path: String) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let backend = yutu_core::InnertubeBackend::connect_with_cookie_file(
                    std::path::Path::new(&path),
                )
                .await?;
                let lib = backend.library_songs().await.unwrap_or_default();
                let liked = backend.liked_songs().await.unwrap_or_default();
                if lib.is_empty() && liked.is_empty() {
                    return Err("empty library — bad cookies file or wrong account?".to_string());
                }
                let liked_ids: Vec<String> = liked.iter().map(|t| t.id.clone()).collect();
                let mut all = lib;
                for t in liked {
                    if !all.iter().any(|x| x.id == t.id) {
                        all.push(t);
                    }
                }
                Ok((all, liked_ids))
            },
            |result| Message::CookieImported { result },
        )
    }

    /// Radio seed: same-artist tracks first, then the rest of the search.
    #[cfg(feature = "innertube")]
    fn radio_task(seed: String, artist: String) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let result = live_radio(&seed, &artist).await;
                (seed, result)
            },
            |(seed, result)| Message::RadioReady { seed, result },
        )
    }

    pub fn theme(&self) -> iced::Theme {
        iced::Theme::Dark
    }

    fn is_nav(&self, route: &Route) -> bool {
        std::mem::discriminant(&self.route) == std::mem::discriminant(route)
    }

    /// One compact track row: green play, title/artist, heart. All other
    /// actions live on detail screens — this killed the button-soup.
    fn track_row(&self, t: &Track) -> iced::Element<'_, Message> {
        use iced::widget::{button, column, mouse_area, row, text};
        use iced::Fill;
        let heart = if self.liked.contains(&t.id) {
            "♥"
        } else {
            "♡"
        };
        mouse_area(
            row![
                self.thumb(t),
                button(text("▶").size(13))
                    .style(green_play)
                    .on_press(Message::PlayNow(t.id.clone()))
                    .padding(6),
                column![
                    text(t.title.clone()).size(14),
                    text(t.artist.clone()).size(12).color(MUTED),
                ]
                .spacing(2)
                .width(Fill),
                text(fmt_time(t.duration_secs as f64)).size(12).color(MUTED),
                button(text(heart).size(15))
                    .style(button::text)
                    .on_press(Message::Like(t.id.clone()))
                    .padding(6),
                button(text("…").size(15))
                    .style(button::text)
                    .on_press(Message::OpenContext(t.id.clone()))
                    .padding(6),
            ]
            .spacing(8)
            .align_y(iced::alignment::Vertical::Center)
            .padding(4),
        )
        .on_right_press(Message::OpenContext(t.id.clone()))
        .into()
    }

    fn section<'a>(
        title: &'a str,
        rows: Vec<iced::Element<'a, Message>>,
    ) -> iced::Element<'a, Message> {
        use iced::widget::{column, text};
        let list: iced::Element<'_, Message> = if rows.is_empty() {
            text("Nothing here yet.").size(12).color(MUTED).into()
        } else {
            column(rows).spacing(2).into()
        };
        column![text(title).size(19), list,].spacing(6).into()
    }

    /// YTM-style sidebar: Home + Search, Library with playlist search,
    /// inline new-playlist creation, and an expand/collapse rail.
    /// Discovery/Radio/Up Next/Settings live in the header, Home and the
    /// context menu — the rail stays two sections + library.
    fn sidebar_view(&self) -> iced::Element<'_, Message> {
        use iced::widget::{button, column, container, row, text, text_input};
        use iced::Fill;
        let expand_btn: iced::Element<'_, Message> =
            button(text(if self.sidebar_expanded { "⏴" } else { "⏵" }).size(14))
                .style(button::text)
                .on_press(Message::ToggleSidebar)
                .padding(8)
                .into();
        if !self.sidebar_expanded {
            return container(
                column![
                    button(text("⌂").size(18))
                        .style(nav_style(self.is_nav(&Route::Home)))
                        .on_press(Message::Navigate(Route::Home))
                        .padding(10),
                    button(text("🔍").size(18))
                        .style(nav_style(self.is_nav(&Route::Search)))
                        .on_press(Message::Navigate(Route::Search))
                        .padding(10),
                    button(text("📚").size(18))
                        .style(nav_style(self.is_nav(&Route::Library)))
                        .on_press(Message::Navigate(Route::Library))
                        .padding(10),
                    expand_btn,
                ]
                .spacing(2),
            )
            .style(sidebar_bg)
            .padding(8)
            .width(56)
            .height(Fill)
            .into();
        }
        let q = self.sidebar_filter.to_lowercase();
        let new_name = self.new_playlist_name.clone();
        let rows: Vec<iced::Element<'_, Message>> = self
            .all_playlists()
            .into_iter()
            .filter(|pl| q.is_empty() || pl.title.to_lowercase().contains(&q))
            .take(20)
            .map(|pl| {
                let art: iced::Element<'_, Message> = pl
                    .tracks
                    .first()
                    .and_then(|t| self.find_track(&t.id))
                    .map(|t| self.thumb(&t))
                    .unwrap_or_else(|| {
                        container(text("♪").size(14).color(MUTED))
                            .style(card_bg)
                            .padding(8)
                            .into()
                    });
                button(
                    row![
                        art,
                        column![
                            text(pl.title.clone()).size(13),
                            text(format!("Playlist · {} songs", pl.tracks.len()))
                                .size(11)
                                .color(MUTED),
                        ]
                        .spacing(1)
                        .width(Fill),
                    ]
                    .spacing(8)
                    .align_y(iced::alignment::Vertical::Center),
                )
                .style(button::text)
                .on_press(Message::OpenPlaylist(pl.id.clone()))
                .padding(4)
                .width(Fill)
                .into()
            })
            .collect();
        let list: iced::Element<'_, Message> = if rows.is_empty() {
            text(if self.all_playlists().is_empty() {
                "No playlists yet — create one below."
            } else {
                "No playlists match."
            })
            .size(12)
            .color(MUTED)
            .into()
        } else {
            column(rows).spacing(2).into()
        };
        container(
            column![
                row![
                    text("yutu-mux")
                        .size(22)
                        .color(iced::Color::WHITE)
                        .width(Fill),
                    expand_btn,
                ]
                .align_y(iced::alignment::Vertical::Center),
                column![
                    button(text("⌂  Home").size(14))
                        .style(nav_style(self.is_nav(&Route::Home)))
                        .on_press(Message::Navigate(Route::Home))
                        .padding(8)
                        .width(Fill),
                    button(text("🔍  Search").size(14))
                        .style(nav_style(self.is_nav(&Route::Search)))
                        .on_press(Message::Navigate(Route::Search))
                        .padding(8)
                        .width(Fill),
                ]
                .spacing(2),
                row![button(text("📚  Library").size(14))
                    .style(nav_style(self.is_nav(&Route::Library)))
                    .on_press(Message::Navigate(Route::Library))
                    .padding(4)
                    .width(Fill),],
                text_input("Filter playlists…", &self.sidebar_filter)
                    .on_input(Message::SidebarFilterChanged)
                    .width(Fill),
                list,
                row![
                    text_input("New playlist…", &self.new_playlist_name)
                        .on_input(Message::NewPlaylistNameChanged)
                        .width(Fill),
                    button(text("＋").size(14))
                        .style(green_play)
                        .on_press(Message::CreatePlaylist(new_name))
                        .padding(7),
                ]
                .spacing(6)
                .align_y(iced::alignment::Vertical::Center),
            ]
            .spacing(10),
        )
        .style(sidebar_bg)
        .padding(12)
        .width(235)
        .height(Fill)
        .into()
    }

    fn right_panel_view(&self) -> iced::Element<'_, Message> {
        use iced::widget::{button, column, container, mouse_area, row, text};
        use iced::Fill;
        let current = self.queue.current().cloned();
        let pos = self.queue.position();
        let now: iced::Element<'_, Message> = match &current {
            Some(t) => column![
                text("Now playing").size(13).color(MUTED),
                row![
                    self.thumb(t),
                    column![
                        text(t.title.clone()).size(15),
                        text(t.artist.clone()).size(12).color(MUTED),
                    ]
                    .spacing(2)
                    .width(Fill),
                ]
                .spacing(8)
                .align_y(iced::alignment::Vertical::Center),
                row![
                    button(text("Lyrics").size(12))
                        .style(button::text)
                        .on_press(Message::OpenLyrics(t.id.clone()))
                        .padding(4),
                    button(text("Radio").size(12))
                        .style(button::text)
                        .on_press(Message::StartRadio(t.id.clone()))
                        .padding(4),
                ]
                .spacing(4),
            ]
            .spacing(4)
            .into(),
            None => column![
                text("Now playing").size(13).color(MUTED),
                text("Nothing queued").size(14).color(MUTED),
            ]
            .spacing(4)
            .into(),
        };
        let upcoming: Vec<iced::Element<'_, Message>> = self
            .queue
            .items()
            .iter()
            .take(9)
            .enumerate()
            .map(|(i, t)| {
                let is_current = Some(i) == pos;
                let menu_id = t.id.clone();
                mouse_area(
                    row![
                        self.thumb(t),
                        button(text(format!("{} — {}", t.title, t.artist)).size(12).color(
                            if is_current {
                                SPOT_GREEN
                            } else {
                                iced::Color::WHITE
                            }
                        ))
                        .style(button::text)
                        .on_press(Message::QueueJump(i))
                        .padding(3)
                        .width(Fill),
                        text(fmt_time(t.duration_secs as f64)).size(11).color(MUTED),
                    ]
                    .spacing(6)
                    .align_y(iced::alignment::Vertical::Center)
                    .padding(2),
                )
                .on_right_press(Message::OpenContext(menu_id))
                .into()
            })
            .collect();
        let tab_queue = button(text("Queue").size(12))
            .style(nav_style(self.right_tab == RightTab::Queue))
            .on_press(Message::SetRightTab(RightTab::Queue))
            .padding(6);
        let tab_recent = button(text("Recent").size(12))
            .style(nav_style(self.right_tab == RightTab::Recent))
            .on_press(Message::SetRightTab(RightTab::Recent))
            .padding(6);
        let from_line: iced::Element<'_, Message> = if self.playing_from.is_empty() {
            text("").size(1).into()
        } else {
            text(format!("Playing from  {}", self.playing_from))
                .size(12)
                .color(MUTED)
                .into()
        };
        let auto_mini: Vec<iced::Element<'_, Message>> = self
            .autoplay
            .iter()
            .enumerate()
            .take(5)
            .map(|(i, t)| {
                row![
                    self.thumb(t),
                    button(
                        text(format!("{} — {}", t.title, t.artist))
                            .size(12)
                            .color(MUTED)
                    )
                    .style(button::text)
                    .on_press(Message::PlayAutoplay(i))
                    .padding(3)
                    .width(Fill),
                    text(fmt_time(t.duration_secs as f64)).size(11).color(MUTED),
                ]
                .spacing(6)
                .align_y(iced::alignment::Vertical::Center)
                .padding(2)
                .into()
            })
            .collect();
        let auto_block: iced::Element<'_, Message> = if auto_mini.is_empty() {
            text("").size(1).into()
        } else {
            column![
                text("Autoplay").size(13).color(MUTED),
                column(auto_mini).spacing(0),
            ]
            .spacing(2)
            .into()
        };
        let list: iced::Element<'_, Message> = match self.right_tab {
            RightTab::Queue => column![
                text("Up next").size(13).color(MUTED),
                column(upcoming).spacing(0),
                auto_block,
                button(text(format!("Clear queue ({})", self.queue.len())).size(12))
                    .style(button::text)
                    .on_press(Message::QueueClear)
                    .padding(4),
            ]
            .spacing(4)
            .into(),
            RightTab::Recent => {
                let mut seen = std::collections::HashSet::new();
                let mut recent: Vec<Track> = Vec::new();
                for id in self.history.iter().rev() {
                    if recent.len() >= 9 {
                        break;
                    }
                    if seen.insert(id.clone()) {
                        if let Some(t) = self.find_track(id) {
                            recent.push(t);
                        }
                    }
                }
                if recent.is_empty() {
                    text("No recent plays yet.").size(12).color(MUTED).into()
                } else {
                    column(
                        recent
                            .iter()
                            .map(|t| {
                                let menu_id = t.id.clone();
                                mouse_area(
                                    row![
                                        self.thumb(t),
                                        button(
                                            text(format!("{} — {}", t.title, t.artist)).size(12)
                                        )
                                        .style(button::text)
                                        .on_press(Message::PlayNow(t.id.clone()))
                                        .padding(3)
                                        .width(Fill),
                                        text(fmt_time(t.duration_secs as f64))
                                            .size(11)
                                            .color(MUTED),
                                    ]
                                    .spacing(6)
                                    .align_y(iced::alignment::Vertical::Center)
                                    .padding(2),
                                )
                                .on_right_press(Message::OpenContext(menu_id))
                                .into()
                            })
                            .collect::<Vec<_>>(),
                    )
                    .spacing(0)
                    .into()
                }
            }
        };
        container(column![row![tab_queue, tab_recent].spacing(4), from_line, now, list,].spacing(8))
            .style(sidebar_bg)
            .padding(12)
            .width(285)
            .height(Fill)
            .into()
    }

    fn player_view(&self) -> iced::Element<'_, Message> {
        use iced::widget::{button, column, container, row, slider, text};
        use iced::Fill;
        let current = self.queue.current().cloned();
        let loading = self.loading_track.is_some();
        let title = if loading {
            "Loading…".into()
        } else {
            current
                .as_ref()
                .map(|t| t.title.clone())
                .unwrap_or_else(|| "Nothing playing".into())
        };
        let artist = current
            .as_ref()
            .map(|t| t.artist.clone())
            .unwrap_or_else(|| "Search for music, then click a song".into());
        let play_symbol = if self.paused { "▶" } else { "⏸" };
        let duration = self.audio_duration();
        let slider_max = duration.max(1.0);
        let shown_pos = self.position_secs.clamp(0.0, slider_max);
        let like_button: iced::Element<'_, Message> = match &current {
            Some(t) => {
                let heart = if self.liked.contains(&t.id) {
                    "♥"
                } else {
                    "♡"
                };
                button(text(heart).size(15).color(SPOT_GREEN))
                    .style(button::text)
                    .on_press(Message::Like(t.id.clone()))
                    .padding(6)
                    .into()
            }
            None => text("").size(1).into(),
        };
        let shuffle_button: iced::Element<'_, Message> = if self.queue.shuffle {
            button(text("🔀").size(14))
                .style(green_play)
                .on_press(Message::ToggleShuffle)
                .padding(7)
                .into()
        } else {
            button(text("🔀").size(14))
                .style(button::text)
                .on_press(Message::ToggleShuffle)
                .padding(7)
                .into()
        };
        let repeat_label: String = match self.queue.repeat {
            yutu_player::RepeatMode::Off => "⟳".into(),
            yutu_player::RepeatMode::All => "⟳ All".into(),
            yutu_player::RepeatMode::One => "⟳ 1".into(),
        };
        container(
            column![
                text(self.status.clone()).size(12).color(MUTED),
                row![
                    row![
                        match &current {
                            Some(t) => self.thumb(t),
                            None => container(text("♪").size(20).color(MUTED))
                                .style(card_bg)
                                .padding(10)
                                .into(),
                        },
                        column![text(title).size(14), text(artist).size(12).color(MUTED),]
                            .spacing(1),
                        like_button,
                    ]
                    .spacing(8)
                    .align_y(iced::alignment::Vertical::Center)
                    .width(Fill),
                    column![
                        row![
                            shuffle_button,
                            button(text("⏮").size(16))
                                .style(button::text)
                                .on_press(Message::Prev)
                                .padding(8),
                            button(text(play_symbol).size(17))
                                .style(green_play)
                                .on_press(Message::TogglePause)
                                .padding(10),
                            button(text("⏭").size(16))
                                .style(button::text)
                                .on_press(Message::Next)
                                .padding(8),
                            button(text(repeat_label).size(13))
                                .style(button::text)
                                .on_press(Message::CycleRepeat)
                                .padding(8),
                        ]
                        .spacing(6)
                        .align_y(iced::alignment::Vertical::Center),
                        row![
                            text(fmt_time(shown_pos)).size(11).color(MUTED),
                            slider(0.0..=slider_max as f32, shown_pos as f32, Message::SeekTo)
                                .step(1.0_f32)
                                .width(Fill),
                            text(fmt_time(duration)).size(11).color(MUTED),
                        ]
                        .spacing(6)
                        .align_y(iced::alignment::Vertical::Center),
                    ]
                    .spacing(2)
                    .align_x(iced::alignment::Horizontal::Center)
                    .width(Fill),
                    row![
                        iced::widget::Space::new().width(Fill),
                        text(format!(
                            "Q:{}  V:{:.0}%",
                            self.queue.len(),
                            self.volume * 100.0
                        ))
                        .size(12)
                        .color(MUTED),
                        slider(0.0..=1.0, self.volume, Message::SetVolume)
                            .step(0.01_f32)
                            .width(120),
                        button(text("▲").size(14))
                            .style(button::text)
                            .on_press(Message::Navigate(Route::NowPlaying))
                            .padding(7),
                    ]
                    .spacing(8)
                    .align_y(iced::alignment::Vertical::Center)
                    .width(Fill),
                ]
                .spacing(12)
                .align_y(iced::alignment::Vertical::Center),
            ]
            .spacing(4),
        )
        .style(player_bg)
        .padding(10)
        .into()
    }

    fn center_view(&self) -> iced::Element<'_, Message> {
        use iced::widget::{
            button, checkbox, column, container, mouse_area, row, scrollable, slider, text,
            text_input,
        };
        use iced::Fill;
        let body: iced::Element<'_, Message> = match &self.route {
            Route::Home => {
                let mut seen = std::collections::HashSet::new();
                let mut recent: Vec<Track> = Vec::new();
                for id in self.history.iter().rev() {
                    if recent.len() >= 8 {
                        break;
                    }
                    if seen.insert(id.clone()) {
                        if let Some(t) = self.find_track(id) {
                            recent.push(t);
                        }
                    }
                }
                // Liked tracks fill an empty history so the grid is alive.
                if recent.is_empty() {
                    let mut liked: Vec<Track> = self
                        .liked
                        .iter()
                        .filter_map(|id| self.find_track(id))
                        .take(8)
                        .collect();
                    liked.sort_by(|a, b| a.title.cmp(&b.title));
                    recent = liked;
                }
                let picks: Vec<Track> = recent;
                let grid: iced::Element<'_, Message> = if picks.is_empty() {
                    column![
                        text("Your recent plays will appear here.").color(MUTED),
                        text("Type in the search bar above — results land as you type.")
                            .size(12)
                            .color(MUTED),
                    ]
                    .spacing(8)
                    .into()
                } else {
                    // Two-across quick picks, Spotify-style: artwork + title.
                    let rows: Vec<iced::Element<'_, Message>> = picks
                        .chunks(2)
                        .map(|pair| {
                            let cards: Vec<iced::Element<'_, Message>> = pair
                                .iter()
                                .map(|t| {
                                    let menu_id = t.id.clone();
                                    mouse_area(
                                        button(
                                            row![
                                                self.thumb_sized(t, 52.0),
                                                column![
                                                    text(t.title.clone()).size(14),
                                                    text(t.artist.clone()).size(12).color(MUTED),
                                                ]
                                                .spacing(2)
                                                .width(Fill),
                                            ]
                                            .spacing(10)
                                            .padding(6)
                                            .align_y(iced::alignment::Vertical::Center),
                                        )
                                        .style(card_button)
                                        .on_press(Message::PlayNow(t.id.clone()))
                                        .padding(0)
                                        .width(Fill),
                                    )
                                    .on_right_press(Message::OpenContext(menu_id))
                                    .into()
                                })
                                .collect();
                            row(cards).spacing(8).into()
                        })
                        .collect();
                    column(rows).spacing(8).into()
                };
                let home_hits: iced::Element<'_, Message> =
                    if self.searching && self.results.tracks.is_empty() {
                        text("Searching YouTube Music…").color(MUTED).into()
                    } else if self.search_query.trim().is_empty() {
                        text("").size(1).into()
                    } else if self.results.tracks.is_empty() {
                        text("No matches yet — keep typing.").color(MUTED).into()
                    } else {
                        let top: Vec<iced::Element<'_, Message>> = self
                            .results
                            .tracks
                            .iter()
                            .take(6)
                            .map(|t| self.track_row(t))
                            .collect();
                        column![
                            text("Top results").size(17),
                            column(top).spacing(2),
                            button(text("See all results →").size(12))
                                .style(button::text)
                                .on_press(Message::Navigate(Route::Search))
                                .padding(4),
                        ]
                        .spacing(4)
                        .into()
                    };
                let home_search_results: iced::Element<'_, Message> = home_hits;
                let weekly_n = self.weekly_mix_tracks().len();
                let forgotten_n = self.forgotten_tracks().len();
                column![
                    text("Good evening").size(24),
                    home_search_results,
                    text("Jump back in").size(19),
                    grid,
                    row![
                        text("Made for you").size(19).width(Fill),
                        button(text("Discovery →").size(12))
                            .style(button::text)
                            .on_press(Message::Navigate(Route::Discovery))
                            .padding(4),
                    ]
                    .align_y(iced::alignment::Vertical::Center),
                    row![
                        button(
                            column![
                                text("Weekly Mix").size(15),
                                text(format!("{weekly_n} tracks")).size(12).color(MUTED),
                            ]
                            .spacing(2)
                            .padding(12)
                            .width(Fill),
                        )
                        .style(card_button)
                        .on_press(Message::PlayWeeklyMix)
                        .padding(0),
                        button(
                            column![
                                text("Forgotten Favorites").size(15),
                                text(format!("{forgotten_n} tracks")).size(12).color(MUTED),
                            ]
                            .spacing(2)
                            .padding(12)
                            .width(Fill),
                        )
                        .style(card_button)
                        .on_press(Message::PlayForgotten)
                        .padding(0),
                    ]
                    .spacing(8),
                ]
                .spacing(12)
                .into()
            }
            Route::Search => {
                let rows: Vec<iced::Element<'_, Message>> = self
                    .results
                    .tracks
                    .iter()
                    .map(|t| self.track_row(t))
                    .collect();
                let albums: Vec<iced::Element<'_, Message>> = self
                    .results
                    .albums
                    .iter()
                    .map(|a| {
                        use iced::widget::{button, row, text};
                        use iced::Fill;
                        row![
                            text(format!("{} — {}", a.title, a.artist))
                                .size(14)
                                .width(Fill),
                            button(text("Open").size(12))
                                .style(button::text)
                                .on_press(Message::OpenAlbum(a.id.clone()))
                                .padding(6),
                        ]
                        .spacing(8)
                        .align_y(iced::alignment::Vertical::Center)
                        .padding(4)
                        .into()
                    })
                    .collect();
                let artists: Vec<iced::Element<'_, Message>> = self
                    .results
                    .artists
                    .iter()
                    .map(|a| {
                        use iced::widget::{button, row, text};
                        use iced::Fill;
                        row![
                            text(a.name.clone()).size(14).width(Fill),
                            button(text("Open").size(12))
                                .style(button::text)
                                .on_press(Message::OpenArtist(a.id.clone()))
                                .padding(6),
                        ]
                        .spacing(8)
                        .align_y(iced::alignment::Vertical::Center)
                        .padding(4)
                        .into()
                    })
                    .collect();
                let search_note: iced::Element<'_, Message> = if self.searching {
                    text("Searching YouTube Music…").color(MUTED).into()
                } else if rows.is_empty() && albums.is_empty() && artists.is_empty() {
                    text("Results land here — type above and hit Enter (live search).")
                        .color(MUTED)
                        .into()
                } else {
                    text("").size(1).into()
                };
                column![
                    text(format!("Results for “{}”", self.search_query)).size(22),
                    search_note,
                    Self::section("Tracks", rows),
                    Self::section("Albums", albums),
                    Self::section("Artists", artists),
                ]
                .spacing(10)
                .into()
            }
            Route::Queue => {
                let pos = self.queue.position();
                let header: iced::Element<'_, Message> = match self.queue.current() {
                    Some(t) => column![
                        text("Now playing").size(12).color(MUTED),
                        text(format!("{} — {}", t.title, t.artist)).size(17),
                        text(format!(
                            "Track {} of {} · {} autoplay · Shuffle {} · Repeat {:?}",
                            pos.map(|p| p + 1).unwrap_or(0),
                            self.queue.len(),
                            self.autoplay.len(),
                            if self.queue.shuffle { "on" } else { "off" },
                            self.queue.repeat,
                        ))
                        .size(12)
                        .color(MUTED),
                    ]
                    .spacing(2)
                    .into(),
                    None => text("Queue is empty — search for music first.")
                        .color(MUTED)
                        .into(),
                };
                let rows: Vec<iced::Element<'_, Message>> = self
                    .queue
                    .items()
                    .iter()
                    .enumerate()
                    .map(|(i, t)| {
                        use iced::widget::{button, mouse_area, row, text};
                        use iced::Fill;
                        let is_current = Some(i) == pos;
                        let marker = if is_current { "● " } else { "" };
                        let jump = t.id.clone();
                        let remove_id = t.id.clone();
                        // The ✕ brightens on hover (idle rows stay clean).
                        let x_color = if self.hovered_queue == Some(i) {
                            iced::Color::WHITE
                        } else {
                            MUTED
                        };
                        mouse_area(
                            row![
                                button(
                                    text(format!("{marker}{}. {} — {}", i + 1, t.title, t.artist))
                                        .size(14)
                                        .color(if is_current {
                                            SPOT_GREEN
                                        } else {
                                            iced::Color::WHITE
                                        })
                                )
                                .style(button::text)
                                .on_press(Message::QueueJump(i))
                                .padding(4)
                                .width(Fill),
                                button(text("↑").size(12))
                                    .style(button::text)
                                    .on_press(Message::QueueUp(i))
                                    .padding(5),
                                button(text("↓").size(12))
                                    .style(button::text)
                                    .on_press(Message::QueueDown(i))
                                    .padding(5),
                                button(text("✕").size(12).color(x_color))
                                    .style(button::text)
                                    .on_press(Message::RemoveFromQueue(remove_id))
                                    .padding(5),
                            ]
                            .spacing(4)
                            .align_y(iced::alignment::Vertical::Center)
                            .padding(2),
                        )
                        .on_right_press(Message::OpenContext(jump))
                        .on_enter(Message::QueueHover(Some(i)))
                        .on_exit(Message::QueueHover(None))
                        .into()
                    })
                    .collect();
                let auto_rows: Vec<iced::Element<'_, Message>> = self
                    .autoplay
                    .iter()
                    .enumerate()
                    .map(|(i, t)| {
                        use iced::widget::{button, mouse_area, row, text};
                        use iced::Fill;
                        let menu_id = t.id.clone();
                        mouse_area(
                            row![
                                button(
                                    text(format!("{}. {} — {}", i + 1, t.title, t.artist))
                                        .size(13)
                                        .color(MUTED),
                                )
                                .style(button::text)
                                .on_press(Message::PlayAutoplay(i))
                                .padding(4)
                                .width(Fill),
                                text(fmt_time(t.duration_secs as f64)).size(11).color(MUTED),
                                button(text("✕").size(12))
                                    .style(button::text)
                                    .on_press(Message::RemoveAutoplay(i))
                                    .padding(5),
                            ]
                            .spacing(4)
                            .align_y(iced::alignment::Vertical::Center)
                            .padding(2),
                        )
                        .on_right_press(Message::OpenContext(menu_id))
                        .into()
                    })
                    .collect();
                let auto_section: iced::Element<'_, Message> = if auto_rows.is_empty() {
                    text("").size(1).into()
                } else {
                    column![
                        text("Autoplay — similar, plays after your queue")
                            .size(13)
                            .color(MUTED),
                        column(auto_rows).spacing(2),
                    ]
                    .spacing(4)
                    .into()
                };
                column![
                    row![
                        text("Up Next").size(22).width(Fill),
                        button(
                            text(if self.queue.shuffle {
                                "🔀 On"
                            } else {
                                "🔀 Off"
                            })
                            .size(12)
                        )
                        .style(button::text)
                        .on_press(Message::ToggleShuffle)
                        .padding(6),
                        button(text(format!("⟳ {:?}", self.queue.repeat)).size(12))
                            .style(button::text)
                            .on_press(Message::CycleRepeat)
                            .padding(6),
                        button(text("Clear").size(12))
                            .style(button::text)
                            .on_press(Message::QueueClear)
                            .padding(6),
                    ]
                    .spacing(4)
                    .align_y(iced::alignment::Vertical::Center),
                    header,
                    text("Queue — your picks").size(13).color(MUTED),
                    column(rows).spacing(2),
                    auto_section,
                ]
                .spacing(10)
                .into()
            }
            Route::Library => {
                let liked_tracks: Vec<Track> = self
                    .liked
                    .iter()
                    .filter_map(|id| self.find_track(id))
                    .collect();
                let liked_missing = self.liked.len().saturating_sub(liked_tracks.len());
                let liked: Vec<iced::Element<'_, Message>> =
                    liked_tracks.iter().map(|t| self.track_row(t)).collect();
                let liked_note: iced::Element<'_, Message> = if liked_missing > 0 {
                    text(format!(
                        "{liked_missing} liked song(s) not in cache — search for them."
                    ))
                    .size(12)
                    .color(MUTED)
                    .into()
                } else {
                    text("").size(1).into()
                };
                let playlists: Vec<iced::Element<'_, Message>> = self
                    .all_playlists()
                    .into_iter()
                    .map(|p| {
                        use iced::widget::{button, row, text};
                        use iced::Fill;
                        row![
                            column![
                                text(p.title.clone()).size(14),
                                text(format!("{} tracks", p.tracks.len()))
                                    .size(12)
                                    .color(MUTED),
                            ]
                            .spacing(2)
                            .width(Fill),
                            button(text("Open").size(12))
                                .style(button::text)
                                .on_press(Message::OpenPlaylist(p.id.clone()))
                                .padding(6),
                        ]
                        .spacing(8)
                        .align_y(iced::alignment::Vertical::Center)
                        .padding(6)
                        .into()
                    })
                    .collect();
                let new_name = self.new_playlist_name.clone();
                column![
                    text("Your Library").size(22),
                    text("Liked").size(19),
                    liked_note,
                    column(liked).spacing(2),
                    Self::section("Playlists", playlists),
                    row![
                        text_input("New playlist name…", &self.new_playlist_name)
                            .on_input(Message::NewPlaylistNameChanged)
                            .width(Fill),
                        button(text("Create").size(13))
                            .style(green_play)
                            .on_press(Message::CreatePlaylist(new_name))
                            .padding(8),
                    ]
                    .spacing(8)
                    .align_y(iced::alignment::Vertical::Center),
                ]
                .spacing(10)
                .into()
            }
            Route::Discovery => {
                let weekly: Vec<iced::Element<'_, Message>> = self
                    .weekly_mix_tracks()
                    .iter()
                    .map(|t| self.track_row(t))
                    .collect();
                let forgotten: Vec<iced::Element<'_, Message>> = self
                    .forgotten_tracks()
                    .iter()
                    .map(|t| self.track_row(t))
                    .collect();
                let weekly_section: iced::Element<'_, Message> = if weekly.is_empty() {
                    text("Play and like some music first.").color(MUTED).into()
                } else {
                    column(weekly).spacing(2).into()
                };
                let forgotten_section: iced::Element<'_, Message> = if forgotten.is_empty() {
                    text("Like some music to fill this.").color(MUTED).into()
                } else {
                    column(forgotten).spacing(2).into()
                };
                column![
                    text("Made for you").size(22),
                    row![
                        text("Weekly Mix").size(17).width(Fill),
                        button(text("▶ Play all").size(13))
                            .style(green_play)
                            .on_press(Message::PlayWeeklyMix)
                            .padding(7),
                    ]
                    .align_y(iced::alignment::Vertical::Center),
                    weekly_section,
                    row![
                        text("Forgotten Favorites").size(17).width(Fill),
                        button(text("▶ Play all").size(13))
                            .style(green_play)
                            .on_press(Message::PlayForgotten)
                            .padding(7),
                    ]
                    .align_y(iced::alignment::Vertical::Center),
                    forgotten_section,
                ]
                .spacing(10)
                .into()
            }
            Route::Settings => {
                let eq_names: Vec<String> = EqPreset::builtin_presets()
                    .into_iter()
                    .map(|p| p.name)
                    .collect();
                let current_eq = self.eq_preset.clone();
                let eq_buttons: Vec<iced::Element<'_, Message>> = eq_names
                    .into_iter()
                    .map(|n| {
                        let label = if n == current_eq {
                            format!("● {n}")
                        } else {
                            n.clone()
                        };
                        button(text(label).size(12))
                            .style(button::text)
                            .on_press(Message::SelectEqPreset(n))
                            .padding(5)
                            .into()
                    })
                    .collect();
                column![
                    text("Settings").size(22),
                    text("Playback").size(17),
                    row![
                        text(format!("Volume {:.0}%", self.volume * 100.0))
                            .size(13)
                            .width(Fill),
                        slider(0.0..=1.0, self.volume, Message::SetVolume)
                            .step(0.01_f32)
                            .width(160),
                    ]
                    .align_y(iced::alignment::Vertical::Center),
                    row(eq_buttons).spacing(4),
                    text(format!("EQ: {current_eq} · {}", self.eq_note()))
                        .size(12)
                        .color(MUTED),
                    text("Scrobbling (submits live when credentials are set)").size(17),
                    checkbox(self.scrobble_enabled)
                        .label("Enable scrobbling")
                        .on_toggle(Message::ScrobbleToggled),
                    text_input("Display name (informational)…", &self.scrobble_user)
                        .on_input(Message::ScrobbleUserChanged),
                    text_input("ListenBrainz token…", &self.scrobble_lb_token)
                        .on_input(Message::ScrobbleLbTokenChanged)
                        .secure(true),
                    text_input("Last.fm API key…", &self.scrobble_fm_key)
                        .on_input(Message::ScrobbleFmKeyChanged),
                    text_input("Last.fm shared secret…", &self.scrobble_fm_secret)
                        .on_input(Message::ScrobbleFmSecretChanged)
                        .secure(true),
                    text_input("Last.fm session key…", &self.scrobble_fm_sk)
                        .on_input(Message::ScrobbleFmSkChanged)
                        .secure(true),
                    text("Google account (import your library)").size(17),
                    self.google_section(),
                    text("Cookie import (no OAuth client needed)").size(17),
                    self.cookie_section(),
                    text("Recent playback errors").size(17),
                    self.error_section(),
                    text("Shortcuts: Space play/pause · n next · p prev · / search")
                        .size(12)
                        .color(MUTED),
                ]
                .spacing(10)
                .into()
            }
            Route::Artist(id) => {
                let name = self.resolve_artist_name(id);
                let rows: Vec<iced::Element<'_, Message>> = self
                    .artist_tracks(&name)
                    .iter()
                    .map(|t| self.track_row(t))
                    .collect();
                let albums: Vec<iced::Element<'_, Message>> = self
                    .artist_albums(&name)
                    .into_iter()
                    .map(|a| {
                        use iced::widget::{button, row, text};
                        use iced::Fill;
                        row![
                            text(a.title.clone()).size(14).width(Fill),
                            button(text("Open").size(12))
                                .style(button::text)
                                .on_press(Message::OpenAlbum(a.id.clone()))
                                .padding(6),
                        ]
                        .spacing(8)
                        .padding(4)
                        .into()
                    })
                    .collect();
                column![
                    text(name.clone()).size(24),
                    text("Artist").size(12).color(MUTED),
                    button(text("▶ Play all").size(13))
                        .style(green_play)
                        .on_press(Message::PlayAll)
                        .padding(8),
                    Self::section("Top tracks", rows),
                    Self::section("Albums", albums),
                ]
                .spacing(10)
                .into()
            }
            Route::Album(id) => {
                let title = self.resolve_album_title(id);
                let rows: Vec<iced::Element<'_, Message>> = self
                    .album_tracks(id)
                    .iter()
                    .map(|t| self.track_row(t))
                    .collect();
                column![
                    text(title).size(24),
                    text("Album").size(12).color(MUTED),
                    button(text("▶ Play all").size(13))
                        .style(green_play)
                        .on_press(Message::PlayAll)
                        .padding(8),
                    column(rows).spacing(2),
                ]
                .spacing(10)
                .into()
            }
            Route::PlaylistDetail(id) => match self.find_playlist(id) {
                None => column![text(format!("Playlist not found: {id}")).size(20)]
                    .spacing(8)
                    .into(),
                Some(p) => {
                    let rows: Vec<iced::Element<'_, Message>> =
                        p.tracks.iter().map(|t| self.track_row(t)).collect();
                    column![
                        text(p.title.clone()).size(24),
                        text(format!("Playlist · {} tracks", p.tracks.len()))
                            .size(12)
                            .color(MUTED),
                        button(text("▶ Play all").size(13))
                            .style(green_play)
                            .on_press(Message::PlayAll)
                            .padding(8),
                        column(rows).spacing(2),
                    ]
                    .spacing(10)
                    .into()
                }
            },
            Route::Lyrics(id) => {
                let title = self
                    .find_track(id)
                    .map(|t| format!("{} — {}", t.title, t.artist))
                    .unwrap_or_else(|| id.clone());
                let content: iced::Element<'_, Message> = match self.lyrics_cache.get(id) {
                    Some(cached) if cached.instrumental => {
                        text("[Instrumental]").size(14).color(MUTED).into()
                    }
                    Some(cached) if !cached.synced.is_empty() => {
                        let pos_ms = (self.position_secs * 1000.0) as u64;
                        let at = yutu_core::line_at(&cached.synced, pos_ms);
                        column(
                            cached
                                .synced
                                .iter()
                                .enumerate()
                                .map(|(i, l)| {
                                    let is_current = Some(i) == at;
                                    let secs = l.start_ms as f32 / 1000.0;
                                    button(
                                        text(format!(
                                            "{} {}",
                                            if is_current { "●" } else { " " },
                                            l.text
                                        ))
                                        .size(if is_current { 16 } else { 14 })
                                        .color(
                                            if is_current {
                                                SPOT_GREEN
                                            } else {
                                                iced::Color::WHITE
                                            },
                                        ),
                                    )
                                    .style(button::text)
                                    .on_press(Message::SeekTo(secs))
                                    .padding(2)
                                    .width(iced::Fill)
                                    .into()
                                })
                                .collect::<Vec<_>>(),
                        )
                        .spacing(3)
                        .into()
                    }
                    Some(cached) => column(
                        cached
                            .plain
                            .lines()
                            .filter(|l| !l.trim().is_empty())
                            .map(|l| text(l.to_string()).size(14).into())
                            .collect::<Vec<_>>(),
                    )
                    .spacing(4)
                    .into(),
                    None if self.lyrics_loading.as_deref() == Some(id.as_str()) => {
                        text("Fetching lyrics (LRCLIB, no login)…")
                            .color(MUTED)
                            .into()
                    }
                    None => text("No lyrics for this track yet.").color(MUTED).into(),
                };
                column![
                    text(title).size(22),
                    row![
                        button(text("▶ Play").size(13))
                            .style(green_play)
                            .on_press(Message::PlayNow(id.clone()))
                            .padding(7),
                        button(text("Radio").size(13))
                            .style(button::text)
                            .on_press(Message::StartRadio(id.clone()))
                            .padding(7),
                    ]
                    .spacing(8),
                    content,
                ]
                .spacing(10)
                .into()
            }
            Route::NowPlaying => match self.queue.current().cloned() {
                None => column![
                    text("Nothing playing").size(22),
                    text("Search above, click a song, hear it.").color(MUTED),
                ]
                .spacing(8)
                .into(),
                Some(t) => {
                    let pos = self.queue.position();
                    let upcoming: Vec<iced::Element<'_, Message>> = self
                        .queue
                        .items()
                        .iter()
                        .enumerate()
                        .map(|(i, q)| {
                            let is_current = Some(i) == pos;
                            row![
                                self.thumb(q),
                                button(text(format!("{} — {}", q.title, q.artist)).size(13).color(
                                    if is_current {
                                        SPOT_GREEN
                                    } else {
                                        iced::Color::WHITE
                                    }
                                ),)
                                .style(button::text)
                                .on_press(Message::QueueJump(i))
                                .padding(4)
                                .width(Fill),
                                text(fmt_time(q.duration_secs as f64)).size(11).color(MUTED),
                            ]
                            .spacing(8)
                            .align_y(iced::alignment::Vertical::Center)
                            .padding(2)
                            .into()
                        })
                        .collect();
                    let heart = if self.liked.contains(&t.id) {
                        "♥"
                    } else {
                        "♡"
                    };
                    column![
                        row![
                            button(text("▼ Close").size(13))
                                .style(button::text)
                                .on_press(Message::NavBack)
                                .padding(7),
                            text("Now playing").size(13).color(MUTED).width(Fill),
                        ]
                        .align_y(iced::alignment::Vertical::Center),
                        row![
                            column![
                                self.thumb_sized(&t, 280.0),
                                text(t.title.clone()).size(26),
                                text(t.artist.clone()).size(16).color(MUTED),
                                text(t.album.clone().unwrap_or_else(|| "Single".into()))
                                    .size(12)
                                    .color(MUTED),
                                text(format!(
                                    "{} / {}",
                                    fmt_time(self.position_secs),
                                    fmt_time(self.audio_duration())
                                ))
                                .size(12)
                                .color(MUTED),
                                row![
                                    button(text("Lyrics").size(13))
                                        .style(button::text)
                                        .on_press(Message::OpenLyrics(t.id.clone()))
                                        .padding(7),
                                    button(text("Radio").size(13))
                                        .style(button::text)
                                        .on_press(Message::StartRadio(t.id.clone()))
                                        .padding(7),
                                    button(text(heart).size(16).color(SPOT_GREEN))
                                        .style(button::text)
                                        .on_press(Message::Like(t.id.clone()))
                                        .padding(7),
                                ]
                                .spacing(8),
                            ]
                            .spacing(8)
                            .width(Fill),
                            column![
                                text("Up next").size(17),
                                column(upcoming).spacing(2),
                                text("Autoplay").size(14).color(MUTED),
                                column(
                                    self.autoplay
                                        .iter()
                                        .enumerate()
                                        .take(8)
                                        .map(|(i, q)| {
                                            row![
                                                self.thumb(q),
                                                button(
                                                    text(format!("{} — {}", q.title, q.artist))
                                                        .size(12)
                                                        .color(MUTED),
                                                )
                                                .style(button::text)
                                                .on_press(Message::PlayAutoplay(i))
                                                .padding(4)
                                                .width(Fill),
                                                text(fmt_time(q.duration_secs as f64))
                                                    .size(11)
                                                    .color(MUTED),
                                            ]
                                            .spacing(8)
                                            .align_y(iced::alignment::Vertical::Center)
                                            .padding(2)
                                            .into()
                                        })
                                        .collect::<Vec<_>>(),
                                )
                                .spacing(2),
                            ]
                            .spacing(6)
                            .width(350),
                        ]
                        .spacing(20),
                    ]
                    .spacing(12)
                    .into()
                }
            },
            Route::Radio(seed) => {
                let rows: Vec<iced::Element<'_, Message>> = self
                    .radio_tracks
                    .iter()
                    .map(|t| self.track_row(t))
                    .collect();
                let seed_label = self
                    .find_track(seed)
                    .map(|t| format!("{} — {}", t.title, t.artist))
                    .unwrap_or_else(|| seed.clone());
                column![
                    text(seed_label).size(24),
                    text("Radio").size(12).color(MUTED),
                    button(text("▶ Start radio").size(13))
                        .style(green_play)
                        .on_press(Message::StartRadio(seed.clone()))
                        .padding(8),
                    column(rows).spacing(2),
                ]
                .spacing(10)
                .into()
            }
        };
        let can_back = !self.nav_back.is_empty();
        let can_forward = !self.nav_forward.is_empty();
        // Global header, Spotify-style: browser nav + always-visible
        // search (as-you-type, stays on page) + clear + shortcuts.
        let back_btn: iced::Element<'_, Message> = if can_back {
            button(text("‹").size(24))
                .style(button::text)
                .on_press(Message::NavBack)
                .padding(10)
                .into()
        } else {
            button(text("‹").size(24)).padding(10).into()
        };
        let fwd_btn: iced::Element<'_, Message> = if can_forward {
            button(text("›").size(24))
                .style(button::text)
                .on_press(Message::NavForward)
                .padding(10)
                .into()
        } else {
            button(text("›").size(24)).padding(10).into()
        };
        let clear_btn: iced::Element<'_, Message> = if self.search_query.is_empty() {
            text("").size(1).into()
        } else {
            button(text("✕").size(13))
                .style(button::text)
                .on_press(Message::HomeSearchChanged(String::new()))
                .padding(6)
                .into()
        };
        let search_state: iced::Element<'_, Message> = if self.searching {
            text("…").size(14).color(MUTED).into()
        } else {
            text("").size(1).into()
        };
        let header: iced::Element<'_, Message> = row![
            back_btn,
            fwd_btn,
            text("🔍").size(14).color(MUTED),
            text_input("Search YouTube Music…", &self.search_query)
                .on_input(Message::HomeSearchChanged)
                .on_submit(Message::SearchStay)
                .width(Fill),
            clear_btn,
            search_state,
            button(text("☰").size(14))
                .style(button::text)
                .on_press(Message::Navigate(Route::Queue))
                .padding(6),
            button(text("⚙").size(14))
                .style(button::text)
                .on_press(Message::Navigate(Route::Settings))
                .padding(6),
        ]
        .spacing(4)
        .align_y(iced::alignment::Vertical::Center)
        .into();
        // Quick-results popup, YouTube-style: suggestions + top tracks.
        // Enter jumps to the full Search page; picking a track plays now.
        let qnow = self.search_query.trim().to_string();
        let suggest_popup: iced::Element<'_, Message> = if self.show_suggest
            && qnow.len() >= 2
            && (!self.suggest.is_empty() || !self.results.tracks.is_empty())
        {
            let sug_rows: Vec<iced::Element<'_, Message>> = self
                .suggest
                .iter()
                .map(|sug| {
                    button(
                        row![
                            text("🔍").size(13).color(MUTED),
                            text(sug.clone()).size(13).width(Fill),
                        ]
                        .spacing(8)
                        .align_y(iced::alignment::Vertical::Center),
                    )
                    .style(button::text)
                    .on_press(Message::HomeSearchChanged(sug.clone()))
                    .padding(5)
                    .width(Fill)
                    .into()
                })
                .collect();
            let top: Vec<iced::Element<'_, Message>> = self
                .results
                .tracks
                .iter()
                .take(5)
                .map(|t| self.track_row(t))
                .collect();
            container(
                column![
                    column(sug_rows).spacing(0),
                    text("Top tracks — Enter for all results")
                        .size(11)
                        .color(MUTED),
                    column(top).spacing(2),
                ]
                .spacing(6),
            )
            .style(card_bg)
            .padding(10)
            .into()
        } else {
            text("").size(1).into()
        };
        column![
            header,
            suggest_popup,
            scrollable(container(body).padding(20).width(Fill)).height(Fill),
        ]
        .spacing(4)
        .into()
    }

    pub fn view(&self) -> iced::Element<'_, Message> {
        use iced::widget::{button, column, container, mouse_area, row, text, Stack};
        use iced::Fill;
        let main = row![
            self.sidebar_view(),
            self.center_view(),
            self.right_panel_view(),
        ]
        .spacing(8)
        .height(Fill);
        let base: iced::Element<'_, Message> =
            container(column![main, self.player_view()].spacing(8).height(Fill))
                .style(app_bg)
                .padding(8)
                .into();
        let Some(menu_id) = self.context_menu.clone() else {
            return base;
        };
        let Some(t) = self.find_track(&menu_id) else {
            return base;
        };
        let heart = if self.liked.contains(&t.id) {
            "♥ Liked — unlike"
        } else {
            "♡ Like"
        };
        let mut items: Vec<iced::Element<'_, Message>> = vec![
            button(text("▶  Play now").size(14))
                .style(button::text)
                .on_press(Message::PlayNow(t.id.clone()))
                .padding(8)
                .width(Fill)
                .into(),
            button(text("⏭  Play next").size(14))
                .style(button::text)
                .on_press(Message::PlayNext(t.id.clone()))
                .padding(8)
                .width(Fill)
                .into(),
            button(text("➕  Add to queue").size(14))
                .style(button::text)
                .on_press(Message::QueueTrack(t.id.clone()))
                .padding(8)
                .width(Fill)
                .into(),
            button(text("📻  Start mix").size(14))
                .style(button::text)
                .on_press(Message::StartRadio(t.id.clone()))
                .padding(8)
                .width(Fill)
                .into(),
            button(text("📝  Lyrics").size(14))
                .style(button::text)
                .on_press(Message::OpenLyrics(t.id.clone()))
                .padding(8)
                .width(Fill)
                .into(),
            button(text(heart).size(14))
                .style(button::text)
                .on_press(Message::Like(t.id.clone()))
                .padding(8)
                .width(Fill)
                .into(),
            button(text("⬇  Download for offline").size(14))
                .style(button::text)
                .on_press(Message::DownloadTrack(t.id.clone()))
                .padding(8)
                .width(Fill)
                .into(),
            button(text("💿  Go to album").size(14))
                .style(button::text)
                .on_press(Message::OpenTrackAlbum(t.id.clone()))
                .padding(8)
                .width(Fill)
                .into(),
            button(text("🧑  Go to artist").size(14))
                .style(button::text)
                .on_press(Message::OpenTrackArtist(t.id.clone()))
                .padding(8)
                .width(Fill)
                .into(),
        ];
        if self.queue.items().iter().any(|q| q.id == t.id) {
            items.push(
                button(text("🗑  Remove from queue").size(14))
                    .style(button::text)
                    .on_press(Message::RemoveFromQueue(t.id.clone()))
                    .padding(8)
                    .width(Fill)
                    .into(),
            );
        }
        if let Some(ai) = self.autoplay.iter().position(|q| q.id == t.id) {
            items.push(
                button(text("🗑  Remove from autoplay").size(14))
                    .style(button::text)
                    .on_press(Message::RemoveAutoplay(ai))
                    .padding(8)
                    .width(Fill)
                    .into(),
            );
        }
        for pl in self.user_playlists.iter().take(5) {
            items.push(
                button(text(format!("📁  Add to {}", pl.title)).size(14))
                    .style(button::text)
                    .on_press(Message::AddToPlaylist {
                        playlist_id: pl.id.clone(),
                        track_id: t.id.clone(),
                    })
                    .padding(8)
                    .width(Fill)
                    .into(),
            );
        }
        // Playlist adds leave the menu open (multi-add); everything else
        // closes it via its own handler.
        let menu = container(
            column![
                text(t.title.clone()).size(16),
                text(t.artist.clone()).size(12).color(MUTED),
                column(items).spacing(0),
            ]
            .spacing(8),
        )
        .style(card_bg)
        .padding(14)
        .width(300);
        // Dismiss layer: left-click outside closes the menu. It sets no
        // right-press handler, so right-clicks pass through to the rows
        // below — right-clicking another song moves the menu in one step.
        let scrim: iced::Element<'_, Message> =
            mouse_area(container(text("").size(1)).width(Fill).height(Fill))
                .on_press(Message::CloseContext)
                .into();
        // Cursor-anchored placement (clamped on screen), YTM-style.
        let (mx, my) = menu_pos(self.menu_at, self.window);
        let placed: iced::Element<'_, Message> = container(column![
            iced::widget::Space::new().height(iced::Length::Fixed(my)),
            row![
                iced::widget::Space::new().width(iced::Length::Fixed(mx)),
                menu,
            ],
        ])
        .width(Fill)
        .height(Fill)
        .into();
        Stack::with_children(vec![base, scrim, placed]).into()
    }
}

pub fn run(mpris: Option<yutu_desktop::SharedPlayer>) -> iced::Result {
    iced::application(
        move || {
            let (mut state, task) = AppState::new();
            state.mpris = mpris.clone();
            state.publish_mpris();
            (state, task)
        },
        AppState::update,
        AppState::view,
    )
    .title("yutu-mux")
    .theme(AppState::theme)
    .subscription(AppState::subscription)
    .run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_DB_SEQ: AtomicU64 = AtomicU64::new(0);

    /// Isolated state per test — never touches the real library.db.
    /// Fabricated track (no fixtures needed for most update-logic tests).
    fn mk_track(id: &str, title: &str, artist: &str) -> Track {
        Track {
            id: id.into(),
            title: title.into(),
            artist: artist.into(),
            album: None,
            duration_secs: 200,
            thumbnail_url: None,
        }
    }

    fn test_state() -> AppState {
        let n = TEST_DB_SEQ.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!("yutu-test-{}-{}.db", std::process::id(), n));
        let (mut s, _) = AppState::with_library_path(path.to_string_lossy().into_owned());
        // Unit-test scaffolding: seed the catalog so id-based flows resolve.
        // User-visible views never read fixtures.
        for t in yutu_core::MockBackend::fixture_tracks() {
            s.catalog.insert(t.id.clone(), t);
        }
        s
    }

    #[test]
    fn navigate() {
        let mut s = test_state();
        s.navigate(Route::Search);
        assert_eq!(s.route, Route::Search);
    }

    #[test]
    fn search_updates_results() {
        let mut s = test_state();
        // Real flow: keystrokes set the query before the payload lands.
        let _ = s.update(Message::HomeSearchChanged("beatles".into()));
        // A completed real search lands in results + catalog.
        let _ = s.update(Message::SearchCompleted {
            query: "beatles".into(),
            result: Ok(SearchResults {
                tracks: vec![
                    mk_track("v1", "Yesterday", "The Beatles"),
                    mk_track("v2", "Yellow Submarine", "The Beatles"),
                ],
                albums: vec![],
                artists: vec![],
            }),
        });
        assert_eq!(s.results.tracks.len(), 2);
        assert!(s.find_track("v1").is_some());
        assert!(!s.searching);
        // Keystrokes filter the landed payload locally (no network).
        let _ = s.update(Message::SearchChanged("yesterday".into()));
        assert_eq!(s.results.tracks.len(), 1);
        assert_eq!(s.results.tracks[0].id, "v1");
        // Failures for the current query report without clobbering.
        let _ = s.update(Message::HomeSearchChanged("yesterday".into()));
        let _ = s.update(Message::SearchCompleted {
            query: "yesterday".into(),
            result: Err("offline".into()),
        });
        assert!(s.status.contains("offline"));
        assert_eq!(s.results.tracks.len(), 1);
        // Stale-query failures stay silent.
        let _ = s.update(Message::SearchCompleted {
            query: "ancient".into(),
            result: Err("offline".into()),
        });
        assert!(s.status.contains("offline"));
    }

    #[test]
    fn radio_ready_queues_fetched_sequence() {
        let mut s = test_state();
        // Unknown seed: no catalog entry, honest error, nothing queued.
        let _ = s.update(Message::StartRadio("unknown-id".into()));
        assert!(s.status.contains("search for it first"));
        assert!(s.queue.is_empty());
        // Known seed (catalog): radio view entered; payload queues on ready.
        let _ = s.update(Message::StartRadio("m1".into()));
        let _ = s.update(Message::RadioReady {
            seed: "m1".into(),
            result: Ok(vec![mk_track("v9", "Come Together", "The Beatles")]),
        });
        assert_eq!(s.route, Route::Radio("m1".into()));
        assert_eq!(s.queue.len(), 1);
        assert_eq!(s.radio_tracks.len(), 1);
        // Failures report cleanly.
        let _ = s.update(Message::RadioReady {
            seed: "m1".into(),
            result: Err("offline".into()),
        });
        assert!(s.status.contains("Radio failed"));
    }

    #[test]
    fn queue_jump_and_repeat_controls() {
        let mut s = test_state();
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        let _ = s.update(Message::EnqueueTrack("m2".into()));
        let _ = s.update(Message::QueueJump(1));
        assert_eq!(s.queue.position(), Some(1));
        let _ = s.update(Message::QueueJump(99));
        assert_eq!(s.queue.position(), Some(1));
        let _ = s.update(Message::ToggleShuffle);
        assert!(s.queue.shuffle);
        let _ = s.update(Message::ToggleShuffle);
        assert!(!s.queue.shuffle);
        let _ = s.update(Message::CycleRepeat);
        assert_eq!(s.queue.repeat, yutu_player::RepeatMode::All);
    }

    #[test]
    fn like_toggles() {
        let mut s = test_state();
        let _ = s.update(Message::Like("m1".into()));
        assert!(s.liked.contains("m1"));
        let _ = s.update(Message::Like("m1".into()));
        assert!(!s.liked.contains("m1"));
    }

    #[test]
    fn playlist_crud() {
        let mut s = test_state();
        let _ = s.update(Message::CreatePlaylist("Gym".into()));
        assert_eq!(s.user_playlists.len(), 1);
        let pid = s.user_playlists[0].id.clone();
        assert_eq!(s.user_playlists[0].title, "Gym");
        // Empty names are rejected.
        let _ = s.update(Message::CreatePlaylist("   ".into()));
        assert_eq!(s.user_playlists.len(), 1);
        // Add + dedupe + unknown track.
        let _ = s.update(Message::AddToPlaylist {
            playlist_id: pid.clone(),
            track_id: "m1".into(),
        });
        assert_eq!(s.user_playlists[0].tracks.len(), 1);
        let _ = s.update(Message::AddToPlaylist {
            playlist_id: pid.clone(),
            track_id: "m1".into(),
        });
        assert_eq!(s.user_playlists[0].tracks.len(), 1);
        let _ = s.update(Message::AddToPlaylist {
            playlist_id: pid.clone(),
            track_id: "nope".into(),
        });
        assert_eq!(s.user_playlists[0].tracks.len(), 1);
        // Unknown playlists are rejected.
        let _ = s.update(Message::AddToPlaylist {
            playlist_id: "nope".into(),
            track_id: "m1".into(),
        });
        assert_eq!(s.user_playlists[0].tracks.len(), 1);
        // Remove + delete.
        let _ = s.update(Message::RemoveFromPlaylist {
            playlist_id: pid.clone(),
            track_id: "m1".into(),
        });
        assert!(s.user_playlists[0].tracks.is_empty());
        let _ = s.update(Message::OpenPlaylist(pid.clone()));
        let _ = s.update(Message::DeletePlaylist(pid.clone()));
        assert!(s.user_playlists.is_empty());
        assert_eq!(s.route, Route::Library);
        // Deleting an unknown playlist is a no-op with a message.
        let _ = s.update(Message::DeletePlaylist("nope".into()));
        assert!(s.user_playlists.is_empty());
    }

    #[test]
    fn radio_generation_is_deterministic() {
        let seq = radio_for("m3");
        assert_eq!(seq.len(), 5);
        assert_eq!(seq[0].id, "m3");
        let ids: Vec<&str> = seq.iter().map(|t| t.id.as_str()).collect();
        for want in ["m1", "m2", "m3", "m4", "m5"] {
            assert!(ids.contains(&want), "missing {want}");
        }
        // Unknown seed falls back to fixture order.
        let seq = radio_for("zzz");
        assert_eq!(seq[0].id, "m1");
        assert_eq!(seq.len(), 5);
    }

    #[test]
    fn route_navigation() {
        let mut s = test_state();
        let _ = s.update(Message::OpenArtist("The Beatles".into()));
        assert_eq!(s.route, Route::Artist("The Beatles".into()));
        let _ = s.update(Message::CreatePlaylist("Gym".into()));
        let pid = s.user_playlists[0].id.clone();
        let _ = s.update(Message::OpenPlaylist(pid.clone()));
        assert_eq!(s.route, Route::PlaylistDetail(pid));
        let _ = s.update(Message::OpenLyrics("m4".into()));
        assert_eq!(s.route, Route::Lyrics("m4".into()));
        let _ = s.update(Message::StartRadio("m1".into()));
        #[cfg(feature = "innertube")]
        assert_eq!(s.route, Route::Radio("m1".into()));
        #[cfg(not(feature = "innertube"))]
        assert_eq!(s.route, Route::Lyrics("m4".into()));
        assert!(s.queue.is_empty());
        let _ = s.update(Message::RadioReady {
            seed: "m1".into(),
            result: Ok(vec![mk_track("v1", "Yesterday", "The Beatles")]),
        });
        assert_eq!(s.queue.len(), 1);
    }

    #[test]
    fn play_all_queues_current_detail_view() {
        let mut s = test_state();
        let _ = s.update(Message::HomeSearchChanged("beatles".into()));
        let _ = s.update(Message::SearchCompleted {
            query: "beatles".into(),
            result: Ok(SearchResults {
                tracks: vec![
                    mk_track("v1", "Yesterday", "The Beatles"),
                    mk_track("v2", "Hey Jude", "The Beatles"),
                ],
                albums: vec![],
                artists: vec![],
            }),
        });
        let _ = s.update(Message::PlayAll);
        assert_eq!(s.queue.len(), 2);
        let _ = s.update(Message::CreatePlaylist("Gym".into()));
        let pid = s.user_playlists[0].id.clone();
        let _ = s.update(Message::AddToPlaylist {
            playlist_id: pid.clone(),
            track_id: "v1".into(),
        });
        let _ = s.update(Message::OpenPlaylist(pid));
        let _ = s.update(Message::PlayAll);
        assert_eq!(s.queue.len(), 3);
    }

    #[test]
    fn enqueue_track_and_toggle_pause() {
        let mut s = test_state();
        let _ = s.update(Message::EnqueueTrack("m2".into()));
        assert_eq!(s.queue.len(), 1);
        assert!(s.history.contains(&"m2".to_string()));
        let _ = s.update(Message::EnqueueTrack("nope".into()));
        assert_eq!(s.queue.len(), 1);
        let _ = s.update(Message::TogglePause);
        assert!(s.paused);
        let _ = s.update(Message::TogglePause);
        assert!(!s.paused);
    }

    #[test]
    fn eq_preset_selection_valid_and_invalid() {
        let mut s = test_state();
        assert_eq!(s.eq_preset, "Flat");
        let _ = s.update(Message::SelectEqPreset("Rock".into()));
        assert_eq!(s.eq_preset, "Rock");
        assert!(s.status.contains("EQ: Rock"));
        assert!(s.status.contains("EQ: Rock"));
        // Case-insensitive lookup keeps the canonical name.
        let _ = s.update(Message::SelectEqPreset("pop".into()));
        assert_eq!(s.eq_preset, "Pop");
        // Unknown names leave the selection unchanged.
        let _ = s.update(Message::SelectEqPreset("Nope".into()));
        assert_eq!(s.eq_preset, "Pop");
        assert!(s.status.contains("Unknown EQ preset"));
    }

    #[test]
    fn discovery_sections_nonempty_with_seeded_history() {
        let mut s = test_state();
        let _ = s.update(Message::Like("m1".into()));
        let _ = s.update(Message::Like("m2".into()));
        // Seed plays older than 24h so the Weekly Mix doesn't skip them.
        let old = now_unix() - 5 * 86_400;
        let store = LibraryStore::open(&s.library_path).unwrap();
        store.record_play("m2", old).unwrap();
        store.record_play("m3", old).unwrap();
        let weekly = s.weekly_mix_tracks();
        assert!(!weekly.is_empty(), "weekly mix should be non-empty");
        let forgotten = s.forgotten_tracks();
        // m1 is liked and never played -> forgotten.
        assert!(forgotten.iter().any(|t| t.id == "m1"));
        // Play-all appends via the existing queue path.
        let _ = s.update(Message::PlayWeeklyMix);
        assert_eq!(s.queue.len(), weekly.len());
        assert!(s.status.contains("Weekly Mix"));
        let _ = s.update(Message::PlayForgotten);
        assert_eq!(s.queue.len(), weekly.len() + forgotten.len());
    }

    #[test]
    fn discovery_play_all_empty_is_graceful() {
        let s = test_state();
        assert!(s.weekly_mix_tracks().is_empty());
        assert!(s.forgotten_tracks().is_empty());
        let mut s = s;
        let _ = s.update(Message::PlayWeeklyMix);
        assert!(s.status.contains("nothing to queue"));
        let _ = s.update(Message::PlayForgotten);
        assert!(s.status.contains("nothing to queue"));
    }

    #[test]
    fn scrobble_status_on_enqueue_when_enabled() {
        let mut s = test_state();
        assert!(!s.scrobble_enabled);
        let _ = s.update(Message::ScrobbleToggled(true));
        assert!(s.scrobble_enabled);
        assert!(s.status.contains("credentials"));
        let _ = s.update(Message::ScrobbleUserChanged("alice".into()));
        assert_eq!(s.scrobble_user, "alice");
        // No credentials: eligibility is reported, nothing submitted.
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        assert!(s.status.contains("Scrobble-ready"));
        #[cfg(feature = "submit")]
        assert!(s.status.contains("Settings"));
        #[cfg(not(feature = "submit"))]
        assert!(s.status.contains("--features submit"));
        // ListenBrainz token set: a submit task is produced (lazy — no
        // network runs until the Iced runtime polls it).
        let _ = s.update(Message::ScrobbleLbTokenChanged("tok".into()));
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        #[cfg(feature = "submit")]
        {
            assert!(s.status.contains("Loading"));
            assert!(!s.status.contains("Scrobble-ready"));
        }
        #[cfg(not(feature = "submit"))]
        assert!(s.status.contains("--features submit"));
        // Credential inputs round-trip.
        let _ = s.update(Message::ScrobbleFmKeyChanged("k".into()));
        let _ = s.update(Message::ScrobbleFmSecretChanged("sec".into()));
        let _ = s.update(Message::ScrobbleFmSkChanged("sk".into()));
        assert_eq!(s.scrobble_fm_key, "k");
        // Disabled by default: no scrobble text on enqueue.
        let mut s2 = test_state();
        let _ = s2.update(Message::EnqueueTrack("m1".into()));
        assert!(!s2.status.contains("Scrobble-ready"));
        // Submit results update the status line honestly.
        let _ = s.update(Message::ScrobbleSubmitted {
            target: "ListenBrainz".into(),
            result: Ok(()),
        });
        assert!(s.status.contains("Scrobbled to ListenBrainz"));
        let _ = s.update(Message::ScrobbleSubmitted {
            target: "Last.fm".into(),
            result: Err("403".into()),
        });
        assert!(s.status.contains("failed: 403"));
    }

    #[test]
    fn mpris_publish_never_panics_headless() {
        // No handle (tests): publish is a no-op.
        let s = test_state();
        s.publish_mpris();
        // Volume clamping + status mapping are covered by publish internals;
        // reaching here without a handle or bus is the assertion.
    }

    #[test]
    fn mpris_publishes_track_and_status_to_shared_handle() {
        let handle: yutu_desktop::SharedPlayer =
            std::sync::Arc::new(std::sync::Mutex::new(yutu_desktop::MprisPlayer::default()));
        let mut s = test_state();
        s.mpris = Some(handle.clone());
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        let player = handle.lock().unwrap();
        assert_eq!(player.now_playing().title, "Come Together");
        assert_eq!(player.now_playing().artist, "The Beatles");
        assert_eq!(player.status(), yutu_desktop::PlaybackStatus::Playing);
        assert_eq!(player.volume(), f64::from(s.volume));
        drop(player);
        // Pause propagates too.
        let _ = s.update(Message::TogglePause);
        assert_eq!(
            handle.lock().unwrap().status(),
            yutu_desktop::PlaybackStatus::Paused
        );
    }

    #[test]
    fn click_to_play_loads_then_resolves_or_reports() {
        let mut s = test_state();
        // Clicking a song marks it loading (fetch runs async, no network here).
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        assert_eq!(s.loading_track.as_deref(), Some("m1"));
        assert!(s.status.contains("Loading"));
        assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m1"));
        // Stale arrival (user moved on): dropped silently.
        let _ = s.update(Message::EnqueueTrack("m2".into()));
        let _ = s.update(Message::StreamResolved {
            video_id: "m1".into(),
            result: Ok(vec![1, 2, 3]),
        });
        assert_eq!(s.loading_track, None);
        assert!(s.queue.current().map(|t| t.id.as_str()) == Some("m2"));
        // Fetch failure for the current track reports honestly.
        let _ = s.update(Message::StreamResolved {
            video_id: "m2".into(),
            result: Err("403 Forbidden".into()),
        });
        assert!(s.status.contains("m2") || s.status.contains("Playback failed"));
    }

    #[test]
    fn stream_resolved_reports_errors() {
        let mut s = test_state();
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        assert_eq!(s.loading_track.as_deref(), Some("m1"));
        let _ = s.update(Message::StreamResolved {
            video_id: "m1".into(),
            result: Err("403 Forbidden".into()),
        });
        assert!(s.status.contains("m1") && s.status.contains("403"));
        assert_eq!(s.loading_track, None);
        // Empty bytes: whatever the backend does (play async / refuse /
        // no device), it must settle loading and report — never panic.
        let _ = s.update(Message::StreamResolved {
            video_id: "m1".into(),
            result: Ok(Vec::new()),
        });
        assert_eq!(s.loading_track, None);
        assert!(
            s.status.contains('\u{266a}')
                || s.status.contains("no audio device")
                || s.status.contains("failed"),
            "unexpected status: {}",
            s.status
        );
    }

    #[test]
    fn user_playlists_survive_restart() {
        let path =
            std::env::temp_dir().join(format!("yutu-test-restart-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let p = path.to_string_lossy().into_owned();
        let (mut s, _) = AppState::with_library_path(p.clone());
        for t in yutu_core::MockBackend::fixture_tracks() {
            s.catalog.insert(t.id.clone(), t);
        }
        let _ = s.update(Message::CreatePlaylist("Gym".into()));
        let pid = s.user_playlists[0].id.clone();
        let _ = s.update(Message::AddToPlaylist {
            playlist_id: pid.clone(),
            track_id: "m1".into(),
        });
        drop(s);
        let (s2, _) = AppState::with_library_path(p);
        assert_eq!(s2.user_playlists.len(), 1);
        assert_eq!(s2.user_playlists[0].title, "Gym");
        assert_eq!(s2.user_playlists[0].tracks.len(), 1);
        assert_eq!(s2.user_playlists[0].tracks[0].id, "m1");
        // Loaded ids reserve the sequence: next create must not collide.
        // (seq resumes from max uN.)
    }

    /// Full click-to-play path (minus audible sound): catalog insert →
    /// EnqueueTrack marks loading → resolve+fetch real bytes → StreamResolved
    /// hands them to the backend → status flips to playing.
    #[tokio::test]
    #[ignore = "live: needs yt-dlp + network + an audio backend (mpv loads headless)"]
    async fn live_click_to_play_real_song() {
        let Some(_) = yutu_core::find_yt_dlp() else {
            eprintln!("SKIP: yt-dlp not installed (or set YUTU_YTDLP_BIN)");
            return;
        };
        let video = std::env::var("YUTU_TEST_VIDEO_ID").unwrap_or_else(|_| "JWW8KK8kd1I".into());
        let mut s = test_state();
        let Some(_) = &s.audio else {
            eprintln!("SKIP: no audio backend on this machine");
            return;
        };
        s.catalog.insert(
            video.clone(),
            Track {
                id: video.clone(),
                title: "live-proof".into(),
                artist: "yutu".into(),
                album: None,
                duration_secs: 200,
                thumbnail_url: None,
            },
        );
        let _ = s.update(Message::EnqueueTrack(video.clone()));
        assert_eq!(s.loading_track.as_deref(), Some(video.as_str()));
        let bytes = super::resolve_and_fetch(&video)
            .await
            .expect("yt-dlp resolve + chunked fetch");
        assert!(bytes.len() > 100_000, "suspiciously small stream");
        let _ = s.update(Message::StreamResolved {
            video_id: video,
            result: Ok(bytes),
        });
        assert_eq!(s.loading_track, None);
        assert!(
            s.status.contains('\u{266a}'),
            "expected playing status, got: {}",
            s.status
        );
        assert!(s.audio_position() >= 0.0);
        assert!(s.audio_duration() > 0.0);
    }

    #[test]
    fn lyrics_ready_caches_and_open_lyrics_fetches_once() {
        use yutu_core::{LyricLine, LyricsResult};
        let mut s = test_state();
        // Unknown id: honest error, no fetch.
        let _ = s.update(Message::OpenLyrics("ghost".into()));
        assert_eq!(s.route, Route::Lyrics("ghost".into()));
        assert!(s.lyrics_loading.is_none());
        // Known track: fetch marked in flight (task runs on the runtime).
        let _ = s.update(Message::OpenLyrics("m1".into()));
        #[cfg(feature = "lyrics")]
        assert_eq!(s.lyrics_loading.as_deref(), Some("m1"));
        #[cfg(not(feature = "lyrics"))]
        assert!(s.lyrics_loading.is_none());
        // Second open while loading: no duplicate fetch.
        let _ = s.update(Message::OpenLyrics("m1".into()));
        #[cfg(feature = "lyrics")]
        assert_eq!(s.lyrics_loading.as_deref(), Some("m1"));
        // Arrival caches; failures report.
        let _ = s.update(Message::LyricsReady {
            track_id: "m1".into(),
            result: Ok(LyricsResult {
                plain: "la\nla".into(),
                synced: vec![LyricLine {
                    start_ms: 0,
                    text: "la".into(),
                }],
                instrumental: false,
            }),
        });
        assert!(s.lyrics_loading.is_none());
        assert_eq!(s.lyrics_cache["m1"].synced.len(), 1);
        let _ = s.update(Message::LyricsReady {
            track_id: "m2".into(),
            result: Err("offline".into()),
        });
        assert!(s.status.contains("Lyrics failed"));
    }

    #[test]
    fn url_expiry_parses_googlevideo_param() {
        assert_eq!(
            url_expiry("https://x.googlevideo.com/v?expire=1791411186&ei=abc"),
            Some(1791411186)
        );
        assert_eq!(url_expiry("https://example.invalid/audio.webm"), None);
        assert_eq!(url_expiry("https://x/?expire=notanumber"), None);
    }

    #[test]
    fn browser_back_forward_walk_history() {
        let mut s = test_state();
        assert!(s.nav_back.is_empty());
        let _ = s.update(Message::NavBack);
        assert_eq!(s.route, Route::Home);
        let _ = s.update(Message::Navigate(Route::Search));
        let _ = s.update(Message::Navigate(Route::Queue));
        assert_eq!(s.route, Route::Queue);
        let _ = s.update(Message::NavBack);
        assert_eq!(s.route, Route::Search);
        let _ = s.update(Message::NavBack);
        assert_eq!(s.route, Route::Home);
        // Exhausted: stays put.
        let _ = s.update(Message::NavBack);
        assert_eq!(s.route, Route::Home);
        let _ = s.update(Message::NavForward);
        assert_eq!(s.route, Route::Search);
        // New navigation clears the forward stack.
        let _ = s.update(Message::Navigate(Route::Library));
        let _ = s.update(Message::NavForward);
        assert_eq!(s.route, Route::Library);
    }

    #[test]
    fn home_search_stays_home_until_navigation() {
        let mut s = test_state();
        let _ = s.update(Message::HomeSearchChanged("beatles".into()));
        assert_eq!(s.search_query, "beatles");
        assert_eq!(s.route, Route::Home);
        // Completion never kicks the user to another page.
        let _ = s.update(Message::SearchCompleted {
            query: "beatles".into(),
            result: Ok(SearchResults {
                tracks: vec![mk_track("v1", "Yesterday", "The Beatles")],
                albums: vec![],
                artists: vec![],
            }),
        });
        assert_eq!(s.route, Route::Home);
        assert_eq!(s.results.tracks.len(), 1);
    }

    #[test]
    fn stale_search_completion_never_clobbers_newer_query() {
        let mut s = test_state();
        let _ = s.update(Message::HomeSearchChanged("bonobo".into()));
        let gen = s.search_gen;
        // User kept typing: newer generation wins, stale debounce drops.
        let _ = s.update(Message::SearchDebounced {
            gen: gen.wrapping_sub(1),
            query: "bon".into(),
        });
        assert!(!s.status.contains("Searching"));
        // Out-of-order payload registers tracks but keeps newer results.
        let _ = s.update(Message::SearchCompleted {
            query: "bon".into(),
            result: Ok(SearchResults {
                tracks: vec![mk_track("old", "Old Song", "Someone")],
                albums: vec![],
                artists: vec![],
            }),
        });
        assert!(s.find_track("old").is_some());
        assert!(s.results.tracks.is_empty());
        // Current payload lands normally.
        let _ = s.update(Message::SearchCompleted {
            query: "bonobo".into(),
            result: Ok(SearchResults {
                tracks: vec![mk_track("new", "Rosewood", "Bonobo")],
                albums: vec![],
                artists: vec![],
            }),
        });
        assert_eq!(s.results.tracks.len(), 1);
        assert_eq!(s.results.tracks[0].id, "new");
    }

    #[test]
    fn sidebar_and_right_tabs_switch() {
        let mut s = test_state();
        assert!(s.sidebar_expanded);
        let _ = s.update(Message::ToggleSidebar);
        assert!(!s.sidebar_expanded);
        let _ = s.update(Message::SidebarFilterChanged("gym".into()));
        assert_eq!(s.sidebar_filter, "gym");
        assert_eq!(s.right_tab, RightTab::Queue);
        let _ = s.update(Message::SetRightTab(RightTab::Recent));
        assert_eq!(s.right_tab, RightTab::Recent);
    }

    #[cfg(feature = "innertube")]
    #[test]
    fn google_login_flow_reports_honestly() {
        let mut s = test_state();
        assert!(!s.logged_in);
        // No flow yet: polling says so.
        let _ = s.update(Message::GoogleLoginPoll);
        assert!(s.status.contains("Sign in"));
        // Start marks busy (network runs on the runtime).
        let _ = s.update(Message::GoogleLoginStart);
        assert!(s.login_busy);
        // Begin failure (no client id in test env) clears busy + reports.
        let _ = s.update(Message::GoogleLoginBegan {
            result: Err("no client".into()),
        });
        assert!(!s.login_busy);
        assert!(s.status.contains("Sign-in failed"));
        // Still-pending poll keeps the flow alive.
        let _ = s.update(Message::GoogleLoginPoll);
        assert!(s.status.contains("Sign in"));
        // Import without sign-in refuses; logout resets.
        let _ = s.update(Message::LibraryImport);
        assert!(s.status.contains("Sign in"));
        let _ = s.update(Message::GoogleLogout);
        assert!(!s.logged_in);
        assert!(s.status.contains("Signed out"));
        // Imported payload merges catalog + likes.
        let _ = s.update(Message::LibraryImported {
            result: Ok((
                vec![mk_track("lib1", "Lied", "Someone")],
                vec!["lib1".into()],
            )),
        });
        assert!(!s.login_busy);
        assert!(s.find_track("lib1").is_some());
        assert!(s.liked.contains("lib1"));
        assert!(s.status.contains("Imported 1"));
        let _ = s.update(Message::LibraryImported {
            result: Err("403".into()),
        });
        assert!(s.status.contains("failed: 403"));
    }

    #[test]
    fn play_now_replaces_queue_and_autoplay_appends() {
        let mut s = test_state();
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        let _ = s.update(Message::EnqueueTrack("m2".into()));
        assert_eq!(s.queue.len(), 2);
        // Click-to-play: queue replaced by the single + autoplay follows.
        let _ = s.update(Message::PlayNow("m3".into()));
        assert_eq!(s.queue.len(), 1);
        assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m3"));
        assert_eq!(s.autoplay_for.as_deref(), Some("m3"));
        assert_eq!(s.playing_from, "Single + autoplay");
        // Unknown id: honest error, queue untouched.
        let _ = s.update(Message::PlayNow("ghost".into()));
        assert_eq!(s.queue.len(), 1);
        // Stale autoplay (user moved on): catalog only.
        let _ = s.update(Message::AutoplayReady {
            seed: "m1".into(),
            result: Ok(vec![mk_track("old", "Old", "Someone")]),
        });
        assert_eq!(s.queue.len(), 1);
        assert!(s.find_track("old").is_some());
        // Current seed: seed + dupes filtered, tail appended (manual
        // queue untouched — Spotify split).
        let _ = s.update(Message::AutoplayReady {
            seed: "m3".into(),
            result: Ok(vec![
                mk_track("m3", "Dup", "X"),
                mk_track("m4", "Fresh", "Y"),
            ]),
        });
        assert_eq!(s.queue.len(), 1);
        assert_eq!(s.autoplay.len(), 1);
        assert_eq!(s.autoplay[0].id, "m4");
        // Peek sees the tail past the manual end (prefetch crosses over).
        assert_eq!(s.peek_next_id().as_deref(), Some("m4"));
        // Failures log without touching either list.
        let _ = s.update(Message::AutoplayReady {
            seed: "m3".into(),
            result: Err("offline".into()),
        });
        assert_eq!(s.queue.len(), 1);
        assert_eq!(s.autoplay.len(), 1);
        assert!(s.error_log.iter().any(|e| e.contains("Autoplay")));
    }

    #[test]
    fn queue_track_appends_without_playing() {
        let mut s = test_state();
        // Empty queue: appends, nothing loads or plays.
        let _ = s.update(Message::QueueTrack("m1".into()));
        assert_eq!(s.queue.len(), 1);
        assert!(s.loading_track.is_none());
        assert!(s.status.contains("Added to queue"));
        // Playing something: appends to the end, current untouched.
        let _ = s.update(Message::PlayNow("m2".into()));
        assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m2"));
        let _ = s.update(Message::QueueTrack("m3".into()));
        assert_eq!(s.queue.len(), 2);
        assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m2"));
        assert_eq!(s.queue.items()[1].id, "m3");
        assert_eq!(s.loading_track.as_deref(), Some("m2"));
        assert!(s.status.contains("Added to queue"));
        // Unknown id: honest error, queue untouched.
        let _ = s.update(Message::QueueTrack("ghost".into()));
        assert!(s.status.contains("not found"));
        assert_eq!(s.queue.len(), 2);
    }

    #[test]
    fn new_load_resets_position_and_marks_loading() {
        let mut s = test_state();
        let _ = s.update(Message::PlayNow("m1".into()));
        assert_eq!(s.loading_track.as_deref(), Some("m1"));
        assert_eq!(s.position_secs, 0.0);
        assert!(s.status.contains("Loading"));
    }

    #[test]
    fn play_next_inserts_after_current() {
        let mut s = test_state();
        // Empty queue: behaves like enqueue-and-play.
        let _ = s.update(Message::PlayNext("m1".into()));
        assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m1"));
        let _ = s.update(Message::EnqueueTrack("m2".into()));
        let _ = s.update(Message::QueueJump(0));
        let _ = s.update(Message::PlayNext("m3".into()));
        assert_eq!(s.queue.items()[1].id, "m3");
        assert_eq!(s.queue.position(), Some(0));
        assert!(s.status.contains("Queued next"));
        let _ = s.update(Message::PlayNext("ghost".into()));
        assert!(s.status.contains("not found"));
    }

    #[test]
    fn context_menu_opens_navigates_and_closes() {
        let mut s = test_state();
        assert!(s.context_menu.is_none());
        let _ = s.update(Message::OpenContext("m1".into()));
        assert_eq!(s.context_menu.as_deref(), Some("m1"));
        let _ = s.update(Message::CloseContext);
        assert!(s.context_menu.is_none());
        // Album/artist jump from the menu (m1 carries both).
        let _ = s.update(Message::OpenContext("m1".into()));
        let _ = s.update(Message::OpenTrackArtist("m1".into()));
        assert!(matches!(s.route, Route::Artist(_)));
        assert!(s.context_menu.is_none());
        // Unknown track: honest error.
        let _ = s.update(Message::OpenTrackAlbum("ghost".into()));
        assert!(s.status.contains("not found"));
        // Download validates the id before touching the network.
        let _ = s.update(Message::DownloadTrack("ghost".into()));
        assert!(s.status.contains("not found"));
        let _ = s.update(Message::Downloaded {
            track_id: "m1".into(),
            result: Ok(()),
        });
        assert!(s.status.contains("Downloaded m1"));
        let _ = s.update(Message::Downloaded {
            track_id: "m1".into(),
            result: Err("403".into()),
        });
        assert!(s.status.contains("failed"));
    }

    #[test]
    fn suggestions_store_and_drop_stale() {
        let mut s = test_state();
        let _ = s.update(Message::HomeSearchChanged("bon".into()));
        let gen = s.suggest_gen;
        assert!(s.show_suggest);
        let _ = s.update(Message::SuggestCompleted {
            gen: gen.wrapping_sub(1),
            result: Ok(vec!["stale".into()]),
        });
        assert!(s.suggest.is_empty());
        let _ = s.update(Message::SuggestCompleted {
            gen,
            result: Ok(vec!["bonobo".into(), "bon iver".into()]),
        });
        assert_eq!(s.suggest.len(), 2);
        // Enter hides the popup and lands on Search.
        let _ = s.update(Message::SearchStay);
        assert!(!s.show_suggest);
        assert_eq!(s.route, Route::Search);
    }

    #[test]
    fn cookie_import_validates_and_merges() {
        let mut s = test_state();
        let _ = s.update(Message::CookiePathChanged("/tmp/x.txt".into()));
        assert_eq!(s.cookie_path, "/tmp/x.txt");
        let _ = s.update(Message::CookieImported {
            result: Ok((
                vec![mk_track("c1", "Cookie Song", "Someone")],
                vec!["c1".into()],
            )),
        });
        assert!(s.find_track("c1").is_some());
        assert!(s.liked.contains("c1"));
        assert!(s.status.contains("Cookie import"));
        let _ = s.update(Message::CookieImported {
            result: Err("bad cookies".into()),
        });
        assert!(s.status.contains("bad cookies"));
    }

    #[test]
    fn eq_selection_reports_backend_honestly() {
        let mut s = test_state();
        let _ = s.update(Message::SelectEqPreset("Rock".into()));
        assert_eq!(s.eq_preset, "Rock");
        assert!(s.status.contains("EQ: Rock"));
    }

    #[test]
    fn now_playing_route_opens() {
        let mut s = test_state();
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        let _ = s.update(Message::Navigate(Route::NowPlaying));
        assert_eq!(s.route, Route::NowPlaying);
    }

    #[test]
    fn menu_pos_anchors_at_cursor_and_clamps() {
        let win = Some(iced::Size::new(1280.0, 800.0));
        // Free space: origin == cursor.
        assert_eq!(
            menu_pos(Some(iced::Point::new(100.0, 120.0)), win),
            (100.0, 120.0)
        );
        // Near the bottom-right: clamped so 300x470 stays visible.
        assert_eq!(
            menu_pos(Some(iced::Point::new(1200.0, 700.0)), win),
            (980.0, 330.0)
        );
        // Unknown cursor/window: sane on-screen default.
        assert_eq!(menu_pos(None, None), (200.0, 150.0));
    }

    #[test]
    fn autoplay_tail_shifts_in_when_queue_ends() {
        let mut s = test_state();
        let _ = s.update(Message::PlayNow("m1".into()));
        let _ = s.update(Message::AutoplayReady {
            seed: "m1".into(),
            result: Ok(vec![mk_track("m4", "Fresh", "Y")]),
        });
        assert_eq!(s.queue.len(), 1);
        // Manual end → head of the tail becomes the current song.
        let _ = s.update(Message::Next);
        assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m4"));
        assert!(s.autoplay.is_empty());
        // Playing a tail item directly moves it into the manual queue.
        let _ = s.update(Message::PlayNow("m1".into()));
        let _ = s.update(Message::AutoplayReady {
            seed: "m1".into(),
            result: Ok(vec![mk_track("m5", "Other", "Z")]),
        });
        let _ = s.update(Message::PlayAutoplay(0));
        assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m5"));
        assert!(s.autoplay.is_empty());
        let _ = s.update(Message::PlayAutoplay(9));
        assert!(s.status.contains("Nothing at autoplay"));
    }

    #[test]
    fn queue_remove_advances_when_current_goes() {
        let mut s = test_state();
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        let _ = s.update(Message::EnqueueTrack("m2".into()));
        let _ = s.update(Message::QueueJump(0));
        // Upcoming instance goes; current untouched.
        let _ = s.update(Message::RemoveFromQueue("m2".into()));
        assert_eq!(s.queue.len(), 1);
        assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m1"));
        assert!(s.status.contains("Removed m2"));
        // Removing the current song moves playback on.
        let _ = s.update(Message::EnqueueTrack("m2".into()));
        let _ = s.update(Message::QueueJump(0));
        let _ = s.update(Message::RemoveFromQueue("m1".into()));
        assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m2"));
        // Absent id: honest error.
        let _ = s.update(Message::RemoveFromQueue("ghost".into()));
        assert!(s.status.contains("Not in queue"));
        // Tail removal by index.
        let _ = s.update(Message::PlayNow("m1".into()));
        let _ = s.update(Message::AutoplayReady {
            seed: "m1".into(),
            result: Ok(vec![mk_track("m4", "Fresh", "Y")]),
        });
        let _ = s.update(Message::RemoveAutoplay(0));
        assert!(s.autoplay.is_empty());
        assert!(s.status.contains("autoplay"));
        let _ = s.update(Message::RemoveAutoplay(9));
        assert!(s.status.contains("Nothing at autoplay"));
    }

    #[test]
    fn queue_hover_tracks_cursor() {
        let mut s = test_state();
        assert!(s.hovered_queue.is_none());
        let _ = s.update(Message::QueueHover(Some(2)));
        assert_eq!(s.hovered_queue, Some(2));
        let _ = s.update(Message::QueueHover(None));
        assert!(s.hovered_queue.is_none());
    }

    #[test]
    fn desktop_commands_drive_playback() {
        use std::sync::{Arc, Mutex};
        let mut s = test_state();
        let handle: yutu_desktop::SharedPlayer =
            Arc::new(Mutex::new(yutu_desktop::MprisPlayer::default()));
        s.mpris = Some(handle.clone());
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        let _ = s.update(Message::EnqueueTrack("m2".into()));
        let _ = s.update(Message::QueueJump(0));
        // Widget pause flips the app within one tick.
        handle
            .lock()
            .unwrap()
            .push_command(yutu_desktop::PlayerCommand::Pause);
        let _ = s.update(Message::Tick(std::time::Instant::now()));
        assert!(s.paused);
        assert!(s.status.contains("desktop"));
        // Widget next advances the real queue.
        handle
            .lock()
            .unwrap()
            .push_command(yutu_desktop::PlayerCommand::Next);
        let _ = s.update(Message::Tick(std::time::Instant::now()));
        assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m2"));
        // Widget volume lands too.
        handle
            .lock()
            .unwrap()
            .push_command(yutu_desktop::PlayerCommand::SetVolume(0.5));
        let _ = s.update(Message::Tick(std::time::Instant::now()));
        assert!((s.volume - 0.5).abs() < 0.001);
    }

    #[test]
    fn seek_hold_advances_manually_then_resyncs() {
        use std::time::{Duration, Instant};
        let mut s = test_state();
        s.position_secs = 10.0;
        let t0 = Instant::now();
        s.last_tick = Some(t0);
        s.seek_hold_until = Some(t0 + Duration::from_secs(5));
        let _ = s.update(Message::Tick(t0 + Duration::from_millis(500)));
        assert!((s.position_secs - 10.5).abs() < 0.05);
        // Expired hold: backend clock wins again.
        s.seek_hold_until = Some(t0);
        s.position_secs = 99.0;
        let _ = s.update(Message::Tick(t0 + Duration::from_secs(10)));
        assert!(s.seek_hold_until.is_none());
        assert!(s.position_secs < 50.0);
    }

    #[test]
    fn playback_failures_land_in_the_error_log() {
        let mut s = test_state();
        assert!(s.error_log.is_empty());
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        let _ = s.update(Message::StreamResolved {
            video_id: "m1".into(),
            result: Err("403 Forbidden".into()),
        });
        assert_eq!(s.error_log.len(), 1);
        assert!(s.error_log[0].contains("m1"));
    }

    #[test]
    fn context_menu_freezes_position_while_open() {
        let mut s = test_state();
        let _ = s.update(Message::CursorMoved(iced::Point::new(100.0, 120.0)));
        let _ = s.update(Message::OpenContext("m1".into()));
        assert_eq!(s.menu_at, Some(iced::Point::new(100.0, 120.0)));
        // Mouse keeps moving (aiming at the menu): the anchor holds.
        let _ = s.update(Message::CursorMoved(iced::Point::new(900.0, 700.0)));
        assert_eq!(s.menu_at, Some(iced::Point::new(100.0, 120.0)));
        assert_eq!(s.context_menu.as_deref(), Some("m1"));
        // Closing drops the anchor.
        let _ = s.update(Message::CloseContext);
        assert!(s.menu_at.is_none());
        assert!(s.context_menu.is_none());
    }

    #[test]
    fn cursor_and_window_tracking_store() {
        let mut s = test_state();
        assert!(s.cursor.is_none());
        let _ = s.update(Message::CursorMoved(iced::Point::new(11.0, 22.0)));
        assert_eq!(s.cursor, Some(iced::Point::new(11.0, 22.0)));
        let _ = s.update(Message::WindowResized(iced::Size::new(1600.0, 900.0)));
        assert_eq!(s.window, Some(iced::Size::new(1600.0, 900.0)));
        // Right-click opens the menu for the row's track.
        let _ = s.update(Message::OpenContext("m1".into()));
        assert_eq!(s.context_menu.as_deref(), Some("m1"));
    }

    #[test]
    fn lyrics_page_follows_track_changes() {
        let mut s = test_state();
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        let _ = s.update(Message::OpenLyrics("m1".into()));
        assert_eq!(s.route, Route::Lyrics("m1".into()));
        // Advancing while on the lyrics page swaps the id in place.
        let _ = s.update(Message::EnqueueTrack("m2".into()));
        assert_eq!(s.route, Route::Lyrics("m2".into()));
        // No back-stack spam from the follow.
        assert!(!s.nav_back.iter().any(|r| *r == Route::Lyrics("m1".into())));
    }

    #[test]
    fn peek_next_id_walks_and_wraps_on_repeat_all() {
        let mut s = test_state();
        assert_eq!(s.peek_next_id(), None);
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        let _ = s.update(Message::EnqueueTrack("m2".into()));
        // Current is m2 (last enqueued); nothing after it, no repeat.
        assert_eq!(s.peek_next_id(), None);
        let _ = s.update(Message::QueueJump(0));
        assert_eq!(s.peek_next_id().as_deref(), Some("m2"));
        // Repeat-all wraps the tail back to the head.
        let _ = s.update(Message::CycleRepeat);
        let _ = s.update(Message::QueueJump(1));
        assert_eq!(s.peek_next_id().as_deref(), Some("m1"));
    }

    #[test]
    fn prefetch_is_kept_only_for_the_upcoming_track() {
        let mut s = test_state();
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        let _ = s.update(Message::EnqueueTrack("m2".into()));
        let _ = s.update(Message::QueueJump(0));
        let _ = s.update(Message::UrlPrefetched {
            track_id: "m2".into(),
            result: Ok("https://example.invalid/m2".into()),
        });
        assert_eq!(
            s.prefetched,
            Some(("m2".into(), "https://example.invalid/m2".into()))
        );
        // A late arrival for any other track is dropped.
        let _ = s.update(Message::UrlPrefetched {
            track_id: "zzz".into(),
            result: Ok("https://example.invalid/z".into()),
        });
        assert_eq!(
            s.prefetched,
            Some(("m2".into(), "https://example.invalid/m2".into()))
        );
        // Failures never poison the slot.
        let _ = s.update(Message::UrlPrefetched {
            track_id: "m2".into(),
            result: Err("boom".into()),
        });
        assert_eq!(
            s.prefetched,
            Some(("m2".into(), "https://example.invalid/m2".into()))
        );
    }

    /// Live composition check: the UI's `live_search` over the real API.
    #[cfg(feature = "innertube")]
    #[tokio::test]
    #[ignore = "live network: UI search composition over core Innertube queries"]
    async fn live_ui_search_returns_tracks() {
        let r = super::live_search("Beatles").await.expect("live UI search");
        assert!(!r.tracks.is_empty(), "expected real tracks for Beatles");
        assert!(
            r.tracks
                .iter()
                .all(|t| !t.id.is_empty() && !t.title.is_empty()),
            "tracks must carry playable ids + titles"
        );
    }
}
