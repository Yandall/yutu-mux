//! Live Innertube backend built on the real `ytmapi-rs` 0.3 client.
//!
//! Feature-gated behind `innertube` so offline builds (and CI without network)
//! keep working with [`crate::backend::MockBackend`].
//!
//! What is live vs stubbed (see `docs/INNERTUBE.md` for the full picture):
//! - LIVE: anonymous + cookie-file + Google-OAuth-device-flow connection,
//!   `search_suggestions`, filtered `search` queries
//!   ([`InnertubeBackend::search_tracks`],
//!   [`InnertubeBackend::search_albums`], [`InnertubeBackend::search_artists`]
//!   via `YtMusic::search_songs` / `search_albums` / `search_artists`),
//!   login-gated library queries ([`InnertubeBackend::library_songs`],
//!   [`InnertubeBackend::liked_songs`] via the `LM` playlist,
//!   [`InnertubeBackend::library_albums`]), radio recommendations
//!   ([`InnertubeBackend::recommendations_for`] — there is no home query in
//!   `ytmapi-rs` 0.3, so no `home_feed`), YTM lyrics
//!   ([`InnertubeBackend::song_lyrics`]), and per-video `player`-endpoint
//!   probes (the same endpoint real stream resolution uses) via
//!   [`InnertubeBackend::resolve_stream`].
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
use ytmapi_rs::auth::oauth::OAuthToken;
use ytmapi_rs::auth::BrowserToken;
use ytmapi_rs::common::{PlaylistID, Thumbnail, VideoID, YoutubeID};
use ytmapi_rs::parse::{
    PlaylistItem, SearchResultAlbum, SearchResultArtist, SearchResultSong, TableListSong,
    WatchPlaylistTrack,
};
use ytmapi_rs::query::GetSongTrackingUrlQuery;
use ytmapi_rs::YtMusic;

