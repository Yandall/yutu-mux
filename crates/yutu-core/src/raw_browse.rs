//! Raw Innertube browse for shelf playlists (artist "top songs" full
//! list): the typed `ytmapi-rs` playlist parse drops the per-row play
//! counts, but the web renders them, so they ride the same response.
//! This module POSTs `youtubei/v1/browse` directly (same endpoint,
//! client, and tolerant philosophy as [`crate::raw_search`]) and
//! parses `musicResponsiveListItemRenderer` rows: title + video id,
//! artist, album, duration, and the plays text. Unknown rows are
//! skipped, never fatal. Counts are global, so the call is always
//! anonymous (no personalization involved).

use serde_json::Value;

use crate::innertube::{is_play_count, parse_duration_secs};
use crate::models::Track;
use crate::raw_search::ORIGIN;

const BROWSE_URL: &str =
    "https://music.youtube.com/youtubei/v1/browse?alt=json&prettyPrint=false&key=AIzaSyC9XL3ZjWddXya6X74dJoCTL-WEYFDNX30";

/// Full top-songs shelf: ordered rows with counts where the server
/// sent them. Empty (never an error) when the shelf has no parseable
/// rows: callers fall back to the typed 5-song shelf.
///
/// Anonymous POST (counts are global, not personalized). 25s cap: a
/// hung shelf must fall back, never wedge the page on "Loading…"
/// (the once-per-id guard would otherwise skip every refetch).
const SHELF_TIMEOUT_SECS: u64 = 25;

pub async fn raw_shelf_tracklist(browse_id: &str) -> Result<Vec<Track>, String> {
    let body = serde_json::json!({
        "context": crate::innertube::web_context(),
        "browseId": browse_id,
    });
    let send = crate::shared_client()
        .post(BROWSE_URL)
        .header("User-Agent", crate::innertube::USER_AGENT)
        .header("X-Origin", ORIGIN)
        .header("Origin", ORIGIN)
        .header("Referer", "https://music.youtube.com/")
        .header("Content-Type", "application/json")
        .json(&body)
        .send();
    // One cap for connect + headers + body: a stall anywhere falls
    // back to the typed 5-song shelf instead of wedging the page.
    let fetch = async {
        let response = send
            .await
            .map_err(|e| format!("shelf browse request: {e}"))?;
        if !response.status().is_success() {
            return Err(format!("shelf browse: HTTP {}", response.status()));
        }
        response
            .text()
            .await
            .map_err(|e| format!("shelf browse body: {e}"))
    };
    let text = tokio::time::timeout(std::time::Duration::from_secs(SHELF_TIMEOUT_SECS), fetch)
        .await
        .map_err(|_| "shelf browse timed out".to_string())??;
    Ok(parse_shelf_tracklist(&text))
}

/// Parse a saved shelf-playlist browse response (see
/// `fixtures/shelf_songs.json`) with the same tolerant mapping as
/// [`raw_shelf_tracklist`]. Unparseable JSON → empty vec.
pub fn parse_shelf_tracklist_fixture(json: &str) -> Vec<Track> {
    serde_json::from_str::<Value>(json)
        .map(|root| shelf_rows(&root))
        .unwrap_or_default()
}

/// Tolerant row parse: title/artist/album/duration/count by column
/// role (not position), so reordered shelves still map. Rows without
/// a video id or title are skipped (footers, continuations).
fn parse_shelf_tracklist(text: &str) -> Vec<Track> {
    serde_json::from_str::<Value>(text)
        .map(|root| shelf_rows(&root))
        .unwrap_or_default()
}

/// All `musicResponsiveListItemRenderer` rows under the shelf.
/// Tries the two-column layout first, then the single-column one:
/// the endpoint serves either shape and only a missing shelf (not a
/// moved one) should fall back.
fn shelf_rows(root: &Value) -> Vec<Track> {
    const POINTERS: &[&str] = &[
        "/contents/twoColumnBrowseResultsRenderer/secondaryContents/sectionListRenderer/contents/0/musicPlaylistShelfRenderer/contents",
        "/contents/singleColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents/0/musicPlaylistShelfRenderer/contents",
    ];
    let mut out = Vec::new();
    for pointer in POINTERS {
        if let Some(items) = root.pointer(pointer).and_then(Value::as_array) {
            for item in items {
                if let Some(row) = item.get("musicResponsiveListItemRenderer") {
                    if let Some(track) = parse_shelf_row(row) {
                        out.push(track);
                    }
                }
            }
            if !out.is_empty() {
                return out;
            }
        }
    }
    out
}

