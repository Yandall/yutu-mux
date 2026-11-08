//! Live Innertube backend built on the real `ytmapi-rs` 0.3 client.
//!
//! Feature-gated behind `innertube` so offline builds stay network-free
//! (tests use the test-only `MockBackend` instead).
//!
//! What is live vs stubbed (see `docs/INNERTUBE.md` for the full picture):
//! - LIVE: anonymous + cookie-file + Google-OAuth-device-flow connection,
//!   `search_suggestions`, filtered `search` queries
//!   ([`InnertubeBackend::search_tracks`],
//!   [`InnertubeBackend::search_albums`], [`InnertubeBackend::search_artists`]
//!   via `YtMusic::search_songs` / `search_albums` / `search_artists`),
//!   and the web-style unfiltered search ([`crate::raw_search`]: raw
//!   `youtubei/v1/search` in server rank order, tolerant of the current
//!   flat item layout that `ytmapi-rs` 0.3 can no longer parse),
//!   login-gated library queries ([`InnertubeBackend::library_songs`],
//!   [`InnertubeBackend::liked_songs`] via the `VLLM` playlist,
//!   [`InnertubeBackend::library_albums`]), radio recommendations
//!   ([`InnertubeBackend::recommendations_for`]: there is no home query in
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
//! responses, never request headers. `ModernBrowserToken`'s `Debug` impl
//! prints only `"[cookie-redacted]"`, so deriving `Debug` below is safe.
//!
//! [`CookieAuth::from_file`]: crate::auth::CookieAuth

use std::path::Path;

use ytmapi_rs::auth::oauth::OAuthToken;
use ytmapi_rs::common::{AlbumID, ArtistChannelID, PlaylistID, Thumbnail, VideoID, YoutubeID};
use ytmapi_rs::parse::{
    AlbumSong, ArtistSong, PlaylistItem, SearchResultAlbum, SearchResultArtist,
    SearchResultFeaturedPlaylist, SearchResultPlaylist, SearchResultSong, TableListSong,
    WatchPlaylistTrack,
};
use ytmapi_rs::query::GetSongTrackingUrlQuery;
use ytmapi_rs::YtMusic;

use crate::auth::{CookieAuth, OAuthPersist};
use crate::models::{Album, Artist, PlaylistHit, Track};
use crate::stream::{StreamClient, StreamError, StreamRequest};
use crate::ytm_token::{ModernAnonymousToken, ModernBrowserToken};

/// Live YouTube Music handle.
///
/// `YtMusic` is generic over its auth token (anonymous vs cookie vs
/// OAuth), so all three are carried in one enum instead of leaking the
/// generic into our public API. The inner client owns a connection
/// pool: clone and re-use it.
#[derive(Debug, Clone)]
pub struct InnertubeBackend {
    inner: Inner,
}

/// Collect every continuation page of a list query (first page plus all
/// follow-ups): single-shot `query()` stops at about 100 items, which
/// silently truncated big playlists and libraries. Page failures fail
/// open (pages already loaded survive, failures go to stderr); only a
/// total failure errors.
async fn collect_pages<A, Q, T>(
    client: &YtMusic<A>,
    query: &Q,
    what: &str,
) -> Result<Vec<T>, String>
where
    A: ytmapi_rs::auth::AuthToken,
    Q: ytmapi_rs::query::Query<A> + ytmapi_rs::query::PostQuery,
    Q::Output: ytmapi_rs::continuations::ParseFromContinuable<Q> + IntoIterator<Item = T>,
{
    collect_pages_report(client, query, what)
        .await
        .map(|(out, _)| out)
}

/// [`collect_pages`] plus the failed-page count, so callers with a
/// lenient fallback (see [`InnertubeBackend::playlist_tracks`]) can tell
/// a clean fetch from a partial one. Total failure (pages errored, none
/// loaded) still errors here.
async fn collect_pages_report<A, Q, T>(
    client: &YtMusic<A>,
    query: &Q,
    what: &str,
) -> Result<(Vec<T>, u32), String>
where
    A: ytmapi_rs::auth::AuthToken,
    Q: ytmapi_rs::query::Query<A> + ytmapi_rs::query::PostQuery,
    Q::Output: ytmapi_rs::continuations::ParseFromContinuable<Q> + IntoIterator<Item = T>,
{
    use futures::StreamExt as _;
    let stream = client.stream(query);
    futures::pin_mut!(stream);
    let mut out = Vec::new();
    let mut pages = 0u32;
    let mut failed = 0u32;
    while let Some(page) = stream.next().await {
        match page {
            Ok(items) => {
                pages += 1;
                out.extend(items);
            }
            Err(e) => {
                failed += 1;
                crate::app_log::warn(
                    "yutu-import",
                    format_args!("{what} page failed ({failed} so far): {e}"),
                );
            }
        }
    }
    if out.is_empty() && failed > 0 {
        return Err(format!("{what} failed: no pages loaded"));
    }
    if failed > 0 {
        crate::app_log::warn(
            "yutu-import",
            format_args!("{what}: partial import ({pages} pages, {failed} failed)"),
        );
    }
    Ok((out, failed))
}

#[derive(Debug, Clone)]
enum Inner {
    Anonymous(YtMusic<ModernAnonymousToken>),
    Cookie(YtMusic<ModernBrowserToken>),
    OAuth(YtMusic<OAuthToken>),
}

/// A YouTube Music library playlist header (id + title). Tracks are fetched
/// separately with [`InnertubeBackend::playlist_tracks`] so imports can
/// fail open per playlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerPlaylist {
    pub id: String,
    pub title: String,
}

/// Map one `get_library_playlists` row to our [`ServerPlaylist`].
/// The browse `playlist_id` arrives `VL`-prefixed (`VLPL...`); it is
/// stored canonicalised (see [`crate::library::canonical_playlist_id`])
/// so later track/add/delete calls send the per-endpoint form.
fn server_playlist_of(list: &ytmapi_rs::parse::LibraryPlaylist) -> ServerPlaylist {
    ServerPlaylist {
        id: crate::library::canonical_playlist_id(list.playlist_id.get_raw()),
        title: list.title.clone(),
    }
}

impl InnertubeBackend {
    /// Connect without credentials (fewer queries supported server-side).
    ///
    /// Performs network I/O (fetches visitor data), so failures surface as a
    /// plain string without secret material (there is none here).
    pub async fn connect_anonymous() -> Result<Self, String> {
        let token = ModernAnonymousToken::fetch()
            .await
            .map_err(|e| format!("innertube anonymous connect failed: {e}"))?;
        Ok(Self {
            inner: Inner::Anonymous(YtMusic::from_auth_token(token)),
        })
    }

    /// Connect with a browser cookie file.
    ///
    /// Accepts Netscape `cookies.txt` or a raw `Cookie` header file (see
    /// [`CookieAuth::from_file`], which normalises both to a header value).
    /// The cookie value itself is never logged; only the ytmapi error
    /// (server response, no request headers) is echoed.
    pub async fn connect_with_cookie_file(path: &Path) -> Result<Self, String> {
        let auth = CookieAuth::from_file(path).map_err(|e| format!("cookie file: {e}"))?;
        Self::connect_with_cookie_str(auth.header_value()).await
    }

    /// Connect with an already-normalised `Cookie` header value (see
    /// [`CookieAuth::header_value`]). This is the stay-signed-in path: the
    /// saved session file holds the header, so no re-export is needed.
    pub async fn connect_with_cookie_str(header: &str) -> Result<Self, String> {
        let token = ModernBrowserToken::from_cookie_header(header)
            .await
            .map_err(|e| format!("innertube cookie login failed ({e}; bad/expired cookie?)"))?;
        Ok(Self {
            inner: Inner::Cookie(YtMusic::from_auth_token(token)),
        })
    }

    /// Connect with a Google OAuth token bundle (see
    /// [`crate::auth::poll_google_login`]).
    ///
    /// Synchronous: `YtMusic::from_auth_token` performs no network I/O
    /// (unlike the cookie path, which fetches the login page up front).
    /// The token material is never logged.
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

    /// Live artist top songs in server rank order with play counts
    /// (see [`artist_song_to_track`]). Artist browse is `AuthToken`,
    /// so anonymous backends get full results, not an error.
    pub async fn artist_top_songs(&self, channel_id: &str) -> Result<Vec<Track>, String> {
        let artist = match &self.inner {
            Inner::Anonymous(client) => client
                .get_artist(ArtistChannelID::from_raw(channel_id))
                .await
                .map_err(|e| format!("artist songs failed: {e}"))?,
            Inner::Cookie(client) => client
                .get_artist(ArtistChannelID::from_raw(channel_id))
                .await
                .map_err(|e| format!("artist songs failed: {e}"))?,
            Inner::OAuth(client) => client
                .get_artist(ArtistChannelID::from_raw(channel_id))
                .await
                .map_err(|e| format!("artist songs failed: {e}"))?,
        };
        Ok(artist
            .top_releases
            .songs
            .map(|s| s.results)
            .unwrap_or_default()
            .iter()
            .map(artist_song_to_track)
            .collect())
    }

    /// Full artist top-songs shelf in server rank order with play
    /// counts (`track_no`-style rank, durations, albums, and thumbs;
    /// see [`crate::raw_browse`]). The typed shelf behind
    /// [`Self::artist_top_songs`] only carries 5 songs; the shelf's
    /// own browse id resolves the whole list (100 rows). Anonymous
    /// (global counts, no personalization). Falls back to the typed
    /// 5-song shelf when the raw list comes back empty.
    pub async fn artist_top_full(&self, channel_id: &str) -> Result<Vec<Track>, String> {
        let browse_id = match &self.inner {
            Inner::Anonymous(client) => client
                .get_artist(ArtistChannelID::from_raw(channel_id))
                .await
                .map_err(|e| format!("artist songs failed: {e}"))?
                .top_releases
                .songs
                .map(|s| s.browse_id.get_raw().to_string()),
            Inner::Cookie(client) => client
                .get_artist(ArtistChannelID::from_raw(channel_id))
                .await
                .map_err(|e| format!("artist songs failed: {e}"))?
                .top_releases
                .songs
                .map(|s| s.browse_id.get_raw().to_string()),
            Inner::OAuth(client) => client
                .get_artist(ArtistChannelID::from_raw(channel_id))
                .await
                .map_err(|e| format!("artist songs failed: {e}"))?
                .top_releases
                .songs
                .map(|s| s.browse_id.get_raw().to_string()),
        };
        if let Some(browse) = browse_id {
            if let Ok(full) = crate::raw_browse::raw_shelf_tracklist(&browse).await {
                if !full.is_empty() {
                    return Ok(full);
                }
            }
        }
        self.artist_top_songs(channel_id).await
    }

