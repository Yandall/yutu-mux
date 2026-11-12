//! yutu-ui tasks: async task constructors and sync helpers (split from lib.rs, no behavior change).

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use pointing_slider::pointing_slider;
#[allow(unused_imports)]
use row_button::row_button;
#[allow(unused_imports)]
use std::collections::{HashMap, HashSet};
#[allow(unused_imports)]
use yutu_core::{
    forgotten_favorites, history_section, should_scrobble, weekly_mix, Album, Artist, HistoryEntry,
    HistorySection, LibraryStore, LyricsProvider, LyricsResult, Playlist, PlaylistHit,
    QueueSnapshot, SearchAll, SearchResults, TopKind, Track,
};
#[cfg(feature = "mpv")]
#[allow(unused_imports)]
use yutu_player::MpvBackend;
#[allow(unused_imports)]
use yutu_player::{EqPreset, Queue, RodioBackend, BAND_COUNT, MAX_GAIN_DB, MIN_GAIN_DB};

impl AppState {
    /// Debounced keystroke: wait 200ms for the user to keep typing, then
    /// fire (suggest path waits 150ms); repeats are cache-served. Only
    /// the latest generation fires a real search.
    #[cfg(feature = "innertube")]
    pub(crate) fn search_debounce_task(gen: u64, query: String) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                (gen, query)
            },
            |(gen, query)| Message::SearchDebounced { gen, query },
        )
    }

    /// Real YouTube Music search (needs the `innertube` feature): one
    /// anonymous connection, three category queries run in sequence.
    /// Partial success is kept: only a total failure errors.
    #[cfg(feature = "innertube")]
    pub(crate) fn search_task(query: String) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let result = live_search(&query).await;
                (query, result)
            },
            |(query, result)| Message::SearchCompleted { query, result },
        )
    }

    /// Live artist top songs for a channel id (typed 5-song head
    /// with counts): paints the Popular head fast. The full ranked
    /// shelf loads lazily on expand ([`Self::artist_full_task`]).
    /// Anonymous fallback when signed out (artist browse needs no
    /// login).
    #[cfg(feature = "innertube")]
    pub(crate) fn artist_top_task(channel_id: String) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let result = async {
                    let backend = Self::detail_backend().await?;
                    backend.artist_top_songs(&channel_id).await
                }
                .await;
                (channel_id, result)
            },
            |(channel_id, result)| Message::ArtistTopLoaded { channel_id, result },
        )
    }

    /// Full ranked top-songs shelf for an expanded artist page:
    /// runs only on "Show more" (one connect + typed shelf lookup +
    /// raw rows, with the 5-song head as fallback).
    #[cfg(feature = "innertube")]
    pub(crate) fn artist_full_task(channel_id: String) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let result = async {
                    let backend = Self::detail_backend().await?;
                    backend.artist_top_full(&channel_id).await
                }
                .await;
                (channel_id, result)
            },
            |(channel_id, result)| Message::ArtistFullLoaded { channel_id, result },
        )
    }

    /// Live album tracklist for an album browse id (`track_no` order
    /// + counts). Same anonymous fallback as [`Self::artist_top_task`].
    #[cfg(feature = "innertube")]
    pub(crate) fn album_live_task(album_id: String) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let result = async {
                    let backend = Self::detail_backend().await?;
                    backend.album_tracklist(&album_id).await
                }
                .await;
                (album_id, result)
            },
            |(album_id, result)| Message::AlbumLiveLoaded { album_id, result },
        )
    }

    /// Bare artist name → channel id through one scoped filtered
    /// search (anonymous: exact matches need no personalization).
    #[cfg(feature = "innertube")]
    pub(crate) fn artist_resolve_task(route_id: String, name: String) -> iced::Task<Message> {
        #[allow(unused_imports)]
        use yutu_core::{raw_search_filtered, SearchAuth, SearchFilterKind};
        iced::Task::perform(
            async move {
                let result = async {
                    let bundle = raw_search_filtered(
                        &name,
                        &SearchAuth::anonymous(),
                        SearchFilterKind::Artists,
                    )
                    .await?;
                    Self::pick_artist_channel(&bundle.artists, &name)
                        .ok_or_else(|| format!("no artist named {name}"))
                }
                .await;
                (route_id, name, result)
            },
            |(route_id, name, result)| Message::ArtistChannelResolved {
                route_id,
                name,
                result,
            },
        )
    }

    /// Bare album title → browse id through a scoped album search
    /// (exact title, owner-disambiguated): unblocks title routes with
    /// no known browse id.
    #[cfg(feature = "innertube")]
    pub(crate) fn album_resolve_task(
        route_id: String,
        title: String,
        owner: Option<String>,
    ) -> iced::Task<Message> {
        #[allow(unused_imports)]
        use yutu_core::{raw_search_filtered, SearchAuth, SearchFilterKind};
        iced::Task::perform(
            async move {
                let result = async {
                    let bundle = raw_search_filtered(
                        &title,
                        &SearchAuth::anonymous(),
                        SearchFilterKind::Albums,
                    )
                    .await?;
                    Self::pick_album_browse(&bundle.albums, &title, owner.as_deref())
                        .ok_or_else(|| format!("no album titled {title}"))
                }
                .await;
                (route_id, title, result)
            },
            |(route_id, title, result)| Message::AlbumBrowseResolved {
                route_id,
                title,
                result,
            },
        )
    }

    /// Resolve a track to a stream URL fast (yt-dlp only, no download).
    /// mpv plays the URL progressively; the rodio path uses `resolve_task`
    /// (full bytes) instead. A hung yt-dlp cannot wedge the loader: the
    /// timeout error flows through the normal retry path.
    pub(crate) fn resolve_url_task(track_id: String) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let result = match tokio::time::timeout(
                    std::time::Duration::from_secs(RESOLVE_TIMEOUT_SECS),
                    resolve_stream_url(&track_id),
                )
                .await
                {
                    Ok(r) => r,
                    Err(_) => Err(format!(
                        "stream resolve timed out after {RESOLVE_TIMEOUT_SECS}s"
                    )),
                };
                (track_id, result)
            },
            |(track_id, result)| Message::UrlResolved { track_id, result },
        )
    }

    /// Lyrics fetch for one catalog track from one provider: LRCLIB
    /// retries up to 3 attempts with search fallback, so a transient
    /// error never loses the chance to show lyrics; YouTube Music
    /// prefers its timed model (synced) and falls back to plain lines
    /// when the track has no timed model. Callers spawn one task per
    /// provider up front (see [`Self::start_lyrics_fetch`]) and the view
    /// displays whichever result is cached, so provider switches never
    /// refetch mid-song.
    #[cfg(any(feature = "lyrics", feature = "innertube"))]
    pub(crate) fn lyrics_task(track: Track, provider: LyricsProvider) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let result = match provider {
                    LyricsProvider::Lrclib => {
                        #[cfg(feature = "lyrics")]
                        {
                            yutu_core::fetch_lrclib(
                                &track.artist,
                                &track.title,
                                track.album.as_deref(),
                                track.duration_secs,
                            )
                            .await
                        }
                        #[cfg(not(feature = "lyrics"))]
                        {
                            Err("lyrics unavailable".to_string())
                        }
                    }
                    LyricsProvider::YouTubeMusic => {
                        #[cfg(feature = "innertube")]
                        {
                            match Self::detail_backend().await {
                                Ok(backend) => {
                                    // Timed model first (synced lines);
                                    // plain description shelf when the
                                    // track has no timed model.
                                    match backend.song_timed_lyrics(&track.id).await {
                                        Ok(lines) => match yutu_core::ytm_timed_to_result(lines) {
                                            ok @ Ok(_) => ok,
                                            Err(_) => match backend.song_lyrics(&track.id).await {
                                                Ok(plain) => yutu_core::ytm_lyrics_to_result(plain),
                                                Err(e) => Err(e),
                                            },
                                        },
                                        Err(timed_err) => {
                                            match backend.song_lyrics(&track.id).await {
                                                Ok(plain) => yutu_core::ytm_lyrics_to_result(plain),
                                                Err(_) => Err(timed_err),
                                            }
                                        }
                                    }
                                }
                                Err(e) => Err(e),
                            }
                        }
                        #[cfg(not(feature = "innertube"))]
                        {
                            Err("lyrics unavailable".to_string())
                        }
                    }
                };
                (track.id, provider, result)
            },
            |(track_id, provider, result)| Message::LyricsReady {
                track_id,
                provider,
                result,
            },
        )
    }

    /// Background prefetch of the next song's URL (never auto-plays).
    /// Same timeout as resolves: a hung prefetch must neither wedge
    /// the loader nor poison the slot (failures simply miss it).
    pub(crate) fn prefetch_task(track_id: String) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let result = match tokio::time::timeout(
                    std::time::Duration::from_secs(RESOLVE_TIMEOUT_SECS),
                    resolve_stream_url(&track_id),
                )
                .await
                {
                    Ok(r) => r,
                    Err(_) => Err(format!(
                        "stream resolve timed out after {RESOLVE_TIMEOUT_SECS}s"
                    )),
                };
                (track_id, result)
            },
            |(track_id, result)| Message::UrlPrefetched { track_id, result },
        )
    }

    /// Google device-flow: request the code pair (network I/O).
    #[cfg(feature = "innertube")]
    pub(crate) fn google_begin_task() -> iced::Task<Message> {
        iced::Task::perform(
            async move { yutu_core::begin_google_login().await },
            |result| Message::GoogleLoginBegan { result },
        )
    }

    /// Google device-flow: one approval poll (caller retries on pending).
    #[cfg(feature = "innertube")]
    pub(crate) fn google_poll_task(login: yutu_core::GoogleLogin) -> iced::Task<Message> {
        iced::Task::perform(
            async move { yutu_core::poll_google_login(&login).await },
            |result| Message::GoogleLoginPolled { result },
        )
    }

    /// Signed-in library import, base stage: songs + liked + playlist
    /// headers over OAuth. Playlist tracks stream in next like cookies.
    #[cfg(feature = "innertube")]
    pub(crate) fn library_import_task(library_path: String) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let persist =
                    yutu_core::OAuthPersist::load_from_file(std::path::Path::new(&oauth_path()))?;
                let backend = yutu_core::InnertubeBackend::connect_with_oauth(&persist)?;
                let base = Self::fetch_base(&backend).await?;
                if base.tracks.is_empty() && base.headers.is_empty() {
                    return Err("library is empty: like songs or make a playlist on music.youtube.com, then import again".to_string());
                }
                Self::persist_base_pull(&library_path, &base);
                Ok(Self::staged_of(backend, base))
            },
            |result| Message::ImportStageReady {
                kind: ImportKind::Library,
                result,
            },
        )
    }

    /// Disk half of a base pull (background): track cache plus the
    /// server-wins like/dislike replace, so restarts keep the base even
    /// if playlist stages never finish.
    #[cfg(feature = "innertube")]
    pub(crate) fn persist_base_pull(library_path: &str, base: &BasePull) {
        Self::persist_imported(library_path, &base.tracks, &[], &[]);
        Self::apply_sync_ratings(library_path, base);
    }

    /// Server-wins replace of the stored like/dislike sets after a base
    /// pull (background): still-pending rates ride over the server sets
    /// so unpushed intent survives. Skipped when the library fetch
    /// failed open (an empty pull must never wipe local state).
    #[cfg(feature = "innertube")]
    pub(crate) fn apply_sync_ratings(library_path: &str, base: &BasePull) {
        if !base.library_ok {
            return;
        }
        let Ok(store) = LibraryStore::open(library_path) else {
            return;
        };
        let _ = store.apply_sync_pull(&base.liked_ids, &base.disliked_ids, true, &[]);
    }

    /// [`StagedImport`] from a fetched [`BasePull`] (moves every field).
    #[cfg(feature = "innertube")]
    pub(crate) fn staged_of(backend: yutu_core::InnertubeBackend, base: BasePull) -> StagedImport {
        StagedImport {
            backend,
            tracks: base.tracks,
            liked_ids: base.liked_ids,
            disliked_ids: base.disliked_ids,
            headers: base.headers,
            headers_clean: base.headers_clean,
            library_ok: base.library_ok,
        }
    }

    /// Any saved login: OAuth token first, else the cookie session.
    /// Powers best-effort server sync (likes, playlist writes) without
    /// caring which flow signed the user in. A broken OAuth file falls
    /// back to a valid cookie session instead of failing outright.
    #[cfg(feature = "innertube")]
    pub(crate) async fn session_backend() -> Result<yutu_core::InnertubeBackend, String> {
        if std::path::Path::new(&oauth_path()).exists() {
            match yutu_core::OAuthPersist::load_from_file(std::path::Path::new(&oauth_path())) {
                Ok(persist) => {
                    return yutu_core::InnertubeBackend::connect_with_oauth(&persist);
                }
                Err(e) => {
                    yutu_core::app_log::warn(
                        "yutu-login",
                        format_args!("saved OAuth token unusable ({e}): trying cookie session"),
                    );
                }
            }
        }
        if cookie_session_exists() {
            return Self::connect_saved_cookie().await;
        }
        Err("sign in first (cookies or Google)".into())
    }

    /// Backend for detail fetches (artist top songs, album tracklists):
    /// saved login first, anonymous fallback (both endpoints are
    /// `AuthToken`, so signed-out users still get live lists).
    #[cfg(feature = "innertube")]
    pub(crate) async fn detail_backend() -> Result<yutu_core::InnertubeBackend, String> {
        match Self::session_backend().await {
            Ok(backend) => Ok(backend),
            Err(_) => yutu_core::InnertubeBackend::connect_anonymous()
                .await
                .map_err(|e| format!("connect failed: {e}")),
        }
    }

    /// Auth headers for raw search, mirroring [`Self::session_backend`]
    /// precedence (OAuth file, else cookie file, else anonymous). The
    /// server personalizes ranking for signed-in sessions, like the
    /// logged-in web client.
    #[cfg(feature = "innertube")]
    pub(crate) fn session_search_auth() -> yutu_core::SearchAuth {
        use yutu_core::SearchAuth;
        if std::path::Path::new(&oauth_path()).exists() {
            if let Ok(persist) =
                yutu_core::OAuthPersist::load_from_file(std::path::Path::new(&oauth_path()))
            {
                if let Some(auth) = SearchAuth::from_oauth(&persist) {
                    return auth;
                }
            }
        }
        if cookie_session_exists() {
            if let Ok(header) = load_cookie_header() {
                if let Ok(auth) = yutu_core::CookieAuth::from_header_str(&header) {
                    return SearchAuth::from_cookie(&auth);
                }
            }
        }
        SearchAuth::anonymous()
    }

    /// True when a like/playlist write can attempt a server sync.
    #[cfg(feature = "innertube")]
    pub(crate) fn has_session() -> bool {
        std::path::Path::new(&oauth_path()).exists() || cookie_session_exists()
    }

    /// Import base read: library songs + liked ids + playlist headers
    /// (tracks stream in next, one task per list). Each list fails open
    /// (empty vec), so one bad endpoint cannot sink the import; callers
    /// reject a fully-empty result with an account-specific hint.
    #[cfg(feature = "innertube")]
    pub(crate) async fn fetch_base(
        backend: &yutu_core::InnertubeBackend,
    ) -> Result<BasePull, String> {
        let (lib, status_liked, status_disliked, library_ok) =
            match backend.library_songs_with_status().await {
                Ok(rows) => {
                    let mut lib = Vec::with_capacity(rows.len());
                    for (t, _) in &rows {
                        lib.push(t.clone());
                    }
                    let (liked, disliked) = yutu_core::split_library_status(&rows);
                    (lib, liked, disliked, true)
                }
                Err(_) => (Vec::new(), Vec::new(), Vec::new(), false),
            };
        // Liked playlist union: either source may miss rows the other has.
        let liked_songs = backend.liked_songs().await.unwrap_or_default();
        let mut liked_ids = status_liked;
        for t in &liked_songs {
            if !liked_ids.iter().any(|x| x == &t.id) {
                liked_ids.push(t.id.clone());
            }
        }
        let mut all = lib;
        for t in liked_songs {
            if !all.iter().any(|x| x.id == t.id) {
                all.push(t);
            }
        }
        let (headers, failed) = match backend.library_playlists_report().await {
            Ok((h, f)) => (h, f),
            Err(_) => (Vec::new(), 1),
        };
        Ok(BasePull {
            tracks: all,
            liked_ids,
            disliked_ids: status_disliked,
            headers,
            headers_clean: failed == 0,
            library_ok,
        })
    }

    /// Push one like/unlike to YouTube Music (best effort: the local
    /// state is already applied, so only failures report back).
    /// Sends `like/like` vs `like/removelike` with a `videoId` target.
    #[cfg(feature = "innertube")]
    pub(crate) fn like_sync_task(track_id: String, liked: bool) -> iced::Task<Message> {
        if !Self::has_session() {
            return iced::Task::none();
        }
        iced::Task::perform(
            async move {
                let result = match Self::session_backend().await {
                    Ok(b) => b.rate_like(&track_id, liked).await,
                    Err(e) => Err(e),
                };
                (track_id, liked, result)
            },
            |(track_id, liked, result)| Message::LikeSynced {
                track_id,
                liked,
                result,
            },
        )
    }

    /// Push one dislike/clear ("don't recommend") to YouTube Music
    /// (best effort, same local-first contract as [`Self::like_sync_task`]).
    /// Sends `like/dislike` vs `like/removelike` with a `videoId` target.
    #[cfg(feature = "innertube")]
    pub(crate) fn dislike_sync_task(track_id: String, disliked: bool) -> iced::Task<Message> {
        if !Self::has_session() {
            return iced::Task::none();
        }
        iced::Task::perform(
            async move {
                let result = match Self::session_backend().await {
                    Ok(b) => b.rate_dislike(&track_id, disliked).await,
                    Err(e) => Err(e),
                };
                (track_id, disliked, result)
            },
            |(track_id, disliked, result)| Message::DislikeSynced {
                track_id,
                disliked,
                result,
            },
        )
    }

    /// Create the playlist on YouTube Music (best effort): success
    /// migrates the local `u{seq}` shell to the real server id.
    #[cfg(feature = "innertube")]
    pub(crate) fn playlist_create_sync_task(
        local_id: String,
        title: String,
    ) -> iced::Task<Message> {
        if !Self::has_session() {
            return iced::Task::none();
        }
        iced::Task::perform(
            async move {
                let backend = Self::session_backend().await;
                let result = match backend {
                    Ok(b) => b.create_server_playlist(&title).await,
                    Err(e) => Err(e),
                };
                (local_id, title, result)
            },
            |(local_id, title, result)| Message::ServerPlaylistCreated {
                local_id,
                title,
                result,
            },
        )
    }

    /// Append one track to a server playlist (best effort).
    #[cfg(feature = "innertube")]
    pub(crate) fn playlist_add_sync_task(
        playlist_id: String,
        track_id: String,
    ) -> iced::Task<Message> {
        if !Self::is_server_playlist(&playlist_id) || !Self::has_session() {
            return iced::Task::none();
        }
        iced::Task::perform(
            async move {
                let result = match Self::session_backend().await {
                    Ok(b) => b.add_to_server_playlist(&playlist_id, &track_id).await,
                    Err(e) => Err(e),
                };
                (playlist_id, track_id, result)
            },
            |(playlist_id, track_id, result)| Message::ServerPlaylistSynced {
                detail: format!("YouTube add of {track_id} to {playlist_id} failed"),
                op: PlaylistSyncOp::Add {
                    playlist_id: playlist_id.clone(),
                    track_id: track_id.clone(),
                },
                result,
            },
        )
    }

    /// Delete a server playlist (best effort).
    /// Sends `playlist/delete` with the server `playlistId`.
    #[cfg(feature = "innertube")]
    pub(crate) fn playlist_delete_sync_task(playlist_id: String) -> iced::Task<Message> {
        if !Self::is_server_playlist(&playlist_id) || !Self::has_session() {
            return iced::Task::none();
        }
        iced::Task::perform(
            async move {
                let result = match Self::session_backend().await {
                    Ok(b) => b.delete_server_playlist(&playlist_id).await,
                    Err(e) => Err(e),
                };
                (playlist_id, result)
            },
            |(playlist_id, result)| Message::ServerPlaylistSynced {
                detail: format!("YouTube delete of {playlist_id} failed"),
                op: PlaylistSyncOp::Delete {
                    playlist_id: playlist_id.clone(),
                },
                result,
            },
        )
    }

    /// Remove one track from a server playlist (best effort).
    /// Sends `browse/edit_playlist` with `ACTION_REMOVE_VIDEO` after
    /// resolving the per-membership `setVideoId` from raw browse JSON.
    #[cfg(feature = "innertube")]
    pub(crate) fn playlist_remove_sync_task(
        playlist_id: String,
        track_id: String,
    ) -> iced::Task<Message> {
        if !Self::is_server_playlist(&playlist_id) || !Self::has_session() {
            return iced::Task::none();
        }
        iced::Task::perform(
            async move {
                let result = match Self::session_backend().await {
                    Ok(b) => b.remove_from_server_playlist(&playlist_id, &track_id).await,
                    Err(e) => Err(e),
                };
                (playlist_id, track_id, result)
            },
            |(playlist_id, track_id, result)| Message::ServerPlaylistSynced {
                detail: format!("YouTube remove of {track_id} from {playlist_id} failed"),
                op: PlaylistSyncOp::Remove {
                    playlist_id: playlist_id.clone(),
                    track_id: track_id.clone(),
                },
                result,
            },
        )
    }

    /// Rename a server playlist (best effort).
    /// Sends `browse/edit_playlist` with `ACTION_SET_PLAYLIST_NAME`.
    #[cfg(feature = "innertube")]
    pub(crate) fn playlist_rename_sync_task(
        playlist_id: String,
        title: String,
    ) -> iced::Task<Message> {
        if !Self::is_server_playlist(&playlist_id) || !Self::has_session() {
            return iced::Task::none();
        }
        iced::Task::perform(
            async move {
                let result = match Self::session_backend().await {
                    Ok(b) => b.rename_server_playlist(&playlist_id, &title).await,
                    Err(e) => Err(e),
                };
                (playlist_id, title, result)
            },
            |(playlist_id, title, result)| Message::ServerPlaylistSynced {
                detail: format!("YouTube rename of {playlist_id} failed"),
                op: PlaylistSyncOp::Rename {
                    playlist_id: playlist_id.clone(),
                    title: title.clone(),
                },
                result,
            },
        )
    }

    /// Offline builds: server sync is a no-op (local state is the truth).
    #[cfg(not(feature = "innertube"))]
    pub(crate) fn like_sync_task(_track_id: String, _liked: bool) -> iced::Task<Message> {
        iced::Task::none()
    }

    /// Offline builds: server sync is a no-op (local state is the truth).
    #[cfg(not(feature = "innertube"))]
    pub(crate) fn dislike_sync_task(_track_id: String, _disliked: bool) -> iced::Task<Message> {
        iced::Task::none()
    }

    /// Offline builds: server sync is a no-op (local state is the truth).
    #[cfg(not(feature = "innertube"))]
    pub(crate) fn playlist_create_sync_task(
        _local_id: String,
        _title: String,
    ) -> iced::Task<Message> {
        iced::Task::none()
    }

    /// Offline builds: server sync is a no-op (local state is the truth).
    #[cfg(not(feature = "innertube"))]
    pub(crate) fn playlist_add_sync_task(
        _playlist_id: String,
        _track_id: String,
    ) -> iced::Task<Message> {
        iced::Task::none()
    }

    /// Offline builds: server sync is a no-op (local state is the truth).
    #[cfg(not(feature = "innertube"))]
    pub(crate) fn playlist_delete_sync_task(_playlist_id: String) -> iced::Task<Message> {
        iced::Task::none()
    }

    /// Offline builds: server sync is a no-op (local state is the truth).
    #[cfg(not(feature = "innertube"))]
    pub(crate) fn playlist_remove_sync_task(
        _playlist_id: String,
        _track_id: String,
    ) -> iced::Task<Message> {
        iced::Task::none()
    }

    /// Offline builds: no HTTP client, nothing to probe.
    #[cfg(not(feature = "update"))]
    pub(crate) fn update_check_task(_current: String) -> iced::Task<Message> {
        iced::Task::none()
    }

    /// Self-update probe (best effort): latest GitHub release tag vs
    /// this build. Silent unless newer (see the completion handler).
    #[cfg(feature = "update")]
    pub(crate) fn update_check_task(current: String) -> iced::Task<Message> {
        iced::Task::perform(
            async move { yutu_core::fetch_update_info(&current).await },
            |result| Message::UpdateCheckCompleted { result },
        )
    }

    /// Self-update download (best effort): the release artifact for
    /// this machine plus its checksum sidecar, with per-chunk progress
    /// forwarded like the playlist import ticks. `gen` quarantines
    /// stale progress after the modal is dismissed mid-download.
    #[cfg(feature = "update")]
    pub(crate) fn update_download_task(tag: String, gen: u64) -> iced::Task<Message> {
        let (tick_tx, tick_rx) = tokio::sync::mpsc::unbounded_channel();
        let fetch = iced::Task::perform(
            async move {
                let result = async {
                    let asset = yutu_core::release_asset_for(&tag)
                        .ok_or_else(|| "no update build for this platform".to_string())?;
                    let dir = std::env::temp_dir().join(yutu_core::update_scratch_name(&tag));
                    yutu_core::download_release_asset(&asset, &dir, &|downloaded, total| {
                        let _ = tick_tx.send((downloaded, total));
                    })
                    .await
                }
                .await;
                (gen, result)
            },
            |(gen, result)| Message::UpdateDownloaded { gen, result },
        );
        // Progress ticks end when the download drops its sender; late
        // ticks after completion are quarantined by generation.
        let forward = iced::Task::stream(Self::update_tick_stream(tick_rx, gen));
        iced::Task::batch([fetch, forward])
    }

    /// Forward update download ticks as messages (same combinator as
    /// the import ticks: order preserved, ends when the download drops
    /// its sender).
    #[cfg(feature = "update")]
    pub(crate) fn update_tick_stream(
        mut tick_rx: tokio::sync::mpsc::UnboundedReceiver<(u64, Option<u64>)>,
        gen: u64,
    ) -> impl futures::Stream<Item = Message> {
        futures::stream::poll_fn(move |cx| {
            tick_rx.poll_recv(cx).map(|opt| {
                opt.map(|(downloaded, total)| Message::UpdateProgress {
                    gen,
                    downloaded,
                    total,
                })
            })
        })
    }

    /// Self-update install (blocking file work on a background task):
    /// verify, unpack/replace per platform, then the completion
    /// handler relaunches (or exits for the Windows installer).
    #[cfg(feature = "update")]
    pub(crate) fn update_install_task(
        downloaded: yutu_core::DownloadedAsset,
    ) -> iced::Task<Message> {
        iced::Task::perform(
            async move { yutu_core::install_update_asset(&downloaded) },
            |result| Message::UpdateInstalled { result },
        )
    }

    /// Offline builds: update installs never start (no HTTP client, so
    /// no download can complete into one).
    #[cfg(not(feature = "update"))]
    pub(crate) fn update_install_task(
        _downloaded: yutu_core::DownloadedAsset,
    ) -> iced::Task<Message> {
        iced::Task::none()
    }

    /// Offline builds: server sync is a no-op (local state is the truth).
    #[cfg(not(feature = "innertube"))]
    pub(crate) fn playlist_rename_sync_task(
        _playlist_id: String,
        _title: String,
    ) -> iced::Task<Message> {
        iced::Task::none()
    }

    /// Start a sync pull (manual or scheduled): replays pending pushes,
    /// then pulls web changes through the saved session (OAuth preferred,
    /// cookie fallback). Silent no-op while busy; honest status when
    /// signed out.
    #[cfg(feature = "innertube")]
    pub(crate) fn start_sync(&mut self, auto: bool) -> iced::Task<Message> {
        if self.import_active || self.login_busy {
            return iced::Task::none();
        }
        let pull = if std::path::Path::new(&oauth_path()).exists() {
            Some(Self::sync_oauth_task(self.library_path.clone()))
        } else if cookie_session_exists() {
            Some(Self::sync_cookie_task(self.library_path.clone()))
        } else {
            None
        };
        let Some(pull) = pull else {
            self.status = self.language.sync_needs_signin();
            self.publish_mpris();
            return iced::Task::none();
        };
        self.login_busy = true;
        self.import_active = true;
        self.sync_active = true;
        self.status = if auto {
            "Auto-syncing with YouTube Music…".into()
        } else {
            "Syncing with YouTube Music…".into()
        };
        self.publish_mpris();
        iced::Task::batch([self.replay_pending_tasks(), pull])
    }

    /// Re-fire every pending push (sync replays leftovers before the
    /// pull reads; successes clear their rows, failures stay queued).
    /// Each task no-ops without a session on its own.
    #[cfg(feature = "innertube")]
    pub(crate) fn replay_pending_tasks(&self) -> iced::Task<Message> {
        let Ok(store) = LibraryStore::open(&self.library_path) else {
            return iced::Task::none();
        };
        let ops = store.pending_ops().unwrap_or_default();
        let mut tasks = Vec::with_capacity(ops.len());
        for op in ops {
            match op.op.as_str() {
                "like" => {
                    if let Some(id) = op.track_id {
                        tasks.push(Self::like_sync_task(id, true));
                    }
                }
                "unlike" => {
                    if let Some(id) = op.track_id {
                        tasks.push(Self::like_sync_task(id, false));
                    }
                }
                "dislike" => {
                    if let Some(id) = op.track_id {
                        tasks.push(Self::dislike_sync_task(id, true));
                    }
                }
                "undislike" => {
                    if let Some(id) = op.track_id {
                        tasks.push(Self::dislike_sync_task(id, false));
                    }
                }
                "add" => {
                    if let (Some(pid), Some(tid)) = (op.playlist_id, op.track_id) {
                        tasks.push(Self::playlist_add_sync_task(pid, tid));
                    }
                }
                "remove" => {
                    if let (Some(pid), Some(tid)) = (op.playlist_id, op.track_id) {
                        tasks.push(Self::playlist_remove_sync_task(pid, tid));
                    }
                }
                "create" => {
                    // Only while the shell still exists locally.
                    if let (Some(local), Some(title)) = (op.playlist_id, op.title) {
                        if self.user_playlists.iter().any(|p| p.id == local) {
                            tasks.push(Self::playlist_create_sync_task(local, title));
                        }
                    }
                }
                "rename" => {
                    // Only while the list still exists locally.
                    if let (Some(pid), Some(title)) = (op.playlist_id, op.title) {
                        if self.user_playlists.iter().any(|p| p.id == pid) {
                            tasks.push(Self::playlist_rename_sync_task(pid, title));
                        }
                    }
                }
                "delete" => {
                    if let Some(pid) = op.playlist_id {
                        tasks.push(Self::playlist_delete_sync_task(pid));
                    }
                }
                _ => {}
            }
        }
        iced::Task::batch(tasks)
    }

    /// Record a completed pull (any kind) for the scheduler + label.
    #[cfg(feature = "innertube")]
    pub(crate) fn mark_synced(&mut self) {
        self.last_sync = Some(std::time::Instant::now());
        let unix = now_unix();
        self.last_sync_unix = Some(unix);
        if let Ok(store) = LibraryStore::open(&self.library_path) {
            let _ = store.set_pref("last_sync_unix", &unix.to_string());
        }
    }

    /// Suggestion popup: one light query (texts only, no tracks).
    #[cfg(feature = "innertube")]
    pub(crate) async fn live_suggest(query: &str) -> Result<Vec<String>, String> {
        use yutu_core::InnertubeBackend;
        let backend = InnertubeBackend::connect_anonymous().await?;
        backend.search_suggestions(query).await
    }

    /// Suggestion debounce (shorter than search: the popup must feel instant).
    #[cfg(feature = "innertube")]
    pub(crate) fn suggest_debounce_task(gen: u64, query: String) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                let result = Self::live_suggest(&query).await;
                (gen, result)
            },
            |(gen, result)| Message::SuggestCompleted { gen, result },
        )
    }

    /// Shared keystroke/pick flow for the search input: keystrokes keep
    /// the popup open, picks close it. Either way the highlight resets
    /// (it indexes the previous suggestion list). One paint per query:
    /// keystrokes clear the old rows and schedule the debounced live
    /// search, so the live payload is the only paint (no premature
    /// catalog snapshot that the server order then replaces).
    pub(crate) fn apply_search_input(
        &mut self,
        q: String,
        popup_open: bool,
    ) -> iced::Task<Message> {
        self.search_query = q.clone();
        let trimmed = q.trim().to_string();
        self.search_gen = self.search_gen.wrapping_add(1);
        self.suggest_gen = self.suggest_gen.wrapping_add(1);
        self.suggest_idx = None;
        if !popup_open {
            // Explicit submit (Enter on a highlighted suggestion or a
            // suggestion click): land on the Search page like SearchStay,
            // which drops the now-playing drawer so results take focus.
            self.go(Route::Search);
        }
        if trimmed.is_empty() {
            self.results = SearchResults::default();
            self.all_results = SearchAll::default();
            self.suggest.clear();
            self.show_suggest = false;
            self.searching = false;
        } else if trimmed.len() >= 2 {
            self.show_suggest = popup_open;
            // Clear, never preview: the live payload paints once below.
            // Text suggestions stay until the fresh list lands (standard
            // autocomplete); result rows clear so nothing stale or
            // premature flashes before the server order arrives.
            self.results = SearchResults::default();
            self.all_results = SearchAll::default();
            #[cfg(feature = "innertube")]
            {
                let gen = self.search_gen;
                let sgen = self.suggest_gen;
                let q2 = trimmed.clone();
                self.searching = true;
                self.publish_mpris();
                let mut tasks = vec![
                    Self::search_debounce_task(gen, trimmed),
                    Self::suggest_debounce_task(sgen, q2),
                ];
                if !popup_open {
                    // Picks replace the whole query: park the cursor at
                    // the end (it keeps its old position otherwise).
                    tasks.push(Self::search_cursor_end());
                }
                return iced::Task::batch(tasks);
            }
        } else {
            // Single character: no stale results, no network yet.
            self.results = SearchResults::default();
            self.all_results = SearchAll::default();
            self.suggest.clear();
            self.searching = false;
        }
        if !popup_open && self.search_query.trim().len() >= 2 {
            return Self::search_cursor_end();
        }
        iced::Task::none()
    }

    /// Park the search input cursor at the end (picks replace the
    /// whole query; without this the cursor keeps its old position).
    pub(crate) fn search_cursor_end() -> iced::Task<Message> {
        iced::widget::operation::move_cursor_to_end("yutu-search")
    }

    /// Rows behind the open suggest popup: text suggestions first,
    /// then the top-track rows. Zero when the popup is closed.
    pub(crate) fn suggest_count(&self) -> usize {
        if !self.suggest_open() {
            return 0;
        }
        self.suggest.len() + self.results.tracks.iter().take(5).count()
    }

    /// Same gate as the popup layer: the query is searchable and there
    /// is something to show.
    pub(crate) fn suggest_open(&self) -> bool {
        let qnow = self.search_query.trim().to_string();
        self.show_suggest
            && qnow.len() >= 2
            && (!self.suggest.is_empty() || !self.results.tracks.is_empty())
    }

    /// Arrow-key step over the open popup (wraps both ends). Dead
    /// when the popup is closed, so arrows never hijack other views.
    pub(crate) fn move_suggest(&mut self, down: bool) {
        let n = self.suggest_count();
        if n == 0 {
            return;
        }
        self.suggest_idx = Some(match self.suggest_idx {
            None => {
                if down {
                    0
                } else {
                    n - 1
                }
            }
            Some(i) => {
                if down {
                    (i + 1) % n
                } else {
                    (i + n - 1) % n
                }
            }
        });
    }

    /// Enter with the popup open: text suggestions search, track rows
    /// play instantly, and no highlight submits the raw query. Fired
    /// from the input submit (single path, never doubled) when the
    /// popup is open; a closed popup submits normally.
    pub(crate) fn pick_suggest(&mut self) -> iced::Task<Message> {
        if !self.suggest_open() {
            self.suggest_idx = None;
            return self.update(Message::SearchStay);
        }
        let n_text = self.suggest.len();
        match self.suggest_idx {
            Some(i) if i < n_text => {
                let sug = self.suggest[i].clone();
                self.apply_search_input(sug, false)
            }
            Some(i) => match self.results.tracks.get(i - n_text).map(|t| t.id.clone()) {
                Some(id) => {
                    self.suggest_idx = None;
                    self.update(Message::PlayNow(id))
                }
                None => iced::Task::none(),
            },
            None => {
                self.suggest_idx = None;
                self.update(Message::SearchStay)
            }
        }
    }

    /// Clamp a stale highlight after fresh suggestions/results land.
    pub(crate) fn clamp_suggest(&mut self) {
        let n = self.suggest_count();
        if self.suggest_idx.is_some_and(|i| i >= n) {
            self.suggest_idx = None;
        }
    }

    /// Autoplay tail for [`Message::PlayNow`]: same-artist radio, seed first
    /// (the caller strips the seed and anything already queued).
    #[cfg(feature = "innertube")]
    pub(crate) fn autoplay_task(seed: String, artist: String) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let result = live_radio(&seed, &artist).await;
                (seed, result)
            },
            |(seed, result)| Message::AutoplayReady { seed, result },
        )
    }

    /// System file picker for the cookies.txt export (portal on Linux,
    /// native panel on macOS). Cancellation is `None`, never an error.
    #[cfg(feature = "innertube")]
    pub(crate) fn cookie_browse_task() -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                rfd::AsyncFileDialog::new()
                    .set_title("Choose cookies.txt export")
                    .add_filter("Text", &["txt"])
                    .pick_file()
                    .await
                    .map(|h| h.path().to_string_lossy().into_owned())
            },
            Message::CookieFilePicked,
        )
    }

    /// Cookie import, base stage: connect, stash the session securely,
    /// fetch songs + liked + playlist headers. The user logs into
    /// music.youtube.com in their own browser and exports cookies to a
    /// file (no OAuth client needed). Playlist tracks stream in next, one
    /// task per list, so progress shows instead of one silent future.
    #[cfg(feature = "innertube")]
    pub(crate) fn cookie_import_task(path: String, library_path: String) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let backend = yutu_core::InnertubeBackend::connect_with_cookie_file(
                    std::path::Path::new(&path),
                )
                .await?;
                let base = Self::fetch_base(&backend).await?;
                if base.tracks.is_empty() && base.headers.is_empty() {
                    return Err("library is empty: like songs or make a playlist on music.youtube.com, then import again".to_string());
                }
                // Stay signed in: stash the normalised header in the OS
                // keychain (0600 file fallback) only for a validated
                // import, so failures leave no session.
                if let Ok(auth) = yutu_core::CookieAuth::from_file(std::path::Path::new(&path)) {
                    let stash = yutu_core::save_cookie_session(
                        auth.header_value(),
                        std::path::Path::new(&cookie_session_path()),
                        std::path::Path::new(&cookie_keyring_marker_path()),
                    );
                    match stash {
                        Ok(yutu_core::SecureBackend::File) => {
                            yutu_core::app_log::warn(
                                "yutu-login",
                                "keychain unavailable: session saved to file",
                            );
                        }
                        Ok(yutu_core::SecureBackend::Keyring) => {}
                        Err(e) => {
                            yutu_core::app_log::error(
                                "yutu-login",
                                format_args!("session stash failed: {e}"),
                            );
                        }
                    }
                }
                Self::persist_base_pull(&library_path, &base);
                Ok(Self::staged_of(backend, base))
            },
            |result| Message::ImportStageReady {
                kind: ImportKind::Cookie,
                result,
            },
        )
    }

    /// Connect with the saved cookie session (keychain first, legacy file
    /// migrates). One place so import, sync, and search share the error.
    #[cfg(feature = "innertube")]
    pub(crate) async fn connect_saved_cookie() -> Result<yutu_core::InnertubeBackend, String> {
        let header = load_cookie_header()?;
        yutu_core::InnertubeBackend::connect_with_cookie_str(&header)
            .await
            .map_err(|e| {
                format!("saved cookie session failed ({e}): import your cookies file again")
            })
    }

    /// Re-import base stage via the saved cookie session (no file picker
    /// round trip). Playlist tracks stream in next like a fresh import.
    #[cfg(feature = "innertube")]
    pub(crate) fn cookie_session_import_task(library_path: String) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let backend = Self::connect_saved_cookie().await?;
                let base = Self::fetch_base(&backend).await?;
                if base.tracks.is_empty() && base.headers.is_empty() {
                    return Err("library is empty: like songs or make a playlist on music.youtube.com, then import again".to_string());
                }
                Self::persist_base_pull(&library_path, &base);
                Ok(Self::staged_of(backend, base))
            },
            |result| Message::ImportStageReady {
                kind: ImportKind::Cookie,
                result,
            },
        )
    }

    /// Sync base stage over OAuth (same pull as an import, "Sync"
    /// wording): replays pending pushes first via [`Self::start_sync`]
    /// tasks, then reconciles web changes. Empty pulls succeed (an
    /// empty account has nothing to reconcile; the flags guard wipes).
    #[cfg(feature = "innertube")]
    pub(crate) fn sync_oauth_task(library_path: String) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let persist =
                    yutu_core::OAuthPersist::load_from_file(std::path::Path::new(&oauth_path()))?;
                let backend = yutu_core::InnertubeBackend::connect_with_oauth(&persist)?;
                let base = Self::fetch_base(&backend).await?;
                Self::persist_base_pull(&library_path, &base);
                Ok(Self::staged_of(backend, base))
            },
            |result| Message::ImportStageReady {
                kind: ImportKind::Sync,
                result,
            },
        )
    }

    /// Sync base stage via the saved cookie session (same pull as an
    /// import, "Sync" wording). See [`Self::sync_oauth_task`].
    #[cfg(feature = "innertube")]
    pub(crate) fn sync_cookie_task(library_path: String) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let backend = Self::connect_saved_cookie().await?;
                let base = Self::fetch_base(&backend).await?;
                Self::persist_base_pull(&library_path, &base);
                Ok(Self::staged_of(backend, base))
            },
            |result| Message::ImportStageReady {
                kind: ImportKind::Sync,
                result,
            },
        )
    }

    /// One playlist's tracks over the stage's reused backend (no
    /// reconnect per list). Each mapped page sends its cumulative song
    /// count through a channel (forwarded as [`Message::PlaylistTick`]),
    /// so the banner bar moves song by song; the full merge still lands
    /// once via [`Message::PlaylistImported`]. Failures report per list;
    /// the import continues with the rest.
    #[cfg(feature = "innertube")]
    pub(crate) fn playlist_import_task(
        backend: yutu_core::InnertubeBackend,
        library_path: String,
        kind: ImportKind,
        gen: u64,
        header: yutu_core::ServerPlaylist,
        index: usize,
        total: usize,
    ) -> iced::Task<Message> {
        let (tick_tx, tick_rx) = tokio::sync::mpsc::unbounded_channel();
        let fetch = iced::Task::perform(
            async move {
                let count = backend.playlist_details_count(&header.id).await.ok();
                let _ = tick_tx.send((0usize, count));
                let result = backend
                    .playlist_tracks_live(&header.id, |arrived| {
                        let _ = tick_tx.send((arrived, count));
                    })
                    .await
                    .map(|tracks| {
                        // Disk first (off the GUI thread): the handler only
                        // touches memory, so arrivals never stall events.
                        // Lists with unpushed edits are not persisted:
                        // the server copy predates local intent. A pending
                        // rename also blocks the persist (the server title
                        // predates the local one); the memory merge keeps
                        // the local title with fresh tracks.
                        let playlist = Playlist {
                            id: header.id,
                            title: header.title.clone(),
                            tracks,
                        };
                        if !Self::playlist_has_pending_store(&library_path, &playlist.id)
                            && !Self::playlist_has_rename_pending_store(&library_path, &playlist.id)
                        {
                            Self::persist_imported(
                                &library_path,
                                &playlist.tracks,
                                &[],
                                std::slice::from_ref(&playlist),
                            );
                        }
                        playlist
                    });
                (kind, gen, header.title, index, total, count, result)
            },
            |(kind, gen, title, index, total, count, result)| Message::PlaylistImported {
                kind,
                gen,
                title,
                index,
                total,
                count,
                result,
            },
        );
        // Channel ticks end when the fetch drops its sender; late ticks
        // after completion are quarantined by generation + finished set.
        let forward = iced::Task::stream(Self::tick_forward_stream(tick_rx, gen, index));
        iced::Task::batch([fetch, forward])
    }

    /// Forward import tick channel items as messages (split out for
    /// headless testing of the combinator: order preserved, ends when
    /// the fetch drops its sender).
    #[cfg(feature = "innertube")]
    pub(crate) fn tick_forward_stream(
        mut tick_rx: tokio::sync::mpsc::UnboundedReceiver<(usize, Option<usize>)>,
        gen: u64,
        index: usize,
    ) -> impl futures::Stream<Item = Message> {
        futures::stream::poll_fn(move |cx| {
            tick_rx.poll_recv(cx).map(|opt| {
                opt.map(|(arrived, count)| Message::PlaylistTick {
                    gen,
                    index,
                    arrived,
                    count,
                })
            })
        })
    }

    /// Radio seed: same-artist tracks first, then the rest of the search.
    #[cfg(feature = "innertube")]
    pub(crate) fn radio_task(seed: String, artist: String) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let result = live_radio(&seed, &artist).await;
                (seed, result)
            },
            |(seed, result)| Message::RadioReady { seed, result },
        )
    }

    /// Search-result playlist fetch: tracks arrive via
    /// [`Message::SearchPlaylistReady`], which queues or reports.
    #[cfg(feature = "innertube")]
    pub(crate) fn search_playlist_task(
        id: String,
        label: String,
        shuffle: bool,
    ) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let result = live_playlist_tracks(&id).await;
                (id, label, shuffle, result)
            },
            |(id, label, shuffle, result)| Message::SearchPlaylistReady {
                id,
                label,
                shuffle,
                result,
            },
        )
    }
}
