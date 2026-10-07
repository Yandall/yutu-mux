//! Live Innertube backend built on the real `ytmapi-rs` 0.3 client.
//!
//! Feature-gated behind `innertube` so offline builds (and CI without network)
//! keep working with [`crate::backend::MockBackend`].
//!
//! What is live vs stubbed (see `docs/INNERTUBE.md` for the full picture):
//! - LIVE: anonymous + cookie-file connection, `search_suggestions`,
//!   per-video `player`-endpoint probes (the same endpoint real stream
//!   resolution uses) via [`InnertubeBackend::resolve_stream`].
//! - STUBBED: direct audio-stream URL extraction. `ytmapi-rs` 0.3 exposes no
//!   `streamingData` parser, so [`extract_audio_url`] best-effort scans the raw
//!   `player` JSON for an `adaptiveFormats` audio `url`. Ciphered
//!   (`signatureCipher`) or missing formats exhaust the fallback chain to
//!   [`StreamError::NotFound`](crate::stream::StreamError::NotFound).
//!
//! Cookie safety: cookie material is never logged. [`CookieAuth::from_file`]
//! validates the file locally first; ytmapi errors echoed here contain server
//! responses, never request headers. `BrowserToken`'s own `Debug` impl prints
//! only `"Private BrowserToken"`, so deriving `Debug` below is safe.
//!
//! [`CookieAuth::from_file`]: crate::auth::CookieAuth

use std::path::Path;

use ytmapi_rs::auth::noauth::NoAuthToken;
use ytmapi_rs::auth::BrowserToken;
use ytmapi_rs::common::{VideoID, YoutubeID};
use ytmapi_rs::query::GetSongTrackingUrlQuery;
use ytmapi_rs::YtMusic;

use crate::auth::CookieAuth;
use crate::stream::{StreamClient, StreamError, StreamRequest};

/// Live YouTube Music handle.
///
/// `YtMusic` is generic over its auth token (`NoAuthToken` vs `BrowserToken`),
/// so both are carried in one enum instead of leaking the generic into our
/// public API. The inner client owns a connection pool — clone and re-use it.
#[derive(Debug, Clone)]
pub struct InnertubeBackend {
    inner: Inner,
}

#[derive(Debug, Clone)]
enum Inner {
    Anonymous(YtMusic<NoAuthToken>),
    Cookie(YtMusic<BrowserToken>),
}

impl InnertubeBackend {
    /// Connect without credentials (fewer queries supported server-side).
    ///
    /// Performs network I/O (fetches visitor data), so failures surface as a
    /// plain string without secret material (there is none here).
    pub async fn connect_anonymous() -> Result<Self, String> {
        YtMusic::new_unauthenticated()
            .await
            .map(|client| Self {
                inner: Inner::Anonymous(client),
            })
            .map_err(|e| format!("innertube anonymous connect failed: {e}"))
    }

    /// Connect with a browser cookie file.
    ///
    /// `CookieAuth::from_file` validates the file first (empty/missing file
    /// fails before any network). The cookie value itself is never logged;
    /// only the ytmapi error (server response, no request headers) is echoed.
    pub async fn connect_with_cookie_file(path: &Path) -> Result<Self, String> {
        // Local validation; the parsed value is intentionally dropped — the
        // real client reads the file itself.
        let _auth = CookieAuth::from_file(path).map_err(|e| format!("cookie file: {e}"))?;
        YtMusic::from_cookie_file(path)
            .await
            .map(|client| Self {
                inner: Inner::Cookie(client),
            })
            .map_err(|_| "innertube cookie login failed (bad/expired cookie?)".to_string())
    }

    /// One real read query, proving the client is wired: returns suggestion
    /// texts for `q` (concatenated runs, formatting stripped).
    pub async fn search_suggestions(&self, q: &str) -> Result<Vec<String>, String> {
        let suggestions = match &self.inner {
            Inner::Anonymous(client) => client
                .get_search_suggestions(q)
                .await
                .map_err(|e| format!("search suggestions failed: {e}"))?,
            Inner::Cookie(client) => client
                .get_search_suggestions(q)
                .await
                .map_err(|e| format!("search suggestions failed: {e}"))?,
        };
        Ok(suggestions.iter().map(|s| s.get_text()).collect())
    }