/// One shelf row: title (+ video id) from the watch-endpoint column,
/// artist/album from their browse-endpoint columns, plays from the
/// bare-text column, duration from the fixed column, thumbs from the
/// row thumbnail.
fn parse_shelf_row(row: &Value) -> Option<Track> {
    let columns = row.get("flexColumns")?.as_array()?;
    let mut title: Option<&str> = None;
    let mut video_id: Option<&str> = None;
    let mut artist: Option<&str> = None;
    let mut album: Option<&str> = None;
    let mut plays: Option<&str> = None;
    for column in columns {
        let renderer = column.get("musicResponsiveListItemFlexColumnRenderer")?;
        let runs = renderer.pointer("/text/runs")?.as_array()?;
        let first = runs.first()?;
        let text = first.get("text")?.as_str()?;
        if let Some(watch) = first.pointer("/navigationEndpoint/watchEndpoint") {
            title = Some(text);
            video_id = watch.get("videoId")?.as_str();
        } else if let Some(browse) = first.pointer("/navigationEndpoint/browseEndpoint") {
            let page = browse
                .pointer("/browseEndpointContextSupportedConfigs/browseEndpointContextMusicConfig/pageType")
                .and_then(Value::as_str)
                .unwrap_or("");
            if page.contains("ARTIST") {
                artist = Some(text);
            } else if page.contains("ALBUM") {
                album = Some(text);
            }
        } else if is_play_count(text) && text.bytes().any(|b| b.is_ascii_digit()) {
            // Bare-text plays column. The digit gate keeps "Coldplay"
            // (keyword but no digits) landing as an artist below.
            plays = Some(text);
        } else if artist.is_none() {
            // Bare-text artist column (no navigation endpoint).
            artist = Some(text);
        }
    }
    let duration = row
        .pointer("/fixedColumns/0/musicResponsiveListItemFixedColumnRenderer/text/runs/0/text")
        .and_then(Value::as_str)
        .unwrap_or("");
    let thumbs = row
        .pointer("/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails")
        .and_then(Value::as_array);
    let (thumb, large) = match thumbs {
        Some(list) => (
            pick_thumb_url_from(list).map(str::to_string),
            pick_large_thumb_url_from(list).map(str::to_string),
        ),
        None => (None, None),
    };
    Some(Track {
        id: video_id?.to_string(),
        title: title?.to_string(),
        artist: artist.unwrap_or("").to_string(),
        album: album.map(str::to_string).filter(|name| !name.is_empty()),
        duration_secs: parse_duration_secs(duration),
        thumbnail_url: thumb,
        thumbnail_large_url: large,
        plays: plays.map(str::to_string),
    })
}

/// Smallest-width thumbnail URL (row raster), mirroring the catalog
/// pickers over a raw JSON array.
fn pick_thumb_url_from(list: &[Value]) -> Option<&str> {
    list.iter()
        .filter_map(|t| {
            let width = t.get("width")?.as_u64()?;
            let url = t.get("url")?.as_str()?;
            (!url.is_empty()).then_some((width, url))
        })
        .min_by_key(|(w, _)| *w)
        .map(|(_, url)| url)
}

/// Largest-width thumbnail URL (hero art), mirroring the catalog
/// pickers over a raw JSON array.
fn pick_large_thumb_url_from(list: &[Value]) -> Option<&str> {
    list.iter()
        .filter_map(|t| {
            let width = t.get("width")?.as_u64()?;
            let url = t.get("url")?.as_str()?;
            (!url.is_empty()).then_some((width, url))
        })
        .max_by_key(|(w, _)| *w)
        .map(|(_, url)| url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shelf_fixture_maps_rows_by_role_not_position() {
        let tracks = parse_shelf_tracklist_fixture(include_str!("../fixtures/shelf_songs.json"));
        assert_eq!(tracks.len(), 2);
        // Rank, counts, album, and duration survive; thumbs pick the
        // small raster plus the hero.
        assert_eq!(tracks[0].title, "Bohemian Rhapsody");
        assert_eq!(tracks[0].artist, "Queen");
        assert_eq!(tracks[0].plays.as_deref(), Some("2.7B plays"));
        assert_eq!(tracks[0].album.as_deref(), Some("A Night At The Opera"));
        assert_eq!(tracks[0].duration_secs, 355);
        assert!(tracks[0].thumbnail_url.is_some());
        assert!(tracks[0].thumbnail_large_url.is_some());
        // Bare-text artist column (no browse endpoint) still maps,
        // and "Coldplay" stays an artist (keyword, no digits).
        assert_eq!(tracks[1].artist, "Coldplay");
        assert_eq!(tracks[1].plays.as_deref(), Some("875M plays"));
        // Date-like columns never pose as counts.
        assert_eq!(tracks[1].duration_secs, 215);
    }

    #[test]
    fn shelf_parse_is_tolerant() {
        // Garbage → empty, never an error.
        assert!(parse_shelf_tracklist("not json").is_empty());
        assert!(parse_shelf_tracklist("{}").is_empty());
        // Rows without video id or title are skipped (footers).
        let rows = serde_json::json!({
            "contents": { "twoColumnBrowseResultsRenderer": { "secondaryContents": {
                "sectionListRenderer": { "contents": [{ "musicPlaylistShelfRenderer": { "contents": [
                    { "musicResponsiveListItemRenderer": { "flexColumns": [] } },
                    { "musicContinuationItemRenderer": {} },
                ] } }] }
            } } }
        });
        assert!(shelf_rows(&rows).is_empty());
    }
}