    /// Live album tracklist in `track_no` order with play counts (see
    /// [`album_song_to_track`]). Same auth story as
    /// [`Self::artist_top_songs`]: the album browse works anonymously.
    pub async fn album_tracklist(&self, album_id: &str) -> Result<Vec<Track>, String> {
        let album = match &self.inner {
            Inner::Anonymous(client) => client
                .get_album(AlbumID::from_raw(album_id))
                .await
                .map_err(|e| format!("album songs failed: {e}"))?,
            Inner::Cookie(client) => client
                .get_album(AlbumID::from_raw(album_id))
                .await
                .map_err(|e| format!("album songs failed: {e}"))?,
            Inner::OAuth(client) => client
                .get_album(AlbumID::from_raw(album_id))
                .await
                .map_err(|e| format!("album songs failed: {e}"))?,
        };
        let artist = join_names(album.artists.iter().map(|a| a.name.as_str()));
        Ok(album
            .tracks
            .iter()
            .map(|t| album_song_to_track(t, &artist, &album.title))
            .collect())
    }

    /// Live community playlist search, same endpoint family as
    /// [`Self::search_tracks`]. Podcast hits are skipped (no podcast
    /// support anywhere else in the app); only real playlists map.
    pub async fn search_community_playlists(
        &self,
        query: &str,
    ) -> Result<Vec<PlaylistHit>, String> {
        let lists = match &self.inner {
            Inner::Anonymous(client) => client
                .search_community_playlists(query)
                .await
                .map_err(|e| format!("search playlists failed: {e}"))?,
            Inner::Cookie(client) => client
                .search_community_playlists(query)
                .await
                .map_err(|e| format!("search playlists failed: {e}"))?,
            Inner::OAuth(client) => client
                .search_community_playlists(query)
                .await
                .map_err(|e| format!("search playlists failed: {e}"))?,
        };
        Ok(lists.iter().filter_map(community_playlist_to_hit).collect())
    }

    /// All songs in the user's library (`FEmusic_liked_videos` browse).
    ///
    /// Streams every continuation page: single-shot queries stop at about
    /// 100 items, which silently truncated big libraries.
    ///
    /// Auth: cookie OR Google OAuth (`GetLibrarySongsQuery` is
    /// `impl<A: LoggedIn>` upstream, and only those two tokens are
    /// `LoggedIn`). Anonymous backends get a local error, no network.
    pub async fn library_songs(&self) -> Result<Vec<Track>, String> {
        Ok(self
            .library_songs_with_status()
            .await?
            .into_iter()
            .map(|(t, _)| t)
            .collect())
    }

    /// [`Self::library_songs`] plus each row's server like status, so
    /// sync can reconcile likes *and* dislikes (the liked `VLLM`
    /// playlist carries no dislikes).
    pub async fn library_songs_with_status(
        &self,
    ) -> Result<Vec<(Track, ytmapi_rs::common::LikeStatus)>, String> {
        use ytmapi_rs::query::GetLibrarySongsQuery;
        let query = GetLibrarySongsQuery::default();
        let songs: Vec<TableListSong> = match &self.inner {
            Inner::Anonymous(_) => {
                return Err("library_songs requires login (cookie or Google OAuth)".into());
            }
            Inner::Cookie(client) => collect_pages(client, &query, "library songs").await?,
            Inner::OAuth(client) => collect_pages(client, &query, "library songs").await?,
        };
        Ok(songs
            .iter()
            .map(|s| (table_song_to_track(s), s.like_status.clone()))
            .collect())
    }

    /// The user's liked-music tracks, via the `VLLM` liked playlist
    /// (see [`Self::playlist_tracks`]: `ytmapi-rs` 0.3 exposes no dedicated
    /// liked-songs query). The browse endpoint wants the `VL`-prefixed
    /// id: bare `LM` 400s (verified live).
    ///
    /// Auth: cookie OR Google OAuth in practice: the query itself is
    /// `impl<A: AuthToken>` so anonymous attempts hit the server and fail
    /// there. Song and video items map to [`Track`]; podcast episodes are
    /// skipped (not music).
    pub async fn liked_songs(&self) -> Result<Vec<Track>, String> {
        self.playlist_tracks("VLLM").await
    }

    /// Tracks of any playlist by id (`VLLM` = liked music, `PL...`/`VL...` =
    /// user playlists). Same mapping as [`Self::liked_songs`]: songs and
    /// videos become [`Track`], podcast episodes are skipped.
    ///
    /// Streams every continuation page (about 100 items each): without
    /// this, playlists over 100 songs import truncated. When a page carries
    /// a row shape the typed parser rejects (e.g. duration in a flex
    /// column instead of `fixedColumns`), the whole fetch restarts over
    /// raw page JSON with a tolerant per-row parser that skips only the
    /// bad rows instead of failing the playlist.
    ///
    /// Auth: the query is `impl<A: AuthToken>` upstream, so this runs on
    /// all backends; private playlists fail server-side without login.
    pub async fn playlist_tracks(&self, playlist_id: &str) -> Result<Vec<Track>, String> {
        self.playlist_tracks_live(playlist_id, |_| {}).await
    }

    /// Playlist tracks with per-page progress (see
    /// [`Self::playlist_tracks`]): `on_page` runs after each mapped page
    /// with the cumulative track count, so importers can move a progress
    /// bar song by song instead of jumping 0 to 100 at the end. Typed
    /// pages first (exact mapping); any typed failure restarts tolerantly
    /// over raw page JSON, keeping typed-mapped rows and appending only
    /// new ids. Errors only when zero tracks survive.
    pub async fn playlist_tracks_live<F>(
        &self,
        playlist_id: &str,
        mut on_page: F,
    ) -> Result<Vec<Track>, String>
    where
        F: FnMut(usize) + Send,
    {
        use ytmapi_rs::query::GetPlaylistTracksQuery;
        // Reads go to `browse`, which wants the `VL`-prefixed browse id.
        let browse = crate::library::browse_playlist_id(playlist_id);
        let query = GetPlaylistTracksQuery::new(PlaylistID::from_raw(browse.as_str()));
        let mut out: Vec<Track> = Vec::new();
        // Typed pages first (exact mapping), reporting each page the
        // moment it arrives: buffering all pages before reporting
        // bunches every tick microseconds before completion, so the bar
        // would still teleport 0 to 100%.
        let mut failed = match &self.inner {
            Inner::Anonymous(client) => {
                stream_typed_tracks(client, &query, &mut on_page, &mut out).await
            }
            Inner::Cookie(client) => {
                stream_typed_tracks(client, &query, &mut on_page, &mut out).await
            }
            Inner::OAuth(client) => {
                stream_typed_tracks(client, &query, &mut on_page, &mut out).await
            }
        };
        if !failed {
            return Ok(out);
        }
        crate::app_log::warn(
            "yutu-import",
            "playlist tracks: typed parse failed, retrying tolerantly",
        );
        failed = match &self.inner {
            Inner::Anonymous(client) => {
                stream_raw_tracks(client, &query, &mut on_page, &mut out).await
            }
            Inner::Cookie(client) => {
                stream_raw_tracks(client, &query, &mut on_page, &mut out).await
            }
            Inner::OAuth(client) => stream_raw_tracks(client, &query, &mut on_page, &mut out).await,
        };
        if out.is_empty() {
            return Err(if failed {
                "playlist tracks failed: no rows parsed".to_string()
            } else {
                "playlist tracks failed: empty playlist".to_string()
            });
        }
        Ok(out)
    }

    /// Best-effort total track count for progress bars: leading integer
    /// of the details `track_count_text` ("248 songs"). Callers fall back
    /// to list-level progress when this errors (endpoint quirks, odd
    /// locales); never fatal.
    pub async fn playlist_details_count(&self, playlist_id: &str) -> Result<usize, String> {
        // Same `browse` endpoint as track reads: `VL`-prefixed id.
        let browse = crate::library::browse_playlist_id(playlist_id);
        let details = match &self.inner {
            Inner::Anonymous(client) => client
                .get_playlist_details(PlaylistID::from_raw(browse.as_str()))
                .await
                .map_err(|e| format!("playlist details failed: {e}"))?,
            Inner::Cookie(client) => client
                .get_playlist_details(PlaylistID::from_raw(browse.as_str()))
                .await
                .map_err(|e| format!("playlist details failed: {e}"))?,
            Inner::OAuth(client) => client
                .get_playlist_details(PlaylistID::from_raw(browse.as_str()))
                .await
                .map_err(|e| format!("playlist details failed: {e}"))?,
        };
        parse_leading_count(&details.track_count_text)
            .ok_or_else(|| format!("no count in {:?}", details.track_count_text))
    }

    /// All playlists in the user's library (id + title; fetch tracks per
    /// playlist with [`Self::playlist_tracks`]).
    ///
    /// Streams every continuation page like the other list endpoints.
    ///
    /// Auth: cookie OR Google OAuth (same `LoggedIn` bound as
    /// [`Self::library_songs`]); anonymous backends get a local error.
    pub async fn library_playlists(&self) -> Result<Vec<ServerPlaylist>, String> {
        Ok(self.library_playlists_report().await?.0)
    }

    /// [`Self::library_playlists`] plus the failed-page count, so sync
    /// can tell a clean header fetch (safe to prune lists absent from
    /// it) from a partial one (never prune: absence may be a failed
    /// page, not a web deletion).
    pub async fn library_playlists_report(&self) -> Result<(Vec<ServerPlaylist>, u32), String> {
        use ytmapi_rs::query::GetLibraryPlaylistsQuery;
        let query = GetLibraryPlaylistsQuery;
        let (lists, failed): (Vec<ytmapi_rs::parse::LibraryPlaylist>, u32) = match &self.inner {
            Inner::Anonymous(_) => {
                return Err("library_playlists requires login (cookie or Google OAuth)".into());
            }
            Inner::Cookie(client) => {
                collect_pages_report(client, &query, "library playlists").await?
            }
            Inner::OAuth(client) => {
                collect_pages_report(client, &query, "library playlists").await?
            }
        };
        Ok((lists.iter().map(server_playlist_of).collect(), failed))
    }

    /// Push a like/unlike to YouTube Music (`LM` membership on the web).
    ///
    /// Auth: cookie OR Google OAuth (`RateSongQuery` is `LoggedIn`
    /// upstream); anonymous backends get a local error. Callers apply
    /// the local state first and treat this as best-effort sync.
    /// Sends `like/like` for likes, `like/removelike` for unlikes.
    pub async fn rate_like(&self, video_id: &str, liked: bool) -> Result<(), String> {
        let rating = if liked {
            ytmapi_rs::common::LikeStatus::Liked
        } else {
            ytmapi_rs::common::LikeStatus::Indifferent
        };
        match &self.inner {
            Inner::Anonymous(_) => Err("rate_like requires login (cookie or Google OAuth)".into()),
            Inner::Cookie(client) => client
                .rate_song(VideoID::from_raw(video_id), rating)
                .await
                .map_err(|e| format!("rate song failed: {e}")),
            Inner::OAuth(client) => client
                .rate_song(VideoID::from_raw(video_id), rating)
                .await
                .map_err(|e| format!("rate song failed: {e}")),
        }
    }

