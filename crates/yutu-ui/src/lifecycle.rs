//! yutu-ui lifecycle: app lifecycle, navigation and subscriptions (split from lib.rs, no behavior change).

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

impl AppState {
    pub fn new() -> (Self, iced::Task<Message>) {
        Self::with_library_path(default_library_path())
    }

    pub fn with_library_path(library_path: String) -> (Self, iced::Task<Message>) {
        let audio = match AudioOut::try_new() {
            Some(a) => {
                yutu_core::app_log::info(
                    "yutu-audio",
                    format_args!("backend: {}", a.backend_name()),
                );
                Some(a)
            }
            None => {
                yutu_core::app_log::error("yutu-audio", "unavailable: no output device");
                None
            }
        };
        if let Some(parent) = std::path::Path::new(&library_path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let (
            liked,
            liked_order,
            disliked,
            disliked_order,
            history_full,
            user_playlists,
            playlist_seq,
            cached,
            playlist_added,
        ) = load_persisted(&library_path);
        let mut state = Self {
            route: Route::Home,
            search_query: String::new(),
            results: SearchResults::default(),
            all_results: SearchAll::default(),
            queue: Queue::default(),
            liked,
            liked_order,
            disliked,
            disliked_order,
            history: history_full
                .iter()
                .take(HISTORY_CAP)
                .map(|e| e.track_id.clone())
                .collect(),
            history_full,
            history_filter: String::new(),
            history_limit: HISTORY_PAGE_SIZE,
            volume: 0.8,
            muted: false,
            status: Language::system_default().ready_status(),
            user_playlists,
            new_playlist_name: String::new(),
            playlist_seq,
            eq_preset: "Flat".into(),
            eq_gains: [0.0; BAND_COUNT],
            eq_cooldown_until: None,
            eq_dirty: false,
            eq_enabled: true,
            eq_saved: None,
            seek_drag: None,
            seek_drag_at: None,
            seek_paused: false,
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
            lyrics_loading: HashSet::new(),
            lyrics_provider: LyricsProvider::Lrclib,
            lyrics_manual: HashSet::new(),
            lyrics_follow: true,
            lyrics_follow_at: None,
            lyrics_target: 0.0,
            lyrics_cmds: [f32::NAN; 3],
            nav_back: Vec::new(),
            nav_forward: Vec::new(),
            right_tab: RightTab::Queue,
            home_shelves: HomeShelves::default(),
            pre_shuffle: None,
            pre_shuffle_cur: None,
            suggest: Vec::new(),
            suggest_gen: 0,
            show_suggest: false,
            suggest_idx: None,
            queue_saved: None,
            queue_saved_at: None,
            pending_resume: None,
            pending_seek: None,
            autoload: false,
            restored_pending: false,
            context_menu: None,
            submenu_playlists_for: None,
            submenu_trigger_hot: false,
            submenu_fly_hot: false,
            submenu_hot_at: None,
            pl_menu: None,
            sort_menu: None,
            coll_menu: None,
            menu_at: None,
            cursor: None,
            window: None,
            sidebar_pinned: true,
            right_pinned: true,
            sidebar_latched: true,
            right_latched: true,
            sidebar_w: 235.0,
            right_w: 285.0,
            drawer_open: false,
            drawer_anim: 0.0,
            animations_enabled: true,
            anim_ms: ANIM_DEFAULT_MS,
            font_family: FontFamily::Adwaita,
            language: Language::system_default(),
            ui_scale: UI_SCALE_DEFAULT,
            system_font_rows: Vec::new(),
            font_rows: font_option_rows(&[]),
            artist_top: HashMap::new(),
            artist_full: HashSet::new(),
            album_live: HashMap::new(),
            artist_alias: HashMap::new(),
            album_alias: HashMap::new(),
            artist_top_order: Vec::new(),
            album_live_order: Vec::new(),
            artist_loading: None,
            album_loading: None,
            artist_full_loading: None,
            artist_resolving: None,
            album_resolving: None,
            artist_expanded: false,
            drawer_scroll: 0.0,
            scroll_hint: HashMap::new(),
            scroll_offsets: HashMap::new(),
            art_buckets: HashMap::new(),
            art_sweep_at: HashMap::new(),
            art_pruned_at: None,
            sidebar_filter: String::new(),
            autoplay_for: None,
            autoplay: Vec::new(),
            hovered_queue: None,
            hovered_history: None,
            hover_remove: None,
            hovered_track: None,
            hovered_recent: None,
            hovered_album: None,
            hovered_artist: None,
            hovered_search_playlist: None,
            hovered_auto: None,
            drag: None,
            press_snap: None,
            hovered_playlist: None,
            just_reordered_at: None,
            drag_gap: 0.0,
            playlist_sorts: HashMap::new(),
            playlist_added,
            pl_drag: None,
            pl_press_snap: None,
            pl_hover: None,
            pl_action_hover: None,
            pl_drag_id: None,
            error_log: Vec::new(),
            seek_hold_until: None,
            paused_seek: false,
            last_tick: None,
            fps: 0.0,
            last_frame: None,
            #[cfg(feature = "debug-panel")]
            view_build_ms: std::cell::Cell::new(0.0),
            cookie_path: String::new(),
            #[cfg(feature = "innertube")]
            pending_cookie_cleanup: None,
            #[cfg(feature = "innertube")]
            pending_cookie_source: None,
            modal_anim: 0.0,
            pending_playlist_delete: None,
            #[cfg(feature = "innertube")]
            pending_signout: false,
            scrobble_modal_open: false,
            artwork: HashMap::new(),
            artwork_loading: HashSet::new(),
            artwork_failed: HashMap::new(),
            hydrate_cooldown: HashMap::new(),
            // Ids with a metadata probe in flight (dedups lazy batches
            // across navigations and ticks: no repeat fetch while pending).
            hydrating: HashSet::new(),
            // First Tick after launch runs the Home lazy batch (startup
            // itself does zero network so the window appears instantly).
            lazy_init_done: false,
            prefetch_attempted: None,
            resolve_attempts: 0,
            heard_audio: false,
            stream_opened_at: None,
            #[cfg(feature = "innertube")]
            google_login: None,
            #[cfg(feature = "innertube")]
            logged_in: oauth_path_exists(),
            #[cfg(feature = "innertube")]
            cookie_logged_in: cookie_session_exists(),
            #[cfg(feature = "innertube")]
            login_busy: false,
            #[cfg(feature = "innertube")]
            import_songs: 0,
            #[cfg(feature = "innertube")]
            import_likes: 0,
            #[cfg(feature = "innertube")]
            import_done: 0,
            #[cfg(feature = "innertube")]
            import_total: 0,
            #[cfg(feature = "innertube")]
            import_gen: 0,
            #[cfg(feature = "innertube")]
            import_active: false,
            #[cfg(feature = "innertube")]
            sync_active: false,
            spin_frame: 0,
            #[cfg(feature = "innertube")]
            import_expected: 0,
            #[cfg(feature = "innertube")]
            import_live: HashMap::new(),
            #[cfg(feature = "innertube")]
            import_finished: HashSet::new(),
            #[cfg(feature = "innertube")]
            import_shown: 0,
            #[cfg(feature = "innertube")]
            auto_sync: true,
            #[cfg(feature = "innertube")]
            last_sync: None,
            #[cfg(feature = "innertube")]
            last_sync_unix: None,
            #[cfg(feature = "innertube")]
            started_at: std::time::Instant::now(),
            end_advanced: false,
            library_path,
            catalog: HashMap::new(),
            search_base: SearchResults::default(),
            search_all_base: SearchAll::default(),
            search_cache: HashMap::new(),
            searching: false,
            search_gen: 0,
            search_filter: SearchFilter::All,
            radio_tracks: Vec::new(),
            mpris: None,
        };
        // Sync prefs: auto-sync toggle + last-sync label survive restarts.
        #[cfg(feature = "innertube")]
        {
            let (auto_sync, last_sync_unix) = load_sync_prefs(&state.library_path);
            state.auto_sync = auto_sync;
            state.last_sync_unix = last_sync_unix;
        }
        // Animation duration survives restarts (garbage falls back to
        // the default; the slider clamps, so stored values are in range).
        if let Ok(store) = LibraryStore::open(&state.library_path) {
            if let Some(ms) = store
                .get_pref("anim_ms")
                .unwrap_or(None)
                .and_then(|v| v.parse::<u64>().ok())
            {
                state.anim_ms = ms.clamp(ANIM_MIN_MS, ANIM_MAX_MS);
            }
        }
        // Scrobbling survives restarts: the toggle + display name live in
        // SQLite with the other prefs, the tokens live in the OS keychain
        // like the cookie session (one read each at launch, silent while
        // the login keychain is unlocked; failures just mean re-entering
        // them, exactly like before).
        if let Ok(store) = LibraryStore::open(&state.library_path) {
            state.scrobble_enabled = store
                .get_pref("scrobble_enabled")
                .unwrap_or(None)
                .as_deref()
                == Some("1");
            if let Some(user) = store.get_pref("scrobble_user").unwrap_or(None) {
                state.scrobble_user = user;
            }
        }
        if let Some(token) = load_scrobble_secret(yutu_core::SCROBBLE_LB_TOKEN) {
            state.scrobble_lb_token = token;
        }
        if let Some(key) = load_scrobble_secret(yutu_core::SCROBBLE_FM_KEY) {
            state.scrobble_fm_key = key;
        }
        if let Some(secret) = load_scrobble_secret(yutu_core::SCROBBLE_FM_SECRET) {
            state.scrobble_fm_secret = secret;
        }
        if let Some(sk) = load_scrobble_secret(yutu_core::SCROBBLE_FM_SK) {
            state.scrobble_fm_sk = sk;
        }
        // Display prefs survive restarts (garbage falls back to the
        // defaults; the scale clamps into range and rejects NaN, which
        // must never reach the renderer). The family applies on
        // relaunch (the renderer font is fixed at startup).
        if let Ok(store) = LibraryStore::open(&state.library_path) {
            if let Some(raw) = store.get_pref("font_family").unwrap_or(None) {
                state.font_family = FontFamily::parse(&raw);
            }
            if let Some(raw) = store.get_pref("ui_scale").unwrap_or(None) {
                if let Ok(parsed) = raw.parse::<f32>() {
                    state.ui_scale = clamp_scale(parsed);
                }
            }
            if let Some(raw) = store.get_pref("language").unwrap_or(None) {
                state.language = Language::parse(&raw);
            }
        }
        // EQ curve survives restarts (garbage falls back to Flat): the
        // name re-derives from the gains, so nothing stored can lie;
        // the backend push stays silent (startup status belongs to
        // Ready/restore). A fresh backend defaults flat, so Flat pushes
        // nothing (there is no reused handle to carry stale EQ).
        if let Ok(store) = LibraryStore::open(&state.library_path) {
            state.eq_enabled = store.get_pref("eq_enabled").unwrap_or(None).as_deref() != Some("0");
            if let Some(gains) = store
                .get_pref("eq_gains")
                .unwrap_or(None)
                .and_then(|v| EqPreset::gains_from_json(&v))
            {
                state.eq_preset =
                    EqPreset::builtin_name_for(&gains).unwrap_or_else(|| "Custom".to_string());
                state.eq_gains = gains;
                if state.eq_preset != "Flat" && state.eq_enabled {
                    if let Some(audio) = &state.audio {
                        let preset = EqPreset {
                            name: state.eq_preset.clone(),
                            bands: gains,
                        };
                        // A rejected chain falls back to Flat rather
                        // than reopening mislabeled (rodio keeps the
                        // stored choice: it applies on mpv, as always).
                        if audio.supports_eq() && audio.apply_eq(&preset).is_err() {
                            state.eq_preset = "Flat".to_string();
                            state.eq_gains = [0.0; BAND_COUNT];
                        }
                    }
                }
            } else {
                state.eq_gains = [0.0; BAND_COUNT];
                state.eq_preset = "Flat".to_string();
            }
            state.eq_saved = Some((state.eq_preset.clone(), state.eq_gains, state.eq_enabled));
        }
        // Lyrics provider survives restarts (unknown values fall back to
        // LRCLIB, which carries synced timing).
        if let Ok(store) = LibraryStore::open(&state.library_path) {
            if let Some(raw) = store.get_pref("lyrics_provider").unwrap_or(None) {
                state.lyrics_provider = LyricsProvider::parse(&raw);
            }
        }
        // Seed the in-memory catalog from the track cache plus
        // persisted playlists so Favorites/history/playlist songs render
        // on first launch with no search. Memory-only (no SQLite writes,
        // no network, no artwork fetches): the window appears instantly
        // with cached titles plus disk-cached covers, then the first Tick
        // lazily hydrates only the Home screenful (see ensure_visible).
        let mut seed: Vec<Track> = cached;
        seed.extend(state.user_playlists.iter().flat_map(|p| p.tracks.clone()));
        // Merge duplicates: prefer rows with artwork so covers survive.
        {
            use std::collections::HashMap;
            let mut merged: HashMap<String, Track> = HashMap::new();
            for t in seed {
                merged
                    .entry(t.id.clone())
                    .and_modify(|e| {
                        if e.thumbnail_url.is_none() && t.thumbnail_url.is_some() {
                            e.thumbnail_url = t.thumbnail_url.clone();
                        }
                        if e.thumbnail_large_url.is_none() && t.thumbnail_large_url.is_some() {
                            e.thumbnail_large_url = t.thumbnail_large_url.clone();
                        }
                        if e.plays.is_none() && t.plays.is_some() {
                            e.plays = t.plays.clone();
                        }
                        if e.title.is_empty() && !t.title.is_empty() {
                            e.title = t.title.clone();
                            e.artist = t.artist.clone();
                            e.album = t.album.clone();
                            e.duration_secs = t.duration_secs;
                        }
                    })
                    .or_insert(t);
            }
            seed = merged.into_values().collect();
        }
        state.seed_catalog_mem(seed);
        // Session restore last: the catalog is warm, so saved ids
        // resolve to full rows with art.
        state.restore_queue();
        // Buffer the restored track paused so the first play/seek is
        // instant (resolving takes ~1s) and the bar never flashes
        // 00:00. Stays silent until the user presses play; skipped
        // with no audio backend (first play loads as before).
        let mut init = vec![system_font_task()];
        if state.audio.is_some() && state.queue.current().is_some() {
            init.push(state.autoload_current_task());
        }
        (state, iced::Task::batch(init))
    }

    /// Browser-style navigation: records back history, clears forward.
    pub fn navigate(&mut self, route: Route) {
        self.go(route);
    }

    pub(crate) fn go(&mut self, route: Route) {
        // Navigation always drops the drawer (it glides shut) and the
        // search popup (results take the page). Playlist manual drags
        // are scoped to one detail list, so any navigation drops them
        // (the queue drag lives in the global panel and survives).
        self.drawer_open = false;
        self.show_suggest = false;
        self.pl_drag = None;
        self.pl_press_snap = None;
        self.pl_drag_id = None;
        self.pl_hover = None;
        self.pl_action_hover = None;
        if route != self.route {
            self.nav_back.push(self.route.clone());
            if self.nav_back.len() > 50 {
                self.nav_back.remove(0);
            }
            self.nav_forward.clear();
        }
        self.route = route;
    }

    pub(crate) fn go_back(&mut self) {
        self.drawer_open = false;
        self.show_suggest = false;
        self.pl_drag = None;
        self.pl_press_snap = None;
        self.pl_drag_id = None;
        self.pl_hover = None;
        self.pl_action_hover = None;
        if let Some(prev) = self.nav_back.pop() {
            self.nav_forward.push(self.route.clone());
            self.route = prev;
        }
    }

    pub(crate) fn go_forward(&mut self) {
        self.drawer_open = false;
        self.show_suggest = false;
        self.pl_drag = None;
        self.pl_press_snap = None;
        self.pl_drag_id = None;
        self.pl_hover = None;
        self.pl_action_hover = None;
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
        // Suggest navigation lives on its own subscription so it only
        // exists while the popup is open: arrows never hijack other
        // views, and Enter stays on the input submit (single path).
        let suggest_keys = if self.suggest_open() {
            iced::keyboard::listen().filter_map(|event| match event {
                iced::keyboard::Event::KeyPressed { key, .. } => match key.as_ref() {
                    iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowUp) => {
                        Some(Message::SuggestMoveUp)
                    }
                    iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowDown) => {
                        Some(Message::SuggestMoveDown)
                    }
                    _ => None,
                },
                _ => None,
            })
        } else {
            iced::Subscription::none()
        };
        let tick = iced::time::every(std::time::Duration::from_millis(500)).map(Message::Tick);
        // Frame meter for the debug FPS readout: only with the
        // debug-panel flag, so builds without it never wake up just
        // to count.
        #[cfg(feature = "debug-panel")]
        let frames = iced::window::frames().map(|_| Message::Frame(std::time::Instant::now()));
        #[cfg(not(feature = "debug-panel"))]
        let frames = iced::Subscription::none();
        let anim = if self.animating() {
            iced::time::every(std::time::Duration::from_millis(ANIM_TICK_MS))
                .map(|_| Message::Animate)
        } else {
            iced::Subscription::none()
        };
        let events = iced::event::listen().filter_map(|event| match event {
            iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                Some(Message::CursorMoved(position))
            }
            iced::Event::Window(iced::window::Event::Resized(size)) => {
                Some(Message::WindowResized(size))
            }
            // Mouse side buttons navigate like a browser (press, not
            // release, for browser feel and no drag-commit collision).
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Back)) => {
                Some(Message::NavBack)
            }
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Forward)) => {
                Some(Message::NavForward)
            }
            // Left release only owns the drag: side/right releases must
            // never commit or abort a reorder in progress.
            iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                Some(Message::DragEnd)
            }
            // Left press anywhere: the handler arms a queue drag only
            // when the cursor sits on a queue row.
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)) => {
                Some(Message::GlobalPress)
            }
            _ => None,
        });
        iced::Subscription::batch([keys, suggest_keys, tick, events, anim, frames])
    }

    /// Repeat mode as stored in the queue snapshot.
    /// Parse a stored repeat mode (unknown falls back to Off).
    pub(crate) fn repeat_parse(s: &str) -> yutu_player::RepeatMode {
        match s {
            "all" => yutu_player::RepeatMode::All,
            "one" => yutu_player::RepeatMode::One,
            _ => yutu_player::RepeatMode::Off,
        }
    }
}
