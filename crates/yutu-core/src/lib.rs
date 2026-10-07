//! yutu-core: models, backend trait, mock fixtures, auth state.
//! Phase 1 will add SQLite cache + real Innertube backend behind a feature.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration_secs: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Album {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub year: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artist {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResults {
    pub tracks: Vec<Track>,
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AuthState {
    #[default]
    Anonymous,
    Cookie,
}

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("backend error: {0}")]
    Backend(String),
}

/// Stability anchor: every backend (mock now, Innertube later) implements this.
pub trait MusicBackend {
    fn search(&self, query: &str) -> SearchResults;
}

/// Deterministic fixtures so UI/player work with no network.
pub struct MockBackend;

impl MockBackend {
    pub fn new() -> Self {
        Self
    }

    pub fn fixture_tracks() -> Vec<Track> {
        vec![
            Track {
                id: "m1".into(),
                title: "Come Together".into(),
                artist: "The Beatles".into(),
                album: Some("Abbey Road".into()),
                duration_secs: 259,
            },
            Track {
                id: "m2".into(),
                title: "Something".into(),
                artist: "The Beatles".into(),
                album: Some("Abbey Road".into()),
                duration_secs: 182,
            },
            Track {
                id: "m3".into(),
                title: "Here Comes The Sun".into(),
                artist: "The Beatles".into(),
                album: Some("Abbey Road".into()),
                duration_secs: 185,
            },
            Track {
                id: "m4".into(),
                title: "Yesterday".into(),
                artist: "The Beatles".into(),
                album: Some("Help!".into()),
                duration_secs: 125,
            },
            Track {
                id: "m5".into(),
                title: "Hey Jude".into(),
                artist: "The Beatles".into(),
                album: None,
                duration_secs: 431,
            },
        ]
    }
}

impl Default for MockBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl MusicBackend for MockBackend {
    fn search(&self, query: &str) -> SearchResults {
        let q = query.to_lowercase();
        let tracks = Self::fixture_tracks()
            .into_iter()
            .filter(|t| {
                q.is_empty()
                    || t.title.to_lowercase().contains(&q)
                    || t.artist.to_lowercase().contains(&q)
            })
            .collect();
        SearchResults {
            tracks,
            albums: vec![Album {
                id: "a1".into(),
                title: "Abbey Road".into(),
                artist: "The Beatles".into(),
                year: Some(1969),
            }],
            artists: vec![Artist {
                id: "ar1".into(),
                name: "The Beatles".into(),
            }],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_search_filters() {
        let b = MockBackend::new();
        assert_eq!(b.search("").tracks.len(), 5);
        assert_eq!(b.search("yesterday").tracks.len(), 1);
        assert_eq!(b.search("no-such-song").tracks.len(), 0);
    }

    #[test]
    fn models_round_trip() {
        let t = MockBackend::fixture_tracks().remove(0);
        let json = serde_json::to_string(&t).expect("serialize");
        let back: Track = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, t);
    }
}
