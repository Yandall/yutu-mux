//! Stream-URL resolution via the `rustypipe` crate.
//!
//! Why this module exists: YouTube ships most audio URLs ciphered
//! (`signatureCipher`) and PO-gated. Deciphering needs the player JS
//! deobfuscator + BotGuard sidecar — both maintained upstream in `rustypipe`
//! (which auto-detects `rustypipe-botguard` in `PATH`). Reimplementing that
//! here would duplicate a moving target, so the full path delegates.
//!
//! License note: `rustypipe` is GPL-3.0. This module (and any binary built
//! with the `rp` feature) links GPL code — see `docs/INNERTUBE.md`. Default
//! builds without `rp` stay MIT-pure.

use crate::stream::{StreamError, StreamRequest};

/// Resolve `video_id` to a playable audio URL using rustypipe's player
/// (client fallback + decipher + sidecar PO tokens handled internally).
///
/// `storage_dir`: rustypipe cache home (player JS, deobfuscator state).
/// `botguard_bin`: explicit sidecar path, or `None` for `PATH`/cwd
/// auto-detection.
pub async fn resolve_audio_url(
    video_id: &str,
    storage_dir: &str,
    botguard_bin: Option<&str>,
) -> Result<crate::innertube::ResolvedStream, StreamError> {
    use rustypipe::client::RustyPipe;
    use rustypipe::param::StreamFilter;

    let mut builder = RustyPipe::builder().storage_dir(storage_dir);
    if let Some(bin) = botguard_bin {
        builder = builder.botguard_bin(bin);
    }
    let rp = builder
        .build()
        .map_err(|e| StreamError::Network(format!("rustypipe setup: {e}")))?;
    eprintln!("[yutu-stream] rustypipe player query for {video_id}");
    let player = rp
        .query()
        .player(video_id)
        .await
        .map_err(|e| classify_rustypipe_error(&e.to_string(), video_id))?;
    let (_, audio) = player.select_video_audio_stream(&StreamFilter::default());
    let stream = audio.ok_or_else(|| StreamError::NotFound {
        video_id: video_id.to_string(),
    })?;
    let url = stream.url.to_string();
    eprintln!("[yutu-stream] rustypipe yielded URL (len {})", url.len());
    Ok(crate::innertube::ResolvedStream {
        url,
        pot_token: None,
    })
}

/// Track which clients were attempted, for diagnostics parity with the raw
/// path. rustypipe walks its own `player_client_order()` internally.
pub fn record_rustypipe_attempt(req: &mut StreamRequest) {
    use crate::stream::StreamClient;
    for client in StreamClient::fallback_chain() {
        if !req.tried.contains(client) {
            req.tried.push(*client);
        }
    }
}

fn classify_rustypipe_error(message: &str, video_id: &str) -> StreamError {
    crate::innertube::classify_player_error(
        message,
        crate::stream::StreamClient::WebMusic,
        video_id,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attempt_recording_marks_chain() {
        let mut req = StreamRequest::new("vid");
        record_rustypipe_attempt(&mut req);
        assert_eq!(req.tried.len(), 3);
    }

    #[test]
    #[ignore = "live network (+sidecar for gated videos): rustypipe audio URL"]
    fn live_rustypipe_resolves_audio_url() {
        let dir = std::env::temp_dir().join("yutu-rp-test");
        let _ = std::fs::create_dir_all(&dir);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let video = std::env::var("YUTU_TEST_VIDEO_ID").unwrap_or_else(|_| "dQw4w9WgXcQ".into());
        let resolved = rt
            .block_on(resolve_audio_url(
                &video,
                &dir.to_string_lossy(),
                crate::pot::find_botguard_bin().as_deref(),
            ))
            .expect("rustypipe resolves audio URL");
        assert!(resolved.url.starts_with("https://"));
    }
}
