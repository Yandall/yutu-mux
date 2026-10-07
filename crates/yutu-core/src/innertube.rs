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

    /// Resolve with automatic PO-token sidecar fallback.
    ///
    /// First tries token-free [`resolve_stream`](Self::resolve_stream) (cheap,
    /// no sidecar latency). On [`StreamError::PoTokenRequired`] it mints tokens
    /// via the sidecar at `botguard_bin` (blocking `Command`, run in
    /// `spawn_blocking`), retries the `player` endpoint as a raw POST carrying
    /// `serviceIntegrityDimensions.poToken`, and returns the audio URL plus a
    /// session token the caller should append as `&pot=` if the bare URL 403s.
    pub async fn resolve_stream_with_sidecar(
        &self,
        req: &mut StreamRequest,
        botguard_bin: &str,
    ) -> Result<ResolvedStream, StreamError> {
        match self.resolve_stream(req).await {
            Ok(url) => Ok(ResolvedStream {
                url,
                pot_token: None,
            }),
            Err(other @ StreamError::NotFound { .. }) | Err(other @ StreamError::Network(_)) => {
                Err(other)
            }
            Err(StreamError::PoTokenRequired { .. }) => {
                let bin = botguard_bin.to_string();
                let video_id = req.video_id.clone();
                eprintln!("[yutu-stream] PO-gated; minting via sidecar {bin}");
                let minted = tokio::task::spawn_blocking({
                    let video_id = video_id.clone();
                    move || crate::pot::fetch_po_token(&bin, &video_id)
                })
                .await
                .map_err(|e| StreamError::Network(format!("sidecar task: {e}")))?
                .map_err(StreamError::Network)?;
                eprintln!(
                    "[yutu-stream] minted {} token(s), snapshot={}",
                    minted.tokens.len(),
                    minted.tokens.is_empty()
                );
                // Token order (content- vs session-bound) is undocumented, so
                // try each minted token as the player poToken in turn.
                let mut last_err = StreamError::PoTokenRequired {
                    client: "web_music",
                };
                for (i, content_token) in minted.tokens.iter().enumerate() {
                    eprintln!("[yutu-stream] player POST attempt with token #{i}");
                    match raw_player_with_token(&video_id, content_token).await {
                        Err(e) => {
                            eprintln!("[yutu-stream] token #{i} POST failed: {e}");
                            last_err = e;
                        }
                        Ok(raw) => {
                            let value: serde_json::Value =
                                serde_json::from_str(&raw).map_err(|e| {
                                    StreamError::Network(format!("token player JSON: {e}"))
                                })?;
                            eprintln!(
                                "[yutu-stream] token #{i} playability={}",
                                value
                                    .pointer("/playabilityStatus/status")
                                    .and_then(serde_json::Value::as_str)
                                    .unwrap_or("?"),
                            );
                            match extract_audio_url(&value) {
                                Some(url) => {
                                    eprintln!("[yutu-stream] token #{i} yielded URL");
                                    return Ok(ResolvedStream {
                                        url,
                                        pot_token: minted.tokens.get(1).cloned(),
                                    });
                                }
                                None => {
                                    last_err =
                                        classify_no_url(&value, StreamClient::WebMusic, &video_id);
                                    eprintln!("[yutu-stream] token #{i} no URL: {last_err}");
                                }
                            }
                        }
                    }
                }
                Err(last_err)
            }
        }
    }
}

/// Resolved audio URL plus an optional session token for `&pot=` retry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedStream {
    pub url: String,
    pub pot_token: Option<String>,
}