    /// Push a dislike ("don't recommend") to YouTube Music.
    ///
    /// Sends `like/dislike` for dislikes, `like/removelike` to clear.
    /// Like and dislike are one status server-side: callers clear the
    /// local like when disliking and vice versa.
    ///
    /// Auth: cookie OR Google OAuth, same as [`Self::rate_like`].
    pub async fn rate_dislike(&self, video_id: &str, disliked: bool) -> Result<(), String> {
        let rating = if disliked {
            ytmapi_rs::common::LikeStatus::Disliked
        } else {
            ytmapi_rs::common::LikeStatus::Indifferent
        };
        match &self.inner {
            Inner::Anonymous(_) => {
                Err("rate_dislike requires login (cookie or Google OAuth)".into())
            }
            Inner::Cookie(client) => client
                .rate_song(VideoID::from_raw(video_id), rating)
                .await
                .map_err(|e| format!("rate dislike failed: {e}")),
            Inner::OAuth(client) => client
                .rate_song(VideoID::from_raw(video_id), rating)
                .await
                .map_err(|e| format!("rate dislike failed: {e}")),
        }
    }

    /// Push a playlist like/unlike (`like/like` vs `like/removelike`
    /// with a `playlistId` target). A liked playlist joins the library.
    ///
    /// Auth: cookie OR Google OAuth; anonymous backends get a local error.
    pub async fn rate_playlist_like(&self, playlist_id: &str, liked: bool) -> Result<(), String> {
        let rating = if liked {
            ytmapi_rs::common::LikeStatus::Liked
        } else {
            ytmapi_rs::common::LikeStatus::Indifferent
        };
        // Playlist targets go to `like/*` in edit (raw) form: a `VL...`
        // browse id 400s with `invalid argument`.
        let edit = crate::library::edit_playlist_id(playlist_id);
        match &self.inner {
            Inner::Anonymous(_) => {
                Err("rate_playlist_like requires login (cookie or Google OAuth)".into())
            }
            Inner::Cookie(client) => client
                .rate_playlist(PlaylistID::from_raw(edit.as_str()), rating)
                .await
                .map_err(|e| format!("rate playlist failed: {e}")),
            Inner::OAuth(client) => client
                .rate_playlist(PlaylistID::from_raw(edit.as_str()), rating)
                .await
                .map_err(|e| format!("rate playlist failed: {e}")),
        }
    }

    /// Push a playlist dislike (`like/dislike` with a `playlistId`
    /// target): reduces its recommendations. `false` clears via
    /// `like/removelike`.
    ///
    /// Auth: cookie OR Google OAuth; anonymous backends get a local error.
    pub async fn rate_playlist_dislike(
        &self,
        playlist_id: &str,
        disliked: bool,
    ) -> Result<(), String> {
        let rating = if disliked {
            ytmapi_rs::common::LikeStatus::Disliked
        } else {
            ytmapi_rs::common::LikeStatus::Indifferent
        };
        // Same edit-form rule as [`Self::rate_playlist_like`].
        let edit = crate::library::edit_playlist_id(playlist_id);
        match &self.inner {
            Inner::Anonymous(_) => {
                Err("rate_playlist_dislike requires login (cookie or Google OAuth)".into())
            }
            Inner::Cookie(client) => client
                .rate_playlist(PlaylistID::from_raw(edit.as_str()), rating)
                .await
                .map_err(|e| format!("rate playlist dislike failed: {e}")),
            Inner::OAuth(client) => client
                .rate_playlist(PlaylistID::from_raw(edit.as_str()), rating)
                .await
                .map_err(|e| format!("rate playlist dislike failed: {e}")),
        }
    }

    /// Rename a YouTube Music playlist you own (`browse/edit_playlist`
    /// with `ACTION_SET_PLAYLIST_NAME`).
    ///
    /// Auth: cookie OR Google OAuth; anonymous backends get a local error.
    pub async fn rename_server_playlist(
        &self,
        playlist_id: &str,
        new_title: &str,
    ) -> Result<(), String> {
        use ytmapi_rs::query::EditPlaylistQuery;
        // `browse/edit_playlist` wants the raw id (see edit-form rule).
        let edit = crate::library::edit_playlist_id(playlist_id);
        let outcome = match &self.inner {
            Inner::Anonymous(_) => {
                return Err(
                    "rename_server_playlist requires login (cookie or Google OAuth)".into(),
                );
            }
            Inner::Cookie(client) => client
                .edit_playlist(EditPlaylistQuery::new_title(
                    PlaylistID::from_raw(edit.as_str()),
                    new_title,
                ))
                .await
                .map_err(|e| format!("rename playlist failed: {e}"))?,
            Inner::OAuth(client) => client
                .edit_playlist(EditPlaylistQuery::new_title(
                    PlaylistID::from_raw(edit.as_str()),
                    new_title,
                ))
                .await
                .map_err(|e| format!("rename playlist failed: {e}"))?,
        };
        match outcome {
            ytmapi_rs::common::ApiOutcome::Success => Ok(()),
            ytmapi_rs::common::ApiOutcome::Failure => {
                Err("rename playlist failed: server returned failure".into())
            }
        }
    }

    /// Subscribe to an artist (adds to the library artist list).
    ///
    /// Auth: cookie OR Google OAuth; anonymous backends get a local error.
    pub async fn subscribe_artist(&self, channel_id: &str) -> Result<(), String> {
        use ytmapi_rs::common::ArtistChannelID;
        use ytmapi_rs::common::YoutubeID as _;
        match &self.inner {
            Inner::Anonymous(_) => {
                Err("subscribe_artist requires login (cookie or Google OAuth)".into())
            }
            Inner::Cookie(client) => client
                .subscribe_artist(ArtistChannelID::from_raw(channel_id))
                .await
                .map_err(|e| format!("subscribe artist failed: {e}")),
            Inner::OAuth(client) => client
                .subscribe_artist(ArtistChannelID::from_raw(channel_id))
                .await
                .map_err(|e| format!("subscribe artist failed: {e}")),
        }
    }

    /// Unsubscribe from one or more artists.
    ///
    /// Auth: cookie OR Google OAuth; anonymous backends get a local error.
    pub async fn unsubscribe_artists(&self, channel_ids: &[String]) -> Result<(), String> {
        use ytmapi_rs::common::ArtistChannelID;
        use ytmapi_rs::common::YoutubeID as _;
        match &self.inner {
            Inner::Anonymous(_) => {
                Err("unsubscribe_artists requires login (cookie or Google OAuth)".into())
            }
            Inner::Cookie(client) => client
                .unsubscribe_artists(
                    channel_ids
                        .iter()
                        .map(|c| ArtistChannelID::from_raw(c.as_str())),
                )
                .await
                .map_err(|e| format!("unsubscribe artists failed: {e}")),
            Inner::OAuth(client) => client
                .unsubscribe_artists(
                    channel_ids
                        .iter()
                        .map(|c| ArtistChannelID::from_raw(c.as_str())),
                )
                .await
                .map_err(|e| format!("unsubscribe artists failed: {e}")),
        }
    }

    /// Per-membership ids for one playlist: every `(video_id,
    /// set_video_id)` pair found in the raw browse JSON. `set_video_id`
    /// is what `browse/edit_playlist` removals need; the typed
    /// `PlaylistItem` parse drops it, so this raw pass recovers it.
    ///
    /// Auth: all backends attempt it (private lists fail server-side).
    /// Errors only when zero pairs survive.
    pub async fn playlist_set_video_map(
        &self,
        playlist_id: &str,
    ) -> Result<Vec<(String, String)>, String> {
        use ytmapi_rs::query::GetPlaylistTracksQuery;
        // Read path: `browse` wants the `VL`-prefixed id.
        let browse = crate::library::browse_playlist_id(playlist_id);
        let query = GetPlaylistTracksQuery::new(PlaylistID::from_raw(browse.as_str()));
        let mut out: Vec<(String, String)> = Vec::new();
        let failed: bool = match &self.inner {
            Inner::Anonymous(client) => collect_set_video_pairs(client, &query, &mut out).await,
            Inner::Cookie(client) => collect_set_video_pairs(client, &query, &mut out).await,
            Inner::OAuth(client) => collect_set_video_pairs(client, &query, &mut out).await,
        };
        if out.is_empty() {
            return Err(if failed {
                "playlist setVideoIds failed: no rows parsed".to_string()
            } else {
                "playlist setVideoIds failed: empty playlist".to_string()
            });
        }
        Ok(out)
    }

    /// Remove every occurrence of a video from a YouTube Music playlist
    /// you own (`browse/edit_playlist` with `ACTION_REMOVE_VIDEO`).
    ///
    /// Resolves the per-membership `setVideoId` first via
    /// [`Self::playlist_set_video_map`], then removes all matching
    /// memberships. Auth: cookie OR Google OAuth.
    pub async fn remove_from_server_playlist(
        &self,
        playlist_id: &str,
        video_id: &str,
    ) -> Result<(), String> {
        use ytmapi_rs::common::SetVideoID;
        use ytmapi_rs::common::YoutubeID as _;
        let map = self.playlist_set_video_map(playlist_id).await?;
        let targets: Vec<SetVideoID<'_>> = map
            .iter()
            .filter(|(vid, _)| vid == video_id)
            .map(|(_, set)| SetVideoID::from_raw(set.as_str()))
            .collect();
        if targets.is_empty() {
            return Err(format!("video {video_id} is not on playlist {playlist_id}"));
        }
        // Removal posts to `browse/edit_playlist`: edit (raw) form.
        let edit = crate::library::edit_playlist_id(playlist_id);
        match &self.inner {
            Inner::Anonymous(_) => {
                Err("remove_from_server_playlist requires login (cookie or Google OAuth)".into())
            }
            Inner::Cookie(client) => client
                .remove_playlist_items(PlaylistID::from_raw(edit.as_str()), targets)
                .await
                .map_err(|e| format!("remove from playlist failed: {e}")),
            Inner::OAuth(client) => client
                .remove_playlist_items(PlaylistID::from_raw(edit.as_str()), targets)
                .await
                .map_err(|e| format!("remove from playlist failed: {e}")),
        }
    }

