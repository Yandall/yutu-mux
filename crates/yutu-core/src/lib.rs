//! yutu-core: models, backend trait, mock fixtures, auth + library store.

pub mod auth;
pub mod backend;
pub mod library;
pub mod models;

pub use auth::{AuthState, CookieAuth};
pub use backend::{MockBackend, MusicBackend};
pub use library::LibraryStore;
pub use models::{Album, Artist, HistoryEntry, Lyrics, LyricsLine, Playlist, SearchResults, Track};