use crate::auth::{CookieAuth, OAuthPersist};
use crate::models::{Album, Artist, Track};
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
    OAuth(YtMusic<OAuthToken>),
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

    /// Connect with a Google OAuth token bundle (see
    /// [`crate::auth::poll_google_login`]).
    ///
    /// Synchronous: `YtMusic::from_auth_token` performs no network I/O (like
    /// the cookie path, credentials are only exercised on first query). The
    /// token material is never logged.
    pub fn connect_with_oauth(persist: &OAuthPersist) -> Result<Self, String> {
        Ok(Self {
            inner: Inner::OAuth(YtMusic::from_auth_token(persist.to_token())),
        })
    }

    /// Refresh the OAuth access token in place and return the fresh bundle.
    ///
    /// `ytmapi-rs` rejects queries made with an expiring token client-side,
    /// so callers holding a long-lived [`OAuthPersist`] should call this when
    /// library/lyrics queries start failing, then re-save the result with
    /// [`OAuthPersist::save_to_file`]. Errors on non-OAuth backends.
    pub async fn refresh_oauth(&mut self) -> Result<OAuthPersist, String> {
        match &mut self.inner {
            Inner::OAuth(client) => client
                .refresh_token()
                .await
                .map(OAuthPersist::from_token)
                .map_err(|e| format!("oauth refresh failed: {e}")),
            _ => Err("refresh_oauth needs a Google-OAuth backend (see connect_with_oauth)".into()),
        }
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
            Inner::OAuth(client) => client
                .get_search_suggestions(q)
                .await
                .map_err(|e| format!("search suggestions failed: {e}"))?,
        };
        Ok(suggestions.iter().map(|s| s.get_text()).collect())
    }

    /// Live song search: filtered `search` endpoint returning song hits mapped
    /// to [`Track`] (`id` = `videoId`, `artist` = the API's joined artist
    /// string, `album` = song album name or `None`, unparseable duration → 0).
    ///
    /// Works on both variants: filtered search is `impl<A: AuthToken>` in
    /// `ytmapi-rs` 0.3 (unlike login-gated library queries), so anonymous
    /// clients get full results, not just suggestions.
    pub async fn search_tracks(&self, query: &str) -> Result<Vec<Track>, String> {
        let songs = match &self.inner {
            Inner::Anonymous(client) => client
                .search_songs(query)
                .await
                .map_err(|e| format!("search songs failed: {e}"))?,
            Inner::Cookie(client) => client
                .search_songs(query)
                .await
                .map_err(|e| format!("search songs failed: {e}"))?,
            Inner::OAuth(client) => client
                .search_songs(query)
                .await
                .map_err(|e| format!("search songs failed: {e}"))?,
        };
        Ok(songs.iter().map(song_to_track).collect())
    }

    /// Live album search, same endpoint family as [`Self::search_tracks`].
    /// `year` is parsed from the API's year string; garbage → `None`.
    pub async fn search_albums(&self, query: &str) -> Result<Vec<Album>, String> {
        let albums = match &self.inner {
            Inner::Anonymous(client) => client
                .search_albums(query)
                .await
                .map_err(|e| format!("search albums failed: {e}"))?,
            Inner::Cookie(client) => client
                .search_albums(query)
                .await
                .map_err(|e| format!("search albums failed: {e}"))?,
            Inner::OAuth(client) => client
                .search_albums(query)
                .await
                .map_err(|e| format!("search albums failed: {e}"))?,
        };
        Ok(albums.iter().map(album_to_album).collect())
    }

    /// Live artist search, same endpoint family as [`Self::search_tracks`].
    pub async fn search_artists(&self, query: &str) -> Result<Vec<Artist>, String> {
        let artists = match &self.inner {
            Inner::Anonymous(client) => client
                .search_artists(query)
                .await
                .map_err(|e| format!("search artists failed: {e}"))?,
            Inner::Cookie(client) => client
                .search_artists(query)
                .await
                .map_err(|e| format!("search artists failed: {e}"))?,
            Inner::OAuth(client) => client
                .search_artists(query)
                .await
                .map_err(|e| format!("search artists failed: {e}"))?,
        };
        Ok(artists.iter().map(artist_to_artist).collect())
    }

    /// All songs in the user's library (`FEmusic_liked_videos` browse).
    ///
    /// Auth: cookie OR Google OAuth (`GetLibrarySongsQuery` is
    /// `impl<A: LoggedIn>` upstream, and only those two tokens are
    /// `LoggedIn`). Anonymous backends get a local error, no network.
    pub async fn library_songs(&self) -> Result<Vec<Track>, String> {
        let songs: Vec<TableListSong> = match &self.inner {
            Inner::Anonymous(_) => {
                return Err("library_songs requires login (cookie or Google OAuth)".into());
            }
            Inner::Cookie(client) => client
                .get_library_songs()
                .await
                .map_err(|e| format!("library songs failed: {e}"))?,
            Inner::OAuth(client) => client
                .get_library_songs()
                .await
                .map_err(|e| format!("library songs failed: {e}"))?,
        };
        Ok(songs.iter().map(table_song_to_track).collect())
    }

    /// The user's liked-music tracks, via the `LM` liked playlist
    /// (`get_playlist_tracks` — `ytmapi-rs` 0.3 exposes no dedicated
    /// liked-songs query).
    ///
    /// Auth: cookie OR Google OAuth in practice — the query itself is
    /// `impl<A: AuthToken>` so anonymous attempts hit the server and fail
    /// there. Song and video items map to [`Track`]; podcast episodes are
    /// skipped (not music).
    pub async fn liked_songs(&self) -> Result<Vec<Track>, String> {
        let items = match &self.inner {
            Inner::Anonymous(client) => client
                .get_playlist_tracks(PlaylistID::from_raw("LM"))
                .await
                .map_err(|e| format!("liked songs failed: {e}"))?,
            Inner::Cookie(client) => client
                .get_playlist_tracks(PlaylistID::from_raw("LM"))
                .await
                .map_err(|e| format!("liked songs failed: {e}"))?,
            Inner::OAuth(client) => client
                .get_playlist_tracks(PlaylistID::from_raw("LM"))
                .await
                .map_err(|e| format!("liked songs failed: {e}"))?,
        };
        Ok(items.iter().filter_map(playlist_item_to_track).collect())
    }

    /// All albums in the user's library.
    ///
    /// Auth: cookie OR Google OAuth (same `LoggedIn` bound as
    /// [`Self::library_songs`); anonymous backends get a local error.
    /// Mapping reuses [`album_to_album`].
    pub async fn library_albums(&self) -> Result<Vec<Album>, String> {
        let albums = match &self.inner {
            Inner::Anonymous(_) => {
                return Err("library_albums requires login (cookie or Google OAuth)".into());
            }
            Inner::Cookie(client) => client
                .get_library_albums()
                .await
                .map_err(|e| format!("library albums failed: {e}"))?,
            Inner::OAuth(client) => client
                .get_library_albums()
                .await
                .map_err(|e| format!("library albums failed: {e}"))?,
        };
        Ok(albums.iter().map(album_to_album).collect())
    }

    /// Radio/autoplay recommendations seeded by `video_id` (watch playlist).
    ///
    /// There is deliberately no `home_feed`: `ytmapi-rs` 0.3 exposes no home
    /// query (only taste-profile and moods/genre queries under
    /// `FEmusic_home`-adjacent browse ids, none returning a track feed), so
    /// recommendations come from `get_watch_playlist_from_video_id` instead.
    ///
    /// Auth: works on ALL backends including anonymous
    /// (`GetWatchPlaylistQuery` is `impl<A: AuthToken>` upstream).
    pub async fn recommendations_for(&self, video_id: &str) -> Result<Vec<Track>, String> {
        let tracks = match &self.inner {
            Inner::Anonymous(client) => client
                .get_watch_playlist_from_video_id(VideoID::from_raw(video_id))
                .await
                .map_err(|e| format!("recommendations failed: {e}"))?,
            Inner::Cookie(client) => client
                .get_watch_playlist_from_video_id(VideoID::from_raw(video_id))
                .await
                .map_err(|e| format!("recommendations failed: {e}"))?,
            Inner::OAuth(client) => client
                .get_watch_playlist_from_video_id(VideoID::from_raw(video_id))
                .await
                .map_err(|e| format!("recommendations failed: {e}"))?,
        };
        Ok(tracks.iter().map(watch_track_to_track).collect())
    }

    /// YouTube Music lyrics for `video_id` as plain text lines (no timing —
    /// timing is LRCLIB's job).
    ///
    /// Two-step upstream flow: `get_lyrics_id` (watch `next` endpoint) then
    /// `get_lyrics` (browse the lyrics id). Both queries are
    /// `impl<A: AuthToken>`, so this is attempted on all backends; in
    /// practice lyric availability is per-track server-side (many tracks have
    /// none — that surfaces as an API error here).
    pub async fn song_lyrics(&self, video_id: &str) -> Result<Vec<String>, String> {
        let lyrics_id = match &self.inner {
            Inner::Anonymous(client) => client
                .get_lyrics_id(VideoID::from_raw(video_id))
                .await
                .map_err(|e| format!("lyrics id failed: {e}"))?,
            Inner::Cookie(client) => client
                .get_lyrics_id(VideoID::from_raw(video_id))
                .await
                .map_err(|e| format!("lyrics id failed: {e}"))?,
            Inner::OAuth(client) => client
                .get_lyrics_id(VideoID::from_raw(video_id))
                .await
                .map_err(|e| format!("lyrics id failed: {e}"))?,
        };
        let lyrics = match &self.inner {
            Inner::Anonymous(client) => client
                .get_lyrics(lyrics_id)
                .await
                .map_err(|e| format!("lyrics failed: {e}"))?,
            Inner::Cookie(client) => client
                .get_lyrics(lyrics_id)
                .await
                .map_err(|e| format!("lyrics failed: {e}"))?,
            Inner::OAuth(client) => client
                .get_lyrics(lyrics_id)
                .await
                .map_err(|e| format!("lyrics failed: {e}"))?,
        };
        Ok(lyrics.lyrics.lines().map(str::to_string).collect())
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
            Inner::OAuth(backend) => {
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

/// Map one `search_songs` hit to our [`Track`].
///
/// The API gives a single joined `artist` string, an optional album (some
/// songs have none — see youtui#174), and a `"m:ss"`/`"h:mm:ss"` duration
/// string; unparseable durations become `0`.
/// Smallest artwork thumbnail (keeps row art light), if any.
fn pick_thumb_url(thumbs: &[Thumbnail]) -> Option<String> {
    thumbs
        .iter()
        .min_by_key(|t| t.width.max(1))
        .map(|t| t.url.clone())
        .filter(|u| !u.is_empty())
}

fn song_to_track(song: &SearchResultSong) -> Track {
    Track {
        id: song.video_id.get_raw().to_string(),
        title: song.title.clone(),
        artist: song.artist.clone(),
        album: song.album.as_ref().map(|a| a.name.clone()),
        duration_secs: parse_duration_secs(&song.duration),
        thumbnail_url: pick_thumb_url(&song.thumbnails),
    }
}

/// Map one `search_albums` hit to our [`Album`]. The API year is a plain
/// string (`"1969"`); anything that is not a bare `u16` becomes `None`.
fn album_to_album(album: &SearchResultAlbum) -> Album {
    Album {
        id: album.album_id.get_raw().to_string(),
        title: album.title.clone(),
        artist: album.artist.clone(),
        year: album.year.trim().parse().ok(),
        thumbnail_url: pick_thumb_url(&album.thumbnails),
    }
}

/// Map one `search_artists` hit to our [`Artist`] (`id` = channel `browseId`).
fn artist_to_artist(artist: &SearchResultArtist) -> Artist {
    Artist {
        id: artist.browse_id.get_raw().to_string(),
        name: artist.artist.clone(),
        thumbnail_url: pick_thumb_url(&artist.thumbnails),
    }
}

/// Map one library/playlist song row to our [`Track`]: artists joined with
/// `", "` (the search path gets a pre-joined string from the API), album
/// always present in this row type, duration parsed like search hits.
fn table_song_to_track(song: &TableListSong) -> Track {
    Track {
        id: song.video_id.get_raw().to_string(),
        title: song.title.clone(),
        artist: join_names(song.artists.iter().map(|a| a.name.as_str())),
        album: Some(song.album.name.clone()).filter(|name| !name.is_empty()),
        duration_secs: parse_duration_secs(&song.duration),
        thumbnail_url: pick_thumb_url(&song.thumbnails),
    }
}

/// Map one liked-playlist item to our [`Track`]. Songs and uploaded songs
/// carry artists + album; plain videos carry a channel name and no album;
/// podcast episodes are not music → `None` (skipped by callers).
fn playlist_item_to_track(item: &PlaylistItem) -> Option<Track> {
    match item {
        PlaylistItem::Song(song) => Some(Track {
            id: song.video_id.get_raw().to_string(),
            title: song.title.clone(),
            artist: join_names(song.artists.iter().map(|a| a.name.as_str())),
            album: Some(song.album.name.clone()).filter(|name| !name.is_empty()),
            duration_secs: parse_duration_secs(&song.duration),
            thumbnail_url: pick_thumb_url(&song.thumbnails),
        }),
        PlaylistItem::Video(video) => Some(Track {
            id: video.video_id.get_raw().to_string(),
            title: video.title.clone(),
            artist: video.channel_name.clone(),
            album: None,
            duration_secs: parse_duration_secs(&video.duration),
            thumbnail_url: pick_thumb_url(&video.thumbnails),
        }),
        PlaylistItem::UploadSong(song) => Some(Track {
            id: song.video_id.get_raw().to_string(),
            title: song.title.clone(),
            artist: join_names(song.artists.iter().map(|a| a.name.as_str())),
            album: song
                .album
                .as_ref()
                .map(|a| a.name.clone())
                .filter(|name| !name.is_empty()),
            duration_secs: parse_duration_secs(&song.duration),
            thumbnail_url: None,
        }),
        PlaylistItem::Episode(_) => None,
    }
}

/// Map one watch-playlist (radio) row to our [`Track`]: no album id survives
/// this endpoint, only author + duration text.
fn watch_track_to_track(track: &WatchPlaylistTrack) -> Track {
    Track {
        id: track.video_id.get_raw().to_string(),
        title: track.title.clone(),
        artist: track.author.clone(),
        album: None,
        duration_secs: parse_duration_secs(&track.duration),
        thumbnail_url: pick_thumb_url(&track.thumbnails),
    }
}

/// Join artist names the way the search API pre-joins them (`, `).
fn join_names<'a>(names: impl Iterator<Item = &'a str>) -> String {
    names.collect::<Vec<_>>().join(", ")
}

/// Parse `"m:ss"` / `"h:mm:ss"` (also bare seconds) to seconds.
/// Empty or malformed input → `0` (never fails: it feeds `duration_secs`).
fn parse_duration_secs(raw: &str) -> u32 {
    let raw = raw.trim();
    if raw.is_empty() {
        return 0;
    }
    let mut total: u64 = 0;
    for part in raw.split(':') {
        let n: u64 = match part.trim().parse() {
            Ok(n) => n,
            Err(_) => return 0,
        };
        total = total.saturating_mul(60).saturating_add(n);
    }
    total.min(u64::from(u32::MAX)) as u32
}

/// Fixture/test helper: parse a saved `search_songs` response (a JSON array
/// of `SearchResultSong`, see `fixtures/search_tracks.json`) into our
/// [`Track`]s using the same [`song_to_track`] mapping as the live
/// [`InnertubeBackend::search_tracks`] path. Unparseable JSON → empty vec.
pub fn parse_search_tracks_fixture(json: &str) -> Vec<Track> {
    serde_json::from_str::<Vec<SearchResultSong>>(json)
        .map(|songs| songs.iter().map(song_to_track).collect())
        .unwrap_or_default()
}

/// Fixture/test helper: parse a saved `get_library_songs` response (a JSON
/// array of `TableListSong`, see `fixtures/library_songs.json`) with the
/// same [`table_song_to_track`] mapping as [`InnertubeBackend::library_songs`].
/// Unparseable JSON → empty vec.
pub fn parse_library_songs_fixture(json: &str) -> Vec<Track> {
    serde_json::from_str::<Vec<TableListSong>>(json)
        .map(|songs| songs.iter().map(table_song_to_track).collect())
        .unwrap_or_default()
}

/// Fixture/test helper: parse a saved `get_watch_playlist_from_video_id`
/// response (a JSON array of `WatchPlaylistTrack`, see
/// `fixtures/watch_playlist.json`) with the same [`watch_track_to_track`]
/// mapping as [`InnertubeBackend::recommendations_for`].
/// Unparseable JSON → empty vec.
pub fn parse_watch_playlist_fixture(json: &str) -> Vec<Track> {
    serde_json::from_str::<Vec<WatchPlaylistTrack>>(json)
        .map(|tracks| tracks.iter().map(watch_track_to_track).collect())
        .unwrap_or_default()
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

    // ---- Library / recommendations / lyrics mapping (offline) ----

    #[test]
    fn library_songs_fixture_parses_to_tracks() {
        let tracks = parse_library_songs_fixture(include_str!("../fixtures/library_songs.json"));
        assert_eq!(tracks.len(), 2);
        assert_eq!(
            tracks[0],
            Track {
                id: "isiofhN6_vc".into(),
                title: "A Day In The Life".into(),
                artist: "The Beatles".into(),
                album: Some("The Beatles 1967 - 1970 (Remastered 2009)".into()),
                duration_secs: 308,
                thumbnail_url: None,
            }
        );
        // Multiple artists join the way the search API pre-joins them.
        assert_eq!(tracks[1].artist, "The Beatles, George Martin");
        assert_eq!(tracks[1].duration_secs, 181);
    }

    #[test]
    fn library_songs_fixture_rejects_garbage() {
        assert!(parse_library_songs_fixture("not json").is_empty());
    }

    #[test]
    fn watch_playlist_fixture_parses_to_tracks() {
        let tracks = parse_watch_playlist_fixture(include_str!("../fixtures/watch_playlist.json"));
        assert_eq!(tracks.len(), 2);
        assert_eq!(
            tracks[0],
            Track {
                id: "45cYwDMibGo".into(),
                title: "Come Together (Remastered 2009)".into(),
                artist: "The Beatles".into(),
                album: None,
                duration_secs: 260,
                thumbnail_url: None,
            }
        );
        assert_eq!(tracks[1].duration_secs, 186);
    }

    #[test]
    fn playlist_items_map_songs_videos_and_skip_episodes() {
        let song: PlaylistItem = serde_json::from_value(serde_json::json!({
            "Song": {
                "video_id": "vid1",
                "track_no": 1,
                "album": {"name": "Album One", "id": "MPRE1"},
                "duration": "2:30",
                "title": "Song One",
                "artists": [{"name": "Somebody"}],
                "like_status": "LIKE",
                "thumbnails": [],
                "explicit": "NotExplicit",
                "is_available": true,
                "playlist_id": "LM"
            }
        }))
        .unwrap();
        let video: PlaylistItem = serde_json::from_value(serde_json::json!({
            "Video": {
                "video_id": "vid2",
                "track_no": 2,
                "duration": "10:00",
                "title": "Video Two",
                "channel_name": "A Channel",
                "channel_id": "UCxyz",
                "like_status": "INDIFFERENT",
                "thumbnails": [],
                "is_available": true,
                "playlist_id": "LM"
            }
        }))
        .unwrap();
        let episode: PlaylistItem = serde_json::from_value(serde_json::json!({
            "Episode": {
                "episode_id": "ep1",
                "track_no": 3,
                "date": "Live",
                "duration": "Live",
                "title": "Episode Three",
                "podcast_name": "A Podcast",
                "podcast_id": "PLpod",
                "like_status": "INDIFFERENT",
                "thumbnails": [],
                "is_available": true
            }
        }))
        .unwrap();
        let tracks: Vec<Track> = [song, video, episode]
            .iter()
            .filter_map(playlist_item_to_track)
            .collect();
        assert_eq!(
            tracks.len(),
            2,
            "episodes are not music and must be skipped"
        );
        assert_eq!(tracks[0].artist, "Somebody");
        assert_eq!(tracks[0].album.as_deref(), Some("Album One"));
        assert_eq!(tracks[1].artist, "A Channel");
        assert_eq!(tracks[1].album, None);
    }

    #[test]
    fn lyrics_response_splits_into_plain_lines() {
        // `Lyrics` is `#[non_exhaustive]`, so build it via serde.
        let lyrics: ytmapi_rs::parse::Lyrics = serde_json::from_value(serde_json::json!({
            "lyrics": "line one\nline two\n\nline four",
            "source": "LyricFind",
        }))
        .unwrap();
        assert_eq!(lyrics.source, "LyricFind");
        let lines: Vec<String> = lyrics.lyrics.lines().map(str::to_string).collect();
        assert_eq!(lines, vec!["line one", "line two", "", "line four"]);
    }

    // ---- OAuth persist (offline: fabricated token JSON, no network) ----

    /// Minimal `OAuthToken` JSON mirroring its serde shape (private fields are
    /// still deserializable from outside the crate).
    fn fabricated_oauth_json() -> String {
        serde_json::json!({
            "token_type": "Bearer",
            "access_token": "ya29.test-access-TOKEN",
            "refresh_token": "1//test-refresh-SECRET",
            "expires_in": 3599,
            "request_time": {"secs_since_epoch": 1790000000, "nanos_since_epoch": 0},
            "client_id": "test-client-id",
            "client_secret": "test-client-SECRET"
        })
        .to_string()
    }

    #[test]
    fn oauth_persist_round_trips_through_file() {
        use crate::auth::OAuthPersist;
        use ytmapi_rs::auth::oauth::OAuthToken;

        let token: OAuthToken =
            serde_json::from_str(&fabricated_oauth_json()).expect("fabricated token parses");
        let persist = OAuthPersist::from_token(token);
        let dir = std::env::temp_dir();
        let path = dir.join("yutu-test-oauth-persist.json");
        persist.save_to_file(&path).expect("save oauth persist");
        let loaded = OAuthPersist::load_from_file(&path).expect("load oauth persist");
        // Same token material survives the round trip (compared via re-serialised form).
        let before = serde_json::to_string(persist.token()).unwrap();
        let after = serde_json::to_string(loaded.token()).unwrap();
        assert_eq!(before, after);
        let _ = std::fs::remove_file(&path);
    }

    #[cfg(unix)]
    #[test]
    fn oauth_persist_file_is_owner_only() {
        use crate::auth::OAuthPersist;
        use std::os::unix::fs::PermissionsExt;
        use ytmapi_rs::auth::oauth::OAuthToken;

        let token: OAuthToken =
            serde_json::from_str(&fabricated_oauth_json()).expect("fabricated token parses");
        let dir = std::env::temp_dir();
        let path = dir.join("yutu-test-oauth-perms.json");
        // Pre-create with loose perms: save must tighten them.
        std::fs::write(&path, "{}").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        OAuthPersist::from_token(token)
            .save_to_file(&path)
            .expect("save oauth persist");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "oauth file must be owner-only, got {mode:o}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn oauth_persist_never_leaks_in_debug_or_errors() {
        use crate::auth::OAuthPersist;
        use ytmapi_rs::auth::oauth::OAuthToken;

        let token: OAuthToken =
            serde_json::from_str(&fabricated_oauth_json()).expect("fabricated token parses");
        let debug = format!("{:?}", OAuthPersist::from_token(token));
        assert!(
            !debug.contains("ya29.test-access-TOKEN"),
            "access token leaked: {debug}"
        );
        assert!(
            !debug.contains("test-refresh-SECRET"),
            "refresh token leaked: {debug}"
        );
        assert!(
            !debug.contains("test-client-SECRET"),
            "client secret leaked: {debug}"
        );

        // Parse errors name the path but never echo file contents.
        let dir = std::env::temp_dir();
        let path = dir.join("yutu-test-oauth-garbage.json");
        std::fs::write(&path, "super-secret-garbage-TOKEN").unwrap();
        let err = OAuthPersist::load_from_file(&path).unwrap_err();
        assert!(
            err.contains("yutu-test-oauth-garbage.json"),
            "error names path: {err}"
        );
        assert!(
            !err.contains("super-secret-garbage-TOKEN"),
            "content leaked: {err}"
        );
        let missing = dir.join("yutu-test-oauth-missing.json");
        let _ = std::fs::remove_file(&missing);
        assert!(OAuthPersist::load_from_file(&missing).is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn oauth_connect_builds_backend_without_network() {
        use crate::auth::OAuthPersist;
        use ytmapi_rs::auth::oauth::OAuthToken;

        let token: OAuthToken =
            serde_json::from_str(&fabricated_oauth_json()).expect("fabricated token parses");
        let backend =
            InnertubeBackend::connect_with_oauth(&OAuthPersist::from_token(token)).unwrap();
        assert!(matches!(backend.inner, Inner::OAuth(_)));
    }

    #[tokio::test]
    #[ignore = "live network: oauth refresh rejects non-OAuth backend (needs anonymous connect)"]
    async fn oauth_refresh_rejects_non_oauth_backend() {
        let mut backend = InnertubeBackend::connect_anonymous()
            .await
            .expect("anonymous connect");
        let err = backend.refresh_oauth().await.unwrap_err();
        assert!(err.contains("OAuth"), "unexpected error: {err}");
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
    #[ignore = "live network: anonymous song search"]
    async fn live_search_tracks() {
        let backend = InnertubeBackend::connect_anonymous()
            .await
            .expect("anonymous connect");
        let tracks = backend
            .search_tracks("Beatles")
            .await
            .expect("search tracks");
        assert!(
            !tracks.is_empty(),
            "expected non-empty track results for 'Beatles', got none"
        );
        assert!(
            tracks
                .iter()
                .all(|t| !t.id.is_empty() && !t.title.is_empty()),
            "tracks must carry id and title: {tracks:?}"
        );
    }

    #[tokio::test]
    #[ignore = "live network: anonymous album search"]
    async fn live_search_albums() {
        let backend = InnertubeBackend::connect_anonymous()
            .await
            .expect("anonymous connect");
        let albums = backend
            .search_albums("Beatles")
            .await
            .expect("search albums");
        assert!(
            !albums.is_empty(),
            "expected non-empty album results for 'Beatles', got none"
        );
    }

    #[tokio::test]
    #[ignore = "live network: anonymous artist search"]
    async fn live_search_artists() {
        let backend = InnertubeBackend::connect_anonymous()
            .await
            .expect("anonymous connect");
        let artists = backend
            .search_artists("Beatles")
            .await
            .expect("search artists");
        assert!(
            !artists.is_empty(),
            "expected non-empty artist results for 'Beatles', got none"
        );
    }

    #[test]
    fn search_tracks_fixture_parses_to_tracks() {
        let tracks = parse_search_tracks_fixture(include_str!("../fixtures/search_tracks.json"));
        assert!(!tracks.is_empty(), "fixture should hold at least one track");
        for track in &tracks {
            assert!(!track.id.is_empty(), "track id empty: {track:?}");
            assert!(!track.title.is_empty(), "track title empty: {track:?}");
            assert!(!track.artist.is_empty(), "track artist empty: {track:?}");
        }
        // The real fixture carries artwork — mapping must keep it.
        assert!(
            tracks.iter().any(|t| t
                .thumbnail_url
                .as_deref()
                .is_some_and(|u| u.starts_with("https://"))),
            "expected at least one thumbnail url"
        );
    }

    #[test]
    fn search_track_mapping_handles_missing_album_and_bad_duration() {
        // `SearchResultSong` is `#[non_exhaustive]`, so build it via serde
        // rather than a struct literal.
        let song: SearchResultSong = serde_json::from_value(serde_json::json!({
            "title": "Song",
            "artist": "Someone",
            "album": null,
            "duration": "not a time",
            "plays": "0",
            "explicit": "NotExplicit",
            "video_id": "vid123",
            "thumbnails": []
        }))
        .unwrap();
        let track = song_to_track(&song);
        assert_eq!(
            track,
            Track {
                id: "vid123".into(),
                title: "Song".into(),
                artist: "Someone".into(),
                album: None,
                duration_secs: 0,
                thumbnail_url: None,
            }
        );
    }

    #[test]
    fn duration_parsing_covers_shapes() {
        assert_eq!(parse_duration_secs("3:45"), 225);
        assert_eq!(parse_duration_secs("1:02:03"), 3723);
        assert_eq!(parse_duration_secs("45"), 45);
        assert_eq!(parse_duration_secs(""), 0);
        assert_eq!(parse_duration_secs("3:xx"), 0);
    }

    #[test]
    fn album_year_parsing_rejects_garbage() {
        // `SearchResultAlbum` is `#[non_exhaustive]`; build via serde.
        let album: SearchResultAlbum = serde_json::from_value(serde_json::json!({
            "title": "Abbey Road",
            "artist": "The Beatles",
            "year": "1969",
            "explicit": "NotExplicit",
            "album_id": "MPREabc",
            "album_type": "Album",
            "thumbnails": []
        }))
        .unwrap();
        let mapped = album_to_album(&album);
        assert_eq!(mapped.year, Some(1969));
        assert_eq!(mapped.id, "MPREabc");
        let mut bad = serde_json::to_value(&album).unwrap();
        bad["year"] = serde_json::json!("unknown");
        let bad: SearchResultAlbum = serde_json::from_value(bad).unwrap();
        assert_eq!(album_to_album(&bad).year, None);
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

    // NOTE (Oct 2026): the raw sidecar chain below cannot resolve ciphered
    // formats without a deobfuscator, so no live test asserts full resolution
    // here. Real end-to-end proof lives in
    // `crates/yutu-player/tests/e2e_audio.rs` (yt-dlp resolve -> fetch ->
    // decode, ignored without binary+network).
    #[tokio::test]
    #[ignore = "live network: sidecar mint still yields tokens (resolution itself is yt-dlp's job now)"]
    async fn live_sidecar_still_mints_tokens() {
        let Some(bin) = crate::pot::find_botguard_bin() else {
            eprintln!("SKIP: rustypipe-botguard not installed");
            return;
        };
        let video = std::env::var("YUTU_TEST_VIDEO_ID").unwrap_or_else(|_| "dQw4w9WgXcQ".into());
        let minted =
            crate::pot::fetch_po_token(&bin, &video).expect("sidecar mints without crashing");
        assert!(
            !minted.tokens.is_empty(),
            "sidecar returned zero tokens for {video}"
        );
    }

    #[tokio::test]
    #[ignore = "live network: OAuth library songs"]
    async fn live_oauth_library_songs() {
        use crate::auth::OAuthPersist;

        let Ok(path) = std::env::var("YUTU_TEST_OAUTH_FILE") else {
            eprintln!("SKIP: set YUTU_TEST_OAUTH_FILE to a saved OAuth token file");
            return;
        };
        let persist = OAuthPersist::load_from_file(Path::new(&path)).expect("load oauth persist");
        let backend = InnertubeBackend::connect_with_oauth(&persist).expect("oauth connect");
        let songs = backend.library_songs().await.expect("library songs");
        assert!(
            !songs.is_empty(),
            "expected non-empty library songs for the OAuth account"
        );
    }

    #[tokio::test]
    #[ignore = "live network: cookie library songs + liked songs"]
    async fn live_cookie_library_songs() {
        let Ok(path) = std::env::var("YUTU_TEST_COOKIE_FILE") else {
            eprintln!("SKIP: set YUTU_TEST_COOKIE_FILE to a browser cookie file");
            return;
        };
        let backend = InnertubeBackend::connect_with_cookie_file(Path::new(&path))
            .await
            .expect("cookie connect");
        let songs = backend.library_songs().await.expect("library songs");
        assert!(
            !songs.is_empty(),
            "expected non-empty library songs for the cookie account"
        );
        // Liked playlist may legitimately be empty; only assert it resolves.
        let _ = backend.liked_songs().await.expect("liked songs");
    }

    #[tokio::test]
    #[ignore = "live network: YTM lyrics for a known video"]
    async fn live_song_lyrics() {
        let Ok(video_id) = std::env::var("YUTU_TEST_LYRICS_VIDEO_ID") else {
            eprintln!("SKIP: set YUTU_TEST_LYRICS_VIDEO_ID to a video id with lyrics");
            return;
        };
        let backend = InnertubeBackend::connect_anonymous()
            .await
            .expect("anonymous connect");
        let lines = backend.song_lyrics(&video_id).await.expect("lyrics");
        assert!(!lines.is_empty(), "expected lyric lines for {video_id}");
    }
}
