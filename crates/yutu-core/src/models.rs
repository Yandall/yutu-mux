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

/// Server-ranked top result kind (the web's "Top result" hero).
/// Station/podcast kinds are not playable here and collapse to Other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TopKind {
    Song,
    Video,
    Artist,
    Album,
    Playlist,
    Other,
}

/// The unfiltered search's top result: display metadata plus the
/// playable/browse id from the card tap target (the card itself is
/// directly actionable, unlike ytmapi's id-less top result).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TopHit {
    pub kind: TopKind,
    pub id: String,
    pub title: String,
    pub artist: String,
    pub thumbnail_url: Option<String>,
}

/// Position of one row in the web mixed order: kind + id into the
/// typed vecs below. Ids survive keystroke narrowing; dangling refs
/// (narrowed away) are skipped when rendering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MixedRef {
    /// 0 = track (song or video), 1 = artist, 2 = album, 3 = playlist.
    pub kind: u8,
    pub id: String,
}

/// One unfiltered (web-style "All") search response: every shelf in
/// server relevance order plus the top hit. Songs and videos share
/// `tracks` (each shelf server-ordered); featured playlists lead
/// community ones; `mixed` is the exact web row order. The All tab
/// renders this order as-is: no client re-sort, so official hits stay
/// on top. Flat items carry no durations (the API omits them), so
/// rows hide zero durations.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchAll {
    pub top: Option<TopHit>,
    pub tracks: Vec<Track>,
    pub artists: Vec<Artist>,
    pub albums: Vec<Album>,
    pub playlists: Vec<PlaylistHit>,
    pub mixed: Vec<MixedRef>,
}

/// Persisted playback session: the queue as track ids in order, the
/// current index, the position in seconds, shuffle/repeat/volume/mute.
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
    pub muted: bool,
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
            muted: false,
        }
    }
}
