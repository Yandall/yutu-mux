//! yutu-ui library_ops: discovery, playlists, likes and artist/album pages (split from lib.rs, no behavior change).

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
    /// Weekly Mix from persisted history + in-memory likes + real catalog.
    /// Disliked ("don't recommend") tracks never appear.
    pub fn weekly_mix_tracks(&self) -> Vec<Track> {
        weekly_mix(
            &self.discovery_history(),
            &self.liked,
            &self.catalog_tracks(),
            10,
            now_unix(),
        )
        .into_iter()
        .filter(|t| !self.disliked.contains(&t.id))
        .collect()
    }

    /// Forgotten Favorites from in-memory likes + persisted history + catalog.
    /// Disliked ("don't recommend") tracks never appear.
    pub fn forgotten_tracks(&self) -> Vec<Track> {
        forgotten_favorites(
            &self.liked,
            &self.discovery_history(),
            &self.catalog_tracks(),
            now_unix(),
        )
        .into_iter()
        .filter(|t| !self.disliked.contains(&t.id))
        .collect()
    }

    /// Virtual Favorites id (never persisted, never a `uN` user id).
    pub(crate) const FAVORITES_ID: &'static str = "__favorites";

    pub(crate) fn all_playlists(&self) -> Vec<Playlist> {
        let mut out = vec![Playlist {
            id: Self::FAVORITES_ID.into(),
            title: "Favorites".into(),
            tracks: self.favorite_tracks(),
        }];
        out.extend(self.user_playlists.clone());
        out
    }

    /// Liked tracks in add order via catalog (unresolved skipped).
    pub(crate) fn favorite_tracks(&self) -> Vec<Track> {
        self.liked_order
            .iter()
            .filter_map(|id| self.find_track(id))
            .collect()
    }

    /// Like helper: membership plus add-order append.
    pub(crate) fn like_id(&mut self, id: &str) {
        if self.liked.insert(id.to_string()) {
            self.liked_order.push(id.to_string());
        }
    }

    /// Unlike helper: membership plus order removal.
    pub(crate) fn unlike_id(&mut self, id: &str) {
        self.liked.remove(id);
        self.liked_order.retain(|x| x != id);
    }

    /// Dislike helper: membership plus add-order append.
    pub(crate) fn dislike_id(&mut self, id: &str) {
        if self.disliked.insert(id.to_string()) {
            self.disliked_order.push(id.to_string());
        }
    }

    /// Undislike helper: membership plus order removal.
    pub(crate) fn undislike_id(&mut self, id: &str) {
        self.disliked.remove(id);
        self.disliked_order.retain(|x| x != id);
    }

    pub(crate) fn find_playlist(&self, id: &str) -> Option<Playlist> {
        // Favorites skips the clone plus sort of the whole library.
        if id == Self::FAVORITES_ID {
            return Some(Playlist {
                id: Self::FAVORITES_ID.into(),
                title: "Favorites".into(),
                tracks: self.favorite_tracks(),
            });
        }
        self.all_playlists().into_iter().find(|p| p.id == id)
    }

    /// Sort key for the pref store.
    pub(crate) fn playlist_sort_key(playlist_id: &str) -> String {
        format!("playlist_sort_{playlist_id}")
    }

    /// Current sort for an own playlist (cached, lazy-loaded from prefs,
    /// default Manual so existing installs keep stored order).
    pub(crate) fn playlist_sort(&mut self, playlist_id: &str) -> PlaylistSort {
        if let Some(s) = self.playlist_sorts.get(playlist_id) {
            return *s;
        }
        let sort = LibraryStore::open(&self.library_path)
            .ok()
            .and_then(|store| store.get_pref(&Self::playlist_sort_key(playlist_id)).ok())
            .flatten()
            .map(|raw| PlaylistSort::parse(&raw))
            .unwrap_or_default();
        self.playlist_sorts.insert(playlist_id.to_string(), sort);
        sort
    }

    /// Cached sort for immutable views (default Manual when not loaded
    /// yet; `OpenPlaylist` preloads, so first frames already agree).
    pub(crate) fn playlist_sort_cached(&self, playlist_id: &str) -> PlaylistSort {
        self.playlist_sorts
            .get(playlist_id)
            .copied()
            .unwrap_or_default()
    }

    /// Persist a sort pick (cache plus pref write-through).
    pub(crate) fn persist_playlist_sort(&mut self, playlist_id: &str, sort: PlaylistSort) {
        self.playlist_sorts.insert(playlist_id.to_string(), sort);
        if let Ok(store) = LibraryStore::open(&self.library_path) {
            let _ = store.set_pref(&Self::playlist_sort_key(playlist_id), sort.stored());
        }
    }

    /// Insertion time for one playlist row (0 when unknown: unknown rows
    /// read as oldest, so they lead oldest-first and trail newest-first;
    /// only tests that bypass the store hit this, real rows backfill).
    pub(crate) fn playlist_added_for(&self, playlist_id: &str, track_id: &str) -> i64 {
        self.playlist_added
            .get(playlist_id)
            .and_then(|m| m.get(track_id))
            .copied()
            .unwrap_or(0)
    }

    /// Display order for an own playlist: stored Manual order untouched,
    /// every other pick a pure sort over it (stable, deterministic).
    /// Favorites never reaches here (its view has no picker).
    pub(crate) fn playlist_ordered_tracks(&self, playlist: &Playlist) -> Vec<Track> {
        let sort = self
            .playlist_sorts
            .get(&playlist.id)
            .copied()
            .unwrap_or_default();
        self.playlist_ordered_tracks_with(playlist, sort)
    }

    /// Sort helper with an explicit pick (pure, unit-tested; the view
    /// passes the cached pick via [`Self::playlist_ordered_tracks`]).
    pub(crate) fn playlist_ordered_tracks_with(
        &self,
        playlist: &Playlist,
        sort: PlaylistSort,
    ) -> Vec<Track> {
        let mut tracks = playlist.tracks.clone();
        match sort {
            PlaylistSort::Manual => {}
            PlaylistSort::OldestFirst => {
                // No times cached (tests that bypass the store): stored
                // order already is insertion order.
                if self.playlist_added.contains_key(&playlist.id) {
                    tracks.sort_by_key(|t| self.playlist_added_for(&playlist.id, &t.id));
                }
            }
            PlaylistSort::NewestFirst => {
                if self.playlist_added.contains_key(&playlist.id) {
                    tracks.sort_by_key(|t| {
                        std::cmp::Reverse(self.playlist_added_for(&playlist.id, &t.id))
                    });
                } else {
                    tracks.reverse();
                }
            }
            PlaylistSort::Title => {
                tracks.sort_by(|a, b| {
                    a.title
                        .to_lowercase()
                        .cmp(&b.title.to_lowercase())
                        .then_with(|| a.artist.to_lowercase().cmp(&b.artist.to_lowercase()))
                        .then_with(|| a.id.cmp(&b.id))
                });
            }
            PlaylistSort::Artist => {
                tracks.sort_by(|a, b| {
                    a.artist
                        .to_lowercase()
                        .cmp(&b.artist.to_lowercase())
                        .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
                        .then_with(|| a.id.cmp(&b.id))
                });
            }
            PlaylistSort::Album => {
                tracks.sort_by(|a, b| {
                    a.album
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .cmp(&b.album.as_deref().unwrap_or("").to_lowercase())
                        .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
                        .then_with(|| a.id.cmp(&b.id))
                });
            }
            PlaylistSort::LikedFirst => {
                tracks.sort_by_key(|t| (!self.liked.contains(&t.id), t.title.to_lowercase()));
            }
        }
        tracks
    }

    /// True while a manual playlist drag has moved past slop.
    pub(crate) fn pl_dragging(&self) -> bool {
        self.pl_drag.is_some_and(|d| d.live)
    }

    /// Latched playlist slot filtered to real moves (own-spot hovers are
    /// None, mirroring the queue drag). Needs the list length: stale
    /// drags from a navigated-away list never open a gap.
    pub(crate) fn pl_effective_slot(&self, len: usize) -> Option<usize> {
        let drag = self.pl_drag?;
        if !drag.live {
            return None;
        }
        self.pl_drag_id.as_deref()?;
        drag.slot
            .filter(|s| slot_commit(len, drag.from, *s).is_some())
    }

    /// Cancel any in-flight manual playlist drag.
    pub(crate) fn cancel_pl_drag(&mut self) {
        self.pl_drag = None;
        self.pl_press_snap = None;
        self.pl_drag_id = None;
    }

    /// Release path for a manual playlist press (shared by the `DragEnd`
    /// release subscription and direct `PlDragEnd` sends): a live drag on
    /// a committing slot reorders (positions persist, `added_at` stable);
    /// otherwise a clean press/release over the same unmutated row plays
    /// it (same snapshot guard, same-song no-op as `PlayPlaylistAt`).
    /// Returns the play task on a click-to-play, else none.
    pub(crate) fn pl_release_task(&mut self) -> iced::Task<Message> {
        let snap = self.pl_press_snap.take();
        let drag = self.pl_drag.take();
        let drag_id = self.pl_drag_id.take();
        let (Some(drag), Some(pid)) = (drag, drag_id) else {
            return iced::Task::none();
        };
        let ordered = self
            .find_playlist(&pid)
            .map(|p| self.playlist_ordered_tracks(&p))
            .unwrap_or_default();
        let current_len = ordered.len();
        let snap_ok = snap.as_ref().is_some_and(|s| {
            s.from == drag.from
                && current_len == s.len
                && ordered.get(s.from).is_some_and(|t| t.id == s.id)
        });
        let manual =
            self.playlist_sorts.get(&pid).copied().unwrap_or_default() == PlaylistSort::Manual;
        // Reorder: only a live drag on a committing slot, Manual only.
        if snap_ok && drag.live && manual {
            if let Some(slot) = drag.slot {
                if let Some(to) = slot_commit(current_len, drag.from, slot) {
                    if let Some(pos) = self.user_playlists.iter().position(|p| p.id == pid) {
                        let mut ids: Vec<String> = self.user_playlists[pos]
                            .tracks
                            .iter()
                            .map(|t| t.id.clone())
                            .collect();
                        if drag.from < ids.len() {
                            let item = ids.remove(drag.from);
                            ids.insert(to.min(ids.len()), item);
                            let mut tracks: Vec<Track> = ids
                                .iter()
                                .filter_map(|id| {
                                    self.user_playlists[pos]
                                        .tracks
                                        .iter()
                                        .find(|t| &t.id == id)
                                        .cloned()
                                })
                                .collect();
                            if tracks.len() == self.user_playlists[pos].tracks.len() {
                                std::mem::swap(&mut self.user_playlists[pos].tracks, &mut tracks);
                                self.persist_playlist_reordered(&pid, &ids);
                                self.status = self.language.playlist_reordered();
                                self.just_reordered_at = Some(std::time::Instant::now());
                                return iced::Task::none();
                            }
                        }
                    }
                }
            }
        }
        // Click-to-play: no commit plus a clean press/release over the
        // same unmutated row with the cursor still on it (a move voids
        // the click via the release distance below; a reorder voids it
        // via the commit above). Modal layers suppress it like every
        // other row click, and the reorder grace eats the click that
        // shares a committing release.
        let modal = self.context_menu.is_some()
            || self.pl_menu.is_some()
            || self.coll_menu.is_some()
            || self.sort_menu.is_some()
            || self.show_suggest;
        let click = !modal
            && snap.as_ref().is_some_and(|s| {
                snap_ok
                    && !self
                        .just_reordered_at
                        .is_some_and(|at| at.elapsed() < std::time::Duration::from_millis(150))
                    && release_is_click_at(s.pos, self.cursor)
                    && self.pl_hover == Some(s.from)
            });
        if !click {
            return iced::Task::none();
        }
        let Some(s) = snap else {
            return iced::Task::none();
        };
        // Same-song no-op (app contract): clicking the live track's own
        // row never restarts audio.
        if self.queue.current().is_some_and(|t| t.id == s.id) {
            return iced::Task::none();
        }
        let Some(p) = self.find_playlist(&pid) else {
            self.status = self.language.playlist_not_found_plain();
            return iced::Task::none();
        };
        // Same suffix rule as the row button it replaces: the displayed
        // (sorted) list plays from the clicked song on, anchored by id
        // with the index as fallback.
        let display = self.playlist_ordered_tracks(&p);
        let start = display
            .iter()
            .position(|t| t.id == s.id)
            .unwrap_or(s.from.min(display.len()));
        let label = self.language.playlist_queue_label(&p.title);
        let tracks: Vec<Track> = display.into_iter().skip(start).collect();
        if tracks.is_empty() {
            self.status = self.language.nothing_to_play(&label);
            return iced::Task::none();
        }
        self.close_menu();
        self.queue.clear();
        self.autoplay.clear();
        self.autoplay_for = None;
        self.drag = None;
        self.queue_many(tracks, &label)
    }

    /// Accept an artist id, a catalog track id, or a plain name.
    pub(crate) fn resolve_artist_name(&self, id_or_name: &str) -> String {
        if let Some(a) = self.results.artists.iter().find(|a| a.id == id_or_name) {
            return a.name.clone();
        }
        if let Some(t) = self.catalog.get(id_or_name) {
            return t.artist.clone();
        }
        if self.results.artists.iter().any(|a| a.name == id_or_name) {
            return id_or_name.to_string();
        }
        if self.catalog.values().any(|t| t.artist == id_or_name) {
            return id_or_name.to_string();
        }
        self.results
            .artists
            .first()
            .map(|a| a.name.clone())
            .unwrap_or_else(|| id_or_name.to_string())
    }

    pub(crate) fn artist_tracks(&self, artist: &str) -> Vec<Track> {
        let mut v: Vec<Track> = self
            .catalog
            .values()
            .filter(|t| t.artist == artist)
            .cloned()
            .collect();
        v.sort_by(|a, b| a.title.cmp(&b.title));
        v
    }

    /// Open an artist page (every entry point funnels here so the
    /// live top-songs fetch fires exactly once per browse id): catalog
    /// paints instantly, the server rank swaps in on arrival. Bare
    /// names without a known channel resolve through a scoped artist
    /// search first, then chain into the same fetch.
    pub(crate) fn open_artist(&mut self, route_id: String) -> iced::Task<Message> {
        self.go(Route::Artist(route_id.clone()));
        self.publish_mpris();
        // Fresh page: Popular collapses to the head (the cached list
        // stays, so expanding again costs no fetch).
        self.artist_expanded = false;
        // Aliases record unconditionally (pure, offline-safe): later
        // searches replacing `results` must keep resolving.
        if let Some(channel) = self.artist_channel_id(&route_id) {
            let name = self.resolve_artist_name(&route_id);
            Self::alias_insert(&mut self.artist_alias, route_id.clone(), channel.clone());
            Self::alias_insert(&mut self.artist_alias, name, channel.clone());
        }
        #[cfg(feature = "innertube")]
        {
            let mut tasks = vec![self.ensure_visible_tasks()];
            if let Some(channel) = self.artist_channel_id(&route_id) {
                if !self.artist_top.contains_key(&channel)
                    && self.artist_loading.as_deref() != Some(channel.as_str())
                {
                    self.artist_loading = Some(channel.clone());
                    yutu_core::app_log::info(
                        "live-detail",
                        format_args!("artist top fetch: {channel}"),
                    );
                    tasks.push(Self::artist_top_task(channel));
                }
            } else {
                yutu_core::app_log::info(
                    "live-detail",
                    format_args!("artist top skipped, no browse id for route: {route_id}"),
                );
                if self.artist_resolving.as_deref() != Some(route_id.as_str()) {
                    self.artist_resolving = Some(route_id.clone());
                    tasks.push(Self::artist_resolve_task(
                        route_id.clone(),
                        self.resolve_artist_name(&route_id),
                    ));
                }
            }
            iced::Task::batch(tasks)
        }
        #[cfg(not(feature = "innertube"))]
        {
            self.ensure_visible_tasks()
        }
    }

    /// Open an album page (same funnel rule as [`Self::open_artist`]:
    /// live `track_no` tracklist once per browse id, title routes
    /// resolving through a scoped album search when unknown).
    pub(crate) fn open_album(&mut self, route_id: String) -> iced::Task<Message> {
        self.go(Route::Album(route_id.clone()));
        self.publish_mpris();
        // The 180px header renders the hires key while row slots
        // only warm row rasters: warm it here (shared key helper).
        let mut tasks = vec![self.ensure_visible_tasks()];
        // Aliases record unconditionally (pure, offline-safe): later
        // title routes must keep resolving after `results` churn.
        let title = match route_id.strip_prefix("ax-") {
            Some(rest) => rest.to_string(),
            None => self.resolve_album_title(&route_id),
        };
        if let Some(browse) = self.album_browse_id(&route_id) {
            Self::alias_insert(&mut self.album_alias, route_id.clone(), browse.clone());
            Self::alias_insert(&mut self.album_alias, title.clone(), browse.clone());
        }
        #[cfg(feature = "innertube")]
        {
            if let Some(browse) = self.album_browse_id(&route_id) {
                if !self.album_live.contains_key(&browse)
                    && self.album_loading.as_deref() != Some(browse.as_str())
                {
                    self.album_loading = Some(browse.clone());
                    yutu_core::app_log::info(
                        "live-detail",
                        format_args!("album tracklist fetch: {browse}"),
                    );
                    tasks.push(Self::album_live_task(browse));
                }
            } else {
                yutu_core::app_log::info(
                    "live-detail",
                    format_args!("album tracklist skipped, no browse id for route: {route_id}"),
                );
                let owner = self
                    .catalog
                    .values()
                    .find(|t| t.album.as_deref() == Some(title.as_str()))
                    .map(|t| t.artist.clone());
                if self.album_resolving.as_deref() != Some(route_id.as_str()) {
                    self.album_resolving = Some(route_id.clone());
                    tasks.push(Self::album_resolve_task(route_id.clone(), title, owner));
                }
            }
        }
        if let Some(url) = self.album_header_cover(&route_id) {
            tasks.extend(self.artwork_tasks_for_urls(&[url]));
        }
        iced::Task::batch(tasks)
    }

    /// Exact artist name match (case-insensitive) for the resolve
    /// fallback: first-hit guessing would land on the wrong artist.
    /// Pure helper (offline tests cover it); only the task needs the
    /// network feature.
    #[cfg_attr(not(feature = "innertube"), allow(dead_code))]
    pub(crate) fn pick_artist_channel(artists: &[Artist], name: &str) -> Option<String> {
        artists
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case(name.trim()))
            .map(|a| a.id.clone())
    }

    /// Album browse match for the resolve fallback: exact title first,
    /// disambiguated by the catalog owner when titles collide.
    /// Pure helper (offline tests cover it); only the task needs the
    /// network feature.
    #[cfg_attr(not(feature = "innertube"), allow(dead_code))]
    pub(crate) fn pick_album_browse(
        albums: &[Album],
        title: &str,
        owner: Option<&str>,
    ) -> Option<String> {
        let mut candidates = albums.iter().filter(|a| a.title == title);
        let first = candidates.next()?;
        if let Some(owner) = owner {
            if let Some(hit) = std::iter::once(first)
                .chain(candidates)
                .find(|a| a.artist == owner)
            {
                return Some(hit.id.clone());
            }
            return None;
        }
        Some(first.id.clone())
    }

    /// "artist · album" subtitle for top-song rows (either side may
    /// be blank upstream): empty parts drop out, never a dangling
    /// separator. Mirrors [`Self::hero_song_subtitle`].
    pub(crate) fn artist_album_sub(artist: &str, album: Option<&str>) -> String {
        let artist = artist.trim();
        let album = album.map(str::trim).filter(|s| !s.is_empty()).unwrap_or("");
        match (artist.is_empty(), album.is_empty()) {
            (false, false) => format!("{artist} · {album}"),
            (false, true) => artist.to_string(),
            (true, false) => album.to_string(),
            (true, true) => String::new(),
        }
    }

    /// Artist top-song row: rank, art, title + "artist · album"
    /// subtitle, plays + duration, song actions. Same shape as
    /// [`Self::numbered_track_row`] (the shared Popular look) with the
    /// album column YouTube shows.
    pub(crate) fn artist_top_row(
        &self,
        t: &Track,
        n: usize,
        artist_id: &str,
    ) -> iced::Element<'_, Message> {
        use iced::widget::{container, mouse_area, row, text};
        use iced::Fill;
        let hovered = self.hovered_track.as_deref() == Some(t.id.as_str());
        let hover_id = t.id.clone();
        let (heart, more) = self.song_actions(t, hovered);
        let play_msg = RowPlay::ArtistAt(artist_id.to_string()).message(&t.id, n);
        mouse_area(
            container(
                row![
                    text(format!("{n}")).size(13).color(MUTED).width(28),
                    self.art_play_button(t, hovered, play_msg),
                    Self::track_text_single(
                        &t.title,
                        &Self::artist_album_sub(&t.artist, t.album.as_deref()),
                        14,
                        iced::Color::WHITE
                    ),
                    song_meta_text(t.plays.as_deref(), t.duration_secs),
                    heart,
                    more,
                ]
                .spacing(8)
                .align_y(iced::alignment::Vertical::Center)
                .padding(8),
            )
            .style(row_bg(hovered))
            .width(Fill)
            .height(iced::Length::Fixed(TRACK_ROW_H))
            .clip(true)
            .align_y(iced::alignment::Vertical::Center),
        )
        .on_right_press(Message::OpenContext(t.id.clone()))
        .on_enter(Message::TrackHover(Some(hover_id.clone())))
        .on_exit(Message::TrackExit(hover_id))
        .into()
    }

    /// Channel browse id for an artist route: real search ids pass
    /// through, bare names resolve via the searched artists, then via
    /// recorded aliases and the live cache itself (later searches may
    /// replace `results` while a cached list survives). Unknown names
    /// mean catalog-only.
    pub(crate) fn artist_channel_id(&self, route_id: &str) -> Option<String> {
        if self.results.artists.iter().any(|a| a.id == route_id) {
            return Some(route_id.to_string());
        }
        if let Some(hit) = self
            .results
            .artists
            .iter()
            .find(|a| a.name == route_id)
            .map(|a| a.id.clone())
        {
            return Some(hit);
        }
        if let Some(hit) = self.artist_alias.get(route_id) {
            return Some(hit.clone());
        }
        // Direct channel route whose results were replaced since.
        if self.artist_top.contains_key(route_id) {
            return Some(route_id.to_string());
        }
        None
    }

    /// Live server-ranked top songs when fetched (None when absent):
    /// the ranked UI (Popular head + full list) reads this; PLAY,
    /// warming, and the radio seed read [`Self::artist_display_tracks`].
    pub(crate) fn artist_live_tracks(&self, route_id: &str) -> Option<Vec<Track>> {
        let channel = self.artist_channel_id(route_id)?;
        let ids = self.artist_top.get(&channel)?;
        let live: Vec<Track> = ids.iter().filter_map(|id| self.find_track(id)).collect();
        (!live.is_empty()).then_some(live)
    }

    /// Displayed artist songs: live server-ranked top songs when
    /// fetched (keyed by channel browse id, so name routes share the
    /// one fetch), else the catalog fallback (alphabetical, counts
    /// only where a search provided them).
    pub(crate) fn artist_display_tracks(&self, route_id: &str) -> Vec<Track> {
        if let Some(live) = self.artist_live_tracks(route_id) {
            return live;
        }
        self.artist_tracks(&self.resolve_artist_name(route_id))
    }

    pub(crate) fn artist_albums(&self, artist: &str) -> Vec<Album> {
        let mut albums: Vec<Album> = self
            .results
            .albums
            .iter()
            .filter(|a| a.artist == artist)
            .cloned()
            .collect();
        for t in self.artist_tracks(artist) {
            if let Some(title) = t.album.clone() {
                if !albums.iter().any(|a| a.title == title) {
                    albums.push(Album {
                        id: format!("ax-{title}"),
                        title,
                        artist: artist.into(),
                        year: None,
                        thumbnail_url: t.thumbnail_url.clone(),
                    });
                }
            }
        }
        albums
    }

    /// Accept an album id, a catalog track id, or a plain title.
    pub(crate) fn resolve_album_title(&self, id_or_title: &str) -> String {
        if let Some(a) = self.results.albums.iter().find(|a| a.id == id_or_title) {
            return a.title.clone();
        }
        if let Some(t) = self.catalog.get(id_or_title) {
            if let Some(album) = t.album.as_deref() {
                return album.to_string();
            }
        }
        // Synthetic cards use ax-Title ids; strip back to the title.
        if let Some(rest) = id_or_title.strip_prefix("ax-") {
            if self
                .catalog
                .values()
                .any(|t| t.album.as_deref() == Some(rest))
            {
                return rest.to_string();
            }
        }
        // Plain title passthrough (OpenTrackAlbum already passes titles).
        if self
            .catalog
            .values()
            .any(|t| t.album.as_deref() == Some(id_or_title))
        {
            return id_or_title.to_string();
        }
        id_or_title.to_string()
    }

    pub(crate) fn album_tracks(&self, id_or_title: &str) -> Vec<Track> {
        let title = self.resolve_album_title(id_or_title);
        let mut v: Vec<Track> = self
            .catalog
            .values()
            .filter(|t| t.album.as_deref() == Some(title.as_str()))
            .cloned()
            .collect();
        v.sort_by(|a, b| a.title.cmp(&b.title));
        v
    }

    /// Album browse id for an album route: real search ids pass
    /// through, synthetic `ax-` cards and plain titles resolve via the
    /// searched albums (disambiguated by the catalog owner when titles
    /// collide across artists), then via recorded aliases and the live
    /// cache itself. Unknown titles mean catalog-only.
    pub(crate) fn album_browse_id(&self, route_id: &str) -> Option<String> {
        if self.results.albums.iter().any(|a| a.id == route_id) {
            return Some(route_id.to_string());
        }
        let title = match route_id.strip_prefix("ax-") {
            Some(rest) => rest.to_string(),
            None => self.resolve_album_title(route_id),
        };
        let mut candidates = self.results.albums.iter().filter(|a| a.title == title);
        if let Some(first) = candidates.next() {
            // Duplicate titles across artists: prefer the one owning
            // catalog tracks under this title.
            let owner = self
                .catalog
                .values()
                .find(|t| t.album.as_deref() == Some(title.as_str()))
                .map(|t| t.artist.clone());
            if let Some(owner) = owner {
                if let Some(hit) = std::iter::once(first)
                    .chain(candidates)
                    .find(|a| a.artist == owner)
                {
                    return Some(hit.id.clone());
                }
            }
            return Some(first.id.clone());
        }
        if let Some(hit) = self
            .album_alias
            .get(route_id)
            .or_else(|| self.album_alias.get(&title))
        {
            return Some(hit.clone());
        }
        // Direct album route whose results were replaced since.
        if self.album_live.contains_key(route_id) {
            return Some(route_id.to_string());
        }
        None
    }

    /// Live `track_no`-ordered tracklist when fetched (None when
    /// absent): the view reads this; PLAY and warming read
    /// [`Self::album_display_tracks`].
    pub(crate) fn album_live_tracks(&self, route_id: &str) -> Option<Vec<Track>> {
        let browse = self.album_browse_id(route_id)?;
        let ids = self.album_live.get(&browse)?;
        let live: Vec<Track> = ids.iter().filter_map(|id| self.find_track(id)).collect();
        (!live.is_empty()).then_some(live)
    }

    /// Displayed album songs: live `track_no`-ordered tracklist when
    /// fetched (keyed by album browse id), else the catalog fallback
    /// (alphabetical).
    pub(crate) fn album_display_tracks(&self, route_id: &str) -> Vec<Track> {
        if let Some(live) = self.album_live_tracks(route_id) {
            return live;
        }
        self.album_tracks(route_id)
    }

    /// Capped detail-cache insert (live lists): past 64 keys the
    /// oldest goes (detail pages are small, so this bounds long
    /// sessions without LRU machinery). Returns the evicted key, if
    /// any, so paired sets (see `artist_full`) can drop it too.
    pub(crate) fn capped_detail_insert(
        map: &mut HashMap<String, Vec<String>>,
        order: &mut Vec<String>,
        key: String,
        value: Vec<String>,
    ) -> Option<String> {
        use std::collections::hash_map::Entry;
        match map.entry(key) {
            Entry::Occupied(mut slot) => {
                slot.insert(value);
                None
            }
            Entry::Vacant(slot) => {
                order.push(slot.key().clone());
                slot.insert(value);
                let mut evicted = None;
                while order.len() > 64 {
                    let old = order.remove(0);
                    map.remove(&old);
                    evicted = Some(old);
                }
                evicted
            }
        }
    }

    /// Alias insert (route/title → browse id): latest wins so a
    /// corrected browse id repairs a stale one in-session. Skips
    /// blank keys (a blank upstream name must never resolve).
    /// Recorded only at fetch launch (fresh context), never on
    /// arrival, so a late response for a same-titled page cannot
    /// poison lookups for the page being viewed. Plain `HashMap`
    /// insert: aliases are tiny strings, capped only by distinct
    /// pages visited.
    pub(crate) fn alias_insert(map: &mut HashMap<String, String>, key: String, value: String) {
        if !key.trim().is_empty() && !value.trim().is_empty() {
            map.insert(key, value);
        }
    }

    /// True while the live artist list is still on its way (fetch or
    /// resolve in flight, nothing cached): the view shows a loading
    /// row instead of the stale catalog fallback, so the page never
    /// flashes two versions. Offline and failed fetches never set the
    /// markers, so they fall straight through to the catalog.
    pub(crate) fn artist_list_pending(&self, route_id: &str) -> bool {
        if self.artist_live_tracks(route_id).is_some() {
            return false;
        }
        let loading = self
            .artist_channel_id(route_id)
            .is_some_and(|c| self.artist_loading.as_deref() == Some(c.as_str()));
        loading || self.artist_resolving.as_deref() == Some(route_id)
    }

    /// Same rule for album tracklists (see [`Self::artist_list_pending`]).
    pub(crate) fn album_list_pending(&self, route_id: &str) -> bool {
        if self.album_live_tracks(route_id).is_some() {
            return false;
        }
        let loading = self
            .album_browse_id(route_id)
            .is_some_and(|b| self.album_loading.as_deref() == Some(b.as_str()));
        loading || self.album_resolving.as_deref() == Some(route_id)
    }

    /// True when the current route is the detail page whose browse id
    /// is `browse` (artist pages match channel ids, album pages album
    /// ids): arrival statuses must not leak onto pages already left.
    pub(crate) fn route_shows_browse(&self, browse: &str, artist: bool) -> bool {
        match &self.route {
            Route::Artist(rid) if artist => self.artist_channel_id(rid).as_deref() == Some(browse),
            Route::Album(rid) if !artist => self.album_browse_id(rid).as_deref() == Some(browse),
            _ => false,
        }
    }

    /// Merge live detail tracks into the catalog without clobbering:
    /// existing art, durations, albums, and counts survive non-empty
    /// live fields; live values fill only blanks. Memory-first, then a
    /// blank-preserving SQLite write-through (titles for brand-new ids
    /// survive restarts; the `tracks` table carries no plays column, so
    /// counts themselves re-fetch on revisit). Returns the
    /// server-ordered ids for the view.
    pub(crate) fn merge_live_tracks(&mut self, tracks: Vec<Track>) -> Vec<String> {
        let ids: Vec<String> = tracks.iter().map(|t| t.id.clone()).collect();
        for t in tracks {
            match self.catalog.get_mut(&t.id) {
                Some(existing) => {
                    if existing.thumbnail_url.is_none() {
                        existing.thumbnail_url = t.thumbnail_url.clone();
                    }
                    if existing.thumbnail_large_url.is_none() {
                        existing.thumbnail_large_url = t.thumbnail_large_url.clone();
                    }
                    if existing.duration_secs == 0 {
                        existing.duration_secs = t.duration_secs;
                    }
                    if existing.plays.is_none() {
                        existing.plays = t.plays.clone();
                    }
                    if existing.album.is_none() {
                        existing.album = t.album.clone();
                    }
                }
                None => {
                    self.catalog.insert(t.id.clone(), t);
                }
            }
        }
        // Write-through of the MERGED rows (never the raw live ones):
        // blank-preserving upsert, so stored art is safe and new ids
        // keep their titles across restarts.
        let merged: Vec<Track> = ids
            .iter()
            .filter_map(|id| self.catalog.get(id).cloned())
            .collect();
        self.cache_tracks(&merged);
        ids
    }

    /// Album header cover (180px): hires of the first track thumb, else
    /// the album thumb. Shared by the album view and `OpenAlbum` warming
    /// so the key never drifts (row slots only warm row rasters now).
    pub(crate) fn album_header_cover(&self, id: &str) -> Option<String> {
        let title = self.resolve_album_title(id);
        self.album_tracks(id)
            .first()
            .and_then(|t| t.thumbnail_url.clone())
            .or_else(|| {
                self.results
                    .albums
                    .iter()
                    .find(|a| a.title == title)
                    .and_then(|a| a.thumbnail_url.clone())
            })
            .map(|u| yutu_core::hires_thumb_url(&u, 544))
    }

    /// Display label for a search-result playlist id (title when known).
    /// Live lookup only runs on innertube builds; tests exercise it on
    /// every configuration.
    #[cfg(any(feature = "innertube", test))]
    pub(crate) fn search_playlist_label(&self, id: &str) -> String {
        self.results
            .playlists
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.title.clone())
            .unwrap_or_else(|| id.to_string())
    }

    /// Play one search-result playlist: fetch its tracks live first
    /// (`shuffle` shuffles the fresh queue like hero actions).
    pub(crate) fn play_search_playlist(
        &mut self,
        id: String,
        shuffle: bool,
    ) -> iced::Task<Message> {
        self.close_menu();
        #[cfg(feature = "innertube")]
        {
            let label = self.search_playlist_label(&id);
            self.status = self.language.loading_playlist(&label);
            self.publish_mpris();
            Self::search_playlist_task(id, label, shuffle)
        }
        #[cfg(not(feature = "innertube"))]
        {
            let _ = (id, shuffle);
            self.status = self.language.needs_innertube_playlists();
            self.publish_mpris();
            iced::Task::none()
        }
    }

    /// Dispatcher: actions with async follow-ups (scrobble submits) return
    /// their task directly; everything else goes through `update_sync`.
    /// MPRIS state is published on every path that returns here.
    pub fn update(&mut self, message: Message) -> iced::Task<Message> {
        match message {
            Message::PlayTrack(i) => {
                self.show_suggest = false;
                let task = self.play_index(i);
                self.publish_mpris();
                task
            }
            Message::EnqueueTrack(id) => {
                self.show_suggest = false;
                self.close_menu();
                self.cancel_drag();
                let task = self.enqueue_track_task(&id);
                self.publish_mpris();
                task
            }
            Message::ScrobbleSubmitted { target, result } => {
                self.status = match &result {
                    Ok(()) => self.language.scrobbled_to(&target),
                    Err(e) => {
                        yutu_core::app_log::warn("yutu-scrobble", format!("{target}: {e}"));
                        Self::failure_status(self.language.fail_scrobble_target(&target), e)
                    }
                };
                self.publish_mpris();
                iced::Task::none()
            }
            message => self.update_sync(message),
        }
    }
}
