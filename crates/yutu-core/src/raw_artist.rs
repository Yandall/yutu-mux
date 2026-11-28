//! Raw Innertube artist page: every carousel the web shows (Albums,
//! Singles & EPs, Videos, Live performances, Featured on, Playlists by
//! the artist, Fans might also like, ...), in server order, plus the
//! discography grids behind the "More" buttons. The typed `ytmapi-rs`
//! parse only keeps four shelves and no "More" links, so this module
//! POSTs `youtubei/v1/browse` itself (same endpoint and tolerant
//! philosophy as [`crate::raw_browse`]). Cards the app cannot open
//! (podcasts, episodes, user channels) are skipped, shelves left empty
//! drop out, and nothing unknown is ever fatal. Anonymous on purpose:
//! the page is public and the same for everyone.

use serde_json::Value;

use crate::models::{ArtistPage, ArtistShelf, ShelfItem, ShelfItemKind, ShelfMore};
use crate::raw_browse::post_browse;

const ARTIST_TIMEOUT_SECS: u64 = 25;

const SECTIONS: &str =
    "/contents/singleColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents";

/// Client context with the UI language (`hl`), so shelf titles and
/// subtitles ("Singles & EPs", "Sencillos y EP") match the app.
fn context(hl: &str) -> Value {
    let mut ctx = crate::innertube::web_context();
    if let Some(client) = ctx.get_mut("client") {
        client["hl"] = Value::String(hl.to_string());
    }
    ctx
}

/// Every shelf of an artist channel page (`hl` = UI language code).
pub async fn raw_artist_page(channel_id: &str, hl: &str) -> Result<ArtistPage, String> {
    let body = serde_json::json!({
        "context": context(hl),
        "browseId": channel_id,
    });
    let text = post_browse(&body, ARTIST_TIMEOUT_SECS).await?;
    let root: Value = serde_json::from_str(&text).map_err(|e| format!("artist page json: {e}"))?;
    Ok(parse_artist_page(&root))
}

/// A "More" grid (the full discography behind Albums or Singles &
/// EPs). Empty when the grid has no parseable cards.
pub async fn raw_artist_grid(
    browse_id: &str,
    params: &str,
    hl: &str,
) -> Result<Vec<ShelfItem>, String> {
    let body = serde_json::json!({
        "context": context(hl),
        "browseId": browse_id,
        "params": params,
    });
    let text = post_browse(&body, ARTIST_TIMEOUT_SECS).await?;
    let root: Value = serde_json::from_str(&text).map_err(|e| format!("artist grid json: {e}"))?;
    Ok(parse_artist_grid(&root))
}

/// Parse a saved artist page response (fixtures, tests).
pub fn parse_artist_page_json(json: &str) -> ArtistPage {
    serde_json::from_str::<Value>(json)
        .map(|root| parse_artist_page(&root))
        .unwrap_or_default()
}

/// Parse a saved "More" grid response (fixtures, tests).
pub fn parse_artist_grid_json(json: &str) -> Vec<ShelfItem> {
    serde_json::from_str::<Value>(json)
        .map(|root| parse_artist_grid(&root))
        .unwrap_or_default()
}

fn parse_artist_page(root: &Value) -> ArtistPage {
    let header = root
        .get("header")
        .and_then(Value::as_object)
        .and_then(|h| h.values().next());
    let name = header
        .and_then(|h| h.pointer("/title/runs"))
        .map(runs_text)
        .unwrap_or_default();
    let thumbnail_url = header.and_then(|h| {
        h.pointer("/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails")
            .and_then(largest_thumb)
    });
    let shelves = root
        .pointer(SECTIONS)
        .and_then(Value::as_array)
        .map(|sections| {
            sections
                .iter()
                .filter_map(|s| s.get("musicCarouselShelfRenderer"))
                .filter_map(parse_shelf)
                .collect()
        })
        .unwrap_or_default();
    ArtistPage {
        name,
        thumbnail_url,
        shelves,
    }
}

fn parse_artist_grid(root: &Value) -> Vec<ShelfItem> {
    let Some(sections) = root.pointer(SECTIONS).and_then(Value::as_array) else {
        return Vec::new();
    };
    sections
        .iter()
        .filter_map(|s| s.pointer("/gridRenderer/items").and_then(Value::as_array))
        .flatten()
        .filter_map(|i| i.get("musicTwoRowItemRenderer"))
        .filter_map(parse_item)
        .collect()
}

