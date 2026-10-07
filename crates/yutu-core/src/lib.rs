//! yutu-core: models, backend trait, mock fixtures, auth + library store.

pub mod auth;
pub mod backend;
#[cfg(feature = "innertube")]
pub mod innertube;
pub mod library;
pub mod models;
pub mod stream;

pub use auth::{AuthState, CookieAuth};
pub use backend::{MockBackend, MusicBackend};
#[cfg(feature = "innertube")]
pub use innertube::InnertubeBackend;
pub use library::LibraryStore;
pub use models::{Album, Artist, HistoryEntry, Lyrics, LyricsLine, Playlist, SearchResults, Track};
pub use stream::{save_report, StreamClient, StreamError, StreamRequest};
