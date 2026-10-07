//! Scrobbling payload builders plus optional network submission.
//!
//! Covers two targets:
//! - Last.fm Audioscrobbler 2.0 (`track.scrobble`): parameter signing via
//!   `api_sig` (md5 of sorted `key+value` concat + shared secret).
//! - ListenBrainz `submit-listens`: JSON body shape (`listen_type: single`).
//!
//! All constructors are pure and always available. Network submission lives
//! behind the `submit` cargo feature (`submit_lastfm_scrobble`,
//! `submit_listenbrainz_listen`) so unit tests never touch the network unless
//! explicitly run as ignored live tests with credentials.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Last.fm Audioscrobbler 2.0 endpoint.
pub const LASTFM_API_URL: &str = "https://ws.audioscrobbler.com/2.0/";

/// ListenBrainz `submit-listens` endpoint.
pub const LISTENBRAINZ_SUBMIT_URL: &str = "https://api.listenbrainz.org/1/submit-listens";

/// One play to scrobble.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scrobble {
    pub artist: String,
    pub track: String,
    pub timestamp_unix: i64,
    pub album: Option<String>,
}

impl Scrobble {
    pub fn new(artist: &str, track: &str, timestamp_unix: i64, album: Option<&str>) -> Self {
        Self {
            artist: artist.to_owned(),
            track: track.to_owned(),
            timestamp_unix,
            album: album.map(str::to_owned),
        }
    }
}

/// Last.fm `api_sig`: md5 hex (lowercase) of the concatenation of every
/// `key + value` in byte-sorted key order, with the shared `secret` appended.
///
/// `params` must NOT already contain `api_sig` or `format`; callers pass the
/// exact map that will be sent (minus the signature itself).
/// `BTreeMap` iteration is already sorted, which is what makes the signature
/// deterministic regardless of insertion order.
pub fn api_sig(params: &BTreeMap<String, String>, secret: &str) -> String {
    let mut buf = String::new();
    for (k, v) in params.iter() {
        buf.push_str(k);
        buf.push_str(v);
    }
    buf.push_str(secret);
    format!("{:x}", md5::compute(buf.as_bytes()))
}

/// Build the signed `track.scrobble` parameter map (ready to POST as
/// `application/x-www-form-urlencoded`).
///
/// Includes `method=track.scrobble`, `api_key`, `sk` (session key), `artist`,
/// `track`, `timestamp`, and `album` when present/non-empty, then computes
/// `api_sig` over all of those and inserts it under the `api_sig` key.
///
/// NOTE: the task sketch lists `(scrobble, api_key, sk)`; `secret` is an
/// extra required argument because the signature cannot be computed without
/// the shared API secret.
pub fn build_scrobble_params(
    scrobble: &Scrobble,
    api_key: &str,
    sk: &str,
    secret: &str,
) -> BTreeMap<String, String> {
    let mut params = BTreeMap::new();
    params.insert("method".to_owned(), "track.scrobble".to_owned());
    params.insert("api_key".to_owned(), api_key.to_owned());
    params.insert("sk".to_owned(), sk.to_owned());
    params.insert("artist".to_owned(), scrobble.artist.clone());
    params.insert("track".to_owned(), scrobble.track.clone());
    params.insert("timestamp".to_owned(), scrobble.timestamp_unix.to_string());
    if let Some(album) = scrobble.album.as_deref() {
        if !album.is_empty() {
            params.insert("album".to_owned(), album.to_owned());
        }
    }
    let sig = api_sig(&params, secret);
    params.insert("api_sig".to_owned(), sig);
    params
}

/// Scrobble eligibility: `played_secs >= min(240, duration_secs / 2)`.
///
/// This mirrors the Last.fm rule (at least half the track, capped at 4
/// minutes). Integer division is used for the halving.
pub fn should_scrobble(duration_secs: u32, played_secs: u32) -> bool {
    let threshold = (duration_secs / 2).min(240);
    played_secs >= threshold
}

/// Build a ListenBrainz `submit-listens` body with `listen_type: single`.
///
/// Shape:
/// ```json
/// {"listen_type":"single","payload":[{"listened_at":123,
///   "track_metadata":{"artist_name":"a","track_name":"t","release_name":"r"}}]}
/// ```
/// `release_name` is omitted when `album` is `None`/empty. An `artist_names`
/// array is also emitted alongside the canonical singular `artist_name` for
/// forward compatibility with newer validators.
pub fn build_listen_payload(
    artist: &str,
    track: &str,
    album: Option<&str>,
    listened_at: i64,
) -> Value {
    let mut metadata = serde_json::Map::new();
    metadata.insert("artist_name".to_owned(), json!(artist));
    metadata.insert("artist_names".to_owned(), json!([artist]));
    metadata.insert("track_name".to_owned(), json!(track));
    if let Some(album) = album {
        if !album.is_empty() {
            metadata.insert("release_name".to_owned(), json!(album));
        }
    }
    json!({
        "listen_type": "single",
        "payload": [
            {
                "listened_at": listened_at,
                "track_metadata": Value::Object(metadata),
            }
        ]
    })
}