/// One carousel: title, cards, and the "More" target when the app
/// can open it. Carousels with no usable card return `None`.
fn parse_shelf(shelf: &Value) -> Option<ArtistShelf> {
    let header = shelf.pointer("/header/musicCarouselShelfBasicHeaderRenderer")?;
    let title = header.pointer("/title/runs").map(runs_text)?;
    if title.trim().is_empty() {
        return None;
    }
    let items: Vec<ShelfItem> = shelf
        .get("contents")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(|i| i.get("musicTwoRowItemRenderer"))
        .filter_map(parse_item)
        .collect();
    if items.is_empty() {
        return None;
    }
    let more = header
        .pointer("/moreContentButton/buttonRenderer/navigationEndpoint/browseEndpoint")
        .or_else(|| header.pointer("/title/runs/0/navigationEndpoint/browseEndpoint"))
        .and_then(parse_more);
    Some(ArtistShelf { title, items, more })
}

/// "More" targets that work anonymously: the discography grid
/// (`MPAD...`) and plain playlists (`VL...`). Channel tabs with
/// params (Live performances, Playlists by ...) answer empty without
/// a session, so those shelves get no "More" button.
fn parse_more(endpoint: &Value) -> Option<ShelfMore> {
    let browse_id = endpoint.get("browseId")?.as_str()?;
    if let Some(list) = browse_id.strip_prefix("VL") {
        return Some(ShelfMore::Playlist(list.to_string()));
    }
    if browse_id.starts_with("MPAD") {
        let params = endpoint.get("params")?.as_str()?;
        // The web sends the params URL-encoded (`%3D`); the API wants
        // the raw base64.
        return Some(ShelfMore::Grid {
            browse_id: browse_id.to_string(),
            params: params.replace("%3D", "="),
        });
    }
    None
}

/// One card. The navigation endpoint decides the kind: a watch
/// endpoint is a video, a browse endpoint's page type an album,
/// playlist or artist. Everything else is skipped.
fn parse_item(item: &Value) -> Option<ShelfItem> {
    let title = item.pointer("/title/runs").map(runs_text)?;
    if title.trim().is_empty() {
        return None;
    }
    let subtitle = item
        .pointer("/subtitle/runs")
        .map(runs_text)
        .unwrap_or_default()
        .replace('\u{2022}', "\u{b7}");
    let thumbnail_url = item
        .pointer("/thumbnailRenderer/musicThumbnailRenderer/thumbnail/thumbnails")
        .and_then(largest_thumb);
    let nav = item
        .get("navigationEndpoint")
        .or_else(|| item.pointer("/title/runs/0/navigationEndpoint"))?;
    let (kind, id) = if let Some(video) = nav.pointer("/watchEndpoint/videoId") {
        (ShelfItemKind::Video, video.as_str()?.to_string())
    } else {
        let browse = nav.get("browseEndpoint")?;
        let id = browse.get("browseId")?.as_str()?;
        let page = browse
            .pointer(
                "/browseEndpointContextSupportedConfigs/browseEndpointContextMusicConfig/pageType",
            )
            .and_then(Value::as_str)
            .unwrap_or("");
        match page {
            "MUSIC_PAGE_TYPE_ALBUM" | "MUSIC_PAGE_TYPE_AUDIOBOOK" => {
                (ShelfItemKind::Album, id.to_string())
            }
            "MUSIC_PAGE_TYPE_PLAYLIST" => (
                ShelfItemKind::Playlist,
                id.strip_prefix("VL").unwrap_or(id).to_string(),
            ),
            "MUSIC_PAGE_TYPE_ARTIST" => (ShelfItemKind::Artist, id.to_string()),
            _ => return None,
        }
    };
    if id.is_empty() {
        return None;
    }
    let year = match kind {
        ShelfItemKind::Album => year_of(&subtitle),
        _ => None,
    };
    Some(ShelfItem {
        kind,
        id,
        title,
        subtitle,
        year,
        thumbnail_url,
    })
}

/// Concatenated text of a `runs` array.
fn runs_text(runs: &Value) -> String {
    runs.as_array()
        .map(|runs| {
            runs.iter()
                .filter_map(|r| r.get("text").and_then(Value::as_str))
                .collect::<String>()
        })
        .unwrap_or_default()
        .replace('\u{a0}', " ")
        .trim()
        .to_string()
}

/// Widest thumbnail URL of a `thumbnails` array.
fn largest_thumb(thumbs: &Value) -> Option<String> {
    thumbs
        .as_array()?
        .iter()
        .filter_map(|t| {
            let url = t.get("url")?.as_str()?;
            let width = t.get("width").and_then(Value::as_u64).unwrap_or(0);
            Some((width, url))
        })
        .max_by_key(|(w, _)| *w)
        .map(|(_, url)| url.to_string())
}

/// Release year in a card subtitle: the last 4-digit token in a
/// plausible range ("EP · 2026" -> 2026, "2025" -> 2025).
pub(crate) fn year_of(subtitle: &str) -> Option<u16> {
    subtitle
        .split(|c: char| !c.is_ascii_digit())
        .rev()
        .filter(|tok| tok.len() == 4)
        .filter_map(|tok| tok.parse::<u16>().ok())
        .find(|y| (1900..=2100).contains(y))
}
