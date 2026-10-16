//! yutu-core: models, backend trait, mock fixtures, auth + library store.

pub mod auth;
pub mod backend;
pub mod discovery;
#[cfg(feature = "innertube")]
pub mod innertube;
pub mod library;
pub mod lyrics_providers;
pub mod models;
pub mod pot;
pub mod scrobble;
pub mod stream;
pub mod ytdlp;

#[cfg(feature = "innertube")]
pub use auth::{begin_google_login, poll_google_login, GoogleLogin, OAuthPersist};
pub use auth::{AuthState, CookieAuth};
pub use backend::{MockBackend, MusicBackend};
pub use discovery::{forgotten_favorites, weekly_mix};
#[cfg(feature = "innertube")]
pub use innertube::InnertubeBackend;
#[cfg(feature = "innertube")]
pub use innertube::ResolvedStream;
#[cfg(feature = "innertube")]
pub use innertube::ServerPlaylist;
pub use library::LibraryStore;
pub use library::{browse_playlist_id, canonical_playlist_id, edit_playlist_id};
#[cfg(feature = "lyrics")]
pub use lyrics_providers::fetch_lrclib;
pub use lyrics_providers::{line_at, parse_lrc, LyricLine, LyricsResult};
pub use models::{
    hires_thumb_url, Album, Artist, HistoryEntry, Lyrics, LyricsLine, Playlist, SearchResults,
    Track,
};
pub use pot::{fetch_po_token, find_botguard_bin, parse_po_output, PoToken};
pub use scrobble::{
    api_sig, build_listen_payload, build_scrobble_params, should_scrobble, Scrobble,
};
#[cfg(feature = "submit")]
pub use scrobble::{submit_lastfm_scrobble, submit_listenbrainz_listen};
pub use stream::{save_report, StreamClient, StreamError, StreamRequest};
pub use ytdlp::{download_audio_bytes, find_yt_dlp, resolve_audio_url};
#[cfg(feature = "innertube")]
pub use ytmapi_rs::auth::oauth::OAuthToken;
