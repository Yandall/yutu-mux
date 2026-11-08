//! yutu-core: models, backend trait, auth + library store.

pub mod app_log;
pub mod auth;
pub mod backend;
pub mod discovery;
pub mod history;
#[cfg(feature = "innertube")]
pub mod innertube;
pub mod library;
pub mod lyrics_providers;
pub mod models;
pub mod pot;
#[cfg(feature = "innertube")]
pub mod raw_browse;
#[cfg(feature = "innertube")]
pub mod raw_search;
pub mod scrobble;
pub mod stream;
pub mod ytdlp;

/// Shared HTTP client for Innertube POSTs (search, player): one pool per
/// process instead of one per request (a search fans out to five POSTs,
/// and a fresh client per POST burns handshakes plus pool memory).
#[cfg(feature = "innertube")]
pub(crate) fn shared_client() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new)
}

#[cfg(feature = "innertube")]
pub use auth::{begin_google_login, poll_google_login, GoogleLogin, OAuthPersist};
pub use auth::{AuthState, CookieAuth};
#[cfg(any(test, feature = "test-fixtures"))]
pub use backend::MockBackend;
pub use backend::MusicBackend;
pub use discovery::{forgotten_favorites, weekly_mix};
pub use history::{
    history_date, history_day, history_hhmm, history_section, history_section_label, history_week,
    history_ymd, month_name, HistorySection,
};
#[cfg(feature = "innertube")]
pub use innertube::split_library_status;
#[cfg(feature = "innertube")]
pub use innertube::InnertubeBackend;
#[cfg(feature = "innertube")]
pub use innertube::ResolvedStream;
#[cfg(feature = "innertube")]
pub use innertube::ServerPlaylist;
pub use library::LibraryStore;
pub use library::{browse_playlist_id, canonical_playlist_id, edit_playlist_id, PendingOp};
#[cfg(feature = "lyrics")]
pub use lyrics_providers::fetch_lrclib;
pub use lyrics_providers::{line_at, parse_lrc, LyricLine, LyricsResult};
pub use models::{
    hires_thumb_url, Album, Artist, HistoryEntry, Lyrics, LyricsLine, MixedRef, Playlist,
    PlaylistHit, QueueSnapshot, SearchAll, SearchResults, TopHit, TopKind, Track,
};
pub use pot::{fetch_po_token, find_botguard_bin, parse_po_output, PoToken};
#[cfg(feature = "innertube")]
pub use raw_search::{
    parse_search_all, raw_search_all, raw_search_filtered, SearchAuth, SearchFilterKind,
};
pub use scrobble::{
    api_sig, build_listen_payload, build_scrobble_params, should_scrobble, Scrobble,
};
#[cfg(feature = "submit")]
pub use scrobble::{submit_lastfm_scrobble, submit_listenbrainz_listen};
pub use stream::{save_report, StreamClient, StreamError, StreamRequest};
pub use ytdlp::{download_audio_bytes, find_yt_dlp, resolve_audio_url};
#[cfg(feature = "innertube")]
pub mod ytm_token;
#[cfg(feature = "innertube")]
pub use ytmapi_rs::auth::oauth::OAuthToken;
