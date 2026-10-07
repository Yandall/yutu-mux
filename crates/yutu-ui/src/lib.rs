//! yutu-ui: native Iced app (Phase 3 — basic YT Music views on MockBackend).

use std::collections::HashSet;
use yutu_core::{LibraryStore, MockBackend, MusicBackend, SearchResults};
use yutu_player::{Queue, RodioBackend};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Route {
    #[default]
    Home,
    Search,
    Library,
    Settings,
}

#[derive(Debug, Clone)]
pub enum Message {
    Navigate(Route),
    SearchChanged(String),
    Search,
    PlayTrack(usize),
    PlayTone,
    Pause,
    Stop,
    Next,
    Prev,
    Like(String),
    SetVolume(f32),
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
    backend: MockBackend,
    audio: Option<RodioBackend>,
    library_path: String,
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

    fn play_index(&mut self, i: usize) {
        if let Some(t) = self.results.tracks.get(i).cloned() {
            self.status = format!("Queued: {} — {}", t.title, t.artist);
            self.history.push(t.id.clone());
            self.persist_play(&t.id);
            self.queue.push(t);
            if let Some(a) = &self.audio {
                a.play_test_tone();
                a.set_volume(self.volume);
            } else {
                self.status.push_str(" (no audio device)");
            }
        }
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
                    self.status = "Paused.".into();
                }
            }
            Message::Stop => {
                if let Some(a) = &self.audio {
                    a.stop();
                    self.status = "Stopped.".into();
                }
            }
            Message::Next => {
                if self.queue.next_track().is_some() {
                    if let Some(a) = &self.audio {
                        a.play_test_tone();
                    }
                    self.status = "Next.".into();
                }
            }
            Message::Prev => {
                if self.queue.prev_track().is_some() {
                    if let Some(a) = &self.audio {
                        a.play_test_tone();
                    }
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
        }
        iced::Task::none()
    }

    pub fn view(&self) -> iced::Element<'_, Message> {
        use iced::widget::{button, column, row, scrollable, slider, text, text_input};
        use iced::{Fill, Shrink};

        let nav = row![
            button("Home").on_press(Message::Navigate(Route::Home)),
            button("Search").on_press(Message::Navigate(Route::Search)),
            button("Library").on_press(Message::Navigate(Route::Library)),
            button("Settings").on_press(Message::Navigate(Route::Settings)),
        ]
        .spacing(8);

        let body: iced::Element<'_, Message> = match self.route {
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
                            button("Play").on_press(Message::PlayTrack(i)),
                        ]
                        .spacing(8)
                        .into()
                    })
                    .collect();
                column![
                    text("Quick picks (mock)").size(20),
                    column(picks).spacing(4)
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
                            button("Play").on_press(Message::PlayTrack(i)),
                        ]
                        .spacing(8)
                        .into()
                    })
                    .collect();
                column![
                    input,
                    button("Search").on_press(Message::Search),
                    scrollable(column(rows).spacing(4))
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
                row![
                    column![text("Liked").size(18), column(liked).spacing(2)].width(Fill),
                    column![text("History").size(18), column(hist).spacing(2)].width(Fill),
                ]
                .spacing(16)
                .into()
            }
            Route::Settings => column![
                text("Settings").size(20),
                text(format!("Volume: {:.0}%", self.volume * 100.0)),
                slider(0.0..=1.0, self.volume, Message::SetVolume),
                text("Auth: anonymous (cookie import: Phase 4)."),
                text("Backend: MockBackend — Innertube behind `innertube` feature (Phase 4)."),
            ]
            .spacing(8)
            .into(),
        };

        let now_playing = self
            .queue
            .current()
            .map(|t| format!("{} — {}", t.title, t.artist))
            .unwrap_or_else(|| "Nothing queued".into());
        let player_bar = row![
            button("⏮").on_press(Message::Prev),
            button("▶ tone").on_press(Message::PlayTone),
            button("⏸").on_press(Message::Pause),
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
}
