//! yutu-ui: native Iced app (Phase 3 — basic YT Music views on MockBackend).

use std::collections::HashSet;
use yutu_core::{
    forgotten_favorites, should_scrobble, weekly_mix, Album, HistoryEntry, LibraryStore,
    MockBackend, MusicBackend, Playlist, SearchResults, Track,
};
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
    PlayTone,
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
    StreamIdChanged(String),
    PlayStreamId,
    StreamResolved {
        video_id: String,
        result: Result<Vec<u8>, String>,
    },
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
    /// Video-ID input for real-stream playback (innertube feature).
    pub stream_id_input: String,
    paused: bool,
    backend: MockBackend,
    audio: Option<RodioBackend>,
    library_path: String,
    /// MPRIS handle for live desktop propagation (`None` in tests/headless).
    mpris: Option<yutu_desktop::SharedPlayer>,
}

/// Deterministic mock radio: seed first, then same-artist tracks, then the rest.
pub fn radio_for(seed_id: &str) -> Vec<Track> {
    let fixtures = MockBackend::fixture_tracks();
    let start = fixtures.iter().position(|t| t.id == seed_id).unwrap_or(0);
    let seed_artist = fixtures
        .get(start)
        .map(|t| t.artist.clone())
        .unwrap_or_default();
    let mut seq = Vec::with_capacity(fixtures.len());
    let seed_actual = fixtures
        .get(start)
        .map(|t| t.id.clone())
        .unwrap_or_default();
    if let Some(t) = fixtures.get(start) {
        seq.push(t.clone());
    }
    for t in &fixtures {
        if t.id != seed_actual && t.artist == seed_artist {
            seq.push(t.clone());
        }
    }
    for t in &fixtures {
        if t.id != seed_actual && t.artist != seed_artist {
            seq.push(t.clone());
        }
    }
    seq
}

fn find_track(id: &str) -> Option<Track> {
    MockBackend::fixture_tracks()
        .into_iter()
        .find(|t| t.id == id)
}