/// Raw Innertube `player` POST mirroring `ytmapi-rs` (WEB_REMIX context, same
/// API key and signature-timestamp scheme) plus `serviceIntegrityDimensions`,
/// which upstream 0.3 cannot express — hence this bypass exists.
async fn raw_player_with_token(video_id: &str, po_token: &str) -> Result<String, StreamError> {
    let body = player_request_body(video_id, Some(po_token));
    let response = reqwest::Client::new()
        .post(PLAYER_URL)
        .header("User-Agent", USER_AGENT)
        .header("X-Origin", "https://music.youtube.com")
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| StreamError::Network(format!("token player request: {e}")))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|e| StreamError::Network(format!("token player body: {e}")))?;
    eprintln!("[yutu-stream] player POST HTTP {status}");
    if !status.is_success() {
        return Err(classify_player_error(
            &format!("{status} {text}"),
            StreamClient::WebMusic,
            video_id,
        ));
    }
    Ok(text)
}

const PLAYER_URL: &str =
    "https://music.youtube.com/youtubei/v1/player?alt=json&prettyPrint=false&key=AIzaSyC9XL3ZjWddXya6X74dJoCTL-WEYFDNX30";
const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:88.0) Gecko/20100101 Firefox/88.0";
/// Dummy WEB_REMIX version, same date-based spirit as upstream's fallback.
const WEB_REMIX_VERSION: &str = "1.20261007.00.00";

/// Build the raw `player` body. `po_token = None` reproduces the plain probe;
/// `Some` attaches `serviceIntegrityDimensions` per the BotGuard contract.
fn player_request_body(video_id: &str, po_token: Option<&str>) -> serde_json::Value {
    let mut body = serde_json::json!({
        "context": {
            "client": {
                "clientName": "WEB_REMIX",
                "clientVersion": WEB_REMIX_VERSION,
                "user": {},
            },
        },
        "videoId": video_id,
        "playbackContext": {
            "contentPlaybackContext": {
                "signatureTimestamp": signature_timestamp_days(),
            },
        },
    });
    if let Some(token) = po_token {
        body["serviceIntegrityDimensions"] = serde_json::json!({ "poToken": token });
    }
    body
}

/// Days since epoch — mirrors upstream's `get_signature_timestamp`.
fn signature_timestamp_days() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() / 86_400)
        .unwrap_or(0)
}

/// Map a `player`-endpoint failure to our error policy: 403-flavoured errors
/// (GVS PO-token enforcement) become [`StreamError::PoTokenRequired`],
/// everything else becomes [`StreamError::Network`].
pub fn classify_player_error(message: &str, client: StreamClient, video_id: &str) -> StreamError {
    let lowered = message.to_lowercase();
    if crate::stream::looks_po_gated(&lowered) {
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
    fn token_request_body_carries_pot_and_context() {
        let body = player_request_body("vid123", Some("TOKEN"));
        assert_eq!(body["videoId"], "vid123");
        assert_eq!(body["serviceIntegrityDimensions"]["poToken"], "TOKEN");
        assert_eq!(body["context"]["client"]["clientName"], "WEB_REMIX");
        let plain = player_request_body("vid123", None);
        assert!(plain.get("serviceIntegrityDimensions").is_none());
    }

    #[test]
    fn signature_timestamp_is_plausible_days() {
        let days = signature_timestamp_days();
        assert!(days > 20_000 && days < 100_000);
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

    #[tokio::test]
    #[ignore = "live network + installed rustypipe-botguard: full token chain"]
    async fn live_sidecar_chain_resolves_url() {
        let Some(bin) = crate::pot::find_botguard_bin() else {
            eprintln!("SKIP: rustypipe-botguard not installed");
            return;
        };
        let backend = InnertubeBackend::connect_anonymous()
            .await
            .expect("anonymous connect");
        let video = std::env::var("YUTU_TEST_VIDEO_ID").unwrap_or_else(|_| "dQw4w9WgXcQ".into());
        let mut req = StreamRequest::new(&video);
        let resolved = backend
            .resolve_stream_with_sidecar(&mut req, &bin)
            .await
            .expect("sidecar chain resolves");
        assert!(resolved.url.starts_with("https://"));
    }
}