    /// Create a private playlist on YouTube Music, returning its server id
    /// in canonical (edit-form) storage form.
    ///
    /// Auth: cookie OR Google OAuth; anonymous backends get a local error.
    pub async fn create_server_playlist(&self, title: &str) -> Result<String, String> {
        use ytmapi_rs::query::playlist::PrivacyStatus;
        use ytmapi_rs::query::CreatePlaylistQuery;
        match &self.inner {
            Inner::Anonymous(_) => {
                Err("create_server_playlist requires login (cookie or Google OAuth)".into())
            }
            Inner::Cookie(client) => client
                .create_playlist(CreatePlaylistQuery::new(
                    title,
                    None,
                    PrivacyStatus::Private,
                ))
                .await
                .map(|id| crate::library::canonical_playlist_id(id.get_raw()))
                .map_err(|e| format!("create playlist failed: {e}")),
            Inner::OAuth(client) => client
                .create_playlist(CreatePlaylistQuery::new(
                    title,
                    None,
                    PrivacyStatus::Private,
                ))
                .await
                .map(|id| crate::library::canonical_playlist_id(id.get_raw()))
                .map_err(|e| format!("create playlist failed: {e}")),
        }
    }

    /// Append a video to a YouTube Music playlist you own.
    ///
    /// Posts to `browse/edit_playlist` (`ACTION_ADD_VIDEO`), which wants
    /// the raw id: a `VL...` browse id 400s with `invalid argument`, so
    /// both stored forms are accepted and converted.
    ///
    /// Auth: cookie OR Google OAuth; anonymous backends get a local error.
    pub async fn add_to_server_playlist(
        &self,
        playlist_id: &str,
        video_id: &str,
    ) -> Result<(), String> {
        let edit = crate::library::edit_playlist_id(playlist_id);
        match &self.inner {
            Inner::Anonymous(_) => {
                Err("add_to_server_playlist requires login (cookie or Google OAuth)".into())
            }
            Inner::Cookie(client) => client
                .add_video_items_to_playlist(
                    PlaylistID::from_raw(edit.as_str()),
                    [VideoID::from_raw(video_id)],
                )
                .await
                .map(|_| ())
                .map_err(|e| format!("add to playlist failed: {e}")),
            Inner::OAuth(client) => client
                .add_video_items_to_playlist(
                    PlaylistID::from_raw(edit.as_str()),
                    [VideoID::from_raw(video_id)],
                )
                .await
                .map(|_| ())
                .map_err(|e| format!("add to playlist failed: {e}")),
        }
    }

    /// Delete a YouTube Music playlist you own (`playlist/delete`).
    ///
    /// Same edit-form rule as [`Self::add_to_server_playlist`]: accepts
    /// either stored form.
    ///
    /// Auth: cookie OR Google OAuth; anonymous backends get a local error.
    /// Single-track removal uses [`Self::remove_from_server_playlist`]
    /// (setVideoId resolved from raw browse JSON).
    pub async fn delete_server_playlist(&self, playlist_id: &str) -> Result<(), String> {
        let edit = crate::library::edit_playlist_id(playlist_id);
        match &self.inner {
            Inner::Anonymous(_) => {
                Err("delete_server_playlist requires login (cookie or Google OAuth)".into())
            }
            Inner::Cookie(client) => client
                .delete_playlist(PlaylistID::from_raw(edit.as_str()))
                .await
                .map_err(|e| format!("delete playlist failed: {e}")),
            Inner::OAuth(client) => client
                .delete_playlist(PlaylistID::from_raw(edit.as_str()))
                .await
                .map_err(|e| format!("delete playlist failed: {e}")),
        }
    }

