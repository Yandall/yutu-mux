//! App language: English with system-locale detection, Spanish opt-in
//! from Settings. User-facing strings live in Fluent catalogs
//! (`assets/locales/<lang>/app.ftl`, one message per [`Language`]
//! method below); the UI resolves them through [`Language::tr`]
//! with an English fallback, so a missing key in a new language can
//! never blank the UI. Translators only edit `.ftl` files.
//!
//! Adding a language: copy `en/app.ftl`, translate the values (keep
//! `{ $placeables }` verbatim), add a [`Language`] variant, wire it
//! into [`Language::label`], [`Language::stored`],
//! [`Language::parse`], [`Language::from_locale_value`], the
//! `tr`/`tr_args` bundle pick, and extend the `is_failure_line`
//! markers if the language's failure words differ. A test pins that
//! every English key exists in every other catalog. A handful of tiny
//! glue matches stay in Rust on purpose (picker labels, repeat badge,
//! failure markers, import-kind heads): everything else is data.
//!
//! Proper nouns (third-party feature names, EQ preset names, font
//! names) stay untranslated at the call site. Log output stays English.

use fluent_bundle::{FluentArgs, FluentBundle, FluentResource, FluentValue};
use unic_langid::{langid, LanguageIdentifier};

/// Parsed English catalog (fallback for every language).
fn en_source() -> &'static str {
    include_str!("../assets/locales/en/app.ftl")
}

/// Parsed Spanish catalog.
fn es_source() -> &'static str {
    include_str!("../assets/locales/es/app.ftl")
}

// Per-thread bundle pair, built once on first use: `FluentBundle`
// is deliberately `!Send` (memoized formatters), and every lookup
// here runs on the calling thread, so thread-locals are the honest
// home for them (no lock, no rebuild per string).
thread_local! {
    static BUNDLES: (FluentBundle<FluentResource>, FluentBundle<FluentResource>) = {
        (load_bundle(langid!("en"), en_source()), load_bundle(langid!("es"), es_source()))
    };
}

/// Parse one catalog; a broken `.ftl` file is a programming error and
/// fails fast at first use (tests render views in both languages).
fn load_bundle(locale: LanguageIdentifier, src: &'static str) -> FluentBundle<FluentResource> {
    let resource = FluentResource::try_new(src.to_string()).expect("valid app.ftl catalog");
    let mut bundle = FluentBundle::new(vec![locale]);
    // No bidi isolation: the extra markers would leak into labels and
    // break width-sensitive truncation.
    bundle.set_use_isolating(false);
    bundle
        .add_resource(resource)
        .expect("no duplicate message ids");
    bundle
}

/// UI language: persisted in SQLite (`language` pref), defaulting to
/// the system locale on first launch. Applies instantly (views read it
/// every frame, so no relaunch is needed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Language {
    #[default]
    English,
    Spanish,
}

impl Language {
    /// Format one message from one bundle (`None` when the id is
    /// missing there).
    fn format_one(
        bundle: &FluentBundle<FluentResource>,
        id: &str,
        args: Option<&FluentArgs<'_>>,
    ) -> Option<String> {
        let message = bundle.get_message(id)?;
        let pattern = message.value()?;
        let mut errors = Vec::new();
        let value = bundle.format_pattern(pattern, args, &mut errors);
        // Tests render views in both languages, so a bad placeable or
        // a missing arg fails there, not in a user's face.
        debug_assert!(errors.is_empty(), "fluent {id}: {errors:?}");
        Some(value.into_owned())
    }

    /// Look up `id` in `primary`, falling back to `fallback`.
    fn resolve_in(
        primary: &FluentBundle<FluentResource>,
        fallback: &FluentBundle<FluentResource>,
        id: &str,
        args: Option<&FluentArgs<'_>>,
    ) -> Option<String> {
        Self::format_one(primary, id, args).or_else(|| Self::format_one(fallback, id, args))
    }

    /// Resolve a plain message id, falling back to English and then
    /// to the id itself (a missing key degrades to English, never to
    /// blank).
    fn tr(&self, id: &str) -> String {
        BUNDLES.with(|both| {
            let (primary, fallback) = match self {
                Self::English => (&both.0, &both.0),
                Self::Spanish => (&both.1, &both.0),
            };
            Self::resolve_in(primary, fallback, id, None).unwrap_or_else(|| id.to_string())
        })
    }

    /// Resolve a message with `$placeable` arguments (same fallback).
    fn tr_args(&self, id: &str, args: &[(&str, &str)]) -> String {
        let mut map = FluentArgs::new();
        for (key, value) in args {
            map.set(*key, FluentValue::from(*value));
        }
        BUNDLES.with(|both| {
            let (primary, fallback) = match self {
                Self::English => (&both.0, &both.0),
                Self::Spanish => (&both.1, &both.0),
            };
            Self::resolve_in(primary, fallback, id, Some(&map)).unwrap_or_else(|| id.to_string())
        })
    }

    /// Picker rows: native names, shared by the Settings picker.
    pub const ALL_LABELS: [&'static str; 2] = ["English", "Español"];
    /// Picker label: language names stay in their own language, same
    /// in both modes (standard picker convention).
    pub fn label(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Spanish => "Español",
        }
    }