    /// Resolve `req.video_id` to a playable URL, walking the
    /// [`StreamClient::fallback_chain`] (`android_vr` first: no PO token
    /// needed; `web_music`/`ios` may 403 without one).
    ///
    /// Each client is recorded in `req.tried` via [`StreamRequest::next_client`].
    /// Per client we hit the live Innertube `player` endpoint (the closest
    /// `ytmapi-rs` 0.3 offers — see [`GetSongTrackingUrlQuery`]) and
    /// best-effort extract an audio URL with [`extract_audio_url`].
    /// HTTP-403-like failures map to [`StreamError::PoTokenRequired`] (see
    /// [`StreamError::hint`](crate::stream::StreamError::hint)); an exhausted
    /// chain maps to [`StreamError::NotFound`](crate::stream::StreamError::NotFound).
    /// A PO-token failure anywhere in the chain takes precedence on exhaustion
    /// because it is the actionable error.
    pub async fn resolve_stream(&self, req: &mut StreamRequest) -> Result<String, StreamError> {
        let mut po_token_client: Option<&'static str> = None;
        while let Some(client) = req.next_client() {
            match self.player_url_for_client(client, &req.video_id).await {
                Ok(url) => return Ok(url),
                Err(StreamError::PoTokenRequired { client }) => {
                    po_token_client = Some(client);
                }
                Err(StreamError::NotFound { .. }) | Err(StreamError::Network(_)) => {
                    // This client cannot play the video; fall through to next.
                }
            }
        }
        match po_token_client {
            Some(client) => Err(StreamError::PoTokenRequired { client }),
            None => Err(StreamError::NotFound {
                video_id: req.video_id.clone(),
            }),
        }
    }

    /// Live `player`-endpoint probe for one fallback client.
    ///
    /// `ytmapi-rs` 0.3 pins a single WEB_REMIX client per handle, so `client`
    /// is currently used for error attribution and chain bookkeeping; the GVS
    /// PO-token policy per client is documented in `docs/INNERTUBE.md`.
    async fn player_url_for_client(
        &self,
        client: StreamClient,
        video_id: &str,
    ) -> Result<String, StreamError> {
        let query = GetSongTrackingUrlQuery::new(VideoID::from_raw(video_id))
            .map_err(|e| StreamError::Network(format!("player query setup: {e}")))?;
        let raw = self.raw_player_json(&query).await;
        let raw = raw.map_err(|e| classify_player_error(&e.to_string(), client, video_id))?;
        let value: serde_json::Value = serde_json::from_str(&raw)
            .map_err(|e| StreamError::Network(format!("player JSON: {e}")))?;
        extract_audio_url(&value).ok_or_else(|| classify_no_url(&value, client, video_id))
    }

    /// Raw `player`-endpoint JSON for one probe. The `Q` turbofish is required:
    /// `&GetSongTrackingUrlQuery` satisfies several `Borrow<_>` impls, so
    /// inference alone fails with E0283.
    async fn raw_player_json(
        &self,
        query: &GetSongTrackingUrlQuery<'_>,
    ) -> Result<String, ytmapi_rs::Error> {
        match &self.inner {
            Inner::Anonymous(backend) => {
                backend
                    .raw_json_query::<GetSongTrackingUrlQuery<'_>>(query)
                    .await
            }
            Inner::Cookie(backend) => {
                backend
                    .raw_json_query::<GetSongTrackingUrlQuery<'_>>(query)
                    .await
            }
        }
    }
}

/// Map a `player`-endpoint failure to our error policy: 403-flavoured errors
/// (GVS PO-token enforcement) become [`StreamError::PoTokenRequired`],
/// everything else becomes [`StreamError::Network`].
pub fn classify_player_error(message: &str, client: StreamClient, video_id: &str) -> StreamError {
    let lowered = message.to_lowercase();
    let po_token = lowered.contains("403")
        || lowered.contains("po token")
        || lowered.contains("potoken")
        || lowered.contains("botguard")
        || lowered.contains("status_failed")
        || lowered.contains("status failed");
    if po_token {
        StreamError::PoTokenRequired {
            client: client.as_str(),
        }
    } else if lowered.contains("unplayable")
        || lowered.contains("login_required")
        || lowered.contains("login required")
    {
        StreamError::NotFound {
            video_id: video_id.to_string(),
        }
    } else {
        StreamError::Network(message.to_string())
    }
}

/// Classify a `player` response that parsed but yielded no audio URL:
/// explicit unplayable/login-required status becomes `NotFound`, otherwise the
/// client is treated as PO-token-gated (GVS 403s surface as empty
/// `streamingData` for web clients).
fn classify_no_url(value: &serde_json::Value, client: StreamClient, video_id: &str) -> StreamError {
    let status = value
        .pointer("/playabilityStatus/status")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    match status {
        "UNPLAYABLE" | "LOGIN_REQUIRED" => StreamError::NotFound {
            video_id: video_id.to_string(),
        },
        _ => StreamError::PoTokenRequired {
            client: client.as_str(),
        },
    }
}

/// Best-effort audio URL extraction from a raw Innertube `player` response.
///
/// Scans `streamingData.adaptiveFormats` (then `streamingData.formats`) for the
/// first entry whose `mimeType` starts with `audio/` and carries a direct
/// `url`. Returns `None` for ciphered (`signatureCipher`-only), missing, or
/// malformed `streamingData` — callers treat that as "this client cannot play
/// it" per the fallback policy.
pub fn extract_audio_url(player: &serde_json::Value) -> Option<String> {
    for pointer in ["/streamingData/adaptiveFormats", "/streamingData/formats"] {
        if let Some(url) = first_audio_url(player.pointer(pointer)) {
            return Some(url);
        }
    }
    None
}

