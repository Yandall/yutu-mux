//! yutu-ui: native Iced app (Phase 3 — basic YT Music views on MockBackend).

use std::collections::HashSet;
use yutu_core::{Album, LibraryStore, MockBackend, MusicBackend, Playlist, SearchResults, Track};
use yutu_player::{Queue, RodioBackend};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Route {
    #[default]
    Home,
    Search,
    Queue,
    Library,
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
    paused: bool,
    backend: MockBackend,
    audio: Option<RodioBackend>,
    library_path: String,
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

/// XDG data dir without extra deps: $XDG_DATA_HOME or ~/.local/share.
pub fn default_library_path() -> String {
    let base = std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        format!("{home}/.local/share")
    });
    format!("{base}/yutu-mux/library.db")
}

fn load_persisted(path: &str) -> (HashSet<String>, Vec<String>) {
    let store = match LibraryStore::open(path) {
        Ok(s) => s,
        Err(_) => return (HashSet::new(), Vec::new()),
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
    (liked, history)
}

impl AppState {
    pub fn new() -> (Self, iced::Task<Message>) {
        let backend = MockBackend::new();
        let results = backend.search("");
        let audio = match RodioBackend::try_new() {
            Ok(a) => Some(a),
            Err(e) => {
                eprintln!("audio unavailable: {e}");
                None
            }
        };
        let library_path = default_library_path();
        if let Some(parent) = std::path::Path::new(&library_path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let (liked, history) = load_persisted(&library_path);
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
                user_playlists: Vec::new(),
                new_playlist_name: String::new(),
                playlist_seq: 0,
                paused: false,
                backend,
                audio,
                library_path,
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

    fn enqueue(&mut self, track: Track) {
        self.status = format!("Queued: {} — {}", track.title, track.artist);
        self.history.push(track.id.clone());
        self.persist_play(&track.id);
        self.queue.push(track);
        if let Some(a) = &self.audio {
            a.play_test_tone();
            a.set_volume(self.volume);
        } else {
            self.status.push_str(" (no audio device)");
        }
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

    fn play_index(&mut self, i: usize) {
        if let Some(t) = self.results.tracks.get(i).cloned() {
            self.enqueue(t);
        }
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

    pub fn update(&mut self, message: Message) -> iced::Task<Message> {
        match message {
            Message::Navigate(r) => self.navigate(r),
            Message::SearchChanged(q) => {
                self.search_query = q;
                self.results = self.backend.search(&self.search_query);
            }
            Message::Search => {
                self.results = self.backend.search(&self.search_query);
                self.route = Route::Search;
            }
            Message::PlayTrack(i) => self.play_index(i),
            Message::EnqueueTrack(id) => match find_track(&id) {
                Some(t) => self.enqueue(t),
                None => self.status = format!("Track not found: {id}."),
            },
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
                    self.new_playlist_name.clear();
                    self.status = format!("Created playlist {title} ({id}).");
                }
            }
            Message::AddToPlaylist {
                playlist_id,
                track_id,
            } => match find_track(&track_id) {
                None => self.status = format!("Track not found: {track_id}."),
                Some(track) => match self.user_playlists.iter_mut().find(|p| p.id == playlist_id) {
                    None => {
                        self.status =
                            format!("Playlist {playlist_id} is built-in (read-only) or missing.");
                    }
                    Some(p) => {
                        if p.tracks.iter().any(|t| t.id == track.id) {
                            self.status = format!("Already in {}: {}.", p.title, track.title);
                        } else {
                            p.tracks.push(track.clone());
                            self.status = format!("Added {} to {}.", track.title, p.title);
                        }
                    }
                },
            },
            Message::RemoveFromPlaylist {
                playlist_id,
                track_id,
            } => match self.user_playlists.iter_mut().find(|p| p.id == playlist_id) {
                None => {
                    self.status =
                        format!("Playlist {playlist_id} is built-in (read-only) or missing.");
                }
                Some(p) => {
                    p.tracks.retain(|t| t.id != track_id);
                    self.status = format!("Removed {track_id} from {}.", p.title);
                }
            },
            Message::DeletePlaylist(id) => {
                let before = self.user_playlists.len();
                self.user_playlists.retain(|p| p.id != id);
                if self.user_playlists.len() < before {
                    if self.route == Route::PlaylistDetail(id.clone()) {
                        self.route = Route::Library;
                    }
                    self.status = format!("Deleted playlist {id}.");
                } else {
                    self.status = format!("Playlist {id} is built-in and can't be deleted.");
                }
            }
        }
        iced::Task::none()
    }

    pub fn view(&self) -> iced::Element<'_, Message> {
        use iced::widget::{button, column, row, scrollable, slider, text, text_input};
        use iced::{Fill, Shrink};

        let radio_seed = self
            .queue
            .current()
            .map(|t| t.id.clone())
            .unwrap_or_else(|| "m1".into());
        let nav = row![
            button("Home").on_press(Message::Navigate(Route::Home)),
            button("Search").on_press(Message::Navigate(Route::Search)),
            button("Queue").on_press(Message::Navigate(Route::Queue)),
            button("Library").on_press(Message::Navigate(Route::Library)),
            button("Settings").on_press(Message::Navigate(Route::Settings)),
            button("Radio").on_press(Message::StartRadio(radio_seed)),
        ]
        .spacing(8);

        let body: iced::Element<'_, Message> = match &self.route {
            Route::Home => {
                let picks: Vec<iced::Element<'_, Message>> = self
                    .results
                    .tracks
                    .iter()
                    .take(5)
                    .enumerate()
                    .map(|(i, t)| {
                        row![
                            text(format!("{} — {}", t.title, t.artist)).width(Fill),
                            button(if self.liked.contains(&t.id) {
                                "♥"
                            } else {
                                "♡"
                            })
                            .on_press(Message::Like(t.id.clone())),
                            button("Lyrics").on_press(Message::OpenLyrics(t.id.clone())),
                            button("Radio").on_press(Message::StartRadio(t.id.clone())),
                            button("Play").on_press(Message::PlayTrack(i)),
                        ]
                        .spacing(8)
                        .into()
                    })
                    .collect();
                let artists: Vec<iced::Element<'_, Message>> = self
                    .results
                    .artists
                    .iter()
                    .map(|a| {
                        row![
                            text(&a.name).width(Fill),
                            button("Open").on_press(Message::OpenArtist(a.id.clone())),
                        ]
                        .spacing(8)
                        .into()
                    })
                    .collect();
                let playlists: Vec<iced::Element<'_, Message>> = self
                    .all_playlists()
                    .into_iter()
                    .map(|p| {
                        row![
                            text(format!("{} ({} tracks)", p.title, p.tracks.len())).width(Fill),
                            button("Open").on_press(Message::OpenPlaylist(p.id.clone())),
                        ]
                        .spacing(8)
                        .into()
                    })
                    .collect();
                column![
                    text("Quick picks (mock)").size(20),
                    column(picks).spacing(4),
                    text("Artists").size(20),
                    column(artists).spacing(4),
                    text("Playlists").size(20),
                    column(playlists).spacing(4),
                ]
                .spacing(8)
                .into()
            }
            Route::Search => {
                let input = text_input("Search mock library…", &self.search_query)
                    .on_input(Message::SearchChanged)
                    .on_submit(Message::Search);
                let rows: Vec<iced::Element<'_, Message>> = self
                    .results
                    .tracks
                    .iter()
                    .enumerate()
                    .map(|(i, t)| {
                        row![
                            text(format!("{} — {} [{}]", t.title, t.artist, t.id)).width(Fill),
                            button(if self.liked.contains(&t.id) {
                                "♥"
                            } else {
                                "♡"
                            })
                            .on_press(Message::Like(t.id.clone())),
                            button("Artist").on_press(Message::OpenArtist(t.artist.clone())),
                            button("Lyrics").on_press(Message::OpenLyrics(t.id.clone())),
                            button("Radio").on_press(Message::StartRadio(t.id.clone())),
                            button("Play").on_press(Message::PlayTrack(i)),
                        ]
                        .spacing(8)
                        .into()
                    })
                    .collect();
                let album_rows: Vec<iced::Element<'_, Message>> = self
                    .results
                    .albums
                    .iter()
                    .map(|a| {
                        row![
                            text(format!("{} — {} [{}]", a.title, a.artist, a.id)).width(Fill),
                            button("Open").on_press(Message::OpenAlbum(a.id.clone())),
                        ]
                        .spacing(8)
                        .into()
                    })
                    .collect();
                let artist_rows: Vec<iced::Element<'_, Message>> = self
                    .results
                    .artists
                    .iter()
                    .map(|a| {
                        row![
                            text(format!("{} [{}]", a.name, a.id)).width(Fill),
                            button("Open").on_press(Message::OpenArtist(a.id.clone())),
                        ]
                        .spacing(8)
                        .into()
                    })
                    .collect();
                column![
                    input,
                    button("Search").on_press(Message::Search),
                    text("Tracks").size(18),
                    scrollable(column(rows).spacing(4)),
                    text("Albums").size(18),
                    column(album_rows).spacing(4),
                    text("Artists").size(18),
                    column(artist_rows).spacing(4),
                ]
                .spacing(8)
                .into()
            }
            Route::Queue => {
                let rows: Vec<iced::Element<'_, Message>> = self
                    .queue
                    .items()
                    .iter()
                    .enumerate()
                    .map(|(i, t)| {
                        row![
                            text(format!("{}. {} — {}", i + 1, t.title, t.artist)).width(Fill),
                            button("Lyrics").on_press(Message::OpenLyrics(t.id.clone())),
                            button("↑").on_press(Message::QueueUp(i)),
                            button("↓").on_press(Message::QueueDown(i)),
                            button("✕").on_press(Message::QueueRemove(i)),
                        ]
                        .spacing(6)
                        .into()
                    })
                    .collect();
                let current_lyrics: iced::Element<'_, Message> = match self.queue.current() {
                    Some(t) => button("Lyrics for current")
                        .on_press(Message::OpenLyrics(t.id.clone()))
                        .into(),
                    None => text("No current track").into(),
                };
                column![
                    text("Up Next").size(20),
                    button("Clear queue").on_press(Message::QueueClear),
                    current_lyrics,
                    scrollable(column(rows).spacing(4)),
                ]
                .spacing(8)
                .into()
            }
            Route::Library => {
                let liked: Vec<iced::Element<'_, Message>> = self
                    .liked
                    .iter()
                    .map(|id| text(format!("♥ {id}")).into())
                    .collect();
                let hist: Vec<iced::Element<'_, Message>> = self
                    .history
                    .iter()
                    .rev()
                    .take(20)
                    .map(|id| text(format!("▶ {id}")).into())
                    .collect();
                let builtin: Vec<iced::Element<'_, Message>> = self
                    .backend
                    .playlists()
                    .into_iter()
                    .map(|p| {
                        row![
                            text(format!("{} ({} tracks)", p.title, p.tracks.len())).width(Fill),
                            button("Open").on_press(Message::OpenPlaylist(p.id.clone())),
                        ]
                        .spacing(8)
                        .into()
                    })
                    .collect();
                let mine: Vec<iced::Element<'_, Message>> = self
                    .user_playlists
                    .iter()
                    .map(|p| {
                        row![
                            text(format!("{} ({} tracks)", p.title, p.tracks.len())).width(Fill),
                            button("Open").on_press(Message::OpenPlaylist(p.id.clone())),
                            button("Delete").on_press(Message::DeletePlaylist(p.id.clone())),
                        ]
                        .spacing(8)
                        .into()
                    })
                    .collect();
                let new_name = self.new_playlist_name.clone();
                column![
                    row![
                        column![text("Liked").size(18), column(liked).spacing(2)].width(Fill),
                        column![text("History").size(18), column(hist).spacing(2)].width(Fill),
                    ]
                    .spacing(16),
                    text("Playlists (built-in)").size(18),
                    column(builtin).spacing(4),
                    text("My playlists").size(18),
                    column(mine).spacing(4),
                    row![
                        text_input("New playlist name…", &self.new_playlist_name)
                            .on_input(Message::NewPlaylistNameChanged)
                            .width(Fill),
                        button("Create").on_press(Message::CreatePlaylist(new_name)),
                    ]
                    .spacing(8),
                ]
                .spacing(8)
                .into()
            }
            Route::Settings => column![
                text("Settings").size(20),
                text(format!("Volume: {:.0}%", self.volume * 100.0)),
                slider(0.0..=1.0, self.volume, Message::SetVolume),
                text("Auth: anonymous (cookie import: Phase 4)."),
                text("Backend: MockBackend — Innertube behind `innertube` feature (Phase 4)."),
                text("Keys: Space play/pause, n next, p prev, / search."),
            ]
            .spacing(8)
            .into(),
            Route::Artist(id) => {
                let name = self.resolve_artist_name(id);
                let tracks = self.artist_tracks(&name);
                let rows: Vec<iced::Element<'_, Message>> = tracks
                    .iter()
                    .map(|t| {
                        row![
                            text(format!("{} — {}", t.title, t.artist)).width(Fill),
                            button(if self.liked.contains(&t.id) {
                                "♥"
                            } else {
                                "♡"
                            })
                            .on_press(Message::Like(t.id.clone())),
                            button("Lyrics").on_press(Message::OpenLyrics(t.id.clone())),
                            button("Radio").on_press(Message::StartRadio(t.id.clone())),
                            button("Queue").on_press(Message::EnqueueTrack(t.id.clone())),
                        ]
                        .spacing(8)
                        .into()
                    })
                    .collect();
                let albums: Vec<iced::Element<'_, Message>> = self
                    .artist_albums(&name)
                    .into_iter()
                    .map(|a| {
                        row![
                            text(format!(
                                "{}{}",
                                a.title,
                                a.year.map(|y| format!(" ({y})")).unwrap_or_default()
                            ))
                            .width(Fill),
                            button("Open").on_press(Message::OpenAlbum(a.id.clone())),
                        ]
                        .spacing(8)
                        .into()
                    })
                    .collect();
                column![
                    text(format!("Artist: {name}")).size(20),
                    button("Play all").on_press(Message::PlayAll),
                    text("Top tracks").size(18),
                    column(rows).spacing(4),
                    text("Albums").size(18),
                    column(albums).spacing(4),
                ]
                .spacing(8)
                .into()
            }
            Route::Album(id) => {
                let title = self.resolve_album_title(id);
                let tracks = self.album_tracks(id);
                let playlist_ids: Vec<(String, String)> = self
                    .user_playlists
                    .iter()
                    .map(|p| (p.id.clone(), p.title.clone()))
                    .collect();
                let rows: Vec<iced::Element<'_, Message>> = tracks
                    .iter()
                    .map(|t| {
                        let mut r = row![
                            text(format!("{} — {}", t.title, t.artist)).width(Fill),
                            button("Lyrics").on_press(Message::OpenLyrics(t.id.clone())),
                            button("Queue").on_press(Message::EnqueueTrack(t.id.clone())),
                        ]
                        .spacing(8);
                        for (pid, pname) in &playlist_ids {
                            r = r.push(button(text(format!("+{pname}"))).on_press(
                                Message::AddToPlaylist {
                                    playlist_id: pid.clone(),
                                    track_id: t.id.clone(),
                                },
                            ));
                        }
                        r.into()
                    })
                    .collect();
                column![
                    text(format!("Album: {title}")).size(20),
                    button("Play all").on_press(Message::PlayAll),
                    column(rows).spacing(4),
                ]
                .spacing(8)
                .into()
            }
            Route::PlaylistDetail(id) => match self.find_playlist(id) {
                None => column![text(format!("Playlist not found: {id}")).size(20)]
                    .spacing(8)
                    .into(),
                Some(p) => {
                    let pid = p.id.clone();
                    let rows: Vec<iced::Element<'_, Message>> = p
                        .tracks
                        .iter()
                        .map(|t| {
                            row![
                                text(format!("{} — {}", t.title, t.artist)).width(Fill),
                                button("Lyrics").on_press(Message::OpenLyrics(t.id.clone())),
                                button("Queue").on_press(Message::EnqueueTrack(t.id.clone())),
                                button("Remove").on_press(Message::RemoveFromPlaylist {
                                    playlist_id: pid.clone(),
                                    track_id: t.id.clone(),
                                }),
                            ]
                            .spacing(8)
                            .into()
                        })
                        .collect();
                    column![
                        text(format!("Playlist: {}", p.title)).size(20),
                        button("Play all").on_press(Message::PlayAll),
                        column(rows).spacing(4),
                    ]
                    .spacing(8)
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
                            .map(|l| text(l.text).into())
                            .collect::<Vec<_>>(),
                    )
                    .spacing(4)
                    .into(),
                    None => text("No lyrics available (mock has lyrics for m4 only).").into(),
                };
                column![
                    text(format!("Lyrics: {title}")).size(20),
                    button("Play").on_press(Message::EnqueueTrack(id.clone())),
                    button("Radio").on_press(Message::StartRadio(id.clone())),
                    content,
                ]
                .spacing(8)
                .into()
            }
            Route::Radio(seed) => {
                let seq = radio_for(seed);
                let seed_label = find_track(seed)
                    .map(|t| format!("{} — {}", t.title, t.artist))
                    .unwrap_or_else(|| seed.clone());
                let rows: Vec<iced::Element<'_, Message>> = seq
                    .iter()
                    .map(|t| {
                        row![
                            text(format!("{} — {}", t.title, t.artist)).width(Fill),
                            button("Lyrics").on_press(Message::OpenLyrics(t.id.clone())),
                            button("Queue").on_press(Message::EnqueueTrack(t.id.clone())),
                        ]
                        .spacing(8)
                        .into()
                    })
                    .collect();
                column![
                    text(format!("Radio (seed: {seed_label})")).size(20),
                    row![
                        button("Start radio (append)").on_press(Message::StartRadio(seed.clone())),
                        button("Play all (append)").on_press(Message::PlayAll),
                    ]
                    .spacing(8),
                    column(rows).spacing(4),
                ]
                .spacing(8)
                .into()
            }
        };

        let now_playing = self
            .queue
            .current()
            .map(|t| format!("{} — {}", t.title, t.artist))
            .unwrap_or_else(|| "Nothing queued".into());
        let pause_label = if self.paused { "▶ resume" } else { "⏸" };
        let player_bar = row![
            button("⏮").on_press(Message::Prev),
            button("▶ tone").on_press(Message::PlayTone),
            button(pause_label).on_press(Message::TogglePause),
            button("⏹").on_press(Message::Stop),
            button("⏭").on_press(Message::Next),
            text(now_playing).width(Fill),
            text(format!(
                "Q:{} V:{:.0}%",
                self.queue.len(),
                self.volume * 100.0
            ))
            .width(Shrink),
        ]
        .spacing(8);

        column![
            nav,
            scrollable(body).height(Fill),
            text(&self.status).size(13),
            player_bar
        ]
        .spacing(10)
        .padding(12)
        .into()
    }
}

