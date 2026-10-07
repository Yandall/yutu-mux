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
    HomeSearchChanged(String),
    SearchStay,
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

    fn apply_eq(&self, preset: &EqPreset) {
        #[cfg(feature = "mpv")]
        if let Self::Mpv(a) = self {
            let _ = a.apply_eq_preset(preset);
        }
        #[cfg(not(feature = "mpv"))]
        let _ = preset;
    }
}

/// Parse a googlevideo `expire` query param (`…&expire=1791411186&…`) into a
/// unix timestamp. Unknown shapes → None (caller re-resolves every time).
fn url_expiry(url: &str) -> Option<u64> {
    url.split(['?', '&'])
        .find_map(|p| p.strip_prefix("expire=").and_then(|v| v.parse().ok()))
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
    /// Artwork by thumbnail URL (iced image handles) + in-flight set.
    artwork: HashMap<String, iced::widget::image::Handle>,
    artwork_loading: HashSet<String>,
    /// Track id a prefetch was already attempted for (no Tick spam).
    prefetch_attempted: Option<String>,
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
                artwork: HashMap::new(),
                artwork_loading: HashSet::new(),
                prefetch_attempted: None,
                end_advanced: false,
                library_path,
                catalog: HashMap::new(),
                search_base: SearchResults::default(),
                searching: false,
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
                _ => None,
            },
            _ => None,
        });
        let tick = iced::time::every(std::time::Duration::from_millis(500)).map(Message::Tick);
        iced::Subscription::batch([keys, tick])
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
        if self.prefetched.as_ref().is_some_and(|(id, _)| id != &t.id) {
            self.prefetched = None;
        }
        self.prefetch_attempted = None;
        self.loading_track = Some(t.id.clone());
        self.end_advanced = false;
        self.paused = false;
        self.position_secs = 0.0;
        // Prefetched URL: instant.
        if let Some((_, url)) = self.prefetched.take() {
            if self.try_play_url(&url, &t) {
                return iced::Task::none();
            }
        }
        // Fresh cached URL: instant (back/prev/replay never re-resolve).
        if let Some((url, expires)) = self.url_cache.get(&t.id).cloned() {
            if expires > now_unix() as u64 + 60 && self.try_play_url(&url, &t) {
                return iced::Task::none();
            }
        }
        self.status = format!("Loading {} — {}…", t.title, t.artist);
        match &self.audio {
            Some(a) if a.wants_stream() => Self::resolve_url_task(t.id),
            _ => Self::resolve_task(t.id),
        }
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
                    self.status = format!("Playback failed for {}: {e}", t.id);
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
            None
        }
    }

    /// Same fetch without touching the status line (bulk actions keep theirs).
    fn fetch_current_task(&mut self) -> iced::Task<Message> {
        match self.queue.current().cloned() {
            None => iced::Task::none(),
            Some(t) => {
                self.loading_track = Some(t.id.clone());
                self.end_advanced = false;
                self.paused = false;
                self.position_secs = 0.0;
                Self::resolve_task(t.id)
            }
        }
    }

    /// Shuffle-aware queue step (no audio here). True when current changed.
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
            self.queue.jump_to(i)
        } else {
            self.queue.next_track().is_some()
        }
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
        use iced::widget::{container, image, text};
        match track
            .thumbnail_url
            .as_deref()
            .and_then(|u| self.artwork.get(u))
        {
            Some(handle) => image(handle.clone()).width(40).height(40).into(),
            None => container(text("♪").size(16).color(MUTED))
                .style(card_bg)
                .padding(8)
                .into(),
        }
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
    fn select_eq_preset(&mut self, name: &str) {
        match EqPreset::preset_by_name(name) {
            Some(p) => match p.validate() {
                Ok(()) => {
                    self.eq_preset = p.name.clone();
                    if let Some(audio) = &self.audio {
                        audio.apply_eq(&p);
                        self.status = format!("EQ: {} (live on {})", p.name, audio.backend_name());
                    } else {
                        self.status = format!("EQ: {} (no audio device)", p.name);
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
                let task = self.play_index(i);
                self.publish_mpris();
                task
            }
            Message::EnqueueTrack(id) => {
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
                unreachable!("async play actions are dispatched in update()")
            }
            Message::ScrobbleSubmitted { .. } => {
                unreachable!("scrobble results are dispatched in update()")
            }
            Message::Navigate(r) => self.navigate(r),
            Message::NavBack => self.go_back(),
            Message::NavForward => self.go_forward(),
            Message::HomeSearchChanged(q) => {
                self.search_query = q;
            }
            Message::SearchStay => {
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
                self.go(Route::Search);
            }
            Message::Search => {
                let q = self.search_query.trim().to_string();
                #[cfg(feature = "innertube")]
                {
                    if q.is_empty() {
                        self.status = "Type something to search YouTube Music.".into();
                        self.go(Route::Search);
                    } else {
                        self.status = format!("Searching YouTube Music for “{q}”…");
                        self.searching = true;
                        self.go(Route::Search);
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
                        self.search_base = search.clone();
                        let tasks = self.artwork_tasks(&search.tracks);
                        self.results = filter_results(&search, &self.search_query);
                        // Empty filter (e.g. capped query) still shows everything.
                        if self.results.tracks.is_empty() && !search.tracks.is_empty() {
                            self.results = search;
                        }
                        self.go(Route::Search);
                        self.status = format!("{n} songs for “{query}”.");
                        self.publish_mpris();
                        return iced::Task::batch(tasks);
                    }
                    Err(e) => {
                        self.status = format!("Search failed: {e}");
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
                self.status = "Queue cleared.".into();
            }
            Message::OpenArtist(id) => self.go(Route::Artist(id)),
            Message::OpenAlbum(id) => self.go(Route::Album(id)),
            Message::OpenPlaylist(id) => self.go(Route::PlaylistDetail(id)),
            Message::OpenLyrics(id) => {
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
            Message::StartRadio(seed) => match self.find_track(&seed) {
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
            },
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
            Message::Tick(_) => {
                self.position_secs = self.audio_position();
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
                            return Self::prefetch_task(id);
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
                    return self.play_current_task();
                }
                self.publish_mpris();
                return iced::Task::none();
            }
            Message::SeekTo(pos) => {
                let pos = pos as f64;
                match &self.audio {
                    None => {
                        self.status = "No audio device.".into();
                    }
                    Some(audio) => match audio.seek_secs(pos) {
                        Ok(()) => {
                            self.position_secs = pos;
                        }
                        Err(e) => {
                            self.status = format!("Seek failed ({e}).");
                        }
                    },
                }
            }
            Message::UrlResolved { track_id, result } => {
                // Only the in-flight track may report; anything else is stale.
                if self.loading_track.as_deref() != Some(track_id.as_str()) {
                    return iced::Task::none();
                }
                self.loading_track = None;
                match result {
                    Err(e) => {
                        self.status = format!("Playback failed for {track_id}: {e}");
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
                        self.status = format!("Playback failed for {video_id}: {e}");
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
                                    self.status = format!("Playback failed for {video_id}: {e}");
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

    /// Radio seed: the current track, if the app has actually seen one.
    fn radio_seed(&self) -> Option<String> {
        self.queue.current().map(|t| t.id.clone())
    }

    fn is_nav(&self, route: &Route) -> bool {
        std::mem::discriminant(&self.route) == std::mem::discriminant(route)
    }

    /// One compact track row: green play, title/artist, heart. All other
    /// actions live on detail screens — this killed the button-soup.
    fn track_row(&self, t: &Track) -> iced::Element<'_, Message> {
        use iced::widget::{button, column, row, text};
        use iced::Fill;
        let heart = if self.liked.contains(&t.id) {
            "♥"
        } else {
            "♡"
        };
        row![
            self.thumb(t),
            button(text("▶").size(13))
                .style(green_play)
                .on_press(Message::EnqueueTrack(t.id.clone()))
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
        ]
        .spacing(8)
        .align_y(iced::alignment::Vertical::Center)
        .padding(4)
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

    fn sidebar_view(&self) -> iced::Element<'_, Message> {
        use iced::widget::{button, column, container, text};
        use iced::Fill;
        let seed = self.radio_seed();
        let radio_active = matches!(self.route, Route::Radio(_));
        let radio_msg = match seed {
            Some(id) => Message::StartRadio(id),
            // Nothing playing yet: radio starts from Search instead.
            None => Message::Navigate(Route::Search),
        };
        let playlists: Vec<iced::Element<'_, Message>> = self
            .all_playlists()
            .into_iter()
            .take(8)
            .map(|p| {
                button(text(format!("{} · {}", p.title, p.tracks.len())).size(13))
                    .style(button::text)
                    .on_press(Message::OpenPlaylist(p.id.clone()))
                    .padding(6)
                    .width(Fill)
                    .into()
            })
            .collect();
        let liked_n = self.liked.len();
        container(
            column![
                text("yutu-mux").size(22).color(iced::Color::WHITE),
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
                    button(text("☰  Up Next").size(14))
                        .style(nav_style(self.is_nav(&Route::Queue)))
                        .on_press(Message::Navigate(Route::Queue))
                        .padding(8)
                        .width(Fill),
                    button(text("📚  Library").size(14))
                        .style(nav_style(self.is_nav(&Route::Library)))
                        .on_press(Message::Navigate(Route::Library))
                        .padding(8)
                        .width(Fill),
                    button(text("✨  Discovery").size(14))
                        .style(nav_style(self.is_nav(&Route::Discovery)))
                        .on_press(Message::Navigate(Route::Discovery))
                        .padding(8)
                        .width(Fill),
                    button(text("📻  Radio").size(14))
                        .style(nav_style(radio_active))
                        .on_press(radio_msg)
                        .padding(8)
                        .width(Fill),
                    button(text("⚙  Settings").size(14))
                        .style(nav_style(self.is_nav(&Route::Settings)))
                        .on_press(Message::Navigate(Route::Settings))
                        .padding(8)
                        .width(Fill),
                ]
                .spacing(2),
                text(format!("Liked · {liked_n}")).size(12).color(MUTED),
                text("Playlists").size(12).color(MUTED),
                column(playlists).spacing(0),
            ]
            .spacing(10),
        )
        .style(sidebar_bg)
        .padding(12)
        .width(225)
        .height(Fill)
        .into()
    }

    fn right_panel_view(&self) -> iced::Element<'_, Message> {
        use iced::widget::{button, column, container, row, text};
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
                .padding(2)
                .into()
            })
            .collect();
        container(
            column![
                now,
                text("Next up").size(13).color(MUTED),
                column(upcoming).spacing(0),
                button(text(format!("Clear queue ({})", self.queue.len())).size(12))
                    .style(button::text)
                    .on_press(Message::QueueClear)
                    .padding(4),
            ]
            .spacing(8),
        )
        .style(sidebar_bg)
        .padding(12)
        .width(275)
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
            button, checkbox, column, container, row, scrollable, slider, text, text_input,
        };
        use iced::Fill;
        let body: iced::Element<'_, Message> = match &self.route {
            Route::Home => {
                let mut seen = std::collections::HashSet::new();
                let mut recent: Vec<Track> = Vec::new();
                for id in self.history.iter().rev() {
                    if recent.len() >= 6 {
                        break;
                    }
                    if seen.insert(id.clone()) {
                        if let Some(t) = self.find_track(id) {
                            recent.push(t);
                        }
                    }
                }
                let picks: Vec<Track> = recent;
                let grid: iced::Element<'_, Message> = if picks.is_empty() {
                    column![
                        text("Your recent plays will appear here.").color(MUTED),
                        button(text("🔍 Search YouTube Music").size(13))
                            .style(green_play)
                            .on_press(Message::Navigate(Route::Search))
                            .padding(8),
                    ]
                    .spacing(8)
                    .into()
                } else {
                    // One card per row (robust in Iced's box layout; reads like
                    // Spotify's quick-pick list at this window size).
                    column(
                        picks
                            .iter()
                            .map(|t| {
                                button(
                                    row![
                                        text(format!("{} — {}", t.title, t.artist))
                                            .size(14)
                                            .width(Fill),
                                        text("▶").size(13).color(SPOT_GREEN),
                                    ]
                                    .spacing(8)
                                    .padding(12)
                                    .align_y(iced::alignment::Vertical::Center),
                                )
                                .style(card_button)
                                .on_press(Message::EnqueueTrack(t.id.clone()))
                                .padding(0)
                                .into()
                            })
                            .collect::<Vec<_>>(),
                    )
                    .spacing(8)
                    .into()
                };
                let home_hits: iced::Element<'_, Message> = if self.searching {
                    text("Searching YouTube Music…").color(MUTED).into()
                } else if self.search_query.trim().is_empty() {
                    text("").size(1).into()
                } else if self.results.tracks.is_empty() {
                    text("No matches — try another search.").color(MUTED).into()
                } else {
                    let top: Vec<iced::Element<'_, Message>> = self
                        .results
                        .tracks
                        .iter()
                        .take(6)
                        .map(|t| self.track_row(t))
                        .collect();
                    column![
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
                    row![
                        text_input("Search YouTube Music…", &self.search_query)
                            .on_input(Message::HomeSearchChanged)
                            .on_submit(Message::SearchStay)
                            .width(Fill),
                        button(text("🔍 Search").size(14))
                            .style(green_play)
                            .on_press(Message::SearchStay)
                            .padding(8),
                    ]
                    .spacing(8)
                    .align_y(iced::alignment::Vertical::Center),
                    home_search_results,
                    text("Jump back in").size(19),
                    grid,
                    text("Made for you").size(19),
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
                    text_input("What do you want to play?", &self.search_query)
                        .on_input(Message::SearchChanged)
                        .on_submit(Message::Search)
                        .width(Fill),
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
                            "Track {} of {} · Shuffle {} · Repeat {:?}",
                            pos.map(|p| p + 1).unwrap_or(0),
                            self.queue.len(),
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
                        use iced::widget::{button, row, text};
                        use iced::Fill;
                        let is_current = Some(i) == pos;
                        let marker = if is_current { "● " } else { "" };
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
                            button(text("✕").size(12))
                                .style(button::text)
                                .on_press(Message::QueueRemove(i))
                                .padding(5),
                        ]
                        .spacing(4)
                        .align_y(iced::alignment::Vertical::Center)
                        .padding(2)
                        .into()
                    })
                    .collect();
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
                    column(rows).spacing(2),
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
                    text(format!("EQ: {current_eq}")).size(12).color(MUTED),
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
                                    text(format!(
                                        "{} {}",
                                        if is_current { "●" } else { " " },
                                        l.text
                                    ))
                                    .size(if is_current { 16 } else { 14 })
                                    .color(if is_current {
                                        SPOT_GREEN
                                    } else {
                                        iced::Color::WHITE
                                    })
                                    .into()
                                })
                                .collect::<Vec<_>>(),
                        )
                        .spacing(5)
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
                            .on_press(Message::EnqueueTrack(id.clone()))
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
        let mut nav_row = row![];
        nav_row = nav_row.push(if can_back {
            button(text("‹").size(16))
                .style(button::text)
                .on_press(Message::NavBack)
                .padding(6)
        } else {
            button(text("‹").size(16)).padding(6)
        });
        nav_row = nav_row.push(if can_forward {
            button(text("›").size(16))
                .style(button::text)
                .on_press(Message::NavForward)
                .padding(6)
        } else {
            button(text("›").size(16)).padding(6)
        });
        column![
            nav_row.spacing(2),
            scrollable(container(body).padding(20).width(Fill)).height(Fill),
        ]
        .spacing(4)
        .into()
    }

    pub fn view(&self) -> iced::Element<'_, Message> {
        use iced::widget::{column, container, row};
        use iced::Fill;
        let main = row![
            self.sidebar_view(),
            self.center_view(),
            self.right_panel_view(),
        ]
        .spacing(8)
        .height(Fill);
        container(column![main, self.player_view()].spacing(8).height(Fill))
            .style(app_bg)
            .padding(8)
            .into()
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
        // Failures report without clobbering landed results.
        let _ = s.update(Message::SearchCompleted {
            query: "x".into(),
            result: Err("offline".into()),
        });
        assert!(s.status.contains("offline"));
        assert_eq!(s.results.tracks.len(), 1);
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
