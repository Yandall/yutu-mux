//! Raw Innertube timed lyrics: the typed `ytmapi-rs` lyrics flow is
//! plain-text only (WEB_REMIX serves the description shelf), but the
//! same lyrics `browseId` served to an ANDROID_MUSIC client carries a
//! `timedLyricsModel` with per-line start timestamps. This module POSTs
//! `youtubei/v1/browse` directly (same endpoint, key, and browser
//! headers as [`crate::raw_browse`]) and parses
//! `timedLyricsData` into [`LyricLine`]s. Unknown shapes read as empty
//! (never fatal): callers fall back to the plain typed flow.

use serde_json::Value;

use crate::lyrics_providers::LyricLine;
use crate::raw_search::ORIGIN;

const BROWSE_URL: &str =
    "https://music.youtube.com/youtubei/v1/browse?alt=json&prettyPrint=false&key=AIzaSyC9XL3ZjWddXya6X74dJoCTL-WEYFDNX30";

/// Anonymous POST cap: a hung lyrics fetch must fall back, never wedge
/// the lyrics pane on "Loading..." (the in-flight guard would otherwise
/// skip every refetch).
const TIMED_LYRICS_TIMEOUT_SECS: u64 = 15;

/// Timed lyric lines for a lyrics `browseId` (see
/// [`crate::innertube::InnertubeBackend::song_timed_lyrics`]). Empty when
/// the server has no timed model for the track: callers fall back to
/// the plain typed lyrics.
pub async fn raw_timed_lyrics(browse_id: &str) -> Result<Vec<LyricLine>, String> {
    let body = serde_json::json!({
        "context": crate::innertube::android_music_context(),
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
    let fetch = async {
        let response = send
            .await
            .map_err(|e| format!("timed lyrics request: {e}"))?;
        if !response.status().is_success() {
            return Err(format!("timed lyrics: HTTP {}", response.status()));
        }
        response
            .text()
            .await
            .map_err(|e| format!("timed lyrics body: {e}"))
    };
    let text = tokio::time::timeout(
        std::time::Duration::from_secs(TIMED_LYRICS_TIMEOUT_SECS),
        fetch,
    )
    .await
    .map_err(|_| "timed lyrics timed out".to_string())??;
    Ok(parse_timed_lyrics(&text))
}

/// Parse a saved timed-lyrics browse response (see
/// `fixtures/timed_lyrics.json`) with the same tolerant mapping as
/// [`raw_timed_lyrics`]. Unparseable JSON or a missing timed model
/// reads as empty, never an error.
pub fn parse_timed_lyrics_fixture(json: &str) -> Vec<LyricLine> {
    parse_timed_lyrics(json)
}

/// Tolerant timed-lyrics parse: first hit wins across the known model
/// shapes, cue times accept string or numeric milliseconds, blank lines
/// are skipped, output is sorted by start time.
fn parse_timed_lyrics(text: &str) -> Vec<LyricLine> {
    let root: Value = match serde_json::from_str(text) {
        Ok(root) => root,
        Err(_) => return Vec::new(),
    };
    const POINTERS: &[&str] = &[
        "/contents/elementRenderer/newElement/type/componentType/model/timedLyricsModel/lyricsData/timedLyricsData",
        "/contents/elementRenderer/newElement/type/componentType/model/timedLyricsModel/timedLyricsData",
    ];
    for pointer in POINTERS {
        if let Some(items) = root.pointer(pointer).and_then(Value::as_array) {
            let mut lines: Vec<LyricLine> = items.iter().filter_map(parse_timed_line).collect();
            if !lines.is_empty() {
                lines.sort_by_key(|l| l.start_ms);
                return lines;
            }
        }
    }
    Vec::new()
}

/// One timed line: `lyricLine` text plus
/// `cueRange.startTimeMilliseconds` (string or number). Blank text or a
/// missing/unparseable timestamp skips the line.
fn parse_timed_line(item: &Value) -> Option<LyricLine> {
    let text = item.get("lyricLine")?.as_str()?;
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let cue = item.get("cueRange")?.get("startTimeMilliseconds")?;
    let start_ms = match cue {
        Value::String(s) => s.trim().parse::<u64>().ok()?,
        Value::Number(n) => n.as_u64()?,
        _ => return None,
    };
    Some(LyricLine {
        start_ms,
        text: text.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timed_fixture_maps_cues_to_lines() {
        let lines = parse_timed_lyrics_fixture(include_str!("../fixtures/timed_lyrics.json"));
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].start_ms, 1_240);
        assert_eq!(lines[0].text, "Hello");
        assert_eq!(lines[1].start_ms, 5_010);
        assert_eq!(lines[2].start_ms, 9_300);
        assert!(lines.windows(2).all(|w| w[0].start_ms <= w[1].start_ms));
    }

    #[test]
    fn timed_parse_is_tolerant() {
        // Garbage or missing model reads as empty, never an error.
        assert!(parse_timed_lyrics("not json").is_empty());
        assert!(parse_timed_lyrics("{}").is_empty());
        // Blank lines and bad cues are skipped; numeric cues parse.
        let rows = serde_json::json!({
            "contents": { "elementRenderer": { "newElement": { "type": { "componentType": {
                "model": { "timedLyricsModel": { "lyricsData": { "timedLyricsData": [
                    { "cueRange": { "startTimeMilliseconds": "1000" }, "lyricLine": "  " },
                    { "cueRange": { "startTimeMilliseconds": "bad" }, "lyricLine": "nope" },
                    { "cueRange": { "startTimeMilliseconds": 2000 }, "lyricLine": "kept" },
                    { "cueRange": {}, "lyricLine": "no cue" },
                ] } } }
            } } } } }
        });
        let lines = parse_timed_lyrics(&rows.to_string());
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].start_ms, 2_000);
        assert_eq!(lines[0].text, "kept");
    }
}