pub fn run() -> iced::Result {
    iced::application(AppState::new, AppState::update, AppState::view)
        .title("yutu-mux")
        .subscription(AppState::subscription)
        .run()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigate() {
        let (mut s, _) = AppState::new();
        s.navigate(Route::Search);
        assert_eq!(s.route, Route::Search);
    }

    #[test]
    fn search_updates_results() {
        let (mut s, _) = AppState::new();
        let _ = s.update(Message::SearchChanged("yesterday".into()));
        assert_eq!(s.results.tracks.len(), 1);
    }

    #[test]
    fn like_toggles() {
        let (mut s, _) = AppState::new();
        let _ = s.update(Message::Like("m1".into()));
        assert!(s.liked.contains("m1"));
        let _ = s.update(Message::Like("m1".into()));
        assert!(!s.liked.contains("m1"));
    }

    #[test]
    fn playlist_crud() {
        let (mut s, _) = AppState::new();
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
        let (mut s, _) = AppState::new();
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
        let (mut s, _) = AppState::new();
        let _ = s.update(Message::OpenAlbum("a1".into()));
        let _ = s.update(Message::PlayAll);
        assert_eq!(s.queue.len(), 3); // Abbey Road: m1, m2, m3.
        let _ = s.update(Message::OpenPlaylist("p1".into()));
        let _ = s.update(Message::PlayAll);
        assert_eq!(s.queue.len(), 6);
    }

    #[test]
    fn enqueue_track_and_toggle_pause() {
        let (mut s, _) = AppState::new();
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
}