/// Offline builds: only the yt-dlp path exists; without the binary installed
/// there is nothing else to try.
#[cfg(not(feature = "innertube"))]
async fn resolve_via_raw(video_id: &str) -> Result<Vec<u8>, String> {
    Err(format!(
        "yt-dlp not found and this is an offline build ({video_id}). Install yt-dlp or rebuild with `--features innertube`."
    ))
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
    // Likes: probe fixture ids (mock phase); real phase lists all.
    let mut liked = HashSet::new();
    for t in MockBackend::fixture_tracks() {
        if store.is_liked(&t.id).unwrap_or(false) {
            liked.insert(t.id.clone());
        }
    }
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
        let backend = MockBackend::new();
        let results = backend.search("");
        let audio = match RodioBackend::try_new() {
            Ok(a) => Some(a),
            Err(e) => {
                eprintln!("audio unavailable: {e}");
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
                results,
                queue: Queue::default(),
                liked,
                history,
                volume: 0.8,
                status: "Ready — mock library, no network yet.".into(),
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
                stream_id_input: String::new(),
                paused: false,
                backend,
                audio,
                library_path,
                mpris: None,
            },
            iced::Task::none(),
        )
    }

    pub fn navigate(&mut self, route: Route) {
        self.route = route;
    }

    pub fn subscription(&self) -> iced::Subscription<Message> {
        iced::keyboard::listen().filter_map(|event| match event {
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
        })
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

    fn enqueue(&mut self, track: Track) -> iced::Task<Message> {
        self.status = format!("Queued: {} — {}", track.title, track.artist);
        self.history.push(track.id.clone());
        self.persist_play(&track.id);
        let scrobble = self.maybe_scrobble_task(&track);
        self.queue.push(track);
        if let Some(a) = &self.audio {
            a.play_test_tone();
            a.set_volume(self.volume);
        } else {
            self.status.push_str(" (no audio device)");
        }
        scrobble
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

    /// Bulk-append without spamming history (one entry-free push per track).
    fn queue_many(&mut self, tracks: Vec<Track>, label: &str) {
        let n = tracks.len();
        for t in tracks {
            self.queue.push(t);
        }
        if let Some(a) = &self.audio {
            a.play_test_tone();
            a.set_volume(self.volume);
        }
        self.paused = false;
        self.status = format!("{label}: queued {n} track(s).");
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
        match find_track(id) {
            Some(t) => self.enqueue(t),
            None => {
                self.status = format!("Track not found: {id}.");
                iced::Task::none()
            }
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
    }

    /// Validate + select an EQ preset (in-memory only; never persisted).
    fn select_eq_preset(&mut self, name: &str) {
        match EqPreset::preset_by_name(name) {
            Some(p) => match p.validate() {
                Ok(()) => {
                    // Honest copy: the rodio path has no EQ node yet, so the
                    // selection only takes effect with the mpv backend.
                    self.eq_preset = p.name.clone();
                    self.status = format!("EQ: {} — takes effect with mpv backend", p.name);
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

    /// Weekly Mix from persisted history + in-memory likes + fixture catalog.
    pub fn weekly_mix_tracks(&self) -> Vec<Track> {
        weekly_mix(
            &self.discovery_history(),
            &self.liked,
            &MockBackend::fixture_tracks(),
            10,
            now_unix(),
        )
    }

    /// Forgotten Favorites from in-memory likes + persisted history + catalog.
    pub fn forgotten_tracks(&self) -> Vec<Track> {
        forgotten_favorites(
            &self.liked,
            &self.discovery_history(),
            &MockBackend::fixture_tracks(),
            now_unix(),
        )
    }

    fn all_playlists(&self) -> Vec<Playlist> {
        let mut all = self.backend.playlists();
        all.extend(self.user_playlists.iter().cloned());
        all
    }

    fn find_playlist(&self, id: &str) -> Option<Playlist> {
        self.all_playlists().into_iter().find(|p| p.id == id)
    }

    /// Accept an artist id ("ar1") or name ("The Beatles").
    fn resolve_artist_name(&self, id_or_name: &str) -> String {
        if let Some(a) = self.results.artists.iter().find(|a| a.id == id_or_name) {
            return a.name.clone();
        }
        if self.results.artists.iter().any(|a| a.name == id_or_name) {
            return id_or_name.to_string();
        }
        if MockBackend::fixture_tracks()
            .iter()
            .any(|t| t.artist == id_or_name)
        {
            return id_or_name.to_string();
        }
        self.results
            .artists
            .first()
            .map(|a| a.name.clone())
            .unwrap_or_else(|| id_or_name.to_string())
    }

    fn artist_tracks(&self, artist: &str) -> Vec<Track> {
        MockBackend::fixture_tracks()
            .into_iter()
            .filter(|t| t.artist == artist)
            .collect()
    }

    fn artist_albums(&self, artist: &str) -> Vec<Album> {
        let mut albums: Vec<Album> = self
            .backend
            .search("")
            .albums
            .into_iter()
            .filter(|a| a.artist == artist)
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

    /// Accept an album id ("a1") or title ("Abbey Road").
    fn resolve_album_title(&self, id_or_title: &str) -> String {
        self.backend
            .search("")
            .albums
            .iter()
            .find(|a| a.id == id_or_title)
            .map(|a| a.title.clone())
            .unwrap_or_else(|| id_or_title.to_string())
    }

    fn album_tracks(&self, id_or_title: &str) -> Vec<Track> {
        let title = self.resolve_album_title(id_or_title);
        MockBackend::fixture_tracks()
            .into_iter()
            .filter(|t| t.album.as_deref() == Some(title.as_str()))
            .collect()
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
            Message::SearchChanged(q) => {
                self.search_query = q;
                self.results = self.backend.search(&self.search_query);
            }
            Message::Search => {
                self.results = self.backend.search(&self.search_query);
                self.route = Route::Search;
            }
            Message::NewPlaylistNameChanged(n) => {
                self.new_playlist_name = n;
            }
            Message::PlayTone => {
                if let Some(a) = &self.audio {
                    a.play_test_tone();
                    self.status = "Playing test tone (real streams: Phase 4).".into();
                } else {
                    self.status = "No audio device.".into();
                }
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
                if self.queue.next_track().is_some() {
                    if let Some(a) = &self.audio {
                        a.play_test_tone();
                    }
                    self.paused = false;
                    self.status = "Next.".into();
                }
            }
            Message::Prev => {
                if self.queue.prev_track().is_some() {
                    if let Some(a) = &self.audio {
                        a.play_test_tone();
                    }
                    self.paused = false;
                    self.status = "Previous.".into();
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
            Message::OpenArtist(id) => self.route = Route::Artist(id),
            Message::OpenAlbum(id) => self.route = Route::Album(id),
            Message::OpenPlaylist(id) => self.route = Route::PlaylistDetail(id),
            Message::OpenLyrics(id) => self.route = Route::Lyrics(id),
            Message::StartRadio(seed) => {
                let seq = radio_for(&seed);
                let label = format!("Radio (seed {seed})");
                self.route = Route::Radio(seed);
                self.queue_many(seq, &label);
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
                    Route::Radio(seed) => (radio_for(seed), format!("Radio (seed {seed})")),
                    Route::Discovery => (self.weekly_mix_tracks(), "Weekly Mix".to_string()),
                    _ => (self.results.tracks.clone(), "Search results".to_string()),
                };
                if tracks.is_empty() {
                    self.status = format!("{label}: nothing to queue.");
                } else {
                    self.queue_many(tracks, &label);
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
            } => match find_track(&track_id) {
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
                        self.route = Route::Library;
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
                    self.queue_many(tracks, "Weekly Mix");
                }
            }
            Message::PlayForgotten => {
                let tracks = self.forgotten_tracks();
                if tracks.is_empty() {
                    self.status = "Forgotten Favorites: nothing to queue.".into();
                } else {
                    self.queue_many(tracks, "Forgotten Favorites");
                }
            }
            Message::StreamIdChanged(id) => {
                self.stream_id_input = id;
            }
            Message::PlayStreamId => {
                let id = self.stream_id_input.trim().to_string();
                if id.is_empty() {
                    self.status = "Paste a YouTube video ID first.".into();
                } else {
                    self.status = format!("Resolving stream {id}…");
                    self.history.push(format!("stream:{id}"));
                    self.publish_mpris();
                    return Self::resolve_task(id);
                }
            }
            Message::StreamResolved { video_id, result } => match result {
                Ok(bytes) => match &self.audio {
                    None => {
                        self.status = format!("Stream {video_id} fetched, but no audio device.");
                    }
                    Some(audio) => match audio.play_bytes(bytes) {
                        Ok(()) => {
                            self.status = format!("Playing real stream {video_id}.");
                            self.paused = false;
                        }
                        Err(e) => {
                            self.status = format!("Decode failed for {video_id}: {e}");
                        }
                    },
                },
                Err(e) => {
                    self.status = format!("Stream {video_id} failed: {e}");
                }
            },
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

    pub fn theme(&self) -> iced::Theme {
        iced::Theme::Dark
    }

    fn radio_seed(&self) -> String {
        self.queue
            .current()
            .map(|t| t.id.clone())
            .unwrap_or_else(|| "m1".into())
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
                        .on_press(Message::StartRadio(seed))
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
        let now: iced::Element<'_, Message> = match &current {
            Some(t) => column![
                text("Now playing").size(13).color(MUTED),
                text(t.title.clone()).size(15),
                text(t.artist.clone()).size(12).color(MUTED),
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
                let marker = if current.as_ref().map(|c| c.id == t.id).unwrap_or(false) && i == 0 {
                    "● "
                } else {
                    ""
                };
                row![text(format!("{marker}{} — {}", t.title, t.artist))
                    .size(12)
                    .width(Fill),]
                .padding(3)
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
        let title = current
            .as_ref()
            .map(|t| t.title.clone())
            .unwrap_or_else(|| "Nothing playing".into());
        let artist = current
            .as_ref()
            .map(|t| t.artist.clone())
            .unwrap_or_else(|| "Pick something from Home or Search".into());
        let play_symbol = if self.paused { "▶" } else { "⏸" };
        container(
            column![
                text(self.status.clone()).size(12).color(MUTED),
                row![
                    column![text(title).size(14), text(artist).size(12).color(MUTED),]
                        .spacing(1)
                        .width(Fill),
                    row![
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
                    ]
                    .spacing(6)
                    .align_y(iced::alignment::Vertical::Center),
                    row![
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
                    .align_y(iced::alignment::Vertical::Center),
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
                let picks: Vec<Track> = self.results.tracks.iter().take(6).cloned().collect();
                let grid: iced::Element<'_, Message> = if picks.is_empty() {
                    text("No tracks — try Search.").color(MUTED).into()
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
                let weekly_n = self.weekly_mix_tracks().len();
                let forgotten_n = self.forgotten_tracks().len();
                column![
                    text("Good evening").size(24),
                    // Real playback lives here now, not buried in Settings.
                    container(
                        row![
                            text_input("Paste a YouTube video ID…", &self.stream_id_input)
                                .on_input(Message::StreamIdChanged)
                                .on_submit(Message::PlayStreamId)
                                .width(Fill),
                            button(text("▶ Play").size(14))
                                .style(green_play)
                                .on_press(Message::PlayStreamId)
                                .padding(8),
                        ]
                        .spacing(8)
                        .align_y(iced::alignment::Vertical::Center),
                    )
                    .style(card_bg)
                    .padding(10),
                    text("Quick picks").size(19),
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
                column![
                    text_input("What do you want to play?", &self.search_query)
                        .on_input(Message::SearchChanged)
                        .on_submit(Message::Search)
                        .width(Fill),
                    Self::section("Tracks", rows),
                    Self::section("Albums", albums),
                    Self::section("Artists", artists),
                ]
                .spacing(10)
                .into()
            }
            Route::Queue => {
                let rows: Vec<iced::Element<'_, Message>> = self
                    .queue
                    .items()
                    .iter()
                    .enumerate()
                    .map(|(i, t)| {
                        use iced::widget::{button, row, text};
                        use iced::Fill;
                        row![
                            text(format!("{}. {} — {}", i + 1, t.title, t.artist))
                                .size(14)
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
                        .padding(3)
                        .into()
                    })
                    .collect();
                column![
                    row![
                        text("Up Next").size(22).width(Fill),
                        button(text("Clear").size(13))
                            .style(button::text)
                            .on_press(Message::QueueClear)
                            .padding(6),
                    ]
                    .align_y(iced::alignment::Vertical::Center),
                    column(rows).spacing(2),
                ]
                .spacing(10)
                .into()
            }
            Route::Library => {
                let liked: Vec<iced::Element<'_, Message>> = self
                    .liked
                    .iter()
                    .filter_map(|id| find_track(id))
                    .map(|t| self.track_row(&t))
                    .collect();
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
                    Self::section("Liked", liked),
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
                    text(format!("EQ: {current_eq} (takes effect with mpv backend)"))
                        .size(12)
                        .color(MUTED),
                    text("Real stream (yt-dlp)").size(17),
                    text_input("YouTube video ID…", &self.stream_id_input)
                        .on_input(Message::StreamIdChanged)
                        .on_submit(Message::PlayStreamId),
                    button(text("Resolve + play").size(13))
                        .style(green_play)
                        .on_press(Message::PlayStreamId)
                        .padding(8),
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
                let title = find_track(id)
                    .map(|t| format!("{} — {}", t.title, t.artist))
                    .unwrap_or_else(|| id.clone());
                let content: iced::Element<'_, Message> = match self.backend.lyrics(id) {
                    Some(ly) => column(
                        ly.lines
                            .into_iter()
                            .map(|l| text(l.text).size(14).into())
                            .collect::<Vec<_>>(),
                    )
                    .spacing(4)
                    .into(),
                    None => text("No lyrics available.").color(MUTED).into(),
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
                let rows: Vec<iced::Element<'_, Message>> =
                    radio_for(seed).iter().map(|t| self.track_row(t)).collect();
                let seed_label = find_track(seed)
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
        scrollable(container(body).padding(20).width(Fill))
            .height(Fill)
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
    fn test_state() -> AppState {
        let n = TEST_DB_SEQ.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!("yutu-test-{}-{}.db", std::process::id(), n));
        let (s, _) = AppState::with_library_path(path.to_string_lossy().into_owned());
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
        let _ = s.update(Message::SearchChanged("yesterday".into()));
        assert_eq!(s.results.tracks.len(), 1);
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
        // Built-in playlists are read-only.
        let _ = s.update(Message::AddToPlaylist {
            playlist_id: "p1".into(),
            track_id: "m1".into(),
        });
        assert_eq!(s.find_playlist("p1").unwrap().tracks.len(), 3);
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
        // Built-in playlists can't be deleted.
        let _ = s.update(Message::DeletePlaylist("p1".into()));
        assert!(s.find_playlist("p1").is_some());
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
        let _ = s.update(Message::OpenArtist("ar1".into()));
        assert_eq!(s.route, Route::Artist("ar1".into()));
        let _ = s.update(Message::OpenAlbum("a1".into()));
        assert_eq!(s.route, Route::Album("a1".into()));
        let _ = s.update(Message::OpenPlaylist("p1".into()));
        assert_eq!(s.route, Route::PlaylistDetail("p1".into()));
        let _ = s.update(Message::OpenLyrics("m4".into()));
        assert_eq!(s.route, Route::Lyrics("m4".into()));
        let _ = s.update(Message::StartRadio("m1".into()));
        assert_eq!(s.route, Route::Radio("m1".into()));
        assert_eq!(s.queue.len(), 5);
    }

    #[test]
    fn play_all_queues_current_detail_view() {
        let mut s = test_state();
        let _ = s.update(Message::OpenAlbum("a1".into()));
        let _ = s.update(Message::PlayAll);
        assert_eq!(s.queue.len(), 3); // Abbey Road: m1, m2, m3.
        let _ = s.update(Message::OpenPlaylist("p1".into()));
        let _ = s.update(Message::PlayAll);
        assert_eq!(s.queue.len(), 6);
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
        assert!(s.status.contains("mpv backend"));
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
            assert!(s.status.starts_with("Queued:"));
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
    fn stream_id_flow_offline_build() {
        let mut s = test_state();
        let _ = s.update(Message::StreamIdChanged("dQw4w9WgXcQ".into()));
        assert_eq!(s.stream_id_input, "dQw4w9WgXcQ");
        // Empty input is rejected before any network.
        s.stream_id_input.clear();
        let _ = s.update(Message::PlayStreamId);
        assert!(s.status.contains("video ID"));
        // The resolve task runs async; synchronously we only see Resolving.
        s.stream_id_input = "dQw4w9WgXcQ".into();
        let _ = s.update(Message::PlayStreamId);
        assert!(s.status.contains("Resolving"));
    }

    #[test]
    fn stream_resolved_reports_errors() {
        let mut s = test_state();
        let _ = s.update(Message::StreamResolved {
            video_id: "vid".into(),
            result: Err("403 Forbidden".into()),
        });
        assert!(s.status.contains("vid") && s.status.contains("403"));
        // Empty bytes deterministically fail decoding (no device needed).
        let _ = s.update(Message::StreamResolved {
            video_id: "vid".into(),
            result: Ok(Vec::new()),
        });
        assert!(s.status.contains("vid"));
    }

    #[test]
    fn user_playlists_survive_restart() {
        let path =
            std::env::temp_dir().join(format!("yutu-test-restart-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let p = path.to_string_lossy().into_owned();
        let (mut s, _) = AppState::with_library_path(p.clone());
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
}