fn first_audio_url(formats: Option<&serde_json::Value>) -> Option<String> {
    formats?.as_array()?.iter().find_map(|f| {
        let mime = f.get("mimeType")?.as_str()?;
        if !mime.starts_with("audio/") {
            return None;
        }
        f.get("url")?.as_str().map(str::to_string)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn po_token_mapping_covers_403_and_botguard() {
        for msg in [
            "Http error code 403 recieved in response. Message: <Forbidden>.",
            "web error <403> received",
            "PO token required for playback",
            "botguard challenge missing",
        ] {
            assert_eq!(
                classify_player_error(msg, StreamClient::WebMusic, "vid"),
                StreamError::PoTokenRequired {
                    client: "web_music"
                }
            );
        }
    }

    #[test]
    fn unplayable_maps_to_not_found_and_other_to_network() {
        assert_eq!(
            classify_player_error("playability UNPLAYABLE", StreamClient::Ios, "vid"),
            StreamError::NotFound {
                video_id: "vid".into()
            }
        );
        assert_eq!(
            classify_player_error("connection reset", StreamClient::AndroidVr, "vid"),
            StreamError::Network("connection reset".into())
        );
    }

    #[test]
    fn po_token_hint_is_actionable() {
        let err = classify_player_error("403 Forbidden", StreamClient::WebMusic, "vid");
        assert!(err.hint().contains("botguard") || err.hint().contains("android_vr"));
    }

    #[test]
    fn extracts_first_audio_url_from_fixture() {
        let raw = include_str!("../fixtures/player_with_audio.json");
        let value: serde_json::Value = serde_json::from_str(raw).unwrap();
        assert_eq!(
            extract_audio_url(&value).as_deref(),
            Some("https://example.invalid/audio.webm")
        );
    }

    #[test]
    fn ciphered_or_missing_streaming_data_yields_none() {
        let ciphered: serde_json::Value =
            serde_json::from_str(include_str!("../fixtures/player_ciphered.json")).unwrap();
        assert_eq!(extract_audio_url(&ciphered), None);
        let missing: serde_json::Value = serde_json::json!({"playabilityStatus": {"status": "OK"}});
        assert_eq!(extract_audio_url(&missing), None);
        let unplayable: serde_json::Value =
            serde_json::from_str(include_str!("../fixtures/player_unplayable.json")).unwrap();
        assert_eq!(extract_audio_url(&unplayable), None);
        assert_eq!(
            classify_no_url(&unplayable, StreamClient::WebMusic, "vid"),
            StreamError::NotFound {
                video_id: "vid".into()
            }
        );
    }

    #[test]
    fn fallback_chain_records_every_client_before_exhaustion() {
        let mut req = StreamRequest::new("vid");
        let mut seen = Vec::new();
        while let Some(client) = req.next_client() {
            seen.push(client);
        }
        assert_eq!(
            seen,
            vec![
                StreamClient::AndroidVr,
                StreamClient::WebMusic,
                StreamClient::Ios
            ]
        );
        assert_eq!(req.tried.len(), 3);
    }

    #[test]
    fn cookie_file_validation_rejects_empty_without_network() {
        let dir = std::env::temp_dir();
        let path = dir.join("yutu-innertube-empty-cookie.txt");
        std::fs::write(&path, "   ").unwrap();
        let err = CookieAuth::from_file(&path).unwrap_err();
        assert!(err.contains("empty"));
        assert!(!err.contains("   "));
    }

    // ---- Live-network tests: always `#[ignore]`; CI runs offline. ----

    #[tokio::test]
    #[ignore = "live network: anonymous Innertube suggestions"]
    async fn live_anonymous_search_suggestions() {
        let backend = InnertubeBackend::connect_anonymous()
            .await
            .expect("anonymous connect");
        let suggestions = backend
            .search_suggestions("Beatles")
            .await
            .expect("suggestions");
        assert!(!suggestions.is_empty());
    }

    #[tokio::test]
    #[ignore = "live network: player probe fallback chain"]
    async fn live_resolve_stream_records_fallback() {
        let backend = InnertubeBackend::connect_anonymous()
            .await
            .expect("anonymous connect");
        // A well-known video id; outcome depends on network/region, so only
        // the chain bookkeeping is asserted.
        let mut req = StreamRequest::new("dQw4w9WgXcQ");
        let _ = backend.resolve_stream(&mut req).await;
        assert_eq!(req.tried.first(), Some(&StreamClient::AndroidVr));
        assert!(!req.tried.is_empty());
    }
}
