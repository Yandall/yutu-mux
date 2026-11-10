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
    pub(crate) inner: Inner,
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
pub(crate) enum Inner {
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
pub(crate) fn server_playlist_of(list: &ytmapi_rs::parse::LibraryPlaylist) -> ServerPlaylist {
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

    /// YouTube Music lyrics for `video_id` as plain text lines.
    ///
    /// Two-step upstream flow: `get_lyrics_id` (watch `next` endpoint) then
    /// `get_lyrics` (browse the lyrics id). Both queries are
    /// `impl<A: AuthToken>`, so this is attempted on all backends; in
    /// practice lyric availability is per-track server-side (many tracks have
    /// none: that surfaces as an API error here). For synced lines see
    /// [`Self::song_timed_lyrics`].
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

    /// YouTube Music synced lyrics for `video_id` as timestamped lines.
    ///
    /// Same lyrics `browseId` as [`Self::song_lyrics`] (via the typed
    /// `get_lyrics_id` flow, so the id lookup stays authenticated), then
    /// a raw ANDROID_MUSIC `browse`: only mobile clients are served the
    /// `timedLyricsModel`, while WEB_REMIX gets the plain description
    /// shelf. Empty when the server has no timed model for the track:
    /// callers fall back to the plain [`Self::song_lyrics`] lines.
    pub async fn song_timed_lyrics(
        &self,
        video_id: &str,
    ) -> Result<Vec<crate::lyrics_providers::LyricLine>, String> {
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
        crate::raw_lyrics::raw_timed_lyrics(lyrics_id.get_raw()).await
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
        if !crate::security::is_valid_video_id(&req.video_id) {
            return Err(StreamError::NotFound {
                video_id: crate::security::truncate_for_log(&req.video_id, 64),
            });
        }
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
        let url =
            extract_audio_url(&value).ok_or_else(|| classify_no_url(&value, client, video_id))?;
        if !crate::security::is_allowed_stream_url(&url) {
            return Err(StreamError::Network(
                "player returned untrusted URL".to_string(),
            ));
        }
        Ok(url)
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
                                    if !crate::security::is_allowed_stream_url(&url) {
                                        last_err = StreamError::Network(
                                            "player returned untrusted URL".to_string(),
                                        );
                                        crate::app_log::warn(
                                            "yutu-stream",
                                            format_args!("token #{i} untrusted URL blocked"),
                                        );
                                    } else {
                                        crate::app_log::info(
                                            "yutu-stream",
                                            format_args!("token #{i} yielded URL"),
                                        );
                                        return Ok(ResolvedStream {
                                            url,
                                            pot_token: minted.tokens.get(1).cloned(),
                                        });
                                    }
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
pub(crate) fn collect_renderers<'v>(
    value: &'v serde_json::Value,
    out: &mut Vec<&'v serde_json::Value>,
) {
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
pub(crate) fn deep_find_first_string(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
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
pub(crate) fn parse_leading_count(text: &str) -> Option<usize> {
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

/// Android music client context: the lyrics `browse` endpoint only
/// serves the `timedLyricsModel` (line timestamps) to mobile clients.
/// WEB_REMIX gets the plain description shelf instead, which is why the
/// typed `ytmapi-rs` flow is plain-text only. Version pinned to a
/// release known to serve timed lyrics (InnerTube tolerates stale
/// mobile versions on browse).
pub(crate) fn android_music_context() -> serde_json::Value {
    serde_json::json!({
        "client": {
            "clientName": "ANDROID_MUSIC",
            "clientVersion": "7.21.50",
            "hl": "en",
            "gl": "US",
        },
        "user": { "lockedSafetyMode": false },
    })
}

/// Build the raw `player` body. `po_token = None` reproduces the plain probe;
/// `Some` attaches `serviceIntegrityDimensions` per the BotGuard contract.
pub(crate) fn player_request_body(video_id: &str, po_token: Option<&str>) -> serde_json::Value {
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
pub(crate) fn signature_timestamp_days() -> u64 {
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
pub(crate) fn classify_no_url(
    value: &serde_json::Value,
    client: StreamClient,
    video_id: &str,
) -> StreamError {
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

pub(crate) fn first_audio_url(formats: Option<&serde_json::Value>) -> Option<String> {
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
pub(crate) fn pick_thumb_url(thumbs: &[Thumbnail]) -> Option<String> {
    thumbs
        .iter()
        .min_by_key(|t| t.width.max(1))
        .map(|t| t.url.clone())
        .filter(|u| !u.is_empty())
}

/// Largest artwork thumbnail (hero views like the Now Playing drawer).
pub(crate) fn pick_large_thumb_url(thumbs: &[Thumbnail]) -> Option<String> {
    thumbs
        .iter()
        .max_by_key(|t| t.width)
        .map(|t| t.url.clone())
        .filter(|u| !u.is_empty())
}

pub(crate) fn song_to_track(song: &SearchResultSong) -> Track {
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
pub(crate) fn album_to_album(album: &SearchResultAlbum) -> Album {
    Album {
        id: album.album_id.get_raw().to_string(),
        title: album.title.clone(),
        artist: album.artist.clone(),
        year: album.year.trim().parse().ok(),
        thumbnail_url: pick_thumb_url(&album.thumbnails),
    }
}

/// Map one `search_artists` hit to our [`Artist`] (`id` = channel `browseId`).
pub(crate) fn artist_to_artist(artist: &SearchResultArtist) -> Artist {
    Artist {
        id: artist.browse_id.get_raw().to_string(),
        name: artist.artist.clone(),
        thumbnail_url: pick_thumb_url(&artist.thumbnails),
    }
}

/// Map one community playlist hit to our [`PlaylistHit`] (featured and
/// community variants; podcasts are `None` and skipped by the caller).
pub(crate) fn community_playlist_to_hit(list: &SearchResultPlaylist) -> Option<PlaylistHit> {
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
pub(crate) fn featured_to_hit(f: &SearchResultFeaturedPlaylist) -> PlaylistHit {
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
pub(crate) fn artist_song_to_track(song: &ArtistSong) -> Track {
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
pub(crate) fn album_song_to_track(song: &AlbumSong, artist: &str, album: &str) -> Track {
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
pub(crate) fn table_song_to_track(song: &TableListSong) -> Track {
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
pub(crate) fn playlist_item_to_track(item: &PlaylistItem) -> Option<Track> {
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
pub(crate) fn watch_track_to_track(track: &WatchPlaylistTrack) -> Track {
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
pub(crate) fn tolerant_playlist_rows(raw: &str) -> Vec<Track> {
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
pub(crate) fn tolerant_row_to_track(item: &serde_json::Value) -> Option<Track> {
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
pub(crate) fn flex_runs(r: &serde_json::Value, col: usize) -> Vec<String> {
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
pub(crate) fn flex_run_text(r: &serde_json::Value, col: usize, run: usize) -> Option<String> {
    let pointer = format!(
        "/flexColumns/{col}/musicResponsiveListItemFlexColumnRenderer/text/runs/{run}/text"
    );
    r.pointer(&pointer)
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// Duration text from `fixedColumns` (`None` when the row has none: some
/// playlists carry the duration in a fourth flex column instead).
pub(crate) fn fixed_text(r: &serde_json::Value) -> Option<String> {
    const BASE: &str = "/fixedColumns/0/musicResponsiveListItemFixedColumnRenderer/text";
    r.pointer(&format!("{BASE}/simpleText"))
        .or_else(|| r.pointer(&format!("{BASE}/runs/0/text")))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// Smallest/largest thumbnails from a raw `thumbnails` array (`None`
/// pair on missing input, mirroring [`pick_thumb_url`] pair behavior).
pub(crate) fn pick_raw_thumbs(
    thumbs: Option<&serde_json::Value>,
) -> (Option<String>, Option<String>) {
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
pub(crate) fn join_names<'a>(names: impl Iterator<Item = &'a str>) -> String {
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
