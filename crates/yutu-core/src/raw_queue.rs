//! Raw Innertube `music/get_queue` for song lengths. Several sources
//! carry no duration (flat search "All" items, artist top songs, some
//! playlist rows), so rows would show a blank length until the song is
//! played once. `get_queue` takes a list of video ids and answers one
//! `playlistPanelVideoRenderer` per id with its `lengthText`: one
//! anonymous request fills a whole screenful. Unplayable or unknown
//! ids simply come back without a length (never an error).

use serde_json::Value;

use crate::innertube::parse_duration_secs;
use crate::raw_search::ORIGIN;

const QUEUE_URL: &str = "https://music.youtube.com/youtubei/v1/music/get_queue?alt=json&prettyPrint=false&key=AIzaSyC9XL3ZjWddXya6X74dJoCTL-WEYFDNX30";

/// Most ids sent in one request (the web client asks for about this
/// many when it builds a queue; callers chunk longer lists).
pub const QUEUE_BATCH_MAX: usize = 50;

/// One cap for connect + headers + body: a stalled probe must free its
/// ids for a later retry instead of hanging forever.
const QUEUE_TIMEOUT_SECS: u64 = 20;

/// Lengths for `ids` as `(video_id, seconds)`, only for ids the server
/// answered with a parseable length. At most [`QUEUE_BATCH_MAX`] ids
/// are sent (the rest are ignored). Anonymous (lengths are global).
pub async fn raw_queue_durations(ids: &[String]) -> Result<Vec<(String, u32)>, String> {
    let ids: Vec<&String> = ids.iter().take(QUEUE_BATCH_MAX).collect();
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let body = serde_json::json!({
        "context": crate::innertube::web_context(),
        "videoIds": ids,
    });
    let send = crate::shared_client()
        .post(QUEUE_URL)
        .header("User-Agent", crate::innertube::USER_AGENT)
        .header("X-Origin", ORIGIN)
        .header("Origin", ORIGIN)
        .header("Referer", "https://music.youtube.com/")
        .header("Content-Type", "application/json")
        .json(&body)
        .send();
    let fetch = async {
        let response = send.await.map_err(|e| format!("queue request: {e}"))?;
        if !response.status().is_success() {
            return Err(format!("queue: HTTP {}", response.status()));
        }
        response
            .text()
            .await
            .map_err(|e| format!("queue body: {e}"))
    };
    let text = tokio::time::timeout(std::time::Duration::from_secs(QUEUE_TIMEOUT_SECS), fetch)
        .await
        .map_err(|_| "queue timed out".to_string())??;
    Ok(parse_queue_durations(&text))
}

/// Parse a `get_queue` response body into `(video_id, seconds)` pairs.
/// Renderers come bare or wrapped (`playlistPanelVideoWrapperRenderer`
/// when a song has a video counterpart); rows without a length
/// (blocked, removed) or with an unparseable one are skipped.
/// Unparseable JSON → empty vec.
pub fn parse_queue_durations(text: &str) -> Vec<(String, u32)> {
    let Ok(root) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let Some(items) = root.get("queueDatas").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for item in items {
        let Some(content) = item.get("content") else {
            continue;
        };
        let renderer = content.get("playlistPanelVideoRenderer").or_else(|| {
            content.pointer(
                "/playlistPanelVideoWrapperRenderer/primaryRenderer/playlistPanelVideoRenderer",
            )
        });
        let Some(renderer) = renderer else {
            continue;
        };
        let Some(id) = renderer.get("videoId").and_then(Value::as_str) else {
            continue;
        };
        let Some(length) = renderer
            .pointer("/lengthText/runs/0/text")
            .and_then(Value::as_str)
        else {
            continue;
        };
        let secs = parse_duration_secs(length);
        if secs > 0 && !out.iter().any(|(seen, _): &(String, u32)| seen == id) {
            out.push((id.to_string(), secs));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bare_and_wrapped_renderers_and_skips_lengthless_rows() {
        let body = serde_json::json!({
            "queueDatas": [
                { "content": { "playlistPanelVideoRenderer": {
                    "videoId": "aaa", "lengthText": { "runs": [{ "text": "3:34" }] } } } },
                { "content": { "playlistPanelVideoWrapperRenderer": { "primaryRenderer": {
                    "playlistPanelVideoRenderer": {
                        "videoId": "bbb", "lengthText": { "runs": [{ "text": "1:02:03" }] } } } } } },
                { "content": { "playlistPanelVideoRenderer": {
                    "videoId": "blocked",
                    "unplayableText": { "runs": [{ "text": "[Video blocked in country]" }] } } } },
                { "content": { "playlistPanelVideoRenderer": {
                    "videoId": "bad", "lengthText": { "runs": [{ "text": "LIVE" }] } } } },
                { "content": { "automixPreviewVideoRenderer": {} } }
            ]
        })
        .to_string();
        assert_eq!(
            parse_queue_durations(&body),
            vec![("aaa".to_string(), 214), ("bbb".to_string(), 3723)]
        );
    }

    #[test]
    fn malformed_bodies_yield_nothing() {
        assert!(parse_queue_durations("not json").is_empty());
        assert!(parse_queue_durations("{}").is_empty());
        assert!(parse_queue_durations(r#"{"queueDatas": 3}"#).is_empty());
    }

    /// Live: needs network. `cargo test -p yutu-core raw_queue -- --ignored`
    #[tokio::test]
    #[ignore]
    async fn live_queue_returns_lengths() {
        let ids = vec!["dQw4w9WgXcQ".to_string(), "JWW8KK8kd1I".to_string()];
        let got = raw_queue_durations(&ids).await.expect("queue request");
        assert!(got
            .iter()
            .any(|(id, secs)| id == "dQw4w9WgXcQ" && *secs > 0));
    }
}