    /// All albums in the user's library.
    ///
    /// Streams every continuation page like the other list endpoints.
    ///
    /// Auth: cookie OR Google OAuth (same `LoggedIn` bound as
    /// [`Self::library_songs`); anonymous backends get a local error.
    /// Mapping reuses [`album_to_album`].
    pub async fn library_albums(&self) -> Result<Vec<Album>, String> {
        use ytmapi_rs::parse::SearchResultAlbum;
        use ytmapi_rs::query::GetLibraryAlbumsQuery;
        let query = GetLibraryAlbumsQuery::default();
        let albums: Vec<SearchResultAlbum> = match &self.inner {
            Inner::Anonymous(_) => {
                return Err("library_albums requires login (cookie or Google OAuth)".into());
            }
            Inner::Cookie(client) => collect_pages(client, &query, "library albums").await?,
            Inner::OAuth(client) => collect_pages(client, &query, "library albums").await?,
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

    /// YouTube Music lyrics for `video_id` as plain text lines (no timing:
    /// timing is LRCLIB's job).
    ///
    /// Two-step upstream flow: `get_lyrics_id` (watch `next` endpoint) then
    /// `get_lyrics` (browse the lyrics id). Both queries are
    /// `impl<A: AuthToken>`, so this is attempted on all backends; in
    /// practice lyric availability is per-track server-side (many tracks have
    /// none: that surfaces as an API error here).
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
    /// `ytmapi-rs` 0.3 offers: see [`GetSongTrackingUrlQuery`]) and
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
                crate::app_log::info(
                    "yutu-stream",
                    format_args!("PO-gated; minting via sidecar {bin}"),
                );
                let minted = tokio::task::spawn_blocking({
                    let video_id = video_id.clone();
                    move || crate::pot::fetch_po_token(&bin, &video_id)
                })
                .await
                .map_err(|e| StreamError::Network(format!("sidecar task: {e}")))?
                .map_err(StreamError::Network)?;
                crate::app_log::info(
                    "yutu-stream",
                    format_args!(
                        "minted {} token(s), snapshot={}",
                        minted.tokens.len(),
                        minted.tokens.is_empty()
                    ),
                );
                // Token order (content- vs session-bound) is undocumented, so
                // try each minted token as the player poToken in turn.
                let mut last_err = StreamError::PoTokenRequired {
                    client: "web_music",
                };
                for (i, content_token) in minted.tokens.iter().enumerate() {
                    crate::app_log::info(
                        "yutu-stream",
                        format_args!("player POST attempt with token #{i}"),
                    );
                    match raw_player_with_token(&video_id, content_token).await {
                        Err(e) => {
                            crate::app_log::warn(
                                "yutu-stream",
                                format_args!("token #{i} POST failed: {e}"),
                            );
                            last_err = e;
                        }
                        Ok(raw) => {
                            let value: serde_json::Value =
                                serde_json::from_str(&raw).map_err(|e| {
                                    StreamError::Network(format!("token player JSON: {e}"))
                                })?;
                            crate::app_log::info(
                                "yutu-stream",
                                format_args!(
                                    "token #{i} playability={}",
                                    value
                                        .pointer("/playabilityStatus/status")
                                        .and_then(serde_json::Value::as_str)
                                        .unwrap_or("?"),
                                ),
                            );
                            match extract_audio_url(&value) {
                                Some(url) => {
                                    crate::app_log::info(
                                        "yutu-stream",
                                        format_args!("token #{i} yielded URL"),
                                    );
                                    return Ok(ResolvedStream {
                                        url,
                                        pot_token: minted.tokens.get(1).cloned(),
                                    });
                                }
                                None => {
                                    last_err =
                                        classify_no_url(&value, StreamClient::WebMusic, &video_id);
                                    crate::app_log::warn(
                                        "yutu-stream",
                                        format_args!("token #{i} no URL: {last_err}"),
                                    );
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

/// Typed pages mapped + reported as each arrives (see
/// [`InnertubeBackend::playlist_tracks_live`]). Returns true when a
/// page failed (upstream stops the stream there).
async fn stream_typed_tracks<A, F>(
    client: &YtMusic<A>,
    query: &ytmapi_rs::query::GetPlaylistTracksQuery<'_>,
    on_page: &mut F,
    out: &mut Vec<Track>,
) -> bool
where
    A: ytmapi_rs::auth::AuthToken,
    F: FnMut(usize) + Send,
{
    use futures::StreamExt as _;
    let stream = client.stream(query);
    futures::pin_mut!(stream);
    let mut failed = false;
    while let Some(page) = stream.next().await {
        match page {
            Ok(items) => {
                out.extend(items.iter().filter_map(playlist_item_to_track));
                on_page(out.len());
            }
            Err(e) => {
                failed = true;
                crate::app_log::warn(
                    "yutu-import",
                    format_args!("playlist tracks page failed: {e}"),
                );
                break;
            }
        }
    }
    failed
}

/// Raw pages mapped tolerantly + reported as each arrives, deduping
/// against typed-mapped rows kept from the failed pass. Returns true on
/// transport error (the caller errors only when nothing survived).
async fn stream_raw_tracks<A, F>(
    client: &YtMusic<A>,
    query: &ytmapi_rs::query::GetPlaylistTracksQuery<'_>,
    on_page: &mut F,
    out: &mut Vec<Track>,
) -> bool
where
    A: ytmapi_rs::auth::AuthToken,
    F: FnMut(usize) + Send,
{
    use futures::StreamExt as _;
    let stream = client.raw_json_stream::<ytmapi_rs::query::GetPlaylistTracksQuery>(query);
    futures::pin_mut!(stream);
    let mut failed = false;
    while let Some(page) = stream.next().await {
        match page {
            Ok(json) => {
                for t in tolerant_playlist_rows(&json) {
                    if !out.iter().any(|x| x.id == t.id) {
                        out.push(t);
                    }
                }
                on_page(out.len());
            }
            Err(e) => {
                failed = true;
                crate::app_log::warn(
                    "yutu-import",
                    format_args!("playlist tracks raw page failed: {e}"),
                );
                break;
            }
        }
    }
    failed
}

/// Raw pages scanned for `(video_id, set_video_id)` pairs (see
/// [`InnertubeBackend::playlist_set_video_map`]). Returns true when a
/// page failed to load (the caller errors only when nothing survived).
async fn collect_set_video_pairs<A>(
    client: &YtMusic<A>,
    query: &ytmapi_rs::query::GetPlaylistTracksQuery<'_>,
    out: &mut Vec<(String, String)>,
) -> bool
where
    A: ytmapi_rs::auth::AuthToken,
{
    use futures::StreamExt as _;
    let stream = client.raw_json_stream::<ytmapi_rs::query::GetPlaylistTracksQuery>(query);
    futures::pin_mut!(stream);
    let mut failed = false;
    while let Some(page) = stream.next().await {
        match page {
            Ok(json) => {
                for pair in extract_set_video_pairs(&json) {
                    if !out.contains(&pair) {
                        out.push(pair);
                    }
                }
            }
            Err(e) => {
                failed = true;
                crate::app_log::warn("yutu-playlist", format_args!("setVideoId page failed: {e}"));
                break;
            }
        }
    }
    failed
}

/// Every `(video_id, set_video_id)` pair in one raw playlist page.
///
/// Walks every `musicResponsiveListItemRenderer` row and deep-searches
/// inside it for a `videoId` (the track) and a `setVideoId` (the
/// per-membership removal handle). Rows missing either are skipped
/// (deleted, greyed-out, episodes). Unparseable JSON yields no pairs.
pub fn extract_set_video_pairs(raw: &str) -> Vec<(String, String)> {
    let value: serde_json::Value = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let mut renderers = Vec::new();
    collect_renderers(&value, &mut renderers);
    renderers
        .iter()
        .filter_map(|r| {
            let video = deep_find_first_string(r, &["videoId"])?;
            let set = deep_find_first_string(r, &["setVideoId", "playlistSetVideoId"])?;
            if video.is_empty() || set.is_empty() {
                return None;
            }
            Some((video, set))
        })
        .collect()
}

/// All `musicResponsiveListItemRenderer` subtrees in a page (both the
/// first-page shelf and continuation items share this row shape).
fn collect_renderers<'v>(value: &'v serde_json::Value, out: &mut Vec<&'v serde_json::Value>) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(r) = map.get("musicResponsiveListItemRenderer") {
                out.push(r);
            }
            for v in map.values() {
                collect_renderers(v, out);
            }
        }
        serde_json::Value::Array(items) => {
            for v in items {
                collect_renderers(v, out);
            }
        }
        _ => {}
    }
}

/// First non-empty string under any of `keys`, depth-first. Finds
/// `videoId` inside `watchEndpoint` as well as top-level `setVideoId`
/// without depending on the exact nesting upstream uses.
fn deep_find_first_string(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    match value {
        serde_json::Value::Object(map) => {
            for key in keys {
                if let Some(serde_json::Value::String(s)) = map.get(*key) {
                    if !s.is_empty() {
                        return Some(s.clone());
                    }
                }
            }
            for v in map.values() {
                if let Some(hit) = deep_find_first_string(v, keys) {
                    return Some(hit);
                }
            }
            None
        }
        serde_json::Value::Array(items) => {
            for v in items {
                if let Some(hit) = deep_find_first_string(v, keys) {
                    return Some(hit);
                }
            }
            None
        }
        _ => None,
    }
}

/// Leading integer of a details count text ("248 songs", "1,234
/// videos"): `None` when no leading number exists.
fn parse_leading_count(text: &str) -> Option<usize> {
    let digits: String = text
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == ',')
        .filter(|c| *c != ',')
        .collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

/// Raw Innertube `player` POST mirroring `ytmapi-rs` (WEB_REMIX context, same
/// API key and signature-timestamp scheme) plus `serviceIntegrityDimensions`,
/// which upstream 0.3 cannot express: hence this bypass exists.
async fn raw_player_with_token(video_id: &str, po_token: &str) -> Result<String, StreamError> {
    let body = player_request_body(video_id, Some(po_token));
    let response = crate::shared_client()
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
    crate::app_log::info("yutu-stream", format_args!("player POST HTTP {status}"));
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
pub(crate) const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/144.0.0.0 Safari/537.36";

/// Rolling WEB_REMIX version, same date-based spirit as upstream's
/// fallback: the web client rolls this daily, and a stale pin drifts
/// ranking. Format `1.YYYYMMDD.01.00`, from days since epoch (no date
/// crate needed).
pub(crate) fn web_remix_version() -> String {
    let days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() / 86_400)
        .unwrap_or(0) as i64;
    // Howard Hinnant's days-to-civil algorithm (days since 1970-01-01
    // to Gregorian y/m/d).
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let y = (if m <= 2 { y + 1 } else { y }) as u32;
    format!("1.{y:04}{m:02}{d:02}.01.00")
}

/// Web client context exactly as music.youtube.com sends it: WEB_REMIX
/// plus the rolling version and locale, with the safety-mode user as a
/// `context` sibling (never nested inside `client`, which the server
/// ignores).
pub(crate) fn web_context() -> serde_json::Value {
    serde_json::json!({
        "client": {
            "clientName": "WEB_REMIX",
            "clientVersion": web_remix_version(),
            "hl": "en",
            "gl": "US",
        },
        "user": { "lockedSafetyMode": false },
    })
}

/// Build the raw `player` body. `po_token = None` reproduces the plain probe;
/// `Some` attaches `serviceIntegrityDimensions` per the BotGuard contract.
fn player_request_body(video_id: &str, po_token: Option<&str>) -> serde_json::Value {
    let mut body = serde_json::json!({
        "context": web_context(),
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

/// Days since epoch: mirrors upstream's `get_signature_timestamp`.
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
/// malformed `streamingData`: callers treat that as "this client cannot play
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
/// songs have none: see youtui#174), and a `"m:ss"`/`"h:mm:ss"` duration
/// string; unparseable durations become `0`.
/// Smallest artwork thumbnail (keeps row art light), if any.
fn pick_thumb_url(thumbs: &[Thumbnail]) -> Option<String> {
    thumbs
        .iter()
        .min_by_key(|t| t.width.max(1))
        .map(|t| t.url.clone())
        .filter(|u| !u.is_empty())
}

/// Largest artwork thumbnail (hero views like the Now Playing drawer).
fn pick_large_thumb_url(thumbs: &[Thumbnail]) -> Option<String> {
    thumbs
        .iter()
        .max_by_key(|t| t.width)
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
        thumbnail_large_url: pick_large_thumb_url(&song.thumbnails),
        plays: is_play_count(&song.plays).then(|| song.plays.clone()),
        is_video: false,
    }
}

/// True when the server text is a real play/view count ("213M plays",
/// "120M views", "2.7B plays", "2725 M reproducciones"): the same flex
/// column sometimes carries an upload date ("Nov 12, 2025", podcast
/// episodes), which must never pose as a count downstream. Empty fails
/// the keyword test on its own.
pub(crate) fn is_play_count(raw: &str) -> bool {
    let lower = raw.to_lowercase();
    lower.contains("play") || lower.contains("view") || lower.contains("reprodu")
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

/// Map one community playlist hit to our [`PlaylistHit`] (featured and
/// community variants; podcasts are `None` and skipped by the caller).
fn community_playlist_to_hit(list: &SearchResultPlaylist) -> Option<PlaylistHit> {
    match list {
        SearchResultPlaylist::Featured(f) => Some(featured_to_hit(f)),
        SearchResultPlaylist::Community(c) => Some(PlaylistHit {
            id: c.playlist_id.get_raw().to_string(),
            title: c.title.clone(),
            author: c.author.clone(),
            detail: c.views.clone(),
            thumbnail_url: pick_thumb_url(&c.thumbnails),
        }),
        SearchResultPlaylist::Podcast(_) => None,
        // Future variants (the enum is non-exhaustive): skip like podcasts.
        _ => None,
    }
}

/// Map one featured playlist (official/web shelves) to our [`PlaylistHit`].
fn featured_to_hit(f: &SearchResultFeaturedPlaylist) -> PlaylistHit {
    PlaylistHit {
        id: f.playlist_id.get_raw().to_string(),
        title: f.title.clone(),
        author: f.author.clone(),
        detail: f.songs.clone(),
        thumbnail_url: pick_thumb_url(&f.thumbnails),
    }
}

/// Map one artist top song to our [`Track`]: server rank order is the
/// true Popular list, and `plays` survives (unlike table listings).
/// Upstream carries no duration or thumbs here, so rows render the
/// bare count (like duration-less search All rows) and art warms from
/// whatever the catalog already holds for the id.
fn artist_song_to_track(song: &ArtistSong) -> Track {
    Track {
        id: song.video_id.get_raw().to_string(),
        title: song.title.clone(),
        artist: join_names(song.artists.iter().map(|a| a.name.as_str())),
        album: Some(song.album.name.clone()).filter(|name| !name.is_empty()),
        duration_secs: 0,
        thumbnail_url: None,
        thumbnail_large_url: None,
        plays: is_play_count(&song.plays).then(|| song.plays.clone()),
        is_video: false,
    }
}

/// Map one album track to our [`Track`]: `track_no` order is the true
/// album order, and `plays` survives. Upstream carries no thumbs or
/// per-row artist (the album context owns both), so the caller passes
/// the album artist and art warms from the catalog like artist songs.
fn album_song_to_track(song: &AlbumSong, artist: &str, album: &str) -> Track {
    Track {
        id: song.video_id.get_raw().to_string(),
        title: song.title.clone(),
        artist: artist.to_string(),
        album: Some(album.to_string()).filter(|name| !name.is_empty()),
        duration_secs: parse_duration_secs(&song.duration),
        thumbnail_url: None,
        thumbnail_large_url: None,
        plays: is_play_count(&song.plays).then(|| song.plays.clone()),
        is_video: false,
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
        thumbnail_large_url: pick_large_thumb_url(&song.thumbnails),
        plays: None,
        is_video: false,
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
            thumbnail_large_url: pick_large_thumb_url(&song.thumbnails),
            plays: None,
            is_video: false,
        }),
        PlaylistItem::Video(video) => Some(Track {
            id: video.video_id.get_raw().to_string(),
            title: video.title.clone(),
            artist: video.channel_name.clone(),
            album: None,
            duration_secs: parse_duration_secs(&video.duration),
            thumbnail_url: pick_thumb_url(&video.thumbnails),
            thumbnail_large_url: pick_large_thumb_url(&video.thumbnails),
            plays: None,
            is_video: true,
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
            thumbnail_large_url: None,
            plays: None,
            is_video: false,
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
        thumbnail_large_url: pick_large_thumb_url(&track.thumbnails),
        plays: None,
        is_video: false,
    }
}

/// Tolerant playlist rows from raw page JSON (fallback inside
/// [`InnertubeBackend::playlist_tracks_live`]).
///
/// Upstream's typed parser rejects a whole page on one unknown row shape
/// (observed live: rows with no `fixedColumns` and no `playlistItemData`,
/// duration sitting in a fourth flex column). This maps what it can per
/// row and skips the rest, so one odd row cannot sink a 300-song import:
/// - title = flex column 0 run 0; artist = flex column 1 artist runs
///   (every other run, joined like [`join_names`]); album = flex column
///   2 run 0 when present.
/// - duration = `fixedColumns` text when present, else flex column 3 when
///   it parses as a duration; unparseable becomes 0.
/// - video id = overlay play-button endpoint, else menu item 0.
/// - deleted songs, greyed-out unavailable rows, podcast episodes and
///   continuation markers are skipped (not music or not rows).
fn tolerant_playlist_rows(raw: &str) -> Vec<Track> {
    let value: serde_json::Value = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let rows = value
        .pointer("/contents/twoColumnBrowseResultsRenderer/secondaryContents/sectionListRenderer/contents/0/musicPlaylistShelfRenderer/contents")
        .or_else(|| {
            value.pointer("/onResponseReceivedActions/0/appendContinuationItemsAction/continuationItems")
        });
    match rows.and_then(serde_json::Value::as_array) {
        Some(rows) => rows.iter().filter_map(tolerant_row_to_track).collect(),
        None => Vec::new(),
    }
}

/// One tolerant row (see [`tolerant_playlist_rows`]): `None` means skip.
fn tolerant_row_to_track(item: &serde_json::Value) -> Option<Track> {
    let r = item.get("musicResponsiveListItemRenderer")?;
    let title = flex_run_text(r, 0, 0)?;
    if title.is_empty() || title == "Song deleted" {
        return None;
    }
    if r.get("musicItemRendererDisplayPolicy")
        .and_then(|p| p.as_str())
        == Some("MUSIC_ITEM_RENDERER_DISPLAY_POLICY_GREY_OUT")
    {
        return None;
    }
    if r
        .pointer("/overlay/musicItemThumbnailOverlayRenderer/content/musicPlayButtonRenderer/playNavigationEndpoint/watchEndpoint/videoType")
        .and_then(|t| t.as_str())
        == Some("MUSIC_VIDEO_TYPE_PODCAST_EPISODE")
    {
        return None;
    }
    let video_id = r
        .pointer("/overlay/musicItemThumbnailOverlayRenderer/content/musicPlayButtonRenderer/playNavigationEndpoint/watchEndpoint/videoId")
        .and_then(|v| v.as_str())
        .or_else(|| {
            r.pointer("/menu/menuRenderer/items/0/menuNavigationItemRenderer/navigationEndpoint/watchEndpoint/videoId")
                .and_then(|v| v.as_str())
        })
        .filter(|v| !v.is_empty())?;
    let artist_runs = flex_runs(r, 1);
    let artist = join_names(artist_runs.iter().step_by(2).map(String::as_str));
    if artist.is_empty() {
        return None;
    }
    let album = flex_run_text(r, 2, 0).filter(|a| !a.is_empty());
    let duration = fixed_text(r)
        .or_else(|| flex_run_text(r, 3, 0))
        .map(|d| parse_duration_secs(&d))
        .unwrap_or(0);
    let (small, large) =
        pick_raw_thumbs(r.pointer("/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails"));
    Some(Track {
        id: video_id.to_string(),
        title,
        artist,
        album,
        duration_secs: duration,
        thumbnail_url: small,
        thumbnail_large_url: large,
        plays: None,
        is_video: false,
    })
}

/// All run texts of flex column `col` (empty when absent).
fn flex_runs(r: &serde_json::Value, col: usize) -> Vec<String> {
    let pointer = format!("/flexColumns/{col}/musicResponsiveListItemFlexColumnRenderer/text/runs");
    r.pointer(&pointer)
        .and_then(|v| v.as_array())
        .map(|runs| {
            runs.iter()
                .filter_map(|run| run.get("text")?.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Run `run` text of flex column `col` (`None` when absent).
fn flex_run_text(r: &serde_json::Value, col: usize, run: usize) -> Option<String> {
    let pointer = format!(
        "/flexColumns/{col}/musicResponsiveListItemFlexColumnRenderer/text/runs/{run}/text"
    );
    r.pointer(&pointer)
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// Duration text from `fixedColumns` (`None` when the row has none: some
/// playlists carry the duration in a fourth flex column instead).
fn fixed_text(r: &serde_json::Value) -> Option<String> {
    const BASE: &str = "/fixedColumns/0/musicResponsiveListItemFixedColumnRenderer/text";
    r.pointer(&format!("{BASE}/simpleText"))
        .or_else(|| r.pointer(&format!("{BASE}/runs/0/text")))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// Smallest/largest thumbnails from a raw `thumbnails` array (`None`
/// pair on missing input, mirroring [`pick_thumb_url`] pair behavior).
fn pick_raw_thumbs(thumbs: Option<&serde_json::Value>) -> (Option<String>, Option<String>) {
    let arr = match thumbs.and_then(|v| v.as_array()) {
        Some(a) if !a.is_empty() => a,
        _ => return (None, None),
    };
    let mut by_width: Vec<(u64, &str)> = arr
        .iter()
        .filter_map(|t| {
            let url = t.get("url")?.as_str()?;
            if url.is_empty() {
                return None;
            }
            let width = t.get("width").and_then(|w| w.as_u64()).unwrap_or(0);
            Some((width.max(1), url))
        })
        .collect();
    if by_width.is_empty() {
        return (None, None);
    }
    by_width.sort_by_key(|(w, _)| *w);
    (
        Some(by_width[0].1.to_string()),
        Some(by_width[by_width.len() - 1].1.to_string()),
    )
}

/// Join artist names the way the search API pre-joins them (`, `).
fn join_names<'a>(names: impl Iterator<Item = &'a str>) -> String {
    names.collect::<Vec<_>>().join(", ")
}

/// Parse `"m:ss"` / `"h:mm:ss"` (also bare seconds) to seconds.
/// Empty or malformed input → `0` (never fails: it feeds `duration_secs`).
pub(crate) fn parse_duration_secs(raw: &str) -> u32 {
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

/// Fixture/test helper: parse a saved artist top-songs response (a JSON
/// array of `ArtistSong`, see `fixtures/artist_songs.json`) into our
/// [`Track`]s using the same [`artist_song_to_track`] mapping as the live
/// [`InnertubeBackend::artist_top_songs`] path. Unparseable JSON → empty vec.
pub fn parse_artist_songs_fixture(json: &str) -> Vec<Track> {
    serde_json::from_str::<Vec<ArtistSong>>(json)
        .map(|songs| songs.iter().map(artist_song_to_track).collect())
        .unwrap_or_default()
}

/// Fixture/test helper: parse a saved album tracklist response (a JSON
/// array of `AlbumSong`, see `fixtures/album_songs.json`) with the same
/// [`album_song_to_track`] mapping as [`InnertubeBackend::album_tracklist`].
/// The album context (artist + title) rides alongside because upstream
/// rows carry neither. Unparseable JSON → empty vec.
pub fn parse_album_songs_fixture(json: &str, artist: &str, album: &str) -> Vec<Track> {
    serde_json::from_str::<Vec<AlbumSong>>(json)
        .map(|songs| {
            songs
                .iter()
                .map(|s| album_song_to_track(s, artist, album))
                .collect()
        })
        .unwrap_or_default()
}

/// Split library rows (see
/// [`InnertubeBackend::library_songs_with_status`]) into liked and
/// disliked id sets for sync reconciliation. `Indifferent` rows land in
/// neither set.
pub fn split_library_status(
    rows: &[(Track, ytmapi_rs::common::LikeStatus)],
) -> (Vec<String>, Vec<String>) {
    let mut liked = Vec::new();
    let mut disliked = Vec::new();
    for (t, status) in rows {
        match status {
            ytmapi_rs::common::LikeStatus::Liked => liked.push(t.id.clone()),
            ytmapi_rs::common::LikeStatus::Disliked => disliked.push(t.id.clone()),
            ytmapi_rs::common::LikeStatus::Indifferent => {}
        }
    }
    (liked, disliked)
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
    fn web_context_matches_music_website_shape() {
        // Rolling version tracks today (never a stale pin), and the
        // context carries locale with the user as a sibling, exactly
        // like music.youtube.com sends it.
        let version = web_remix_version();
        assert!(version.starts_with("1.20"), "{version}");
        assert!(version.ends_with(".01.00"), "{version}");
        let ctx = web_context();
        assert_eq!(
            ctx.pointer("/client/clientName").and_then(|v| v.as_str()),
            Some("WEB_REMIX")
        );
        assert_eq!(
            ctx.pointer("/client/hl").and_then(|v| v.as_str()),
            Some("en")
        );
        assert_eq!(
            ctx.pointer("/client/gl").and_then(|v| v.as_str()),
            Some("US")
        );
        assert_eq!(
            ctx.pointer("/client/clientVersion")
                .and_then(|v| v.as_str()),
            Some(version.as_str())
        );
        assert!(ctx.pointer("/client/user").is_none());
        assert_eq!(
            ctx.pointer("/user/lockedSafetyMode")
                .and_then(|v| v.as_bool()),
            Some(false)
        );
    }

    #[test]
    fn play_count_filter_keeps_counts_drops_dates() {
        for raw in [
            "213M plays",
            "1B plays",
            "120K views",
            "55K views",
            "1 play",
            "2725 M reproducciones",
            "1,2 M reproduções",
        ] {
            assert!(is_play_count(raw), "{raw}");
        }
        // Upload dates ride the same column (podcast episodes): they
        // are not counts and must never render as one.
        for raw in ["Nov 12, 2025", "2025", "", "  "] {
            assert!(!is_play_count(raw), "{raw}");
        }
    }

    #[test]
    fn artist_songs_fixture_keeps_order_counts_and_artists() {
        let tracks = parse_artist_songs_fixture(include_str!("../fixtures/artist_songs.json"));
        assert_eq!(tracks.len(), 2);
        // Server rank order survives (Popular is this order, not
        // alphabetical); counts survive; artists join like search.
        assert_eq!(tracks[0].title, "Bicycle Race");
        assert_eq!(tracks[0].artist, "Queen");
        assert_eq!(tracks[0].plays.as_deref(), Some("76M plays"));
        assert_eq!(tracks[0].album.as_deref(), Some("The Game"));
        // No duration or thumbs upstream: rows render the bare count
        // with a placeholder cover.
        assert_eq!(tracks[0].duration_secs, 0);
        assert!(tracks[0].thumbnail_url.is_none());
        // The date riding the plays column is dropped (not a count).
        assert_eq!(tracks[1].plays, None);
    }

    #[test]
    fn album_songs_fixture_keeps_track_order_counts_and_context() {
        let tracks = parse_album_songs_fixture(
            include_str!("../fixtures/album_songs.json"),
            "Queen",
            "Test Album",
        );
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].title, "First Song");
        assert_eq!(tracks[0].artist, "Queen");
        assert_eq!(tracks[0].album.as_deref(), Some("Test Album"));
        assert_eq!(tracks[0].duration_secs, 179);
        assert_eq!(tracks[0].plays.as_deref(), Some("100M plays"));
        assert_eq!(tracks[1].plays.as_deref(), Some("50M plays"));
    }

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
                thumbnail_large_url: None,
                plays: None,
                is_video: false,
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
                thumbnail_large_url: None,
                plays: None,
                is_video: false,
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

    /// Fabricated first-page JSON with the row shapes seen live: a
    /// standard row (duration in `fixedColumns`), a flex-duration row
    /// (no `fixedColumns`, duration in flex column 3), plus rows that
    /// must be skipped (deleted, greyed-out, episode, continuation).
    fn fabricated_playlist_page() -> String {
        // One flex column with a single text run.
        let col = |text: &str| {
            format!(
                r#"{{"musicResponsiveListItemFlexColumnRenderer": {{"text": {{"runs": [{{"text": "{text}"}}]}}}}}}"#
            )
        };
        // One row; empty `fixed`/`policy` strings omit those fields.
        let row = |cols: String, fixed: &str, video_id: &str, policy: &str, video_type: &str| {
            let fixed_json = if fixed.is_empty() {
                String::new()
            } else {
                format!(
                    r#""fixedColumns": [{{"musicResponsiveListItemFixedColumnRenderer": {{"text": {{"simpleText": "{fixed}"}}}}}}],"#
                )
            };
            let policy_json = if policy.is_empty() {
                String::new()
            } else {
                format!(r#""musicItemRendererDisplayPolicy": "{policy}","#)
            };
            format!(
                r#"{{"musicResponsiveListItemRenderer": {{{fixed_json}{policy_json}"flexColumns": [{cols}],"overlay": {{"musicItemThumbnailOverlayRenderer": {{"content": {{"musicPlayButtonRenderer": {{"playNavigationEndpoint": {{"watchEndpoint": {{"videoId": "{video_id}", "videoType": "{video_type}"}}}}}}}}}}}},"thumbnail": {{"musicThumbnailRenderer": {{"thumbnail": {{"thumbnails": [{{"url": "https://example.invalid/s", "width": 60}}, {{"url": "https://example.invalid/l", "width": 544}}]}}}}}},"menu": {{"menuRenderer": {{"items": []}}}}}}}}"#
            )
        };
        let multi_artist = format!(
            "{},{}",
            col("Song A"),
            r#"{"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Alice"}, {"text": ", "}, {"text": "Bob"}]}}}"#
        );
        let rows = [
            row(
                format!("{},{},{}", col("Song A"), col("x"), col("y")),
                "3:20",
                "vid11111111",
                "",
                "MUSIC_VIDEO_TYPE_ATV",
            ),
            row(
                multi_artist,
                "4:05",
                "vid22222222",
                "",
                "MUSIC_VIDEO_TYPE_ATV",
            ),
            row(
                format!(
                    "{},{},{},{}",
                    col("Flex Song"),
                    col("Solo"),
                    col("Some Album"),
                    col("2:30")
                ),
                "",
                "vid33333333",
                "",
                "MUSIC_VIDEO_TYPE_ATV",
            ),
            row(
                col("Song deleted"),
                "1:00",
                "vid44444444",
                "",
                "MUSIC_VIDEO_TYPE_ATV",
            ),
            row(
                format!("{},{}", col("Grey Song"), col("No One")),
                "1:00",
                "vid55555555",
                "MUSIC_ITEM_RENDERER_DISPLAY_POLICY_GREY_OUT",
                "MUSIC_VIDEO_TYPE_ATV",
            ),
            row(
                format!("{},{}", col("Episode One"), col("A Podcast")),
                "30:00",
                "vid66666666",
                "",
                "MUSIC_VIDEO_TYPE_PODCAST_EPISODE",
            ),
            r#"{"continuationItemRenderer": {}}"#.to_string(),
        ];
        format!(
            r#"{{"contents": {{"twoColumnBrowseResultsRenderer": {{"secondaryContents": {{"sectionListRenderer": {{"contents": [{{"musicPlaylistShelfRenderer": {{"contents": [{}]}}}}]}}}}}}}}}}"#,
            rows.join(",")
        )
    }

    #[test]
    fn tolerant_rows_map_both_shapes_and_skip_junk() {
        let tracks = tolerant_playlist_rows(&fabricated_playlist_page());
        assert_eq!(
            tracks.len(),
            3,
            "standard + multi-artist + flex-duration: {tracks:?}"
        );
        assert_eq!(tracks[0].id, "vid11111111");
        assert_eq!(tracks[0].title, "Song A");
        assert_eq!(tracks[0].duration_secs, 200);
        assert_eq!(
            tracks[0].thumbnail_url.as_deref(),
            Some("https://example.invalid/s")
        );
        assert_eq!(
            tracks[0].thumbnail_large_url.as_deref(),
            Some("https://example.invalid/l")
        );
        assert_eq!(tracks[1].artist, "Alice, Bob");
        assert_eq!(tracks[1].duration_secs, 245);
        // Flex-duration row: no fixedColumns, duration in flex column 3.
        assert_eq!(tracks[2].id, "vid33333333");
        assert_eq!(tracks[2].title, "Flex Song");
        assert_eq!(tracks[2].artist, "Solo");
        assert_eq!(tracks[2].album.as_deref(), Some("Some Album"));
        assert_eq!(tracks[2].duration_secs, 150);
    }

    #[test]
    fn tolerant_rows_parse_continuation_pages_and_reject_garbage() {
        let cont = serde_json::json!({
            "onResponseReceivedActions": [{"appendContinuationItemsAction": {"continuationItems": [
                {"musicResponsiveListItemRenderer": {
                    "flexColumns": [{"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Late Song"}]}}}, {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Late Artist"}]}}}],
                    "overlay": {"musicItemThumbnailOverlayRenderer": {"content": {"musicPlayButtonRenderer": {"playNavigationEndpoint": {"watchEndpoint": {"videoId": "vid99999999", "videoType": "MUSIC_VIDEO_TYPE_ATV"}}}}}},
                    "thumbnail": {"musicThumbnailRenderer": {"thumbnail": {"thumbnails": []}}},
                    "menu": {"menuRenderer": {"items": []}},
                }},
            ]}}]
        });
        let tracks = tolerant_playlist_rows(&cont.to_string());
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].id, "vid99999999");
        assert_eq!(tracks[0].duration_secs, 0);
        assert!(tolerant_playlist_rows("not json").is_empty());
        assert!(tolerant_playlist_rows("{}").is_empty());
    }

    #[test]
    fn library_status_split_covers_like_dislike_indifferent() {
        use ytmapi_rs::common::LikeStatus;
        let mut songs: Vec<TableListSong> =
            serde_json::from_str(include_str!("../fixtures/library_songs.json")).unwrap();
        assert!(songs.len() >= 2);
        songs[0].like_status = LikeStatus::Liked;
        songs[1].like_status = LikeStatus::Disliked;
        let rows: Vec<(Track, LikeStatus)> = songs
            .iter()
            .map(|s| (table_song_to_track(s), s.like_status.clone()))
            .collect();
        let (liked, disliked) = split_library_status(&rows);
        assert_eq!(liked, vec![songs[0].video_id.get_raw().to_string()]);
        assert_eq!(disliked, vec![songs[1].video_id.get_raw().to_string()]);
        // Indifferent rows land in neither set.
        songs[1].like_status = LikeStatus::Indifferent;
        let rows: Vec<(Track, LikeStatus)> = songs
            .iter()
            .map(|s| (table_song_to_track(s), s.like_status.clone()))
            .collect();
        let (liked, disliked) = split_library_status(&rows);
        assert_eq!(liked.len(), 1);
        assert!(disliked.is_empty());
    }

    #[test]
    fn leading_count_parses_details_totals() {
        assert_eq!(parse_leading_count("248 songs"), Some(248));
        assert_eq!(parse_leading_count("1,234 videos"), Some(1234));
        assert_eq!(parse_leading_count("  7"), Some(7));
        assert_eq!(parse_leading_count("songs"), None);
        assert_eq!(parse_leading_count(""), None);
    }

    // ---- Write-operation HTTP shapes (offline: query construction only) ----

    #[test]
    fn rate_song_queries_hit_like_endpoints_with_video_target() {
        use ytmapi_rs::common::PlaylistID;
        use ytmapi_rs::common::{LikeStatus, VideoID, YoutubeID as _};
        use ytmapi_rs::query::{PostQuery, RatePlaylistQuery, RateSongQuery};

        let like = RateSongQuery::new(VideoID::from_raw("vid123"), LikeStatus::Liked);
        assert_eq!(like.path(), "like/like");
        assert_eq!(
            like.header()["target"],
            serde_json::json!({"videoId": "vid123"})
        );
        let dislike = RateSongQuery::new(VideoID::from_raw("vid123"), LikeStatus::Disliked);
        assert_eq!(dislike.path(), "like/dislike");
        let clear = RateSongQuery::new(VideoID::from_raw("vid123"), LikeStatus::Indifferent);
        assert_eq!(clear.path(), "like/removelike");

        let pl_like = RatePlaylistQuery::new(PlaylistID::from_raw("PLabc"), LikeStatus::Liked);
        assert_eq!(pl_like.path(), "like/like");
        assert_eq!(
            pl_like.header()["target"],
            serde_json::json!({"playlistId": "PLabc"})
        );
        let pl_dislike =
            RatePlaylistQuery::new(PlaylistID::from_raw("PLabc"), LikeStatus::Disliked);
        assert_eq!(pl_dislike.path(), "like/dislike");
    }

    #[test]
    fn playlist_write_queries_hit_expected_paths_and_ids() {
        use ytmapi_rs::common::{PlaylistID, SetVideoID, VideoID, YoutubeID as _};
        use ytmapi_rs::query::{playlist::PrivacyStatus, AddPlaylistItemsQuery};
        use ytmapi_rs::query::{
            CreatePlaylistQuery, DeletePlaylistQuery, EditPlaylistQuery, PostQuery,
            RemovePlaylistItemsQuery,
        };

        let create = CreatePlaylistQuery::new("Road Trip", None, PrivacyStatus::Private);
        assert_eq!(create.path(), "playlist/create");
        let header = create.header();
        assert_eq!(header["title"], "Road Trip");
        assert_eq!(header["privacyStatus"], "PRIVATE");

        let delete = DeletePlaylistQuery::new(PlaylistID::from_raw("PLabc123"));
        assert_eq!(delete.path(), "playlist/delete");
        assert_eq!(delete.header()["playlistId"], "PLabc123");

        let add = AddPlaylistItemsQuery::new_from_videos(
            PlaylistID::from_raw("PLabc123"),
            [VideoID::from_raw("vid999")],
            Default::default(),
        );
        assert_eq!(add.path(), "browse/edit_playlist");
        assert_eq!(add.header()["playlistId"], "PLabc123");
        let actions = add.header()["actions"].clone();
        let actions = actions.as_array().expect("actions array");
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0]["action"], "ACTION_ADD_VIDEO");
        assert_eq!(actions[0]["addedVideoId"], "vid999");

        let remove = RemovePlaylistItemsQuery::new(
            PlaylistID::from_raw("PLabc123"),
            [SetVideoID::from_raw("set123")],
        );
        assert_eq!(remove.path(), "browse/edit_playlist");
        assert_eq!(remove.header()["playlistId"], "PLabc123");
        let actions = remove.header()["actions"].clone();
        let actions = actions.as_array().expect("remove actions array");
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0]["action"], "ACTION_REMOVE_VIDEO");
        assert_eq!(actions[0]["setVideoId"], "set123");

        let rename = EditPlaylistQuery::new_title(PlaylistID::from_raw("PLabc123"), "New Name");
        assert_eq!(rename.path(), "browse/edit_playlist");
        assert_eq!(rename.header()["playlistId"], "PLabc123");
        let actions = rename.header()["actions"].clone();
        let actions = actions.as_array().expect("rename actions array");
        assert!(
            actions
                .iter()
                .any(|a| a["action"] == "ACTION_SET_PLAYLIST_NAME"
                    && a["playlistName"] == "New Name"),
            "rename must carry ACTION_SET_PLAYLIST_NAME, got {actions:?}"
        );
    }

    #[test]
    fn subscribe_queries_hit_expected_paths() {
        use ytmapi_rs::common::{ArtistChannelID, YoutubeID as _};
        use ytmapi_rs::query::{PostQuery, SubscribeArtistQuery, UnsubscribeArtistsQuery};

        let sub = SubscribeArtistQuery::new(ArtistChannelID::from_raw("UCabc"));
        assert_eq!(sub.path(), "subscription/subscribe");

        let unsub = UnsubscribeArtistsQuery::new([ArtistChannelID::from_raw("UCabc")]);
        assert_eq!(unsub.path(), "subscription/unsubscribe");
    }

    #[test]
    fn set_video_pairs_extract_from_both_page_shapes() {
        let page = serde_json::json!({
            "contents": {"twoColumnBrowseResultsRenderer": {"secondaryContents": {"sectionListRenderer": {"contents": [
                {"musicPlaylistShelfRenderer": {"contents": [
                    {"musicResponsiveListItemRenderer": {
                        "overlay": {"musicItemThumbnailOverlayRenderer": {"content": {"musicPlayButtonRenderer": {"playNavigationEndpoint": {"watchEndpoint": {"videoId": "vidAAA"}}}}}},
                        "playlistItemData": {"playlistSetVideoId": "setAAA", "videoId": "vidAAA"},
                    }},
                    {"musicResponsiveListItemRenderer": {
                        "overlay": {"musicItemThumbnailOverlayRenderer": {"content": {"musicPlayButtonRenderer": {"playNavigationEndpoint": {"watchEndpoint": {"videoId": "vidBBB"}}}}}},
                        "menu": {"menuRenderer": {"items": [{"menuServiceItemRenderer": {"serviceEndpoint": {"playlistEditEndpoint": {"actions": [{"setVideoId": "setBBB"}]}}}}]}},
                    }},
                    {"musicResponsiveListItemRenderer": {
                        "flexColumns": [{"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Song deleted"}]}}}],
                    }},
                ]}}
            ]}}}}
        });
        let pairs = extract_set_video_pairs(&page.to_string());
        assert_eq!(
            pairs,
            vec![
                ("vidAAA".to_string(), "setAAA".to_string()),
                ("vidBBB".to_string(), "setBBB".to_string()),
            ]
        );
        let cont = serde_json::json!({
            "onResponseReceivedActions": [{"appendContinuationItemsAction": {"continuationItems": [
                {"musicResponsiveListItemRenderer": {
                    "overlay": {"musicItemThumbnailOverlayRenderer": {"content": {"musicPlayButtonRenderer": {"playNavigationEndpoint": {"watchEndpoint": {"videoId": "vidCCC"}}}}}},
                    "playlistItemData": {"videoId": "vidCCC", "setVideoId": "setCCC"},
                }},
            ]}}]
        });
        assert_eq!(
            extract_set_video_pairs(&cont.to_string()),
            vec![("vidCCC".to_string(), "setCCC".to_string())]
        );
        assert!(extract_set_video_pairs("not json").is_empty());
        assert!(extract_set_video_pairs("{}").is_empty());
    }