    /// Stored pref value (`en` / `es`).
    pub fn stored(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Spanish => "es",
        }
    }

    /// Parse a stored value, picker label, or locale-ish tag; unknown
    /// input falls back to English (prefs must never break the UI).
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_lowercase().as_str() {
            "es" | "espanol" | "español" | "spanish" | "castellano" => Self::Spanish,
            _ => Self::from_locale_value(raw),
        }
    }

    /// Match a locale value (`es_ES.UTF-8`, `es`, `en_US`): the part
    /// before any `:`, `_`, `.` or `@` decides; `es` anywhere in that
    /// tag means Spanish. Pure so tests can pin it without env mutation.
    pub fn from_locale_value(value: &str) -> Self {
        let tag = value
            .split(':')
            .next()
            .unwrap_or("")
            .split(['_', '.', '@'])
            .next()
            .unwrap_or("")
            .trim()
            .to_lowercase();
        if tag == "es" || tag.starts_with("es-") {
            Self::Spanish
        } else {
            Self::English
        }
    }

    /// First-launch default from the system locale (`LANGUAGE`, then
    /// `LC_ALL`, `LC_MESSAGES`, then `LANG`): Spanish when the tag is
    /// `es`, English otherwise (including unset/unreadable).
    pub fn system_default() -> Self {
        for key in ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"] {
            if let Ok(value) = std::env::var(key) {
                // `LANGUAGE` may list several (`es:en`): the first wins.
                let first = value.split(':').next().unwrap_or("").trim();
                if !first.is_empty() {
                    return Self::from_locale_value(first);
                }
            }
        }
        Self::English
    }

    // Settings: language section.
    pub fn language_section(self) -> String {
        self.tr("language-section")
    }

    // Settings page title.
    pub fn settings_title(self) -> String {
        self.tr("settings-title")
    }

    // Home greeting for a 24h local hour (same 5/12/18 bounds as before).
    pub fn greeting(self, hour: u8) -> String {
        match hour {
            5..12 => self.tr("greeting-morning"),
            12..18 => self.tr("greeting-day"),
            18..23 => self.tr("greeting-evening"),
            _ => self.tr("greeting-night"),
        }
    }

    /// Release status filter: only failures surface. English keys on
    /// "fail"; Spanish on its own failure words. The two lists are
    /// pinned to the same verdicts by test: info lines (queued,
    /// cleared, gone-air honest notes) stay silent in both.
    pub fn is_failure_line(self, status: &str) -> bool {
        let lower = status.to_lowercase();
        match self {
            Self::English => lower.contains("fail"),
            Self::Spanish => lower.contains("fall") || lower.contains("error"),
        }
    }

    /// Relative sync time (`never`, `just now`, `5m ago` ...).
    pub fn sync_ago(self, last_sync_unix: Option<i64>, now_unix: i64) -> String {
        match last_sync_unix {
            None => self.tr("sync-never"),
            Some(t) => {
                let ago = now_unix.saturating_sub(t);
                if ago < 60 {
                    self.tr("sync-just-now")
                } else if ago < 3_600 {
                    self.tr_args("sync-m", &[("n", &(ago / 60).to_string())])
                } else if ago < 86_400 {
                    self.tr_args("sync-h", &[("n", &(ago / 3_600).to_string())])
                } else {
                    self.tr_args("sync-d", &[("n", &(ago / 86_400).to_string())])
                }
            }
        }
    }

    /// History group header for a section (older months read
    /// `septiembre de 2026` in Spanish).
    pub fn history_section(self, section: &yutu_core::HistorySection) -> String {
        use yutu_core::HistorySection as S;
        match section {
            S::Today => self.tr("history-today"),
            S::Yesterday => self.tr("history-yesterday"),
            S::ThisWeek => self.tr("history-this-week"),
            S::ThisMonth => self.tr("history-this-month"),
            S::Older { year, month } => {
                let name = self.tr(&format!("month-{month}"));
                self.tr_args(
                    "history-older",
                    &[("month", &name), ("year", &year.to_string())],
                )
            }
        }
    }

    // Sidebar + header nav.
    pub fn nav_home(self) -> String {
        self.tr("nav-home")
    }
    pub fn nav_search(self) -> String {
        self.tr("nav-search")
    }
    pub fn nav_history(self) -> String {
        self.tr("nav-history")
    }
    pub fn search_placeholder(self) -> &'static str {
        match self {
            Self::English => "Search YouTube Music…",
            Self::Spanish => "Buscar en YouTube Music…",
        }
    }
    pub fn history_filter_placeholder(self) -> &'static str {
        match self {
            Self::English => "Search history…",
            Self::Spanish => "Buscar en el historial…",
        }
    }
    pub fn sidebar_filter_placeholder(self) -> &'static str {
        match self {
            Self::English => "Filter playlists…",
            Self::Spanish => "Filtrar listas…",
        }
    }
    pub fn new_playlist_placeholder(self) -> &'static str {
        match self {
            Self::English => "New playlist…",
            Self::Spanish => "Nueva lista…",
        }
    }
    pub fn history_title(self) -> String {
        self.tr("history-title")
    }
    pub fn history_plays(self, total: usize) -> String {
        self.tr_args("history-plays", &[("total", &total.to_string())])
    }
    pub fn history_matches(self, total: usize) -> String {
        self.tr_args("history-matches", &[("total", &total.to_string())])
    }
    pub fn no_playlists_sidebar(self) -> String {
        self.tr("no-playlists-sidebar")
    }
    pub fn no_playlists_below(self) -> String {
        self.tr("no-playlists-below")
    }
    pub fn no_playlists_match(self) -> String {
        self.tr("no-playlists-match")
    }
    // Queue panel tabs + headers.
    pub fn queue_tab(self) -> String {
        self.tr("queue-tab")
    }
    pub fn recent_tab(self) -> String {
        self.tr("recent-tab")
    }
    pub fn now_playing(self) -> String {
        self.tr("now-playing")
    }
    pub fn nothing_queued(self) -> String {
        self.tr("nothing-queued")
    }
    pub fn up_next(self) -> String {
        self.tr("up-next")
    }
    pub fn autoplay(self) -> String {
        self.tr("autoplay")
    }
    pub fn no_recent_plays(self) -> String {
        self.tr("no-recent-plays")
    }
    pub fn clear_queue(self, n: usize) -> String {
        self.tr_args("clear-queue", &[("n", &n.to_string())])
    }
    pub fn nothing_playing(self) -> String {
        self.tr("nothing-playing")
    }
    pub fn loading_ellipsis(self) -> String {
        self.tr("loading-ellipsis")
    }
    // Home shelves.
    pub fn quick_picks(self) -> String {
        self.tr("quick-picks")
    }
    pub fn play_all(self) -> String {
        self.tr("play-all")
    }
    pub fn from_library(self) -> String {
        self.tr("from-library")
    }
    pub fn recommended_albums(self) -> String {
        self.tr("recommended-albums")
    }
    pub fn more_from(self, artist: &str) -> String {
        self.tr_args("more-from", &[("artist", artist)])
    }
    pub fn jump_back_in(self) -> String {
        self.tr("jump-back-in")
    }
    pub fn home_empty_hint(self) -> String {
        self.tr("home-empty-hint")
    }
    pub fn home_empty_hint2(self) -> String {
        self.tr("home-empty-hint2")
    }
    pub fn made_for_you(self) -> String {
        self.tr("made-for-you")
    }
    pub fn discovery_arrow(self) -> String {
        self.tr("discovery-arrow")
    }
    pub fn weekly_mix(self) -> String {
        self.tr("weekly-mix")
    }
    pub fn forgotten(self) -> String {
        self.tr("forgotten")
    }
    pub fn track_count(self, n: usize) -> String {
        self.tr_args("track-count", &[("n", &n.to_string())])
    }
    pub fn playlist_sub(self, n: usize) -> String {
        self.tr_args("playlist-sub", &[("n", &n.to_string())])
    }
    // Search page.
    pub fn search_hint_empty(self) -> String {
        self.tr("search-hint-empty")
    }
    pub fn search_hint_short(self) -> String {
        self.tr("search-hint-short")
    }
    pub fn no_results(self) -> String {
        self.tr("no-results")
    }
    pub fn searching(self, dots: usize) -> String {
        self.tr_args("searching", &[("dots", &".".repeat(dots))])
    }
    pub fn suggest_hint(self) -> String {
        self.tr("suggest-hint")
    }
    pub fn searching_for(self, q: &str) -> String {
        self.tr_args("searching-for", &[("q", q)])
    }
    pub fn type_to_search(self) -> String {
        self.tr("type-to-search")
    }
    pub fn top_result(self) -> String {
        self.tr("top-result")
    }
    pub fn artist_label(self) -> String {
        self.tr("artist-label")
    }
    pub fn album_label(self) -> String {
        self.tr("album-label")
    }
    /// Song vs video kind tag for rows (same separator as Playing status).
    pub fn track_kind(self, is_video: bool) -> String {
        if is_video {
            self.tr("track-video")
        } else {
            self.tr("track-song")
        }
    }
    pub fn filter_all(self) -> String {
        self.tr("filter-all")
    }
    pub fn filter_songs(self) -> String {
        self.tr("filter-songs")
    }
    pub fn filter_albums(self) -> String {
        self.tr("filter-albums")
    }
    pub fn filter_artists(self) -> String {
        self.tr("filter-artists")
    }
    pub fn filter_playlists(self) -> String {
        self.tr("filter-playlists")
    }
    pub fn no_songs_for(self, q: &str) -> String {
        self.tr_args("no-songs-for", &[("q", q)])
    }
    pub fn no_albums_for(self, q: &str) -> String {
        self.tr_args("no-albums-for", &[("q", q)])
    }
    pub fn no_artists_for(self, q: &str) -> String {
        self.tr_args("no-artists-for", &[("q", q)])
    }
    pub fn no_playlists_for(self, q: &str) -> String {
        self.tr_args("no-playlists-for", &[("q", q)])
    }
    pub fn album_sub_year(self, artist: &str, year: u16) -> String {
        self.tr_args(
            "album-sub-year",
            &[("artist", artist), ("year", &year.to_string())],
        )
    }
    pub fn album_sub(self, artist: &str) -> String {
        self.tr_args("album-sub", &[("artist", artist)])
    }
    pub fn playlist_sub_full(self, author: &str, detail: &str) -> String {
        self.tr_args(
            "playlist-sub-full",
            &[("author", author), ("detail", detail)],
        )
    }
    // Detail pages (artist / album / playlist / lyrics / radio).
    pub fn popular(self) -> String {
        self.tr("popular")
    }
    pub fn albums_section(self) -> String {
        self.tr("albums-section")
    }
    pub fn loading_top_songs(self) -> String {
        self.tr("loading-top-songs")
    }
    pub fn loading_album_songs(self) -> String {
        self.tr("loading-album-songs")
    }
    pub fn loading_playlist(self, label: &str) -> String {
        self.tr_args("loading-playlist", &[("label", label)])
    }
    pub fn show_more(self) -> String {
        self.tr("show-more")
    }
    pub fn show_more_older(self, n: usize) -> String {
        self.tr_args("show-more-older", &[("n", &n.to_string())])
    }
    pub fn play_label(self) -> String {
        self.tr("play-label")
    }
    pub fn shuffle_label(self) -> String {
        self.tr("shuffle-label")
    }
    pub fn radio_label(self) -> String {
        self.tr("radio-label")
    }
    pub fn start_radio(self) -> String {
        self.tr("start-radio")
    }
    pub fn artist_songs_albums(self, songs: usize, albums: usize, total: &str) -> String {
        self.tr_args(
            "artist-songs-albums",
            &[
                ("songs", &songs.to_string()),
                ("albums", &albums.to_string()),
                ("total", total),
            ],
        )
    }
    pub fn playlist_not_found(self, id: &str) -> String {
        self.tr_args("playlist-not-found", &[("id", id)])
    }
    pub fn playlist_not_found_plain(self) -> String {
        self.tr("playlist-not-found-plain")
    }
    pub fn lyrics_for(self, title: &str) -> String {
        self.tr_args("lyrics-for", &[("title", title)])
    }
    pub fn instrumental(self) -> String {
        self.tr("instrumental")
    }
    pub fn no_lyrics(self) -> String {
        self.tr("no-lyrics")
    }
    pub fn loading_lyrics(self, provider: &str) -> String {
        self.tr_args("loading-lyrics", &[("provider", provider)])
    }
    pub fn fetching_lyrics(self, provider: &str) -> String {
        self.tr_args("fetching-lyrics", &[("provider", provider)])
    }
    pub fn no_track_selected(self) -> String {
        self.tr("no-track-selected")
    }
    // Context menu rows.
    pub fn play_now(self) -> String {
        self.tr("play-now")
    }
    pub fn play_next(self) -> String {
        self.tr("play-next")
    }
    pub fn add_to_queue(self) -> String {
        self.tr("add-to-queue")
    }
    pub fn start_mix(self) -> String {
        self.tr("start-mix")
    }
    pub fn lyrics_menu(self) -> String {
        self.tr("lyrics-menu")
    }
    pub fn add_to_favorites(self) -> String {
        self.tr("add-to-favorites")
    }
    pub fn remove_from_favorites(self) -> String {
        self.tr("remove-from-favorites")
    }
    pub fn dont_recommend(self) -> String {
        self.tr("dont-recommend")
    }
    pub fn recommend_again(self) -> String {
        self.tr("recommend-again")
    }
    pub fn go_to_album(self) -> String {
        self.tr("go-to-album")
    }
    pub fn go_to_artist(self) -> String {
        self.tr("go-to-artist")
    }
    pub fn remove_from_queue(self) -> String {
        self.tr("remove-from-queue")
    }
    pub fn remove_from_autoplay(self) -> String {
        self.tr("remove-from-autoplay")
    }
    pub fn shuffle_play(self) -> String {
        self.tr("shuffle-play")
    }
    pub fn delete_playlist(self) -> String {
        self.tr("delete-playlist")
    }
    pub fn playlists_toggle(self, n: usize) -> String {
        self.tr_args("playlists-toggle", &[("n", &n.to_string())])
    }
    pub fn no_playlists_menu(self) -> String {
        self.tr("no-playlists-menu")
    }
    // Confirm dialogs.
    pub fn signout_title(self) -> String {
        self.tr("signout-title")
    }
    pub fn signout_body(self) -> String {
        self.tr("signout-body")
    }
    pub fn sign_out(self) -> String {
        self.tr("sign-out")
    }
    pub fn stay_signed_in(self) -> String {
        self.tr("stay-signed-in")
    }
    pub fn delete_playlist_title(self) -> String {
        self.tr("delete-playlist-title")
    }
    pub fn delete_playlist_body(self, title: &str, n: usize) -> String {
        self.tr_args(
            "delete-playlist-body",
            &[("title", title), ("n", &n.to_string())],
        )
    }
    pub fn keep_playlist(self) -> String {
        self.tr("keep-playlist")
    }
    pub fn cleanup_title(self) -> String {
        self.tr("cleanup-title")
    }
    pub fn cleanup_body(self) -> String {
        self.tr("cleanup-body")
    }
    pub fn delete_export(self) -> String {
        self.tr("delete-export")
    }
    pub fn keep_file(self) -> String {
        self.tr("keep-file")
    }
    // Settings sections.
    pub fn playback_section(self) -> String {
        self.tr("playback-section")
    }
    pub fn volume_muted(self, pct: f32) -> String {
        self.tr_args("volume-muted", &[("pct", &format!("{pct:.0}"))])
    }
    pub fn volume(self, pct: f32) -> String {
        self.tr_args("volume", &[("pct", &format!("{pct:.0}"))])
    }
    pub fn lyrics_section(self) -> String {
        self.tr("lyrics-section")
    }
    pub fn default_lyrics_provider(self) -> String {
        self.tr("default-lyrics-provider")
    }
    pub fn appearance_section(self) -> String {
        self.tr("appearance-section")
    }
    pub fn display_font(self) -> String {
        self.tr("display-font")
    }
    pub fn stale_font_note(self) -> String {
        self.tr("stale-font-note")
    }
    pub fn relaunch_to_apply(self) -> String {
        self.tr("relaunch-to-apply")
    }
    pub fn interface_scale(self, pct: f32) -> String {
        self.tr_args("interface-scale", &[("pct", &format!("{pct:.0}"))])
    }
    pub fn scale_hint(self) -> String {
        self.tr("scale-hint")
    }
    pub fn animations_section(self) -> String {
        self.tr("animations-section")
    }
    pub fn enable_animations(self) -> String {
        self.tr("enable-animations")
    }
    pub fn animation_duration(self, ms: u64) -> String {
        self.tr_args("animation-duration", &[("ms", &ms.to_string())])
    }
    pub fn scrobbling_section(self) -> String {
        self.tr("scrobbling-section")
    }
    pub fn setup_scrobbling(self) -> String {
        self.tr("setup-scrobbling")
    }
    pub fn enable_scrobbling(self) -> String {
        self.tr("enable-scrobbling")
    }
    pub fn scrobble_user_placeholder(self) -> &'static str {
        match self {
            Self::English => "Display name (informational)…",
            Self::Spanish => "Nombre visible (informativo)…",
        }
    }
    pub fn scrobble_lb_placeholder(self) -> &'static str {
        match self {
            Self::English => "ListenBrainz token…",
            Self::Spanish => "Token de ListenBrainz…",
        }
    }
    pub fn scrobble_fm_key_placeholder(self) -> &'static str {
        match self {
            Self::English => "Last.fm API key…",
            Self::Spanish => "Clave API de Last.fm…",
        }
    }
    pub fn scrobble_fm_secret_placeholder(self) -> &'static str {
        match self {
            Self::English => "Last.fm shared secret…",
            Self::Spanish => "Secreto compartido de Last.fm…",
        }
    }
    pub fn scrobble_fm_sk_placeholder(self) -> &'static str {
        match self {
            Self::English => "Last.fm session key…",
            Self::Spanish => "Clave de sesión de Last.fm…",
        }
    }
    pub fn done_button(self) -> String {
        self.tr("done-button")
    }
    pub fn cookie_section_title(self) -> String {
        self.tr("cookie-section-title")
    }
    pub fn signed_in_cookies(self) -> String {
        self.tr("signed-in-cookies")
    }
    pub fn cookie_explainer(self) -> String {
        self.tr("cookie-explainer")
    }
    pub fn choose_cookies_file(self) -> String {
        self.tr("choose-cookies-file")
    }
    pub fn import_button(self) -> String {
        self.tr("import-button")
    }
    pub fn sync_section_title(self) -> String {
        self.tr("sync-section-title")
    }
    pub fn sync_now(self) -> String {
        self.tr("sync-now")
    }
    pub fn auto_sync_label(self) -> String {
        self.tr("auto-sync-label")
    }
    pub fn last_sync(self, ago: &str) -> String {
        self.tr_args("last-sync", &[("ago", ago)])
    }
    pub fn error_section_title(self) -> String {
        self.tr("error-section-title")
    }
    pub fn no_errors(self) -> String {
        self.tr("no-errors")
    }
    pub fn cache_section_title(self) -> String {
        self.tr("cache-section-title")
    }
    pub fn shortcuts_hint(self) -> String {
        self.tr("shortcuts-hint")
    }
    pub fn enable_custom_eq(self) -> String {
        self.tr("enable-custom-eq")
    }
    pub fn eq_needs_mpv(self) -> String {
        self.tr("eq-needs-mpv")
    }
    pub fn eq_no_device(self) -> String {
        self.tr("eq-no-device")
    }
    /// Repeat mode tag for the desktop status line.
    pub fn repeat_mode(self, mode: yutu_player::RepeatMode) -> String {
        use yutu_player::RepeatMode as R;
        match mode {
            R::Off => self.tr("repeat-off"),
            R::All => self.tr("repeat-all"),
            R::One => self.tr("repeat-one"),
        }
    }
    // Discovery page.
    pub fn discovery_hint_play(self) -> String {
        self.tr("discovery-hint-play")
    }
    pub fn discovery_hint_like(self) -> String {
        self.tr("discovery-hint-like")
    }
    pub fn needs_innertube_sync(self) -> String {
        self.tr("needs-innertube-sync")
    }
    pub fn needs_innertube_import(self) -> String {
        self.tr("needs-innertube-import")
    }
    pub fn needs_innertube_import_offline(self) -> String {
        self.tr("needs-innertube-import-offline")
    }
    pub fn needs_innertube_search_offline(self) -> String {
        self.tr("needs-innertube-search-offline")
    }
    pub fn needs_innertube_playlists(self) -> String {
        self.tr("needs-innertube-playlists")
    }
    pub fn needs_innertube_radio(self) -> String {
        self.tr("needs-innertube-radio")
    }
    pub fn searching_short(self) -> String {
        self.tr("searching-short")
    }
    pub fn section_empty(self) -> String {
        self.tr("section-empty")
    }
    pub fn album_song_meta(self, artist: &str, n: usize, time: &str) -> String {
        self.tr_args(
            "album-song-meta",
            &[("artist", artist), ("n", &n.to_string()), ("time", time)],
        )
    }
    pub fn album_song_meta_year(self, artist: &str, year: &str, n: usize, time: &str) -> String {
        self.tr_args(
            "album-song-meta-year",
            &[
                ("artist", artist),
                ("year", year),
                ("n", &n.to_string()),
                ("time", time),
            ],
        )
    }
    pub fn importing_title(self) -> String {
        self.tr("importing-title")
    }
    pub fn import_progress_songs(self, shown: usize, expected: usize) -> String {
        self.tr_args(
            "import-progress-songs",
            &[
                ("shown", &shown.to_string()),
                ("expected", &expected.to_string()),
            ],
        )
    }
    pub fn import_progress_songs_simple(self, shown: usize) -> String {
        self.tr_args(
            "import-progress-songs-simple",
            &[("shown", &shown.to_string())],
        )
    }
    pub fn import_progress_detail(
        self,
        songs: &str,
        liked: usize,
        done: usize,
        total: usize,
    ) -> String {
        self.tr_args(
            "import-progress-detail",
            &[
                ("songs", songs),
                ("liked", &liked.to_string()),
                ("done", &done.to_string()),
                ("total", &total.to_string()),
            ],
        )
    }
    pub fn import_progress_reading(self, shown: usize, liked: usize) -> String {
        self.tr_args(
            "import-progress-reading",
            &[("shown", &shown.to_string()), ("liked", &liked.to_string())],
        )
    }
    pub fn cache_census(
        self,
        art: usize,
        catalog: usize,
        lyrics: usize,
        urls: usize,
        history: usize,
        searches: usize,
    ) -> String {
        self.tr_args(
            "cache-census",
            &[
                ("art", &art.to_string()),
                ("catalog", &catalog.to_string()),
                ("lyrics", &lyrics.to_string()),
                ("urls", &urls.to_string()),
                ("history", &history.to_string()),
                ("searches", &searches.to_string()),
            ],
        )
    }
    pub fn file_picker_title(self) -> String {
        self.tr("file-picker-title")
    }
    /// Hero play pill (uppercase like the design).
    pub fn play_pill_label(self) -> String {
        self.tr("play-pill-label")
    }
    /// Hero shuffle pill (uppercase like the design).
    pub fn shuffle_pill_label(self) -> String {
        self.tr("shuffle-pill-label")
    }
    // Queue action labels (doubled as status heads).
    pub fn search_results_label(self) -> String {
        self.tr("search-results-label")
    }
    pub fn artist_queue_label(self, name: &str) -> String {
        self.tr_args("artist-queue-label", &[("name", name)])
    }
    pub fn album_queue_label(self, title: &str) -> String {
        self.tr_args("album-queue-label", &[("title", title)])
    }
    pub fn playlist_queue_label(self, title: &str) -> String {
        self.tr_args("playlist-queue-label", &[("title", title)])
    }
    // Import heads per kind (the noun itself translates).
    #[cfg(feature = "innertube")]
    pub fn import_head(self, kind: crate::ImportKind) -> String {
        match kind {
            crate::ImportKind::Cookie => self.tr("import-head-cookie"),
            crate::ImportKind::Library => self.tr("import-head-library"),
            crate::ImportKind::Sync => self.tr("import-head-sync"),
        }
    }
    #[cfg(feature = "innertube")]
    pub fn import_failed(self, kind: crate::ImportKind) -> String {
        match kind {
            crate::ImportKind::Cookie => self.tr("import-failed-cookie"),
            crate::ImportKind::Library => self.tr("import-failed-library"),
            crate::ImportKind::Sync => self.tr("import-failed-sync"),
        }
    }
    #[cfg(feature = "innertube")]
    pub fn import_done_line(self, kind: crate::ImportKind, n: usize, m: usize) -> String {
        let head = self.import_head(kind);
        self.tr_args(
            "import-done-line",
            &[
                ("head", &head),
                ("n", &n.to_string()),
                ("m", &m.to_string()),
            ],
        )
    }
    #[cfg(feature = "innertube")]
    pub fn import_fetching_line(
        self,
        kind: crate::ImportKind,
        n: usize,
        m: usize,
        total: usize,
    ) -> String {
        let head = self.import_head(kind);
        self.tr_args(
            "import-fetching-line",
            &[
                ("head", &head),
                ("n", &n.to_string()),
                ("m", &m.to_string()),
                ("total", &total.to_string()),
            ],
        )
    }
    #[cfg(feature = "innertube")]
    pub fn import_done_lists(
        self,
        kind: crate::ImportKind,
        songs: usize,
        liked: usize,
        total: usize,
    ) -> String {
        let head = self.import_head(kind);
        self.tr_args(
            "import-done-lists",
            &[
                ("head", &head),
                ("songs", &songs.to_string()),
                ("liked", &liked.to_string()),
                ("total", &total.to_string()),
            ],
        )
    }
    #[cfg(feature = "innertube")]
    pub fn import_done_lists_errors(
        self,
        kind: crate::ImportKind,
        songs: usize,
        liked: usize,
        done: usize,
        total: usize,
    ) -> String {
        let head = self.import_head(kind);
        self.tr_args(
            "import-done-lists-errors",
            &[
                ("head", &head),
                ("songs", &songs.to_string()),
                ("liked", &liked.to_string()),
                ("done", &done.to_string()),
                ("total", &total.to_string()),
            ],
        )
    }
    #[cfg(feature = "innertube")]
    pub fn playlist_fetch_progress(
        self,
        title: &str,
        n: usize,
        done: usize,
        total: usize,
    ) -> String {
        self.tr_args(
            "playlist-fetch-progress",
            &[
                ("title", title),
                ("n", &n.to_string()),
                ("done", &done.to_string()),
                ("total", &total.to_string()),
            ],
        )
    }
    // Playback + queue statuses.
    pub fn loading_track(self, title: &str, artist: &str) -> String {
        self.tr_args("loading-track", &[("title", title), ("artist", artist)])
    }
    pub fn stream_retrying(self, attempt: u8, max: u8) -> String {
        self.tr_args(
            "stream-retrying",
            &[("attempt", &attempt.to_string()), ("max", &max.to_string())],
        )
    }
    pub fn stream_skipped(self, title: &str, max: u8) -> String {
        self.tr_args(
            "stream-skipped",
            &[("title", title), ("max", &max.to_string())],
        )
    }
    pub fn playing_line(self, title: &str, artist: &str) -> String {
        self.tr_args("playing-line", &[("title", title), ("artist", artist)])
    }
    pub fn paused_line(self, title: &str, artist: &str) -> String {
        self.tr_args("paused-line", &[("title", title), ("artist", artist)])
    }
    pub fn stream_no_device(self) -> String {
        self.tr("stream-no-device")
    }
    pub fn stream_fetched_no_device(self, video_id: &str) -> String {
        self.tr_args("stream-fetched-no-device", &[("video_id", video_id)])
    }
    pub fn queued_tracks(self, label: &str, n: usize) -> String {
        self.tr_args("queued-tracks", &[("label", label), ("n", &n.to_string())])
    }
    pub fn nothing_to_queue(self, label: &str) -> String {
        self.tr_args("nothing-to-queue", &[("label", label)])
    }
    pub fn nothing_to_play_yet(self, label: &str) -> String {
        self.tr_args("nothing-to-play-yet", &[("label", label)])
    }
    pub fn nothing_to_play(self, label: &str) -> String {
        self.tr_args("nothing-to-play", &[("label", label)])
    }
    pub fn no_track_at(self, i: usize) -> String {
        self.tr_args("no-track-at", &[("i", &i.to_string())])
    }
    pub fn album_not_cached(self, album: &str) -> String {
        self.tr_args("album-not-cached", &[("album", album)])
    }
    pub fn album_gone(self) -> String {
        self.tr("album-gone")
    }
    pub fn result_gone(self) -> String {
        self.tr("result-gone")
    }
    pub fn no_audio_device(self) -> String {
        self.tr("no-audio-device")
    }
    pub fn paused_word(self) -> String {
        self.tr("paused-word")
    }
    pub fn resumed_word(self) -> String {
        self.tr("resumed-word")
    }
    pub fn stopped_word(self) -> String {
        self.tr("stopped-word")
    }
    pub fn desktop_paused(self) -> String {
        self.tr("desktop-paused")
    }
    pub fn desktop_resumed(self) -> String {
        self.tr("desktop-resumed")
    }
    pub fn desktop_stopped(self) -> String {
        self.tr("desktop-stopped")
    }
    pub fn desktop_repeat(self, mode: yutu_player::RepeatMode) -> String {
        self.tr_args("desktop-repeat", &[("mode", &format!("{mode:?}"))])
    }
    pub fn repeat_line(self, mode: yutu_player::RepeatMode) -> String {
        self.tr_args("repeat-line", &[("mode", &format!("{mode:?}"))])
    }
    // EQ statuses.
    pub fn eq_saved(self, name: &str) -> String {
        self.tr_args("eq-saved", &[("name", name)])
    }
    pub fn eq_preset_invalid(self, e: impl std::fmt::Display) -> String {
        self.tr_args("eq-preset-invalid", &[("e", &e.to_string())])
    }
    pub fn eq_preset_unknown(self, name: &str) -> String {
        self.tr_args("eq-preset-unknown", &[("name", name)])
    }
    pub fn eq_adjusting(self, name: &str) -> String {
        self.tr_args("eq-adjusting", &[("name", name)])
    }
    pub fn eq_no_device_name(self, name: &str) -> String {
        self.tr_args("eq-no-device-name", &[("name", name)])
    }
    pub fn eq_active(self, name: &str, backend: &str) -> String {
        self.tr_args("eq-active", &[("name", name), ("backend", backend)])
    }
    pub fn eq_failed(self, name: &str, e: &str) -> String {
        self.tr_args("eq-failed", &[("name", name), ("e", e)])
    }
    pub fn eq_off_no_device(self) -> String {
        self.tr("eq-off-no-device")
    }
    pub fn eq_off_skipped(self, backend: &str) -> String {
        self.tr_args("eq-off-skipped", &[("backend", backend)])
    }
    pub fn eq_off_failed(self, e: &str) -> String {
        self.tr_args("eq-off-failed", &[("e", e)])
    }
    // Likes, radio, playlists.
    pub fn removed_favorite(self, id: &str) -> String {
        self.tr_args("removed-favorite", &[("id", id)])
    }
    pub fn added_favorite(self, id: &str) -> String {
        self.tr_args("added-favorite", &[("id", id)])
    }
    pub fn added_favorite_fetching(self, id: &str) -> String {
        self.tr_args("added-favorite-fetching", &[("id", id)])
    }
    pub fn undislike_removed(self, id: &str) -> String {
        self.tr_args("undislike-removed", &[("id", id)])
    }
    pub fn disliked(self, id: &str) -> String {
        self.tr_args("disliked", &[("id", id)])
    }
    pub fn fetching_info(self, id: &str) -> String {
        self.tr_args("fetching-info", &[("id", id)])
    }
    pub fn lyrics_failed(self, e: &str) -> String {
        self.tr_args("lyrics-failed", &[("e", e)])
    }
    pub fn starting_radio(self, title: &str, artist: &str) -> String {
        self.tr_args("starting-radio", &[("title", title), ("artist", artist)])
    }
    pub fn radio_no_results(self, seed: &str) -> String {
        self.tr_args("radio-no-results", &[("seed", seed)])
    }
    pub fn playlist_nothing_playable(self, label: &str) -> String {
        self.tr_args("playlist-nothing-playable", &[("label", label)])
    }
    pub fn playlist_detail_error(self, label: &str, id: &str, e: &str) -> String {
        self.tr_args(
            "playlist-detail-error",
            &[("label", label), ("id", id), ("e", e)],
        )
    }
    pub fn playlist_name_empty(self) -> String {
        self.tr("playlist-name-empty")
    }
    pub fn playlist_created(self, title: &str, id: &str) -> String {
        self.tr_args("playlist-created", &[("title", title), ("id", id)])
    }
    pub fn already_in_favorites(self, title: &str) -> String {
        self.tr_args("already-in-favorites", &[("title", title)])
    }
    pub fn added_to_favorites(self, title: &str) -> String {
        self.tr_args("added-to-favorites", &[("title", title)])
    }
    pub fn playlist_readonly(self, id: &str) -> String {
        self.tr_args("playlist-readonly", &[("id", id)])
    }
    pub fn already_in_playlist(self, title: &str, list: &str) -> String {
        self.tr_args("already-in-playlist", &[("list", list), ("title", title)])
    }
    pub fn added_to_playlist(self, title: &str, list: &str) -> String {
        self.tr_args("added-to-playlist", &[("title", title), ("list", list)])
    }
    pub fn track_not_in_favorites(self, id: &str) -> String {
        self.tr_args("track-not-in-favorites", &[("id", id)])
    }
    pub fn removed_track_favorites(self, id: &str) -> String {
        self.tr_args("removed-track-favorites", &[("id", id)])
    }
    pub fn removed_track_playlist(self, id: &str, title: &str) -> String {
        self.tr_args("removed-track-playlist", &[("id", id), ("title", title)])
    }
    pub fn favorites_locked(self) -> String {
        self.tr("favorites-locked")
    }
    pub fn playlist_locked(self, id: &str) -> String {
        self.tr_args("playlist-locked", &[("id", id)])
    }
    pub fn deleted_playlist(self, id: &str) -> String {
        self.tr_args("deleted-playlist", &[("id", id)])
    }
    pub fn weekly_empty(self) -> String {
        self.tr("weekly-empty")
    }
    pub fn forgotten_empty(self) -> String {
        self.tr("forgotten-empty")
    }
    pub fn quick_empty(self) -> String {
        self.tr("quick-empty")
    }
    pub fn queue_next(self, title: &str, artist: &str) -> String {
        self.tr_args("queue-next", &[("title", title), ("artist", artist)])
    }
    pub fn queued_confirm(self, title: &str, artist: &str, press_play: bool) -> String {
        if press_play {
            self.tr_args(
                "queued-confirm-press",
                &[("title", title), ("artist", artist)],
            )
        } else {
            self.tr_args("queued-confirm", &[("title", title), ("artist", artist)])
        }
    }
    pub fn no_autoplay_at(self, i: usize) -> String {
        self.tr_args("no-autoplay-at", &[("i", &i.to_string())])
    }
    pub fn removed_from_queue(self, id: &str) -> String {
        self.tr_args("removed-from-queue", &[("id", id)])
    }
    pub fn not_in_queue(self, id: &str) -> String {
        self.tr_args("not-in-queue", &[("id", id)])
    }
    pub fn removed_from_autoplay(self, title: &str) -> String {
        self.tr_args("removed-from-autoplay", &[("title", title)])
    }
    pub fn queue_reordered(self) -> String {
        self.tr("queue-reordered")
    }
    pub fn playlist_nothing_to_queue(self, title: &str) -> String {
        self.tr_args("playlist-nothing-to-queue", &[("title", title)])
    }
    pub fn playlist_queued(self, title: &str, n: usize) -> String {
        self.tr_args(
            "playlist-queued",
            &[("title", title), ("n", &n.to_string())],
        )
    }
    pub fn no_album_for(self, title: &str, artist: &str) -> String {
        self.tr_args("no-album-for", &[("title", title), ("artist", artist)])
    }
    pub fn relaunch_cancelled(self) -> String {
        self.tr("relaunch-cancelled")
    }
    pub fn reading_cookies(self) -> String {
        self.tr("reading-cookies")
    }
    pub fn cookie_session_lost(self) -> String {
        self.tr("cookie-session-lost")
    }
    pub fn importing_library(self) -> String {
        self.tr("importing-library")
    }
    pub fn importing_tracks(self) -> String {
        self.tr("importing-tracks")
    }
    pub fn cookie_cleared(self) -> String {
        self.tr("cookie-cleared")
    }
    pub fn export_deleted(self) -> String {
        self.tr("export-deleted")
    }
    pub fn export_already_gone(self) -> String {
        self.tr("export-already-gone")
    }
    pub fn created_on_youtube(self, title: &str, server_id: &str) -> String {
        self.tr_args(
            "created-on-youtube",
            &[("title", title), ("server_id", server_id)],
        )
    }
    pub fn created_synced(self, title: &str) -> String {
        self.tr_args("created-synced", &[("title", title)])
    }
    pub fn google_device_code(self) -> String {
        self.tr("google-device-code")
    }
    pub fn google_signin_first(self) -> String {
        self.tr("google-signin-first")
    }
    pub fn checking_approval(self) -> String {
        self.tr("checking-approval")
    }
    pub fn signed_in_importing(self) -> String {
        self.tr("signed-in-importing")
    }
    pub fn signed_out_token(self) -> String {
        self.tr("signed-out-token")
    }
    pub fn google_signin_needed(self) -> String {
        self.tr("google-signin-needed")
    }
    pub fn reading_library(self) -> String {
        self.tr("reading-library")
    }
    pub fn radio_seed_label(self, seed: &str) -> String {
        self.tr_args("radio-seed-label", &[("seed", seed)])
    }
    pub fn playlist_fetch_failed(self, title: &str, e: &str, done: usize, total: usize) -> String {
        self.tr_args(
            "playlist-fetch-failed",
            &[
                ("title", title),
                ("e", e),
                ("done", &done.to_string()),
                ("total", &total.to_string()),
            ],
        )
    }
    pub fn restored_songs(self, n: usize) -> String {
        self.tr_args("restored-songs", &[("n", &n.to_string())])
    }
    pub fn ready_status(self) -> String {
        self.tr("ready-status")
    }
    /// Repeat badge on the player bar icon (`All`/`1`, empty when off).
    pub fn repeat_badge(self, mode: yutu_player::RepeatMode) -> &'static str {
        use yutu_player::RepeatMode as R;
        match self {
            Self::English => match mode {
                R::Off => "",
                R::All => "All",
                R::One => "1",
            },
            Self::Spanish => match mode {
                R::Off => "",
                R::All => "Todo",
                R::One => "1",
            },
        }
    }
    pub fn home_album_sub(self, artist: &str, n: usize) -> String {
        self.tr_args(
            "home-album-sub",
            &[("artist", artist), ("n", &n.to_string())],
        )
    }
    pub fn sync_needs_signin(self) -> String {
        self.tr("sync-needs-signin")
    }
    // `failure_status` human labels (never provider internals).
    pub fn fail_search(self) -> String {
        self.tr("fail-search")
    }
    pub fn fail_radio(self) -> String {
        self.tr("fail-radio")
    }
    pub fn fail_sync(self) -> String {
        self.tr("fail-sync")
    }
    pub fn fail_signout(self) -> String {
        self.tr("fail-signout")
    }
    pub fn fail_signin(self) -> String {
        self.tr("fail-signin")
    }
    pub fn fail_check(self) -> String {
        self.tr("fail-check")
    }
    pub fn fail_live_top(self) -> String {
        self.tr("fail-live-top")
    }
    pub fn fail_live_album(self) -> String {
        self.tr("fail-live-album")
    }
    pub fn fail_artist_lookup(self) -> String {
        self.tr("fail-artist-lookup")
    }
    pub fn fail_album_lookup(self) -> String {
        self.tr("fail-album-lookup")
    }
    pub fn fail_scrobble_save(self) -> String {
        self.tr("fail-scrobble-save")
    }
    pub fn fail_scrobble_target(self, target: &str) -> String {
        self.tr_args("fail-scrobble-target", &[("target", target)])
    }
    pub fn scrobbled_to(self, target: &str) -> String {
        self.tr_args("scrobbled-to", &[("target", target)])
    }
    pub fn fail_playback(self, title: &str) -> String {
        self.tr_args("fail-playback", &[("title", title)])
    }
    pub fn fail_export_delete(self) -> String {
        self.tr("fail-export-delete")
    }
    pub fn fail_playback_tail(self, title: &str) -> String {
        self.tr_args("fail-playback-tail", &[("title", title)])
    }
    /// Server like/dislike push that failed locally-kept (verb is the
    /// pending-op name: like, unlike, don't-recommend, clear-dislike).
    pub fn fail_youtube_op(self, verb: &str, track_id: &str) -> String {
        let what = match verb {
            "like" => self.tr("verb-like"),
            "unlike" => self.tr("verb-unlike"),
            "don't-recommend" => self.tr("verb-dont-recommend"),
            "clear-dislike" => self.tr("verb-clear-dislike"),
            _ => verb.to_string(),
        };
        self.tr_args("fail-youtube-op", &[("what", &what), ("track", track_id)])
    }
    pub fn fail_created_local(self, title: &str) -> String {
        self.tr_args("fail-created-local", &[("title", title)])
    }
    /// Sync-op detail stays technical; only the kept-locally note translates.
    pub fn fail_kept_locally(self, detail: &str) -> String {
        self.tr_args("fail-kept-locally", &[("detail", detail)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Message ids declared in a catalog source (top-level `id =` lines).
    fn catalog_ids(src: &str) -> Vec<String> {
        src.lines()
            .filter_map(|l| {
                let l = l.trim();
                if l.is_empty() || l.starts_with('#') {
                    return None;
                }
                l.split_once(" = ").map(|(id, _)| id.trim().to_string())
            })
            .collect()
    }

    #[test]
    fn catalogs_parse_without_duplicates() {
        for src in [en_source(), es_source()] {
            let ids = catalog_ids(src);
            let mut seen = std::collections::HashSet::new();
            for id in &ids {
                assert!(seen.insert(id.clone()), "duplicate message id: {id}");
            }
            assert!(!ids.is_empty());
        }
    }

    #[test]
    fn every_english_key_exists_in_spanish() {
        let en = catalog_ids(en_source());
        let es: std::collections::HashSet<String> = catalog_ids(es_source()).into_iter().collect();
        let mut missing = vec![];
        for id in &en {
            if !es.contains(id) {
                // A missing key degrades to English at runtime; the
                // test keeps the catalogs in lockstep instead.
                missing.push(id.clone());
            }
        }
        assert!(missing.is_empty(), "missing in es/app.ftl: {missing:?}");
        // No strays either: translators work from the English file.
        assert_eq!(es.len(), en.len(), "es catalog has extra keys");
    }

    #[test]
    fn missing_keys_fall_back_to_english() {
        fn bundle(locale: &str, src: &str) -> FluentBundle<FluentResource> {
            let mut b = FluentBundle::new(vec![locale.parse().unwrap()]);
            b.set_use_isolating(false);
            b.add_resource(FluentResource::try_new(src.to_string()).unwrap())
                .unwrap();
            b
        }
        let en = bundle("en-US", "only-here = English value\nshared = en { $n }");
        let es = bundle("es", "shared = es { $n }");
        // Present in Spanish: Spanish wins.
        let mut args = FluentArgs::new();
        args.set("n", FluentValue::from(3i64));
        assert_eq!(
            Language::resolve_in(&es, &en, "shared", Some(&args)),
            Some("es 3".to_string())
        );
        // Missing in Spanish: English fallback, same args.
        assert_eq!(
            Language::resolve_in(&es, &en, "only-here", Some(&args)),
            Some("English value".to_string())
        );
        // Missing everywhere: no value (callers show the id).
        assert_eq!(Language::resolve_in(&es, &en, "ghost-key", None), None);
    }

    #[test]
    fn formatted_messages_render_in_both_languages() {
        assert_eq!(Language::English.track_count(3), "3 tracks");
        assert_eq!(Language::Spanish.track_count(3), "3 canciones");
        assert_eq!(
            Language::English.queued_tracks("Mezcla semanal", 2),
            "Mezcla semanal: 2 tracks queued."
        );
        assert_eq!(
            Language::Spanish.history_section(&yutu_core::HistorySection::Older {
                year: 2026,
                month: 9
            }),
            "septiembre de 2026"
        );
    }
}
