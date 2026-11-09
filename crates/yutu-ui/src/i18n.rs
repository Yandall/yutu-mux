//! App language: English with system-locale detection, Spanish opt-in
//! from Settings. Every user-facing string renders through
//! [`Language`] methods (one method per string, grouped by screen
//! area below) so both languages stay complete by construction: adding
//! UI text means adding a method here with both versions. Proper nouns
//! (app feature names owned by third parties, EQ preset names, font
//! names) stay untranslated at the call site. Log output stays English.

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
    pub fn language_section(self) -> &'static str {
        match self {
            Self::English => "Language",
            Self::Spanish => "Idioma",
        }
    }

    // Settings page title.
    pub fn settings_title(self) -> &'static str {
        match self {
            Self::English => "Settings",
            Self::Spanish => "Ajustes",
        }
    }

    // Home greeting for a 24h local hour (same 5/12/18 bounds as before).
    pub fn greeting(self, hour: u8) -> &'static str {
        match self {
            Self::English => match hour {
                5..12 => "Good morning",
                12..18 => "Good afternoon",
                18..23 => "Good evening",
                _ => "Good night",
            },
            Self::Spanish => match hour {
                5..12 => "Buenos días",
                12..18 => "Buenas tardes",
                18..23 => "Buenas noches",
                _ => "Buenas noches",
            },
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
            None => match self {
                Self::English => "never".into(),
                Self::Spanish => "nunca".into(),
            },
            Some(t) => {
                let ago = now_unix.saturating_sub(t);
                if ago < 60 {
                    match self {
                        Self::English => "just now".into(),
                        Self::Spanish => "ahora mismo".into(),
                    }
                } else if ago < 3_600 {
                    match self {
                        Self::English => format!("{}m ago", ago / 60),
                        Self::Spanish => format!("hace {} min", ago / 60),
                    }
                } else if ago < 86_400 {
                    match self {
                        Self::English => format!("{}h ago", ago / 3_600),
                        Self::Spanish => format!("hace {} h", ago / 3_600),
                    }
                } else {
                    match self {
                        Self::English => format!("{}d ago", ago / 86_400),
                        Self::Spanish => format!("hace {} d", ago / 86_400),
                    }
                }
            }
        }
    }

    /// History group header for a section (older months read
    /// `septiembre de 2026` in Spanish).
    pub fn history_section(self, section: &yutu_core::HistorySection) -> String {
        use yutu_core::HistorySection as S;
        match self {
            Self::English => match section {
                S::Today => "Today".into(),
                S::Yesterday => "Yesterday".into(),
                S::ThisWeek => "This week".into(),
                S::ThisMonth => "This month".into(),
                S::Older { year, month } => {
                    format!("{} {year}", Self::month_name_en(*month))
                }
            },
            Self::Spanish => match section {
                S::Today => "Hoy".into(),
                S::Yesterday => "Ayer".into(),
                S::ThisWeek => "Esta semana".into(),
                S::ThisMonth => "Este mes".into(),
                S::Older { year, month } => {
                    format!("{} de {year}", Self::month_name_es(*month))
                }
            },
        }
    }

    /// English month names (mirrors core for the English UI copy).
    fn month_name_en(month: u32) -> &'static str {
        match month {
            1 => "January",
            2 => "February",
            3 => "March",
            4 => "April",
            5 => "May",
            6 => "June",
            7 => "July",
            8 => "August",
            9 => "September",
            10 => "October",
            11 => "November",
            12 => "December",
            _ => "?",
        }
    }

    /// Spanish month names (lowercase, as running text requires).
    fn month_name_es(month: u32) -> &'static str {
        match month {
            1 => "enero",
            2 => "febrero",
            3 => "marzo",
            4 => "abril",
            5 => "mayo",
            6 => "junio",
            7 => "julio",
            8 => "agosto",
            9 => "septiembre",
            10 => "octubre",
            11 => "noviembre",
            12 => "diciembre",
            _ => "?",
        }
    }

    // Sidebar + header nav.
    pub fn nav_home(self) -> &'static str {
        match self {
            Self::English => "Home",
            Self::Spanish => "Inicio",
        }
    }
    pub fn nav_search(self) -> &'static str {
        match self {
            Self::English => "Search",
            Self::Spanish => "Buscar",
        }
    }
    pub fn nav_history(self) -> &'static str {
        match self {
            Self::English => "History",
            Self::Spanish => "Historial",
        }
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
    pub fn history_title(self) -> &'static str {
        match self {
            Self::English => "History",
            Self::Spanish => "Historial",
        }
    }
    pub fn history_plays(self, total: usize) -> String {
        match self {
            Self::English => format!("{total} plays"),
            Self::Spanish => format!("{total} reproducciones"),
        }
    }
    pub fn history_matches(self, total: usize) -> String {
        match self {
            Self::English => format!("{total} matches"),
            Self::Spanish => format!("{total} coincidencias"),
        }
    }
    pub fn no_playlists_sidebar(self) -> &'static str {
        match self {
            Self::English => "No playlists yet: create one in the sidebar.",
            Self::Spanish => "Sin listas todavía: crea una en la barra lateral.",
        }
    }
    pub fn no_playlists_below(self) -> &'static str {
        match self {
            Self::English => "No playlists yet: create one below.",
            Self::Spanish => "Sin listas todavía: crea una abajo.",
        }
    }
    pub fn no_playlists_match(self) -> &'static str {
        match self {
            Self::English => "No playlists match.",
            Self::Spanish => "Ninguna lista coincide.",
        }
    }
    // Queue panel tabs + headers.
    pub fn queue_tab(self) -> &'static str {
        match self {
            Self::English => "Queue",
            Self::Spanish => "Cola",
        }
    }
    pub fn recent_tab(self) -> &'static str {
        match self {
            Self::English => "Recent",
            Self::Spanish => "Recientes",
        }
    }
    pub fn now_playing(self) -> &'static str {
        match self {
            Self::English => "Now playing",
            Self::Spanish => "Reproduciendo ahora",
        }
    }
    pub fn nothing_queued(self) -> &'static str {
        match self {
            Self::English => "Nothing queued",
            Self::Spanish => "Nada en cola",
        }
    }
    pub fn up_next(self) -> &'static str {
        match self {
            Self::English => "Up next",
            Self::Spanish => "A continuación",
        }
    }
    pub fn autoplay(self) -> &'static str {
        match self {
            Self::English => "Autoplay",
            Self::Spanish => "Reproducción automática",
        }
    }
    pub fn no_recent_plays(self) -> &'static str {
        match self {
            Self::English => "No recent plays yet.",
            Self::Spanish => "Sin reproducciones recientes todavía.",
        }
    }
    pub fn clear_queue(self, n: usize) -> String {
        match self {
            Self::English => format!("Clear queue ({n})"),
            Self::Spanish => format!("Vaciar cola ({n})"),
        }
    }
    pub fn nothing_playing(self) -> &'static str {
        match self {
            Self::English => "Nothing playing",
            Self::Spanish => "Nada en reproducción",
        }
    }
    pub fn loading_ellipsis(self) -> &'static str {
        match self {
            Self::English => "Loading…",
            Self::Spanish => "Cargando…",
        }
    }
    // Home shelves.
    pub fn quick_picks(self) -> &'static str {
        match self {
            Self::English => "Quick picks",
            Self::Spanish => "Selección rápida",
        }
    }
    pub fn play_all(self) -> &'static str {
        match self {
            Self::English => "Play all",
            Self::Spanish => "Reproducir todo",
        }
    }
    pub fn from_library(self) -> &'static str {
        match self {
            Self::English => "From your library",
            Self::Spanish => "De tu biblioteca",
        }
    }
    pub fn recommended_albums(self) -> &'static str {
        match self {
            Self::English => "Recommended albums",
            Self::Spanish => "Álbumes recomendados",
        }
    }
    pub fn more_from(self, artist: &str) -> String {
        match self {
            Self::English => format!("More from {artist}"),
            Self::Spanish => format!("Más de {artist}"),
        }
    }
    pub fn jump_back_in(self) -> &'static str {
        match self {
            Self::English => "Jump back in",
            Self::Spanish => "Volver a escuchar",
        }
    }
    pub fn home_empty_hint(self) -> &'static str {
        match self {
            Self::English => "Your recent plays will appear here.",
            Self::Spanish => "Tus reproducciones recientes aparecerán aquí.",
        }
    }
    pub fn home_empty_hint2(self) -> &'static str {
        match self {
            Self::English => "Type in the search bar above: results land as you type.",
            Self::Spanish => "Escribe en la barra de búsqueda de arriba: los resultados aparecen mientras escribes.",
        }
    }
    pub fn made_for_you(self) -> &'static str {
        match self {
            Self::English => "Made for you",
            Self::Spanish => "Hecho para ti",
        }
    }
    pub fn discovery_arrow(self) -> &'static str {
        match self {
            Self::English => "Discovery →",
            Self::Spanish => "Descubrimiento →",
        }
    }
    pub fn weekly_mix(self) -> &'static str {
        match self {
            Self::English => "Weekly Mix",
            Self::Spanish => "Mezcla semanal",
        }
    }
    pub fn forgotten(self) -> &'static str {
        match self {
            Self::English => "Forgotten Favorites",
            Self::Spanish => "Favoritos olvidados",
        }
    }
    pub fn track_count(self, n: usize) -> String {
        match self {
            Self::English => format!("{n} tracks"),
            Self::Spanish => format!("{n} canciones"),
        }
    }
    pub fn playlist_sub(self, n: usize) -> String {
        match self {
            Self::English => format!("Playlist · {n} songs"),
            Self::Spanish => format!("Lista · {n} canciones"),
        }
    }
    // Search page.
    pub fn search_hint_empty(self) -> &'static str {
        match self {
            Self::English => "Write something in the search bar to find music.",
            Self::Spanish => "Escribe algo en la barra de búsqueda para encontrar música.",
        }
    }
    pub fn search_hint_short(self) -> &'static str {
        match self {
            Self::English => "Write something in the search bar above: results land as you type.",
            Self::Spanish => "Escribe algo en la barra de búsqueda de arriba: los resultados aparecen mientras escribes.",
        }
    }
    pub fn no_results(self) -> &'static str {
        match self {
            Self::English => "No results found",
            Self::Spanish => "Sin resultados",
        }
    }
    pub fn searching(self, dots: usize) -> String {
        match self {
            Self::English => format!("Searching YouTube Music{}", ".".repeat(dots)),
            Self::Spanish => format!("Buscando en YouTube Music{}", ".".repeat(dots)),
        }
    }
    pub fn suggest_hint(self) -> &'static str {
        match self {
            Self::English => "Top tracks: Enter for all results",
            Self::Spanish => "Canciones populares: Enter para ver todos los resultados",
        }
    }
    pub fn searching_for(self, q: &str) -> String {
        match self {
            Self::English => format!("Searching YouTube Music “{q}”…"),
            Self::Spanish => format!("Buscando en YouTube Music “{q}”…"),
        }
    }
    pub fn type_to_search(self) -> &'static str {
        match self {
            Self::English => "Type something to search YouTube Music.",
            Self::Spanish => "Escribe algo para buscar en YouTube Music.",
        }
    }
    pub fn top_result(self) -> &'static str {
        match self {
            Self::English => "Top result",
            Self::Spanish => "Mejor resultado",
        }
    }
    pub fn artist_label(self) -> &'static str {
        match self {
            Self::English => "Artist",
            Self::Spanish => "Artista",
        }
    }
    pub fn album_label(self) -> &'static str {
        match self {
            Self::English => "Album",
            Self::Spanish => "Álbum",
        }
    }
    pub fn song_label(self) -> &'static str {
        match self {
            Self::English => "Song",
            Self::Spanish => "Canción",
        }
    }
    pub fn video_label(self) -> &'static str {
        match self {
            Self::English => "Video",
            Self::Spanish => "Vídeo",
        }
    }
    /// Song vs video kind tag for rows (same separator as Playing status).
    pub fn track_kind(self, is_video: bool) -> &'static str {
        if is_video {
            self.video_label()
        } else {
            self.song_label()
        }
    }
    pub fn filter_all(self) -> &'static str {
        match self {
            Self::English => "All",
            Self::Spanish => "Todo",
        }
    }
    pub fn filter_songs(self) -> &'static str {
        match self {
            Self::English => "Songs",
            Self::Spanish => "Canciones",
        }
    }
    pub fn filter_albums(self) -> &'static str {
        match self {
            Self::English => "Albums",
            Self::Spanish => "Álbumes",
        }
    }
    pub fn filter_artists(self) -> &'static str {
        match self {
            Self::English => "Artists",
            Self::Spanish => "Artistas",
        }
    }
    pub fn filter_playlists(self) -> &'static str {
        match self {
            Self::English => "Playlists",
            Self::Spanish => "Listas",
        }
    }
    pub fn no_songs_for(self, q: &str) -> String {
        match self {
            Self::English => format!("No songs for “{q}”."),
            Self::Spanish => format!("Sin canciones para «{q}»."),
        }
    }
    pub fn no_albums_for(self, q: &str) -> String {
        match self {
            Self::English => format!("No albums for “{q}”."),
            Self::Spanish => format!("Sin álbumes para «{q}»."),
        }
    }
    pub fn no_artists_for(self, q: &str) -> String {
        match self {
            Self::English => format!("No artists for “{q}”."),
            Self::Spanish => format!("Sin artistas para «{q}»."),
        }
    }
    pub fn no_playlists_for(self, q: &str) -> String {
        match self {
            Self::English => format!("No playlists for “{q}”."),
            Self::Spanish => format!("Sin listas para «{q}»."),
        }
    }
    pub fn album_sub_year(self, artist: &str, year: u16) -> String {
        match self {
            Self::English => format!("Album · {artist} · {year}"),
            Self::Spanish => format!("Álbum · {artist} · {year}"),
        }
    }
    pub fn album_sub(self, artist: &str) -> String {
        match self {
            Self::English => format!("Album · {artist}"),
            Self::Spanish => format!("Álbum · {artist}"),
        }
    }
    pub fn playlist_sub_full(self, author: &str, detail: &str) -> String {
        match self {
            Self::English => format!("Playlist · {author} · {detail}"),
            Self::Spanish => format!("Lista · {author} · {detail}"),
        }
    }
    // Detail pages (artist / album / playlist / lyrics / radio).
    pub fn popular(self) -> &'static str {
        match self {
            Self::English => "Popular",
            Self::Spanish => "Populares",
        }
    }
    pub fn albums_section(self) -> &'static str {
        match self {
            Self::English => "Albums",
            Self::Spanish => "Álbumes",
        }
    }
    pub fn loading_top_songs(self) -> &'static str {
        match self {
            Self::English => "Loading top songs…",
            Self::Spanish => "Cargando canciones populares…",
        }
    }
    pub fn loading_album_songs(self) -> &'static str {
        match self {
            Self::English => "Loading album songs…",
            Self::Spanish => "Cargando canciones del álbum…",
        }
    }
    pub fn loading_playlist(self, label: &str) -> String {
        match self {
            Self::English => format!("Loading playlist {label}…"),
            Self::Spanish => format!("Cargando lista {label}…"),
        }
    }
    pub fn show_more(self) -> &'static str {
        match self {
            Self::English => "Show more",
            Self::Spanish => "Mostrar más",
        }
    }
    pub fn show_more_older(self, n: usize) -> String {
        match self {
            Self::English => format!("Show more ({n} older)"),
            Self::Spanish => format!("Mostrar más ({n} anteriores)"),
        }
    }
    pub fn play_label(self) -> &'static str {
        match self {
            Self::English => "Play",
            Self::Spanish => "Reproducir",
        }
    }
    pub fn shuffle_label(self) -> &'static str {
        match self {
            Self::English => "Shuffle",
            Self::Spanish => "Aleatorio",
        }
    }
    pub fn radio_label(self) -> &'static str {
        match self {
            Self::English => "Radio",
            Self::Spanish => "Radio",
        }
    }
    pub fn start_radio(self) -> &'static str {
        match self {
            Self::English => "Start radio",
            Self::Spanish => "Iniciar radio",
        }
    }
    pub fn artist_songs_albums(self, songs: usize, albums: usize, total: &str) -> String {
        match self {
            Self::English => format!("{songs} songs · {albums} albums · {total}"),
            Self::Spanish => format!("{songs} canciones · {albums} álbumes · {total}"),
        }
    }
    pub fn playlist_not_found(self, id: &str) -> String {
        match self {
            Self::English => format!("Playlist not found: {id}"),
            Self::Spanish => format!("Lista no encontrada: {id}"),
        }
    }
    pub fn playlist_not_found_plain(self) -> &'static str {
        match self {
            Self::English => "Playlist not found.",
            Self::Spanish => "Lista no encontrada.",
        }
    }
    pub fn lyrics_for(self, title: &str) -> String {
        match self {
            Self::English => format!("{title} · lyrics"),
            Self::Spanish => format!("{title} · letra"),
        }
    }
    pub fn instrumental(self) -> &'static str {
        match self {
            Self::English => "[Instrumental]",
            Self::Spanish => "[Instrumental]",
        }
    }
    pub fn no_lyrics(self) -> &'static str {
        match self {
            Self::English => "No lyrics for this track yet.",
            Self::Spanish => "Sin letra para esta pista todavía.",
        }
    }
    pub fn loading_lyrics(self, provider: &str) -> String {
        match self {
            Self::English => format!("Loading lyrics ({provider})"),
            Self::Spanish => format!("Cargando letra ({provider})"),
        }
    }
    pub fn fetching_lyrics(self, provider: &str) -> String {
        match self {
            Self::English => format!("Fetching lyrics ({provider})."),
            Self::Spanish => format!("Obteniendo letra ({provider})."),
        }
    }
    pub fn no_track_selected(self) -> &'static str {
        match self {
            Self::English => "Nothing selected: pick a song to see its lyrics.",
            Self::Spanish => "Nada seleccionado: elige una canción para ver su letra.",
        }
    }
    // Context menu rows.
    pub fn play_now(self) -> &'static str {
        match self {
            Self::English => "Play now",
            Self::Spanish => "Reproducir ahora",
        }
    }
    pub fn play_next(self) -> &'static str {
        match self {
            Self::English => "Play next",
            Self::Spanish => "Reproducir a continuación",
        }
    }
    pub fn add_to_queue(self) -> &'static str {
        match self {
            Self::English => "Add to queue",
            Self::Spanish => "Añadir a la cola",
        }
    }
    pub fn start_mix(self) -> &'static str {
        match self {
            Self::English => "Start mix",
            Self::Spanish => "Iniciar mezcla",
        }
    }
    pub fn lyrics_menu(self) -> &'static str {
        match self {
            Self::English => "Lyrics",
            Self::Spanish => "Letra",
        }
    }
    pub fn add_to_favorites(self) -> &'static str {
        match self {
            Self::English => "Add to favorites",
            Self::Spanish => "Añadir a favoritos",
        }
    }
    pub fn remove_from_favorites(self) -> &'static str {
        match self {
            Self::English => "Remove from favorites",
            Self::Spanish => "Quitar de favoritos",
        }
    }
    pub fn dont_recommend(self) -> &'static str {
        match self {
            Self::English => "Don't recommend",
            Self::Spanish => "No recomendar",
        }
    }
    pub fn recommend_again(self) -> &'static str {
        match self {
            Self::English => "Recommend again",
            Self::Spanish => "Recomendar de nuevo",
        }
    }
    pub fn go_to_album(self) -> &'static str {
        match self {
            Self::English => "Go to album",
            Self::Spanish => "Ir al álbum",
        }
    }
    pub fn go_to_artist(self) -> &'static str {
        match self {
            Self::English => "Go to artist",
            Self::Spanish => "Ir al artista",
        }
    }
    pub fn remove_from_queue(self) -> &'static str {
        match self {
            Self::English => "Remove from queue",
            Self::Spanish => "Quitar de la cola",
        }
    }
    pub fn remove_from_autoplay(self) -> &'static str {
        match self {
            Self::English => "Remove from autoplay",
            Self::Spanish => "Quitar de la reproducción automática",
        }
    }
    pub fn shuffle_play(self) -> &'static str {
        match self {
            Self::English => "Shuffle play",
            Self::Spanish => "Reproducción aleatoria",
        }
    }
    pub fn delete_playlist(self) -> &'static str {
        match self {
            Self::English => "Delete playlist",
            Self::Spanish => "Eliminar lista",
        }
    }
    pub fn playlists_toggle(self, n: usize) -> String {
        match self {
            Self::English => format!("Playlists ({n})"),
            Self::Spanish => format!("Listas ({n})"),
        }
    }
    pub fn no_playlists_menu(self) -> &'static str {
        match self {
            Self::English => "No playlists yet: create one in the sidebar.",
            Self::Spanish => "Sin listas todavía: crea una en la barra lateral.",
        }
    }
    // Confirm dialogs.
    pub fn signout_title(self) -> &'static str {
        match self {
            Self::English => "Sign out?",
            Self::Spanish => "¿Cerrar sesión?",
        }
    }
    pub fn signout_body(self) -> &'static str {
        match self {
            Self::English => {
                "Clears the saved session on this device. Your local library stays put."
            }
            Self::Spanish => {
                "Borra la sesión guardada en este dispositivo. Tu biblioteca local se conserva."
            }
        }
    }
    pub fn sign_out(self) -> &'static str {
        match self {
            Self::English => "Sign out",
            Self::Spanish => "Cerrar sesión",
        }
    }
    pub fn stay_signed_in(self) -> &'static str {
        match self {
            Self::English => "Stay signed in",
            Self::Spanish => "Mantener la sesión",
        }
    }
    pub fn delete_playlist_title(self) -> &'static str {
        match self {
            Self::English => "Delete this playlist?",
            Self::Spanish => "¿Eliminar esta lista?",
        }
    }
    pub fn delete_playlist_body(self, title: &str, n: usize) -> String {
        match self {
            Self::English => {
                format!("{title} ({n} songs) will leave your library. This cannot be undone.")
            }
            Self::Spanish => format!(
                "{title} ({n} canciones) saldrá de tu biblioteca. Esto no se puede deshacer."
            ),
        }
    }
    pub fn keep_playlist(self) -> &'static str {
        match self {
            Self::English => "Keep playlist",
            Self::Spanish => "Conservar lista",
        }
    }
    pub fn cleanup_title(self) -> &'static str {
        match self {
            Self::English => "Signed in. Delete the export file?",
            Self::Spanish => "Sesión iniciada. ¿Eliminar el archivo exportado?",
        }
    }
    pub fn cleanup_body(self) -> &'static str {
        match self {
            Self::English => "Your session is saved in the app securely. The export is plaintext and no longer needed.",
            Self::Spanish => "Tu sesión está guardada en la aplicación de forma segura. La exportación es texto sin formato y ya no es necesaria.",
        }
    }
    pub fn delete_export(self) -> &'static str {
        match self {
            Self::English => "Delete export",
            Self::Spanish => "Eliminar exportación",
        }
    }
    pub fn keep_file(self) -> &'static str {
        match self {
            Self::English => "Keep file",
            Self::Spanish => "Conservar archivo",
        }
    }
    // Settings sections.
    pub fn playback_section(self) -> &'static str {
        match self {
            Self::English => "Playback",
            Self::Spanish => "Reproducción",
        }
    }
    pub fn volume_muted(self, pct: f32) -> String {
        match self {
            Self::English => format!("Volume {pct:.0}% (muted)"),
            Self::Spanish => format!("Volumen {pct:.0}% (silenciado)"),
        }
    }
    pub fn volume(self, pct: f32) -> String {
        match self {
            Self::English => format!("Volume {pct:.0}%"),
            Self::Spanish => format!("Volumen {pct:.0}%"),
        }
    }
    pub fn lyrics_section(self) -> &'static str {
        match self {
            Self::English => "Lyrics",
            Self::Spanish => "Letra",
        }
    }
    pub fn default_lyrics_provider(self) -> &'static str {
        match self {
            Self::English => "Default lyrics provider",
            Self::Spanish => "Proveedor de letras predeterminado",
        }
    }
    pub fn appearance_section(self) -> &'static str {
        match self {
            Self::English => "Appearance",
            Self::Spanish => "Apariencia",
        }
    }
    pub fn display_font(self) -> &'static str {
        match self {
            Self::English => "Display font (takes effect on relaunch)",
            Self::Spanish => "Fuente de pantalla (se aplica al reiniciar)",
        }
    }
    pub fn stale_font_note(self) -> &'static str {
        match self {
            Self::English => "That family is no longer installed: text falls back until you pick another.",
            Self::Spanish => "Esa familia ya no está instalada: el texto usa la opción alternativa hasta que elijas otra.",
        }
    }
    pub fn relaunch_to_apply(self) -> &'static str {
        match self {
            Self::English => "Relaunch now to apply",
            Self::Spanish => "Reiniciar ahora para aplicar",
        }
    }
    pub fn interface_scale(self, pct: f32) -> String {
        match self {
            Self::English => format!("Interface scale: {pct:.0}%"),
            Self::Spanish => format!("Escala de la interfaz: {pct:.0}%"),
        }
    }
    pub fn scale_hint(self) -> &'static str {
        match self {
            Self::English => "Scales text and controls together.",
            Self::Spanish => "Escala el texto y los controles juntos.",
        }
    }
    pub fn animations_section(self) -> &'static str {
        match self {
            Self::English => "Animations",
            Self::Spanish => "Animaciones",
        }
    }
    pub fn enable_animations(self) -> &'static str {
        match self {
            Self::English => "Enable animations",
            Self::Spanish => "Activar animaciones",
        }
    }
    pub fn animation_duration(self, ms: u64) -> String {
        match self {
            Self::English => format!("Animation duration: {ms} ms"),
            Self::Spanish => format!("Duración de la animación: {ms} ms"),
        }
    }
    pub fn scrobbling_section(self) -> &'static str {
        match self {
            Self::English => "Scrobbling",
            Self::Spanish => "Scrobble",
        }
    }
    pub fn setup_scrobbling(self) -> &'static str {
        match self {
            Self::English => "Set up scrobbling",
            Self::Spanish => "Configurar scrobbling",
        }
    }
    pub fn enable_scrobbling(self) -> &'static str {
        match self {
            Self::English => "Enable scrobbling",
            Self::Spanish => "Activar scrobbling",
        }
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
    pub fn done_button(self) -> &'static str {
        match self {
            Self::English => "Done",
            Self::Spanish => "Listo",
        }
    }
    pub fn cookie_section_title(self) -> &'static str {
        match self {
            Self::English => "Cookie import",
            Self::Spanish => "Importación de cookies",
        }
    }
    pub fn signed_in_cookies(self) -> &'static str {
        match self {
            Self::English => "Signed in with cookies",
            Self::Spanish => "Sesión iniciada con cookies",
        }
    }
    pub fn cookie_explainer(self) -> &'static str {
        match self {
            Self::English => "Log into music.youtube.com in your browser, export your cookies with the Get cookies.txt extension, then choose the file and press Import.",
            Self::Spanish => "Inicia sesión en music.youtube.com en tu navegador, exporta tus cookies con la extensión Get cookies.txt, luego elige el archivo y pulsa Importar.",
        }
    }
    pub fn choose_cookies_file(self) -> &'static str {
        match self {
            Self::English => "Choose cookies.txt file…",
            Self::Spanish => "Elige el archivo cookies.txt…",
        }
    }
    pub fn import_button(self) -> &'static str {
        match self {
            Self::English => "Import",
            Self::Spanish => "Importar",
        }
    }
    pub fn sync_section_title(self) -> &'static str {
        match self {
            Self::English => "Sync web changes",
            Self::Spanish => "Sincronizar cambios web",
        }
    }
    pub fn sync_now(self) -> &'static str {
        match self {
            Self::English => "Sync now",
            Self::Spanish => "Sincronizar ahora",
        }
    }
    pub fn auto_sync_label(self) -> &'static str {
        match self {
            Self::English => "Auto-sync every 15 min",
            Self::Spanish => "Sincronizar cada 15 min",
        }
    }
    pub fn last_sync(self, ago: &str) -> String {
        match self {
            Self::English => format!("Last sync: {ago}"),
            Self::Spanish => format!("Última sincronización: {ago}"),
        }
    }
    pub fn error_section_title(self) -> &'static str {
        match self {
            Self::English => "Recent playback errors",
            Self::Spanish => "Errores recientes de reproducción",
        }
    }
    pub fn no_errors(self) -> &'static str {
        match self {
            Self::English => "No errors recorded.",
            Self::Spanish => "Sin errores registrados.",
        }
    }
    pub fn cache_section_title(self) -> &'static str {
        match self {
            Self::English => "Memory caches (all capped)",
            Self::Spanish => "Cachés en memoria (todas limitadas)",
        }
    }
    pub fn shortcuts_hint(self) -> &'static str {
        match self {
            Self::English => "Shortcuts: Space play/pause · n next · p prev · / search",
            Self::Spanish => {
                "Atajos: Espacio reproducir/pausar · n siguiente · p anterior · / buscar"
            }
        }
    }
    pub fn enable_custom_eq(self) -> &'static str {
        match self {
            Self::English => "Enable custom EQ",
            Self::Spanish => "Activar EQ personalizado",
        }
    }
    pub fn eq_needs_mpv(self) -> &'static str {
        match self {
            Self::English => {
                "EQ needs the mpv backend: install libmpv / rebuild with --features mpv."
            }
            Self::Spanish => {
                "El EQ necesita el motor mpv: instala libmpv o recompila con --features mpv."
            }
        }
    }
    pub fn eq_no_device(self) -> &'static str {
        match self {
            Self::English => "EQ: no audio device.",
            Self::Spanish => "EQ: sin dispositivo de audio.",
        }
    }
    /// Repeat mode tag for the desktop status line.
    pub fn repeat_mode(self, mode: yutu_player::RepeatMode) -> String {
        use yutu_player::RepeatMode as R;
        match self {
            Self::English => match mode {
                R::Off => "off".into(),
                R::All => "all".into(),
                R::One => "one".into(),
            },
            Self::Spanish => match mode {
                R::Off => "no".into(),
                R::All => "todo".into(),
                R::One => "una".into(),
            },
        }
    }
    // Discovery page.
    pub fn discovery_hint_play(self) -> &'static str {
        match self {
            Self::English => "Play and like some music first.",
            Self::Spanish => "Reproduce y añade favoritos primero.",
        }
    }
    pub fn discovery_hint_like(self) -> &'static str {
        match self {
            Self::English => "Like some music to fill this.",
            Self::Spanish => "Marca música como favorita para llenar esto.",
        }
    }
    pub fn needs_innertube_sync(self) -> &'static str {
        match self {
            Self::English => "Sync needs `--features innertube`.",
            Self::Spanish => "La sincronización necesita `--features innertube`.",
        }
    }
    pub fn needs_innertube_import(self) -> &'static str {
        match self {
            Self::English => "Cookie import needs `--features innertube`.",
            Self::Spanish => "La importación de cookies necesita `--features innertube`.",
        }
    }
    pub fn needs_innertube_import_offline(self) -> &'static str {
        match self {
            Self::English => "Cookie import needs `--features innertube` (offline build).",
            Self::Spanish => {
                "La importación de cookies necesita `--features innertube` (versión sin conexión)."
            }
        }
    }
    pub fn needs_innertube_search_offline(self) -> &'static str {
        match self {
            Self::English => "Real search needs `--features innertube` (offline build).",
            Self::Spanish => {
                "La búsqueda real necesita `--features innertube` (versión sin conexión)."
            }
        }
    }
    pub fn needs_innertube_playlists(self) -> &'static str {
        match self {
            Self::English => "Playlists need `--features innertube`.",
            Self::Spanish => "Las listas necesitan `--features innertube`.",
        }
    }
    pub fn needs_innertube_radio(self) -> &'static str {
        match self {
            Self::English => "Radio needs `--features innertube`.",
            Self::Spanish => "La radio necesita `--features innertube`.",
        }
    }
    pub fn searching_short(self) -> &'static str {
        match self {
            Self::English => "Searching…",
            Self::Spanish => "Buscando…",
        }
    }
    pub fn section_empty(self) -> &'static str {
        match self {
            Self::English => "Nothing here yet.",
            Self::Spanish => "Nada aquí todavía.",
        }
    }
    pub fn album_song_meta(self, artist: &str, n: usize, time: &str) -> String {
        match self {
            Self::English => format!("{artist} · {n} songs · {time}"),
            Self::Spanish => format!("{artist} · {n} canciones · {time}"),
        }
    }
    pub fn album_song_meta_year(self, artist: &str, year: &str, n: usize, time: &str) -> String {
        match self {
            Self::English => format!("{artist} · {year} · {n} songs · {time}"),
            Self::Spanish => format!("{artist} · {year} · {n} canciones · {time}"),
        }
    }
    pub fn importing_title(self) -> &'static str {
        match self {
            Self::English => "Importing your library",
            Self::Spanish => "Importando tu biblioteca",
        }
    }
    pub fn import_progress_songs(self, shown: usize, expected: usize) -> String {
        match self {
            Self::English => format!("{shown}/{expected} songs"),
            Self::Spanish => format!("{shown}/{expected} canciones"),
        }
    }
    pub fn import_progress_songs_simple(self, shown: usize) -> String {
        match self {
            Self::English => format!("{shown} songs"),
            Self::Spanish => format!("{shown} canciones"),
        }
    }
    pub fn import_progress_detail(
        self,
        songs: &str,
        liked: usize,
        done: usize,
        total: usize,
    ) -> String {
        match self {
            Self::English => format!("{songs} · {liked} liked · {done}/{total} playlists"),
            Self::Spanish => format!("{songs} · {liked} en favoritos · {done}/{total} listas"),
        }
    }
    pub fn import_progress_reading(self, shown: usize, liked: usize) -> String {
        match self {
            Self::English => format!("{shown} songs · {liked} liked · reading library…"),
            Self::Spanish => {
                format!("{shown} canciones · {liked} en favoritos · leyendo biblioteca…")
            }
        }
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
        match self {
            Self::English => format!(
                "art mem {art} · catalog {catalog} · lyrics {lyrics} · stream urls {urls} · history {history} · searches {searches}"
            ),
            Self::Spanish => format!(
                "arte {art} · catálogo {catalog} · letras {lyrics} · urls {urls} · historial {history} · búsquedas {searches}"
            ),
        }
    }
    pub fn file_picker_title(self) -> &'static str {
        match self {
            Self::English => "Choose cookies.txt export",
            Self::Spanish => "Elige el archivo cookies.txt",
        }
    }
    /// Hero play pill (uppercase like the design).
    pub fn play_pill_label(self) -> &'static str {
        match self {
            Self::English => "PLAY",
            Self::Spanish => "REPRODUCIR",
        }
    }
    /// Hero shuffle pill (uppercase like the design).
    pub fn shuffle_pill_label(self) -> &'static str {
        match self {
            Self::English => "SHUFFLE",
            Self::Spanish => "ALEATORIO",
        }
    }
    // Queue action labels (doubled as status heads).
    pub fn search_results_label(self) -> &'static str {
        match self {
            Self::English => "Search results",
            Self::Spanish => "Resultados de búsqueda",
        }
    }
    pub fn artist_queue_label(self, name: &str) -> String {
        match self {
            Self::English => format!("Artist {name}"),
            Self::Spanish => format!("Artista {name}"),
        }
    }
    pub fn album_queue_label(self, title: &str) -> String {
        match self {
            Self::English => format!("Album {title}"),
            Self::Spanish => format!("Álbum {title}"),
        }
    }
    pub fn playlist_queue_label(self, title: &str) -> String {
        match self {
            Self::English => format!("Playlist {title}"),
            Self::Spanish => format!("Lista {title}"),
        }
    }
    // Import heads per kind (the noun itself translates).
    #[cfg(feature = "innertube")]
    pub fn import_head(self, kind: crate::ImportKind) -> &'static str {
        use crate::ImportKind as K;
        match self {
            Self::English => kind.done_label(),
            Self::Spanish => match kind {
                K::Cookie => "Importación de cookies",
                K::Library => "Importación",
                K::Sync => "Sincronización",
            },
        }
    }
    #[cfg(feature = "innertube")]
    pub fn import_failed(self, kind: crate::ImportKind) -> String {
        use crate::ImportKind as K;
        match self {
            Self::English => format!("{} import failed", kind.label()),
            Self::Spanish => match kind {
                K::Cookie => "Falló la importación de cookies".into(),
                K::Library => "Falló la importación de la biblioteca".into(),
                K::Sync => "Falló la sincronización".into(),
            },
        }
    }
    #[cfg(feature = "innertube")]
    pub fn import_done_line(self, kind: crate::ImportKind, n: usize, m: usize) -> String {
        match self {
            Self::English => format!("{}: {n} library tracks ({m} liked).", kind.done_label()),
            Self::Spanish => {
                format!(
                    "{}: {n} pistas de la biblioteca ({m} en favoritos).",
                    self.import_head(kind)
                )
            }
        }
    }
    #[cfg(feature = "innertube")]
    pub fn import_fetching_line(
        self,
        kind: crate::ImportKind,
        n: usize,
        m: usize,
        total: usize,
    ) -> String {
        match self {
            Self::English => {
                format!(
                    "{}: {n} songs ({m} liked), fetching {total} playlists…",
                    kind.done_label()
                )
            }
            Self::Spanish => format!(
                "{}: {n} canciones ({m} en favoritos), obteniendo {total} listas…",
                self.import_head(kind)
            ),
        }
    }
    #[cfg(feature = "innertube")]
    pub fn import_done_lists(
        self,
        kind: crate::ImportKind,
        songs: usize,
        liked: usize,
        total: usize,
    ) -> String {
        match self {
            Self::English => format!(
                "{} complete: {songs} songs ({liked} liked), {total} playlists.",
                kind.done_label()
            ),
            Self::Spanish => format!(
                "{} completa: {songs} canciones ({liked} en favoritos), {total} listas.",
                self.import_head(kind)
            ),
        }
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
        match self {
            Self::English => format!(
                "{} complete with failures: {songs} songs ({liked} liked), {done}/{total} playlists.",
                kind.done_label()
            ),
            Self::Spanish => format!(
                "{} completa con errores: {songs} canciones ({liked} en favoritos), {done}/{total} listas.",
                self.import_head(kind)
            ),
        }
    }
    #[cfg(feature = "innertube")]
    pub fn playlist_fetch_progress(
        self,
        title: &str,
        n: usize,
        done: usize,
        total: usize,
    ) -> String {
        match self {
            Self::English => format!("Playlist '{title}' done ({n} tracks) - {done}/{total}."),
            Self::Spanish => format!("Lista «{title}» lista ({n} canciones): {done}/{total}."),
        }
    }
    // Playback + queue statuses.
    pub fn loading_track(self, title: &str, artist: &str) -> String {
        match self {
            Self::English => format!("Loading {title} · {artist}…"),
            Self::Spanish => format!("Cargando {title} · {artist}…"),
        }
    }
    pub fn stream_retrying(self, attempt: u8, max: u8) -> String {
        match self {
            Self::English => format!("Stream failed, retrying ({attempt}/{max})…"),
            Self::Spanish => format!("Transmisión fallida, reintentando ({attempt}/{max})…"),
        }
    }
    pub fn stream_skipped(self, title: &str, max: u8) -> String {
        match self {
            Self::English => format!("Skipped {title}: stream failed {max} times."),
            Self::Spanish => format!("Omitido {title}: la transmisión falló {max} veces."),
        }
    }
    pub fn playing_line(self, title: &str, artist: &str) -> String {
        match self {
            Self::English => format!("Playing: {title} · {artist}."),
            Self::Spanish => format!("Reproduciendo: {title} · {artist}."),
        }
    }
    pub fn paused_line(self, title: &str, artist: &str) -> String {
        match self {
            Self::English => format!("Paused: {title} · {artist}."),
            Self::Spanish => format!("En pausa: {title} · {artist}."),
        }
    }
    pub fn stream_no_device(self) -> &'static str {
        match self {
            Self::English => "Stream ready, but no audio device.",
            Self::Spanish => "Transmisión lista, pero sin dispositivo de audio.",
        }
    }
    pub fn stream_fetched_no_device(self, video_id: &str) -> String {
        match self {
            Self::English => format!("Stream {video_id} fetched, but no audio device."),
            Self::Spanish => {
                format!("Transmisión {video_id} obtenida, pero sin dispositivo de audio.")
            }
        }
    }
    pub fn queued_tracks(self, label: &str, n: usize) -> String {
        match self {
            Self::English => format!("{label}: {n} tracks queued."),
            Self::Spanish => format!("{label}: {n} pistas en cola."),
        }
    }
    pub fn nothing_to_queue(self, label: &str) -> String {
        match self {
            Self::English => format!("{label}: nothing to queue."),
            Self::Spanish => format!("{label}: nada para la cola."),
        }
    }
    pub fn nothing_to_play_yet(self, label: &str) -> String {
        match self {
            Self::English => format!("{label}: nothing to play yet."),
            Self::Spanish => format!("{label}: nada para reproducir aún."),
        }
    }
    pub fn nothing_to_play(self, label: &str) -> String {
        match self {
            Self::English => format!("{label}: nothing to play."),
            Self::Spanish => format!("{label}: nada para reproducir."),
        }
    }
    pub fn no_track_at(self, i: usize) -> String {
        match self {
            Self::English => format!("No track at index {i}."),
            Self::Spanish => format!("No hay pista en el índice {i}."),
        }
    }
    pub fn album_not_cached(self, album: &str) -> String {
        match self {
            Self::English => format!("Album {album} is not cached anymore."),
            Self::Spanish => format!("El álbum {album} ya no está en caché."),
        }
    }
    pub fn album_gone(self) -> &'static str {
        match self {
            Self::English => "That album is not cached anymore.",
            Self::Spanish => "Ese álbum ya no está en caché.",
        }
    }
    pub fn result_gone(self) -> &'static str {
        match self {
            Self::English => "That result is gone: search again.",
            Self::Spanish => "Ese resultado ya no existe: busca de nuevo.",
        }
    }
    pub fn no_audio_device(self) -> &'static str {
        match self {
            Self::English => "No audio device.",
            Self::Spanish => "Sin dispositivo de audio.",
        }
    }
    pub fn paused_word(self) -> &'static str {
        match self {
            Self::English => "Paused.",
            Self::Spanish => "En pausa.",
        }
    }
    pub fn resumed_word(self) -> &'static str {
        match self {
            Self::English => "Resumed.",
            Self::Spanish => "Reanudado.",
        }
    }
    pub fn stopped_word(self) -> &'static str {
        match self {
            Self::English => "Stopped.",
            Self::Spanish => "Detenido.",
        }
    }
    pub fn desktop_paused(self) -> &'static str {
        match self {
            Self::English => "Paused (desktop).",
            Self::Spanish => "En pausa (escritorio).",
        }
    }
    pub fn desktop_resumed(self) -> &'static str {
        match self {
            Self::English => "Resumed (desktop).",
            Self::Spanish => "Reanudado (escritorio).",
        }
    }
    pub fn desktop_stopped(self) -> &'static str {
        match self {
            Self::English => "Stopped (desktop).",
            Self::Spanish => "Detenido (escritorio).",
        }
    }
    pub fn desktop_repeat(self, mode: yutu_player::RepeatMode) -> String {
        match self {
            Self::English => format!("Repeat: {mode:?} (desktop)."),
            Self::Spanish => format!("Repetición: {mode:?} (escritorio)."),
        }
    }
    pub fn repeat_line(self, mode: yutu_player::RepeatMode) -> String {
        match self {
            Self::English => format!("Repeat: {mode:?}."),
            Self::Spanish => format!("Repetición: {mode:?}."),
        }
    }
    // EQ statuses.
    pub fn eq_saved(self, name: &str) -> String {
        match self {
            Self::English => format!("EQ: {name} (kept; toggle EQ on to hear it)"),
            Self::Spanish => format!("EQ: {name} (guardado; activa el EQ para escucharlo)"),
        }
    }
    pub fn eq_preset_invalid(self, e: impl std::fmt::Display) -> String {
        match self {
            Self::English => format!("EQ preset invalid: {e}."),
            Self::Spanish => format!("Preajuste de EQ no válido: {e}."),
        }
    }
    pub fn eq_preset_unknown(self, name: &str) -> String {
        match self {
            Self::English => format!("Unknown EQ preset: {name}."),
            Self::Spanish => format!("Preajuste de EQ desconocido: {name}."),
        }
    }
    pub fn eq_adjusting(self, name: &str) -> String {
        match self {
            Self::English => format!("EQ: {name} (tuning…)"),
            Self::Spanish => format!("EQ: {name} (ajustando…)"),
        }
    }
    pub fn eq_no_device_name(self, name: &str) -> String {
        match self {
            Self::English => format!("EQ: {name} (no audio device)"),
            Self::Spanish => format!("EQ: {name} (sin dispositivo de audio)"),
        }
    }
    pub fn eq_active(self, name: &str, backend: &str) -> String {
        match self {
            Self::English => format!("EQ: {name} (live on {backend})"),
            Self::Spanish => format!("EQ: {name} (activo en {backend})"),
        }
    }
    pub fn eq_failed(self, name: &str, e: &str) -> String {
        match self {
            Self::English => format!("EQ: {name} (saved; {e})"),
            Self::Spanish => format!("EQ: {name} (guardado; {e})"),
        }
    }
    pub fn eq_off_no_device(self) -> &'static str {
        match self {
            Self::English => "EQ off (no audio device)",
            Self::Spanish => "EQ desactivado (sin dispositivo de audio)",
        }
    }
    pub fn eq_off_skipped(self, backend: &str) -> String {
        match self {
            Self::English => format!("EQ off (bypassed on {backend})"),
            Self::Spanish => format!("EQ desactivado (omitido en {backend})"),
        }
    }
    pub fn eq_off_failed(self, e: &str) -> String {
        match self {
            Self::English => format!("EQ off (saved; {e})"),
            Self::Spanish => format!("EQ desactivado (guardado; {e})"),
        }
    }
    // Likes, radio, playlists.
    pub fn removed_favorite(self, id: &str) -> String {
        match self {
            Self::English => format!("Removed from favorites: {id}."),
            Self::Spanish => format!("Eliminado de favoritos: {id}."),
        }
    }
    pub fn added_favorite(self, id: &str) -> String {
        match self {
            Self::English => format!("Added to favorites: {id}."),
            Self::Spanish => format!("Añadido a favoritos: {id}."),
        }
    }
    pub fn added_favorite_fetching(self, id: &str) -> String {
        match self {
            Self::English => format!("Added to favorites: {id} (fetching info…)."),
            Self::Spanish => format!("Añadido a favoritos: {id} (obteniendo información…)."),
        }
    }
    pub fn undislike_removed(self, id: &str) -> String {
        match self {
            Self::English => format!("Removed don't-recommend for {id}."),
            Self::Spanish => format!("«No recomendar» eliminado para {id}."),
        }
    }
    pub fn disliked(self, id: &str) -> String {
        match self {
            Self::English => format!("Won't recommend {id} again."),
            Self::Spanish => format!("Ya no se recomendará {id}."),
        }
    }
    pub fn fetching_info(self, id: &str) -> String {
        match self {
            Self::English => format!("Fetching info for {id}…"),
            Self::Spanish => format!("Obteniendo información de {id}…"),
        }
    }
    pub fn lyrics_failed(self, e: &str) -> String {
        match self {
            Self::English => format!("Lyrics failed: {e}"),
            Self::Spanish => format!("Letra fallida: {e}"),
        }
    }
    pub fn starting_radio(self, title: &str, artist: &str) -> String {
        match self {
            Self::English => format!("Starting radio for {title} · {artist}…"),
            Self::Spanish => format!("Iniciando radio para {title} · {artist}…"),
        }
    }
    pub fn radio_no_results(self, seed: &str) -> String {
        match self {
            Self::English => format!("Radio (seed {seed}): nothing found."),
            Self::Spanish => format!("Radio (semilla {seed}): nada encontrado."),
        }
    }
    pub fn playlist_nothing_playable(self, label: &str) -> String {
        match self {
            Self::English => format!("Playlist {label}: nothing playable."),
            Self::Spanish => format!("Lista {label}: nada reproducible."),
        }
    }
    pub fn playlist_detail_error(self, label: &str, id: &str, e: &str) -> String {
        match self {
            Self::English => format!("Playlist {label} ({id}): {e}."),
            Self::Spanish => format!("Lista {label} ({id}): {e}."),
        }
    }
    pub fn playlist_name_empty(self) -> &'static str {
        match self {
            Self::English => "Playlist name is empty.",
            Self::Spanish => "El nombre de la lista está vacío.",
        }
    }
    pub fn playlist_created(self, title: &str, id: &str) -> String {
        match self {
            Self::English => format!("Created playlist {title} ({id})."),
            Self::Spanish => format!("Lista {title} creada ({id})."),
        }
    }
    pub fn already_in_favorites(self, title: &str) -> String {
        match self {
            Self::English => format!("Already in Favorites: {title}."),
            Self::Spanish => format!("Ya está en Favoritos: {title}."),
        }
    }
    pub fn added_to_favorites(self, title: &str) -> String {
        match self {
            Self::English => format!("Added {title} to Favorites."),
            Self::Spanish => format!("Añadido {title} a Favoritos."),
        }
    }
    pub fn playlist_readonly(self, id: &str) -> String {
        match self {
            Self::English => format!("Playlist {id} is built-in (read-only) or missing."),
            Self::Spanish => format!("La lista {id} es integrada (solo lectura) o no existe."),
        }
    }
    pub fn already_in_playlist(self, title: &str, list: &str) -> String {
        match self {
            Self::English => format!("Already in {list}: {title}."),
            Self::Spanish => format!("Ya está en {list}: {title}."),
        }
    }
    pub fn added_to_playlist(self, title: &str, list: &str) -> String {
        match self {
            Self::English => format!("Added {title} to {list}."),
            Self::Spanish => format!("Añadido {title} a {list}."),
        }
    }
    pub fn track_not_in_favorites(self, id: &str) -> String {
        match self {
            Self::English => format!("{id} is not in Favorites."),
            Self::Spanish => format!("{id} no está en Favoritos."),
        }
    }
    pub fn removed_track_favorites(self, id: &str) -> String {
        match self {
            Self::English => format!("{id} removed from Favorites."),
            Self::Spanish => format!("{id} eliminado de Favoritos."),
        }
    }
    pub fn removed_track_playlist(self, id: &str, title: &str) -> String {
        match self {
            Self::English => format!("Removed {id} from {title}."),
            Self::Spanish => format!("{id} eliminado de {title}."),
        }
    }
    pub fn favorites_locked(self) -> &'static str {
        match self {
            Self::English => "Favorites is built-in and can't be deleted.",
            Self::Spanish => "Favoritos es una lista integrada y no se puede eliminar.",
        }
    }
    pub fn playlist_locked(self, id: &str) -> String {
        match self {
            Self::English => format!("Playlist {id} is built-in and can't be deleted."),
            Self::Spanish => format!("La lista {id} es integrada y no se puede eliminar."),
        }
    }
    pub fn deleted_playlist(self, id: &str) -> String {
        match self {
            Self::English => format!("Deleted playlist {id}."),
            Self::Spanish => format!("Lista {id} eliminada."),
        }
    }
    pub fn weekly_empty(self) -> &'static str {
        match self {
            Self::English => "Weekly Mix: nothing to queue.",
            Self::Spanish => "Mezcla semanal: nada para la cola.",
        }
    }
    pub fn forgotten_empty(self) -> &'static str {
        match self {
            Self::English => "Forgotten Favorites: nothing to queue.",
            Self::Spanish => "Favoritos olvidados: nada para la cola.",
        }
    }
    pub fn quick_empty(self) -> &'static str {
        match self {
            Self::English => "Quick picks: nothing to queue yet.",
            Self::Spanish => "Selección rápida: nada para la cola aún.",
        }
    }
    pub fn queue_next(self, title: &str, artist: &str) -> String {
        match self {
            Self::English => format!("Queued next: {title} · {artist}."),
            Self::Spanish => format!("Siguiente en cola: {title} · {artist}."),
        }
    }
    pub fn queued_confirm(self, title: &str, artist: &str, press_play: bool) -> String {
        match self {
            Self::English => {
                if press_play {
                    format!("Added to queue: {title} · {artist} (press play).")
                } else {
                    format!("Added to queue: {title} · {artist}.")
                }
            }
            Self::Spanish => {
                if press_play {
                    format!("Añadido a la cola: {title} · {artist} (pulsa reproducir).")
                } else {
                    format!("Añadido a la cola: {title} · {artist}.")
                }
            }
        }
    }
    pub fn no_autoplay_at(self, i: usize) -> String {
        match self {
            Self::English => format!("Nothing at autoplay index {i}."),
            Self::Spanish => format!("Nada en el índice {i} de reproducción automática."),
        }
    }
    pub fn removed_from_queue(self, id: &str) -> String {
        match self {
            Self::English => format!("Removed {id} from the queue."),
            Self::Spanish => format!("{id} eliminado de la cola."),
        }
    }
    pub fn not_in_queue(self, id: &str) -> String {
        match self {
            Self::English => format!("Not in queue: {id}."),
            Self::Spanish => format!("No está en la cola: {id}."),
        }
    }
    pub fn removed_from_autoplay(self, title: &str) -> String {
        match self {
            Self::English => format!("Removed {title} from autoplay."),
            Self::Spanish => format!("{title} eliminado de la reproducción automática."),
        }
    }
    pub fn queue_reordered(self) -> &'static str {
        match self {
            Self::English => "Queue reordered.",
            Self::Spanish => "Cola reordenada.",
        }
    }
    pub fn playlist_nothing_to_queue(self, title: &str) -> String {
        match self {
            Self::English => format!("Playlist {title}: nothing to queue."),
            Self::Spanish => format!("Lista {title}: nada para la cola."),
        }
    }
    pub fn playlist_queued(self, title: &str, n: usize) -> String {
        match self {
            Self::English => format!("Playlist {title}: queued {n} track(s)."),
            Self::Spanish => format!("Lista {title}: {n} pistas en cola."),
        }
    }
    pub fn no_album_for(self, title: &str, artist: &str) -> String {
        match self {
            Self::English => format!("No album for {title} · {artist}."),
            Self::Spanish => format!("Sin álbum para {title} · {artist}."),
        }
    }
    pub fn relaunch_cancelled(self) -> &'static str {
        match self {
            Self::English => "Relaunch cancelled: could not save the session.",
            Self::Spanish => "Reinicio cancelado: no se pudo guardar la sesión.",
        }
    }
    pub fn reading_cookies(self) -> &'static str {
        match self {
            Self::English => "Reading cookies + contacting YouTube Music…",
            Self::Spanish => "Leyendo cookies y contactando YouTube Music…",
        }
    }
    pub fn cookie_session_lost(self) -> &'static str {
        match self {
            Self::English => "Cookie session is gone: import your cookies file again.",
            Self::Spanish => {
                "La sesión de cookies se perdió: importa tu archivo de cookies de nuevo."
            }
        }
    }
    pub fn importing_library(self) -> &'static str {
        match self {
            Self::English => "Importing your library…",
            Self::Spanish => "Importando tu biblioteca…",
        }
    }
    pub fn importing_tracks(self) -> &'static str {
        match self {
            Self::English => "Importing your library…",
            Self::Spanish => "Importando tu biblioteca…",
        }
    }
    pub fn cookie_cleared(self) -> &'static str {
        match self {
            Self::English => "Cookie session cleared.",
            Self::Spanish => "Sesión de cookies eliminada.",
        }
    }
    pub fn export_deleted(self) -> &'static str {
        match self {
            Self::English => "Signed in. Export file deleted.",
            Self::Spanish => "Sesión iniciada. Archivo exportado eliminado.",
        }
    }
    pub fn export_already_gone(self) -> &'static str {
        match self {
            Self::English => "Signed in. Export file is already gone.",
            Self::Spanish => "Sesión iniciada. El archivo exportado ya no existe.",
        }
    }
    pub fn created_on_youtube(self, title: &str, server_id: &str) -> String {
        match self {
            Self::English => {
                format!("{title} created on YouTube ({server_id}): local list removed.")
            }
            Self::Spanish => {
                format!("{title} creada en YouTube ({server_id}): la lista local se eliminó.")
            }
        }
    }
    pub fn created_synced(self, title: &str) -> String {
        match self {
            Self::English => format!("Created playlist {title} (synced to YouTube)."),
            Self::Spanish => format!("Lista {title} creada (sincronizada con YouTube)."),
        }
    }
    pub fn google_device_code(self) -> &'static str {
        match self {
            Self::English => "Contacting Google for a device code…",
            Self::Spanish => "Contactando con Google para obtener un código de dispositivo…",
        }
    }
    pub fn google_signin_first(self) -> &'static str {
        match self {
            Self::English => "Press “Sign in with Google” first.",
            Self::Spanish => "Pulsa «Sign in with Google» primero.",
        }
    }
    pub fn checking_approval(self) -> &'static str {
        match self {
            Self::English => "Checking approval…",
            Self::Spanish => "Comprobando aprobación…",
        }
    }
    pub fn signed_in_importing(self) -> &'static str {
        match self {
            Self::English => "Signed in: importing your library…",
            Self::Spanish => "Sesión iniciada: importando tu biblioteca…",
        }
    }
    pub fn signed_out_token(self) -> &'static str {
        match self {
            Self::English => "Signed out (token file removed).",
            Self::Spanish => "Sesión cerrada (archivo de token eliminado).",
        }
    }
    pub fn google_signin_needed(self) -> &'static str {
        match self {
            Self::English => "Sign in with Google first.",
            Self::Spanish => "Inicia sesión con Google primero.",
        }
    }
    pub fn reading_library(self) -> &'static str {
        match self {
            Self::English => "Reading library + contacting YouTube Music…",
            Self::Spanish => "Leyendo la biblioteca y contactando YouTube Music…",
        }
    }
    pub fn radio_seed_label(self, seed: &str) -> String {
        match self {
            Self::English => format!("Radio (seed {seed})"),
            Self::Spanish => format!("Radio (semilla {seed})"),
        }
    }
    pub fn playlist_fetch_failed(self, title: &str, e: &str, done: usize, total: usize) -> String {
        match self {
            Self::English => format!("Playlist '{title}' failed ({e}) - {done}/{total}."),
            Self::Spanish => format!("Lista «{title}» falló ({e}): {done}/{total}."),
        }
    }
    pub fn restored_songs(self, n: usize) -> String {
        match self {
            Self::English => format!("Restored {n} saved song(s)."),
            Self::Spanish => format!("Restauradas {n} canciones guardadas."),
        }
    }
    pub fn ready_status(self) -> &'static str {
        match self {
            Self::English => "Ready: search for music.",
            Self::Spanish => "Listo: busca música.",
        }
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
        match self {
            Self::English => format!("Album · {artist} · {n} tracks"),
            Self::Spanish => format!("Álbum · {artist} · {n} canciones"),
        }
    }
    pub fn sync_needs_signin(self) -> &'static str {
        match self {
            Self::English => "Sync needs sign-in (cookies or Google).",
            Self::Spanish => "La sincronización necesita inicio de sesión (cookies o Google).",
        }
    }
    // `failure_status` human labels (never provider internals).
    pub fn fail_search(self) -> &'static str {
        match self {
            Self::English => "Search failed",
            Self::Spanish => "Búsqueda fallida",
        }
    }
    pub fn fail_radio(self) -> &'static str {
        match self {
            Self::English => "Radio failed",
            Self::Spanish => "Error de radio",
        }
    }
    pub fn fail_sync(self) -> &'static str {
        match self {
            Self::English => "Sync failed",
            Self::Spanish => "Error de sincronización",
        }
    }
    pub fn fail_signout(self) -> &'static str {
        match self {
            Self::English => "Sign-out failed",
            Self::Spanish => "Error al cerrar sesión",
        }
    }
    pub fn fail_signin(self) -> &'static str {
        match self {
            Self::English => "Sign-in failed",
            Self::Spanish => "Error al iniciar sesión",
        }
    }
    pub fn fail_check(self) -> &'static str {
        match self {
            Self::English => "Check failed",
            Self::Spanish => "Error de comprobación",
        }
    }
    pub fn fail_live_top(self) -> &'static str {
        match self {
            Self::English => "Live top songs failed",
            Self::Spanish => "Error al cargar populares",
        }
    }
    pub fn fail_live_album(self) -> &'static str {
        match self {
            Self::English => "Live album songs failed",
            Self::Spanish => "Error al cargar canciones del álbum",
        }
    }
    pub fn fail_artist_lookup(self) -> &'static str {
        match self {
            Self::English => "Artist lookup failed",
            Self::Spanish => "Error al buscar artista",
        }
    }
    pub fn fail_album_lookup(self) -> &'static str {
        match self {
            Self::English => "Album lookup failed",
            Self::Spanish => "Error al buscar álbum",
        }
    }
    pub fn fail_scrobble_save(self) -> &'static str {
        match self {
            Self::English => "Scrobble credentials save failed",
            Self::Spanish => "Error al guardar credenciales de scrobble",
        }
    }
    pub fn fail_scrobble_target(self, target: &str) -> String {
        match self {
            Self::English => format!("Scrobble to {target} failed"),
            Self::Spanish => format!("Error de scrobble a {target}"),
        }
    }
    pub fn scrobbled_to(self, target: &str) -> String {
        match self {
            Self::English => format!("Scrobbled to {target}."),
            Self::Spanish => format!("Scrobble enviado a {target}."),
        }
    }
    pub fn fail_playback(self, title: &str) -> String {
        match self {
            Self::English => format!("Playback failed for {title}"),
            Self::Spanish => format!("Error de reproducción: {title}"),
        }
    }
    pub fn fail_export_delete(self) -> &'static str {
        match self {
            Self::English => "Could not delete export file",
            Self::Spanish => "No se pudo eliminar el archivo exportado",
        }
    }
    pub fn fail_playback_tail(self, title: &str) -> String {
        match self {
            Self::English => format!("Playback failed for {title} (end of queue)"),
            Self::Spanish => format!("Error de reproducción: {title} (fin de la cola)"),
        }
    }
    /// Server like/dislike push that failed locally-kept (verb is the
    /// pending-op name: like, unlike, don't-recommend, clear-dislike).
    pub fn fail_youtube_op(self, verb: &str, track_id: &str) -> String {
        match self {
            Self::English => format!("YouTube {verb} of {track_id} failed (kept locally)"),
            Self::Spanish => {
                let what = match verb {
                    "like" => "«me gusta»",
                    "unlike" => "«no me gusta»",
                    "don't-recommend" => "«no recomendar»",
                    "clear-dislike" => "quitar «no recomendar»",
                    _ => verb,
                };
                format!("Falló {what} de {track_id} en YouTube (se conserva local)")
            }
        }
    }
    pub fn fail_created_local(self, title: &str) -> String {
        match self {
            Self::English => format!("Created {title} locally (YouTube sync failed)"),
            Self::Spanish => {
                format!("{title} creado localmente (falló la sincronización con YouTube)")
            }
        }
    }
    /// Sync-op detail stays technical; only the kept-locally note translates.
    pub fn fail_kept_locally(self, detail: &str) -> String {
        match self {
            Self::English => format!("{detail} (kept locally)"),
            Self::Spanish => format!("{detail} (se conserva local)"),
        }
    }
}