    #[test]
    fn playlist_ids_convert_between_browse_and_edit_forms() {
        use crate::library::{browse_playlist_id, canonical_playlist_id, edit_playlist_id};

        // Library browse returns VL-prefixed ids; storage is edit form.
        assert_eq!(canonical_playlist_id("VLPLabc123"), "PLabc123");
        assert_eq!(canonical_playlist_id("VLLM"), "LM");
        assert_eq!(canonical_playlist_id("PLabc123"), "PLabc123");
        assert_eq!(canonical_playlist_id("LM"), "LM");
        // Reads accept either form.
        assert_eq!(browse_playlist_id("PLabc123"), "VLPLabc123");
        assert_eq!(browse_playlist_id("VLPLabc123"), "VLPLabc123");
        assert_eq!(browse_playlist_id("LM"), "VLLM");
        assert_eq!(browse_playlist_id("VLLM"), "VLLM");
        // Writes accept either form (the reported 400 came from sending
        // a VL browse id to browse/edit_playlist).
        assert_eq!(edit_playlist_id("VLPLabc123"), "PLabc123");
        assert_eq!(edit_playlist_id("PLabc123"), "PLabc123");
        assert_eq!(edit_playlist_id("VLLM"), "LM");
    }

    #[test]
    fn library_playlist_rows_store_canonical_ids() {
        let row: ytmapi_rs::parse::LibraryPlaylist = serde_json::from_value(serde_json::json!({
            "playlist_id": "VLPLserver123",
            "title": "Road Trip",
            "thumbnails": [],
            "tracks": "10 songs",
            "author": "Someone",
            "author_id": null,
        }))
        .unwrap();
        assert_eq!(
            server_playlist_of(&row),
            ServerPlaylist {
                id: "PLserver123".into(),
                title: "Road Trip".into(),
            }
        );
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

    #[test]
    fn library_playlist_rows_map_to_server_ids() {
        let row: ytmapi_rs::parse::LibraryPlaylist = serde_json::from_value(serde_json::json!({
            "playlist_id": "PLserver123",
            "title": "Road Trip",
            "thumbnails": [],
            "tracks": "10 songs",
            "author": "Someone",
            "author_id": null,
        }))
        .unwrap();
        assert_eq!(
            server_playlist_of(&row),
            ServerPlaylist {
                id: "PLserver123".into(),
                title: "Road Trip".into(),
            }
        );
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
    #[ignore = "live network: anonymous full top-songs shelf"]
    async fn live_artist_top_full() {
        let backend = InnertubeBackend::connect_anonymous()
            .await
            .expect("anonymous connect");
        let artists = backend
            .search_artists("Queen")
            .await
            .expect("search artists");
        let queen = artists
            .iter()
            .find(|a| a.name == "Queen")
            .expect("Queen in artist results");
        let top = backend
            .artist_top_full(&queen.id)
            .await
            .expect("full top songs");
        // Full shelf, not the 5-song typed head.
        assert!(top.len() > 5, "expected full shelf, got {}", top.len());
        assert!(
            top.iter().all(|t| !t.id.is_empty() && !t.title.is_empty()),
            "tracks must carry id and title: {top:?}"
        );
        // Playlist rows carry durations + albums; most rows carry counts.
        assert!(top.iter().any(|t| t.duration_secs > 0));
        assert!(top.iter().any(|t| t.album.is_some()));
        let counted = top.iter().filter(|t| t.plays.is_some()).count();
        assert!(counted * 2 >= top.len(), "expected counts on most rows");
    }

    #[tokio::test]
    #[ignore = "live network: anonymous artist top songs with play counts"]
    async fn live_artist_top_songs() {
        let backend = InnertubeBackend::connect_anonymous()
            .await
            .expect("anonymous connect");
        let artists = backend
            .search_artists("Queen")
            .await
            .expect("search artists");
        let queen = artists
            .iter()
            .find(|a| a.name == "Queen")
            .expect("Queen in artist results");
        let top = backend
            .artist_top_songs(&queen.id)
            .await
            .expect("top songs");
        assert!(!top.is_empty(), "expected top songs for Queen");
        assert!(
            top.iter().all(|t| !t.id.is_empty() && !t.title.is_empty()),
            "tracks must carry id and title: {top:?}"
        );
        assert!(
            top.iter().any(|t| t.plays.is_some()),
            "expected some play counts: {top:?}"
        );
    }

    #[tokio::test]
    #[ignore = "live network: anonymous album tracklist with play counts"]
    async fn live_album_tracklist() {
        let backend = InnertubeBackend::connect_anonymous()
            .await
            .expect("anonymous connect");
        let albums = backend
            .search_albums("A Night at the Opera Queen")
            .await
            .expect("search albums");
        let album = albums
            .iter()
            .find(|a| a.title == "A Night At The Opera" || a.title == "A Night at the Opera")
            .expect("album in results");
        let tracks = backend.album_tracklist(&album.id).await.expect("tracklist");
        assert!(!tracks.is_empty(), "expected album tracks");
        assert!(
            tracks
                .iter()
                .all(|t| !t.id.is_empty() && !t.title.is_empty()),
            "tracks must carry id and title: {tracks:?}"
        );
        assert!(
            tracks.iter().any(|t| t.plays.is_some()),
            "expected some play counts: {tracks:?}"
        );
    }

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

    #[tokio::test]
    #[ignore = "live network: anonymous raw unfiltered search"]
    async fn live_search_all() {
        let all = crate::raw_search::raw_search_all(
            "Beatles",
            &crate::raw_search::SearchAuth::anonymous(),
        )
        .await
        .expect("search all");
        assert!(
            !all.tracks.is_empty(),
            "expected non-empty track results for 'Beatles', got none"
        );
        assert!(
            all.tracks
                .iter()
                .all(|t| !t.id.is_empty() && !t.title.is_empty()),
            "tracks must carry id and title: {:?}",
            all.tracks
        );
        assert!(!all.mixed.is_empty(), "expected web mixed order");
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
    #[ignore = "live network: big playlist streams past the 100-item first page"]
    async fn live_playlist_tracks_stream_all_pages() {
        let Ok(path) = std::env::var("YUTU_TEST_COOKIE_FILE") else {
            eprintln!("SKIP: set YUTU_TEST_COOKIE_FILE to a browser cookie file");
            return;
        };
        let Ok(playlist_id) = std::env::var("YUTU_TEST_BIG_PLAYLIST_ID") else {
            eprintln!("SKIP: set YUTU_TEST_BIG_PLAYLIST_ID to a 100+ song playlist id");
            return;
        };
        let backend = InnertubeBackend::connect_with_cookie_file(Path::new(&path))
            .await
            .expect("cookie connect");
        let mut calls = Vec::new();
        let tracks = backend
            .playlist_tracks_live(&playlist_id, |n| calls.push(n))
            .await
            .expect("tracks");
        assert!(
            tracks.len() > 100,
            "expected more than one page of tracks, got {}",
            tracks.len()
        );
        // Progress hook: per-page cumulative counts ending at the total.
        assert!(calls.len() >= 2, "expected page ticks, got {calls:?}");
        assert!(
            calls.windows(2).all(|w| w[0] < w[1]),
            "ticks must grow, got {calls:?}"
        );
        assert_eq!(*calls.last().unwrap(), tracks.len());
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