/// POST a signed `track.scrobble` map to Last.fm.
///
/// Requires the `submit` cargo feature. Sends `params` as
/// `application/x-www-form-urlencoded` with a 15 s timeout. Returns `Ok(())`
/// on HTTP 2xx unless the response body contains `<error` (Last.fm failure
/// payload). Never logs `params`: it carries the session key (`sk`), which is
/// secret-adjacent.
#[cfg(feature = "submit")]
pub async fn submit_lastfm_scrobble(params: &BTreeMap<String, String>) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("last.fm client build failed: {e}"))?;
    let response = client
        .post(LASTFM_API_URL)
        .form(params)
        .send()
        .await
        .map_err(|e| format!("last.fm request failed: {e}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| format!("last.fm read body failed: {e}"))?;
    if !status.is_success() {
        let snippet: String = body.chars().take(500).collect();
        return Err(format!("last.fm scrobble failed: http {status}: {snippet}"));
    }
    if body.contains("<error") {
        let snippet: String = body.chars().take(500).collect();
        return Err(format!("last.fm scrobble failed: {snippet}"));
    }
    Ok(())
}

/// POST a `submit-listens` payload to ListenBrainz.
///
/// Requires the `submit` cargo feature. Sends `payload` as JSON with
/// `Authorization: Token {token}` and a 15 s timeout. Returns `Ok(())` on
/// HTTP 2xx. The token itself is never included in error messages.
#[cfg(feature = "submit")]
pub async fn submit_listenbrainz_listen(payload: &Value, token: &str) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("listenbrainz client build failed: {e}"))?;
    let response = client
        .post(LISTENBRAINZ_SUBMIT_URL)
        .header("Authorization", format!("Token {token}"))
        .json(payload)
        .send()
        .await
        .map_err(|e| format!("listenbrainz request failed: {e}"))?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        let snippet: String = body.chars().take(500).collect();
        return Err(format!(
            "listenbrainz submit failed: http {status}: {snippet}"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn sig_empty_params_is_md5_of_secret() {
        // md5("secret") — pins the algorithm + hex encoding.
        let sig = api_sig(&BTreeMap::new(), "secret");
        assert_eq!(sig, "5ebe2294ecd0e0f08eab7690d2a6ee69");
    }

    #[test]
    fn sig_is_insertion_order_independent() {
        let a = map(&[("b", "2"), ("a", "1")]);
        let b = map(&[("a", "1"), ("b", "2")]);
        assert_eq!(api_sig(&a, "s"), api_sig(&b, "s"));
    }

    #[test]
    fn sig_changes_when_a_value_changes() {
        let a = map(&[("artist", "Cher"), ("track", "Believe")]);
        let b = map(&[("artist", "Cher"), ("track", "Believe!")]);
        assert_ne!(api_sig(&a, "s"), api_sig(&b, "s"));
    }

    #[test]
    fn sig_keys_sort_bytewise() {
        // "B" (0x42) sorts before "a" (0x61) bytewise.
        let params = map(&[("a", "1"), ("B", "2")]);
        // concat "B2a1" + "s"
        let expect = format!("{:x}", md5::compute("B2a1s".as_bytes()));
        assert_eq!(api_sig(&params, "s"), expect);
    }

    #[test]
    fn scrobble_params_include_signature() {
        let s = Scrobble::new("The Beatles", "Yesterday", 1_700_000_000, Some("Help!"));
        let params = build_scrobble_params(&s, "APIKEY", "SK", "SECRET");
        assert_eq!(params["method"], "track.scrobble");
        assert_eq!(params["artist"], "The Beatles");
        assert_eq!(params["track"], "Yesterday");
        assert_eq!(params["timestamp"], "1700000000");
        assert_eq!(params["album"], "Help!");
        let mut unsigned = params.clone();
        let sig = unsigned.remove("api_sig").unwrap();
        assert_eq!(sig, api_sig(&unsigned, "SECRET"));
    }

    #[test]
    fn scrobble_params_omit_empty_album() {
        let s = Scrobble::new("A", "T", 42, None);
        let params = build_scrobble_params(&s, "K", "SK", "S");
        assert!(!params.contains_key("album"));
        assert!(params.contains_key("api_sig"));
    }

    #[test]
    fn scrobble_rule_half_or_240_cap() {
        // Short track: half of 200 = 100.
        assert!(should_scrobble(200, 100));
        assert!(!should_scrobble(200, 99));
        // Long track: min(240, 300) = 240.
        assert!(should_scrobble(600, 240));
        assert!(!should_scrobble(600, 239));
        // Odd duration truncates: 125 / 2 = 62.
        assert!(should_scrobble(125, 62));
        assert!(!should_scrobble(125, 61));
    }

    #[test]
    fn listen_payload_shape() {
        let v = build_listen_payload("The Beatles", "Yesterday", Some("Help!"), 123);
        assert_eq!(v["listen_type"], json!("single"));
        let p = &v["payload"][0];
        assert_eq!(p["listened_at"], json!(123));
        assert_eq!(p["track_metadata"]["track_name"], json!("Yesterday"));
        assert_eq!(p["track_metadata"]["artist_name"], json!("The Beatles"));
        assert_eq!(p["track_metadata"]["release_name"], json!("Help!"));
    }

    #[test]
    fn listen_payload_omits_empty_release() {
        let v = build_listen_payload("A", "T", None, 7);
        assert!(v["payload"][0]["track_metadata"]
            .get("release_name")
            .is_none());
        // artist_names array present for newer validators.
        assert_eq!(
            v["payload"][0]["track_metadata"]["artist_names"],
            json!(["A"])
        );
    }

    #[test]
    fn scrobble_endpoint_urls() {
        assert_eq!(LASTFM_API_URL, "https://ws.audioscrobbler.com/2.0/");
        assert_eq!(
            LISTENBRAINZ_SUBMIT_URL,
            "https://api.listenbrainz.org/1/submit-listens"
        );
        assert!(LASTFM_API_URL.starts_with("https://"));
        assert!(LISTENBRAINZ_SUBMIT_URL.starts_with("https://"));
    }

    #[test]
    fn built_params_still_sign_correctly() {
        let s = Scrobble::new("A", "T", 1, Some("R"));
        let params = build_scrobble_params(&s, "K", "SK", "S");
        let mut unsigned = params.clone();
        let sig = unsigned.remove("api_sig").expect("api_sig present");
        assert_eq!(sig, api_sig(&unsigned, "S"));
        // Same inputs sign identically; a different secret signs differently.
        let again = build_scrobble_params(&s, "K", "SK", "S");
        assert_eq!(again["api_sig"], sig);
        let other = build_scrobble_params(&s, "K", "SK", "OTHER");
        assert_ne!(other["api_sig"], sig);
    }

    #[cfg(feature = "submit")]
    #[tokio::test]
    #[ignore]
    async fn live_lastfm_scrobble_submits() {
        let api_key = std::env::var("LASTFM_TEST_API_KEY").unwrap_or_default();
        if api_key.is_empty() {
            println!("SKIP: LASTFM_TEST_API_KEY not set");
            return;
        }
        let sk = std::env::var("LASTFM_TEST_SK").unwrap_or_default();
        if sk.is_empty() {
            println!("SKIP: LASTFM_TEST_SK not set");
            return;
        }
        let secret = std::env::var("LASTFM_TEST_SECRET").unwrap_or_default();
        if secret.is_empty() {
            println!("SKIP: LASTFM_TEST_SECRET not set");
            return;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(1_700_000_000);
        let scrobble = Scrobble::new(
            "yutu-mux test artist",
            "yutu-mux test track",
            now - 60,
            None,
        );
        let params = build_scrobble_params(&scrobble, &api_key, &sk, &secret);
        if let Err(e) = submit_lastfm_scrobble(&params).await {
            panic!("live Last.fm scrobble failed: {e}");
        }
    }

    #[cfg(feature = "submit")]
    #[tokio::test]
    #[ignore]
    async fn live_listenbrainz_submit_sends_single() {
        let token = std::env::var("LISTENBRAINZ_TEST_TOKEN").unwrap_or_default();
        if token.is_empty() {
            println!("SKIP: LISTENBRAINZ_TEST_TOKEN not set");
            return;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(1_700_000_000);
        let payload = build_listen_payload(
            "yutu-mux test artist",
            "yutu-mux test track",
            None,
            now - 60,
        );
        if let Err(e) = submit_listenbrainz_listen(&payload, &token).await {
            panic!("live ListenBrainz submit failed: {e}");
        }
    }
}
