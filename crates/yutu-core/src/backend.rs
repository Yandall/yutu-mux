//! Backend trait + deterministic mock (no network).

use crate::models::{Album, Artist, Lyrics, LyricsLine, Playlist, SearchResults, Track};

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("backend error: {0}")]
    Backend(String),
}

/// Stability anchor: mock now, Innertube later (Phase 4, async + `innertube` feature).
pub trait MusicBackend {
    fn search(&self, query: &str) -> SearchResults;
    fn playlists(&self) -> Vec<Playlist> {
        Vec::new()
    }
    fn lyrics(&self, _track_id: &str) -> Option<Lyrics> {
        None
    }
}

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
                thumbnail_url: None,
                thumbnail_large_url: None,
                plays: None,
            },
            Track {
                id: "m2".into(),
                title: "Something".into(),
                artist: "The Beatles".into(),
                album: Some("Abbey Road".into()),
                duration_secs: 182,
                thumbnail_url: None,
                thumbnail_large_url: None,
                plays: None,
            },
            Track {
                id: "m3".into(),
                title: "Here Comes The Sun".into(),
                artist: "The Beatles".into(),
                album: Some("Abbey Road".into()),
                duration_secs: 185,
                thumbnail_url: None,
                thumbnail_large_url: None,
                plays: None,
            },
            Track {
                id: "m4".into(),
                title: "Yesterday".into(),
                artist: "The Beatles".into(),
                album: Some("Help!".into()),
                duration_secs: 125,
                thumbnail_url: None,
                thumbnail_large_url: None,
                plays: None,
            },
            Track {
                id: "m5".into(),
                title: "Hey Jude".into(),
                artist: "The Beatles".into(),
                album: None,
                duration_secs: 431,
                thumbnail_url: None,
                thumbnail_large_url: None,
                plays: None,
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
                thumbnail_url: None,
            }],
            artists: vec![Artist {
                id: "ar1".into(),
                name: "The Beatles".into(),
                thumbnail_url: None,
            }],
        }
    }

    fn playlists(&self) -> Vec<Playlist> {
        vec![Playlist {
            id: "p1".into(),
            title: "Abbey Road highlights".into(),
            tracks: Self::fixture_tracks().into_iter().take(3).collect(),
        }]
    }

    fn lyrics(&self, track_id: &str) -> Option<Lyrics> {
        if track_id == "m4" {
            Some(Lyrics {
                track_id: track_id.into(),
                lines: vec![
                    LyricsLine {
                        start_ms: 0,
                        text: "Yesterday, all my troubles seemed so far away".into(),
                    },
                    LyricsLine {
                        start_ms: 8000,
                        text: "Now it looks as though they're here to stay".into(),
                    },
                ],
            })
        } else {
            None
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
    fn mock_playlists_and_lyrics() {
        let b = MockBackend::new();
        assert_eq!(b.playlists().len(), 1);
        assert!(b.lyrics("m4").is_some());
        assert!(b.lyrics("m1").is_none());
    }
}
