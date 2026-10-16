//! Core data models (serde-stable,_backend-agnostic).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration_secs: u32,
    /// Small artwork URL (YTM thumbnail) if the source provided one.
    /// Missing on old rows: always `#[serde(default)]`.
    #[serde(default)]
    pub thumbnail_url: Option<String>,
    /// Largest artwork URL (hero views); same default policy as above.
    #[serde(default)]
    pub thumbnail_large_url: Option<String>,
    /// Play count text from search hits (e.g. `"213M plays"`).
    /// Search-only metadata: only the Innertube search mapping populates
    /// it, everything else leaves it `None`, and the SQLite cache never
    /// persists it (stale counts must not survive restarts).
    #[serde(default)]
    pub plays: Option<String>,
}

/// Rewrite a YTM `=wNN-hNN` thumbnail URL to the requested width (same
/// backing image, larger raster; YTM serves arbitrary sizes server-side).
/// Non-matching URLs (other hosts, mocks, empty) pass through unchanged.
pub fn hires_thumb_url(url: &str, width: u32) -> String {
    let Some(eq) = url.find("=w") else {
        return url.to_owned();
    };
    let after_w = &url[eq + 2..];
    let w_end = after_w
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(after_w.len());
    if w_end == 0 {
        return url.to_owned();
    }
    let rest = &after_w[w_end..];
    let Some(h_digits) = rest.strip_prefix("-h") else {
        return url.to_owned();
    };
    let h_end = h_digits
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(h_digits.len());
    if h_end == 0 {
        return url.to_owned();
    }
    format!("{}=w{width}-h{width}{}", &url[..eq], &h_digits[h_end..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hires_rewrites_ytm_sizes_and_keeps_suffix() {
        assert_eq!(
            hires_thumb_url("https://yt3.googleusercontent.com/abc=w60-h60-l90-rj", 544),
            "https://yt3.googleusercontent.com/abc=w544-h544-l90-rj"
        );
        assert_eq!(
            hires_thumb_url(
                "https://yt3.googleusercontent.com/abc=w120-h120-l90-rj",
                544
            ),
            "https://yt3.googleusercontent.com/abc=w544-h544-l90-rj"
        );
    }

    #[test]
    fn hires_passes_through_non_matching() {
        let other = "https://i.ytimg.com/vi/x/hqdefault.jpg";
        assert_eq!(hires_thumb_url(other, 544), other);
        assert_eq!(hires_thumb_url("", 544), "");
        let no_digits = "https://example.invalid/img=w-h10-rj";
        assert_eq!(hires_thumb_url(no_digits, 544), no_digits);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Album {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub year: Option<u16>,
    /// Cover art URL when the source provided one.
    #[serde(default)]
    pub thumbnail_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artist {
    pub id: String,
    pub name: String,
    /// Artist photo URL when the source provided one.
    #[serde(default)]
    pub thumbnail_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Playlist {
    pub id: String,
    pub title: String,
    pub tracks: Vec<Track>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LyricsLine {
    pub start_ms: u64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lyrics {
    pub track_id: String,
    pub lines: Vec<LyricsLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub track_id: String,
    pub played_at_unix: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResults {
    pub tracks: Vec<Track>,
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
    pub playlists: Vec<PlaylistHit>,
}

/// One community playlist search hit: id + title + author + a detail
/// line (view or song count) + art. Tracks stay unfetched until the
/// user plays it (one live fetch, then it queues like anything else).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaylistHit {
    pub id: String,
    pub title: String,
    pub author: String,
    pub detail: String,
    pub thumbnail_url: Option<String>,
}

/// Persisted playback session: the queue as track ids in order, the
/// current index, the position in seconds, shuffle/repeat/volume.
/// Tracks resolve back through the track cache on launch; unknown ids
/// are dropped.
#[derive(Debug, Clone, PartialEq)]
pub struct QueueSnapshot {
    pub ids: Vec<String>,
    pub index: Option<usize>,
    pub position_secs: f64,
    pub shuffle: bool,
    pub repeat: String,
    pub volume: f32,
}

impl Default for QueueSnapshot {
    fn default() -> Self {
        Self {
            ids: Vec::new(),
            index: None,
            position_secs: 0.0,
            shuffle: false,
            repeat: "off".into(),
            volume: 0.8,
        }
    }
}
