//! yutu-ui catalog: split from lib.rs (no behavior change).

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
    /// Resolve a track id from the real catalog (search/radio/playlist data).
    pub(crate) fn find_track(&self, id: &str) -> Option<Track> {
        self.catalog.get(id).cloned()
    }

    /// True when `track_id` is already in the playlist (drives the
    /// Add-to/Remove-from label in the context menu).
    pub(crate) fn playlist_has(&self, playlist_id: &str, track_id: &str) -> bool {
        if playlist_id == Self::FAVORITES_ID {
            return self.liked.contains(track_id);
        }
        self.user_playlists
            .iter()
            .find(|p| p.id == playlist_id)
            .is_some_and(|p| p.tracks.iter().any(|t| t.id == track_id))
    }

    /// Cached results for a normalized query (miss on absent or stale).
    pub(crate) fn search_cache_get(&self, query: &str) -> Option<SearchBundle> {
        let (at, bundle) = self.search_cache.get(&search_cache_key(query))?;
        if at.elapsed().as_secs() > SEARCH_CACHE_TTL_SECS {
            return None;
        }
        Some(bundle.clone())
    }

    /// Store results under the normalized query (evict the oldest when
    /// full, never the whole cache).
    pub(crate) fn search_cache_put(&mut self, query: &str, bundle: SearchBundle) {
        if self.search_cache.len() >= SEARCH_CACHE_CAP {
            if let Some(oldest) = self
                .search_cache
                .iter()
                .min_by_key(|(_, (at, _))| *at)
                .map(|(k, _)| k.clone())
            {
                self.search_cache.remove(&oldest);
            }
        }
        self.search_cache
            .insert(search_cache_key(query), (std::time::Instant::now(), bundle));
    }

    /// Cache-first search: paint cached results with no network task,
    /// else None so the caller fires live (or reports offline).
    pub(crate) fn search_cache_hit_task(&mut self, q: &str) -> Option<iced::Task<Message>> {
        let hit = self.search_cache_get(q)?;
        self.searching = false;
        Some(self.apply_search_hit(q, hit))
    }

    /// Register tracks and queue artwork (no painting). Shared by live
    /// completions (fresh or stale) and cache hits. Both halves register:
    /// track menus resolve through the catalog off either list. Raw goes
    /// first so filtered durations win (flat items carry none).
    pub(crate) fn stage_search(
        &mut self,
        bundle: SearchBundle,
    ) -> (SearchBundle, Vec<iced::Task<Message>>) {
        let all_tracks = self.register_tracks(bundle.all.tracks);
        let filtered_tracks = self.register_tracks(bundle.filtered.tracks);
        let bundle = SearchBundle {
            all: SearchAll {
                tracks: all_tracks,
                ..bundle.all
            },
            filtered: SearchResults {
                tracks: filtered_tracks,
                ..bundle.filtered
            },
        };
        // Row keys for collection hits: every list row renders the
        // small raster, so stage it (heroes, cards, and menus warm
        // their own 544px keys on demand via sweeps and menu opens:
        // staging both would double-fetch under the task gates).
        // Album rows share `album_row_cover` exactly (the catalog
        // already holds this bundle's tracks, registered above);
        // artists/playlists use the hit's own thumb, like their views.
        let mut seen = std::collections::HashSet::new();
        let mut art_urls: Vec<String> = Vec::new();
        for a in bundle
            .filtered
            .albums
            .iter()
            .chain(bundle.all.albums.iter())
        {
            // `album_row_cover` returns the rewritten key already.
            if let Some(url) = self.album_row_cover(a) {
                if seen.insert(url.clone()) {
                    art_urls.push(url);
                }
            }
        }
        for u in bundle
            .filtered
            .artists
            .iter()
            .chain(bundle.all.artists.iter())
            .filter_map(|a| a.thumbnail_url.clone())
            .chain(
                bundle
                    .filtered
                    .playlists
                    .iter()
                    .chain(bundle.all.playlists.iter())
                    .filter_map(|p| p.thumbnail_url.clone()),
            )
            .map(|u| Self::row_art_url(&u))
        {
            if seen.insert(u.clone()) {
                art_urls.push(u);
            }
        }
        let mut tasks = self.artwork_tasks(&bundle.filtered.tracks);
        tasks.extend(self.artwork_tasks(&bundle.all.tracks));
        tasks.extend(self.artwork_tasks_for_urls(&art_urls));
        (bundle, tasks)
    }

    /// Paint results into results/base/status/artwork (shared by live
    /// completions and cache hits). Returns the artwork batch.
    pub(crate) fn apply_search_hit(
        &mut self,
        query: &str,
        bundle: SearchBundle,
    ) -> iced::Task<Message> {
        let n = bundle.filtered.tracks.len();
        let m = bundle.filtered.playlists.len();
        let (bundle, tasks) = self.stage_search(bundle);
        self.search_base = bundle.filtered.clone();
        self.search_all_base = bundle.all.clone();
        self.results = filter_results(&bundle.filtered, &self.search_query);
        if self.results.tracks.is_empty() && !bundle.filtered.tracks.is_empty() {
            self.results = bundle.filtered.clone();
        }
        self.all_results = filter_all(&bundle.all, &self.search_query);
        if self.all_results.tracks.is_empty() && !bundle.all.tracks.is_empty() {
            self.all_results = bundle.all;
        }
        self.prune_coll_menu();
        self.status = if m == 0 {
            format!("{n} songs for “{query}”.")
        } else {
            format!("{n} songs + {m} playlists for “{query}”.")
        };
        self.publish_mpris();
        let mut tasks = tasks;
        // Fresh data under a static viewport: sweep the stored window
        // (forced: new urls must fetch without waiting for a scroll)
        // plus the popup top tracks when open.
        tasks.push(self.sweep_visible_art("body", true));
        tasks.extend(self.popup_art_tasks());
        iced::Task::batch(tasks)
    }

    /// Merge a stored row with its catalog entry: catalog wins, but
    /// art the catalog lacks is filled from the stored row (and healed
    /// back into the catalog). Queues then own full Tracks even for
    /// pre-migration rows.
    pub(crate) fn resolve_for_play(&mut self, t: &Track) -> Track {
        let out = self.resolve_for_play_mem(t);
        // Write-through so the healed row survives restarts.
        self.cache_tracks(std::slice::from_ref(&out));
        out
    }

    /// In-memory half of [`Self::resolve_for_play`]: catalog merge with
    /// no SQLite traffic. Bulk paths resolve everything first, then
    /// persist once, so long playlists never fsync per track on the GUI
    /// thread.
    pub(crate) fn resolve_for_play_mem(&mut self, t: &Track) -> Track {
        match self.find_track(&t.id) {
            Some(mut full) => {
                let mut healed = false;
                if full.thumbnail_url.is_none() && t.thumbnail_url.is_some() {
                    full.thumbnail_url = t.thumbnail_url.clone();
                    healed = true;
                }
                if full.thumbnail_large_url.is_none() && t.thumbnail_large_url.is_some() {
                    full.thumbnail_large_url = t.thumbnail_large_url.clone();
                    healed = true;
                }
                if full.plays.is_none() && t.plays.is_some() {
                    full.plays = t.plays.clone();
                    healed = true;
                }
                if healed {
                    self.catalog.insert(full.id.clone(), full.clone());
                }
                full
            }
            None => {
                self.catalog.insert(t.id.clone(), t.clone());
                t.clone()
            }
        }
    }

    /// Register freshly fetched tracks; returns them for chaining.
    /// Also upserts into the SQLite track cache so restarts rebuild the
    /// catalog (Favorites/history) with no search-first step.
    pub(crate) fn register_tracks(&mut self, tracks: Vec<Track>) -> Vec<Track> {
        for t in &tracks {
            self.catalog.insert(t.id.clone(), t.clone());
        }
        self.cache_tracks(&tracks);
        tracks
    }

    /// Memory-only catalog seed (startup path): inserts without touching
    /// SQLite, so opening the app never blocks on writes. Fresh network
    /// data still goes through [`Self::register_tracks`] (small batches).
    pub(crate) fn seed_catalog_mem(&mut self, tracks: Vec<Track>) {
        for t in tracks {
            self.catalog.insert(t.id.clone(), t);
        }
    }

    /// First screenful of track ids for the current route (lazy scope).
    /// Home shows the recent grid (or Favorites fallback); detail routes
    /// show the head of their list; the sidebar covers come from the head
    /// of each playlist's first track. Everything is capped: never a
    /// whole-library sweep.
    pub(crate) fn visible_ids(&self) -> Vec<String> {
        pub(crate) const CAP: usize = 12;
        match &self.route {
            Route::Home => {
                let mut ids: Vec<String> = Vec::new();
                let mut seen = std::collections::HashSet::new();
                for id in self.history.iter().rev() {
                    if ids.len() >= 8 {
                        break;
                    }
                    if seen.insert(id.clone()) {
                        ids.push(id.clone());
                    }
                }
                if ids.is_empty() {
                    ids.extend(self.liked_order.iter().take(8).cloned());
                }
                ids
            }
            Route::History => self
                .history_full
                .iter()
                .take(CAP)
                .map(|e| e.track_id.clone())
                .collect(),
            Route::PlaylistDetail(id) => self
                .find_playlist(id)
                .map(|p| p.tracks.iter().take(15).map(|t| t.id.clone()).collect())
                .unwrap_or_default(),
            Route::Artist(id) => self
                .artist_display_tracks(id)
                .into_iter()
                .take(CAP)
                .map(|t| t.id)
                .collect(),
            Route::Album(id) => self
                .album_display_tracks(id)
                .into_iter()
                .take(CAP)
                .map(|t| t.id)
                .collect(),
            Route::Search => self
                .results
                .tracks
                .iter()
                .take(CAP)
                .map(|t| t.id.clone())
                .collect(),
            Route::Discovery => {
                let mut ids: Vec<String> = self.liked_order.iter().take(6).cloned().collect();
                for id in self.history.iter().rev().take(6) {
                    if !ids.contains(id) {
                        ids.push(id.clone());
                    }
                }
                ids
            }
            Route::Radio(_) => self
                .radio_tracks
                .iter()
                .take(15)
                .map(|t| t.id.clone())
                .collect(),
            Route::Lyrics(id) => vec![id.clone()],
            Route::Settings => Vec::new(),
        }
    }

    /// Visible tracks (resolved) for artwork warming: catalog rows first,
    /// falling back to the stored playlist row so covers show even before
    /// hydration fills the catalog.
    pub(crate) fn visible_tracks_for_art(&self) -> Vec<Track> {
        pub(crate) const CAP: usize = 12;
        match &self.route {
            Route::PlaylistDetail(id) => self
                .find_playlist(id)
                .map(|p| p.tracks.into_iter().take(15).collect())
                .unwrap_or_default(),
            Route::Home => {
                let mut out = Vec::new();
                let mut seen = std::collections::HashSet::new();
                for id in self.history.iter().rev() {
                    if out.len() >= 8 {
                        break;
                    }
                    if seen.insert(id.clone()) {
                        if let Some(t) = self.find_track(id) {
                            out.push(t);
                        }
                    }
                }
                if out.is_empty() {
                    out = self.favorite_tracks().into_iter().take(8).collect();
                }
                out
            }
            _ => self
                .visible_ids()
                .into_iter()
                .filter_map(|id| self.find_track(&id))
                .take(CAP)
                .collect(),
        }
    }

    /// Sidebar playlist covers (first track of the first playlists):
    /// disk-cached art shows instantly via [`Self::art_handle`]; this
    /// warms only covers missing from both memory and disk.
    pub(crate) fn sidebar_cover_tracks(&self) -> Vec<Track> {
        self.all_playlists()
            .into_iter()
            .take(8)
            .filter_map(|p| {
                let first = p.tracks.first()?;
                // Catalog row wins (fresh art); stored row is the fallback.
                Some(self.find_track(&first.id).unwrap_or_else(|| first.clone()))
            })
            .collect()
    }

    /// Warm the popup's top tracks (always on screen when shown).
    pub(crate) fn popup_art_tasks(&mut self) -> Vec<iced::Task<Message>> {
        if !self.suggest_open() {
            return Vec::new();
        }
        let tracks: Vec<Track> = self.results.tracks.iter().take(5).cloned().collect();
        self.artwork_tasks(&tracks)
    }

    /// Lazy batch for what is on screen right now: metadata probes for
    /// visible ids lacking a catalog row or art (deduped via `hydrating`,
    /// cooldown-skipped when a probe came back empty, capped small), plus
    /// artwork fetches for visible covers missing from both memory and
    /// disk (also capped). Cache hits render instantly; network fills the
    /// rest without ever sweeping the library.
    pub(crate) fn ensure_visible_tasks(&mut self) -> iced::Task<Message> {
        pub(crate) const META_CAP: usize = 8;
        // Shelf rankings are memoized (views read the cache, never
        // recompute): refresh on navigation while the shelves are
        // visible so the first paint is already correct.
        if matches!(self.route, Route::Home | Route::Discovery) {
            self.refresh_home_shelves();
        }
        let mut need_meta: Vec<String> = Vec::new();
        for id in self.visible_ids() {
            if need_meta.len() >= META_CAP {
                break;
            }
            if self.hydrating.contains(&id) || Self::cooled(&self.hydrate_cooldown, &id) {
                continue;
            }
            let needs = match self.catalog.get(&id) {
                None => true,
                Some(t) => t.thumbnail_url.is_none() && t.thumbnail_large_url.is_none(),
            };
            if needs {
                self.hydrating.insert(id.clone());
                need_meta.push(id);
            }
        }
        let mut tasks = Vec::new();
        if !need_meta.is_empty() {
            tasks.push(Self::hydrate_task(need_meta));
        }
        // Artwork: visible rows plus sidebar covers, disk-first. Row
        // slots carry the row raster (what rows render); hero keys warm
        // only for hero displays, so the same image is never fetched at
        // two rasters. The right panel shows on every route, so its head
        // warms too.
        let mut art_tracks = self.visible_tracks_for_art();
        art_tracks.extend(self.sidebar_cover_tracks());
        art_tracks.extend(self.queue.items().iter().take(6).cloned());
        art_tracks.extend(self.autoplay.iter().take(2).cloned());
        tasks.extend(self.artwork_tasks(&art_tracks));
        // Home-pick cards render heroes but live in the horizontal strip
        // (no sweep slots of their own): warm them here on the Home
        // route, deduped by the usual memory/in-flight/disk gates.
        // Same for the new shelf strips (library + albums cards).
        if matches!(self.route, Route::Home) {
            for t in self.home_pick_tracks() {
                if let Some(task) = self.large_art_task(&t) {
                    tasks.push(task);
                }
            }
            let mut strip_art: Vec<Track> = self
                .home_strip_playlists()
                .into_iter()
                .filter_map(|p| {
                    p.tracks
                        .first()
                        .and_then(|t| self.find_track(&t.id))
                        .or_else(|| p.tracks.first().cloned())
                })
                .take(12)
                .collect();
            strip_art.extend(
                self.home_shelves
                    .albums
                    .iter()
                    .filter_map(|g| g.tracks.first().cloned())
                    .take(12),
            );
            for t in &strip_art {
                if let Some(task) = self.large_art_task(t) {
                    tasks.push(task);
                }
            }
        }
        // Viewport sweeps for the scrolled lists (forced: new content
        // must fetch its window without waiting for a scroll event).
        for key in ["body", "home", "queue"] {
            tasks.push(self.sweep_visible_art(key, true));
        }
        iced::Task::batch(tasks)
    }

    /// True when `key` failed within the retry cooldown (stops refetch
    /// loops: completion chaining re-scans visible on every arrival).
    pub(crate) fn cooled(map: &HashMap<String, std::time::Instant>, key: &str) -> bool {
        map.get(key)
            .is_some_and(|at| at.elapsed().as_secs() < FAILED_COOLDOWN_SECS)
    }

    /// Record a failure with now (capped map: drop history past 500).
    pub(crate) fn note_failed(map: &mut HashMap<String, std::time::Instant>, key: String) {
        if map.len() >= 500 {
            map.clear();
        }
        map.insert(key, std::time::Instant::now());
    }

    /// Clear a failure on success (a later URL rotation refetches cleanly).
    pub(crate) fn note_success(map: &mut HashMap<String, std::time::Instant>, key: &str) {
        map.remove(key);
    }

    /// Best-effort write-through to the track metadata cache (never
    /// fails the caller: a missing DB just means next launch re-fetches).
    pub(crate) fn cache_tracks(&self, tracks: &[Track]) {
        if tracks.is_empty() {
            return;
        }
        if let Ok(store) = LibraryStore::open(&self.library_path) {
            let _ = store.upsert_tracks(tracks);
        }
    }

    /// Background fetch for ids the catalog lacks. Small lazy batches
    /// only (callers cap to the visible screenful): never a library sweep.
    /// One concurrent probe per id (not one sequential task): covers on a
    /// screenful land together instead of one watch-playlist round trip at
    /// a time. Each completion registers independently.
    pub(crate) fn hydrate_task(ids: Vec<String>) -> iced::Task<Message> {
        if ids.is_empty() {
            return iced::Task::none();
        }
        // Cap: 8 concurrent probes per batch.
        let mut tasks = Vec::new();
        for id in ids.into_iter().take(8) {
            let requested = vec![id.clone()];
            tasks.push(iced::Task::perform(
                async move { hydrate_one(id).await },
                move |tracks| Message::TracksHydrated {
                    requested: requested.clone(),
                    tracks,
                },
            ));
        }
        iced::Task::batch(tasks)
    }

    /// On-demand fetch for one unknown id: reports "fetching" and fires
    /// the hydration probe so the song appears without a manual search.
    /// Skips when a probe for the id is already in flight (Tick or
    /// navigation fired it first): no duplicate network work.
    pub(crate) fn unknown_track_task(&mut self, id: &str) -> iced::Task<Message> {
        self.status = self.language.fetching_info(id);
        self.publish_mpris();
        if !self.hydrating.insert(id.to_string()) {
            return iced::Task::none();
        }
        Self::hydrate_task(vec![id.to_string()])
    }

    /// Merge an import into memory only (catalog + likes + playlists):
    /// returns artwork tasks for the new tracks. SQLite writes happen in
    /// the background tasks via [`Self::persist_imported`], never on the
    /// GUI thread, so big imports cannot stall the event loop (the OS
    /// flags the window frozen after seconds without event pumping).
    #[cfg(feature = "innertube")]
    pub(crate) fn merge_imported(
        &mut self,
        tracks: Vec<Track>,
        liked_ids: Vec<String>,
        playlists: Vec<Playlist>,
    ) -> Vec<iced::Task<Message>> {
        // Playlist rows widen the catalog: register them alongside the
        // library tracks so covers and titles survive restarts.
        let mut every = tracks;
        for p in &playlists {
            for t in &p.tracks {
                if !every.iter().any(|x| x.id == t.id) {
                    every.push(t.clone());
                }
            }
        }
        let tracks = self.register_tracks_mem(every);
        for id in &liked_ids {
            self.like_id(id);
        }
        // Re-import refreshes in place (same slot, new rows); new server
        // lists append after the local ones. Lists with unpushed edits
        // keep their local contents until the pushes land (a replace
        // would clobber intent the server hasn't seen).
        for p in playlists {
            if self.playlist_has_pending(&p.id) {
                continue;
            }
            match self.user_playlists.iter().position(|x| x.id == p.id) {
                Some(i) => self.user_playlists[i] = p,
                None => self.user_playlists.push(p),
            }
        }
        self.artwork_tasks(&tracks)
    }

    /// Memory-only track registration (no SQLite): the import path
    /// persists separately in background tasks.
    #[cfg(feature = "innertube")]
    pub(crate) fn register_tracks_mem(&mut self, tracks: Vec<Track>) -> Vec<Track> {
        for t in &tracks {
            self.catalog.insert(t.id.clone(), t.clone());
        }
        tracks
    }

    /// SQLite half of an import merge (runs inside background tasks, not
    /// in `update`): track cache upsert, likes with metadata when the
    /// payload carries it (bare ids otherwise), server playlists mirrored
    /// by id. Best effort throughout (an unopenable DB just means the
    /// next launch re-fetches).
    #[cfg(feature = "innertube")]
    pub(crate) fn persist_imported(
        library_path: &str,
        tracks: &[Track],
        liked_ids: &[String],
        playlists: &[Playlist],
    ) {
        let Ok(store) = LibraryStore::open(library_path) else {
            return;
        };
        let _ = store.upsert_tracks(tracks);
        let by_id: std::collections::HashMap<&str, &Track> =
            tracks.iter().map(|t| (t.id.as_str(), t)).collect();
        for id in liked_ids {
            if let Some(t) = by_id.get(id.as_str()) {
                let _ = store.like_track(t);
            } else {
                let _ = store.like(id);
            }
        }
        // Server playlists (real ids, not `u{seq}` shells): mirror by id
        // so re-imports refresh renames and removals instead of drifting
        // from what is on screen. Local `u{seq}` rows never collide with
        // server ids.
        for p in playlists {
            let _ = store.sync_server_playlist(&p.id, &p.title, &p.tracks);
        }
    }

    /// True for playlists that live on YouTube Music (real server ids from
    /// an import or a synced create). Local `u{seq}` shells, the virtual
    /// Favorites list, and the `LM` liked-music pseudo-list
    /// (likes/dislikes sync via `rate_song`, never playlist edits) never
    /// sync as playlists.
    #[cfg_attr(not(feature = "innertube"), allow(dead_code))]
    pub(crate) fn is_server_playlist(id: &str) -> bool {
        if id == Self::FAVORITES_ID || id == "LM" {
            return false;
        }
        if id.len() < 6 {
            return false;
        }
        if let Some(rest) = id.strip_prefix('u') {
            if rest.parse::<u64>().is_ok() {
                return false;
            }
        }
        true
    }

    /// Ease the banner's displayed song count toward the real arrived
    /// total (halving the gap per Tick): pages land in ~100-song bursts
    /// microseconds apart, so raw counts would teleport 0 to 100%. The
    /// display never exceeds the truth and converges exactly; the status
    /// line always shows real numbers.
    #[cfg(feature = "innertube")]
    pub(crate) fn ease_import_shown(&mut self) {
        if !self.import_active {
            return;
        }
        let target = self.import_song_target();
        if self.import_shown < target {
            let gap = target - self.import_shown;
            self.import_shown += gap.div_ceil(2).max(1);
        } else {
            self.import_shown = target;
        }
    }

    /// Real arrived songs for the banner: merged songs plus in-flight
    /// rows from lists with known totals (unknown-total rows join at
    /// merge, never mid-flight, so the bar never overshoots).
    #[cfg(feature = "innertube")]
    pub(crate) fn import_song_target(&self) -> usize {
        self.import_songs
            + self
                .import_live
                .values()
                .filter(|e| e.count.is_some())
                .map(|e| e.arrived)
                .sum::<usize>()
    }

    /// Everything the app has really seen (discovery candidate pool).
    pub(crate) fn catalog_tracks(&self) -> Vec<Track> {
        let mut v: Vec<Track> = self.catalog.values().cloned().collect();
        v.sort_by(|a, b| a.title.cmp(&b.title));
        v
    }

    /// Move the captured fresh-import source into the cleanup modal once
    /// the connection succeeds (base stage). Missing files arm nothing;
    /// session re-imports and syncs never capture a source, so they
    /// never prompt.
    #[cfg(feature = "innertube")]
    pub(crate) fn arm_cookie_cleanup(&mut self) {
        if let Some(src) = self.pending_cookie_source.take() {
            if !src.is_empty() && std::path::Path::new(&src).exists() {
                self.pending_cookie_cleanup = Some(src);
                // Restart the entrance so the card rises from below
                // (unless the scrobbling dialog is already settled on
                // the shared ticker).
                if !self.scrobble_modal_open {
                    self.modal_anim = 0.0;
                }
            }
        }
    }

    /// Entrance target for the modal layer: 1 while any dialog is open.
    pub(crate) fn modal_target(&self) -> f32 {
        #[cfg(feature = "innertube")]
        if self.pending_cookie_cleanup.is_some() {
            return 1.0;
        }
        if self.scrobble_modal_open {
            return 1.0;
        }
        #[cfg(feature = "innertube")]
        if self.pending_signout {
            return 1.0;
        }
        if self.pending_playlist_delete.is_some() {
            1.0
        } else {
            0.0
        }
    }

    /// True while a modal entrance still needs ticker frames.
    pub(crate) fn modal_animating(&self) -> bool {
        self.modal_anim != self.modal_target()
    }

    /// Merge the import base stage (songs + liked) immediately so results
    /// show without waiting for playlists, then fan out one task per
    /// playlist header. Each arrival updates the status line, so a slow
    /// import reads as progress instead of a frozen window. Bumps the run
    /// generation so stragglers from an older run are ignored.
    ///
    /// Sync pulls reconcile server-wins (web likes/dislikes replace the
    /// local sets, absent server lists are pruned, still-pending pushes
    /// ride over both); headers with pending delete/add/remove rows are
    /// never fetched, so unpushed intent cannot be clobbered.
    #[cfg(feature = "innertube")]
    pub(crate) fn begin_playlist_stages(
        &mut self,
        kind: ImportKind,
        staged: StagedImport,
    ) -> iced::Task<Message> {
        if kind == ImportKind::Cookie {
            self.cookie_logged_in = cookie_session_exists();
        }
        // Never resurrect lists the user deleted locally but hasn't
        // pushed yet, and never refresh lists with unpushed edits.
        let headers: Vec<yutu_core::ServerPlaylist> = staged
            .headers
            .iter()
            .filter(|h| {
                !self.playlist_has_pending(&h.id)
                    && !self.has_pending_op("delete", Some(&h.id), None)
            })
            .cloned()
            .collect();
        let n = staged.tracks.len();
        let m = staged.liked_ids.len();
        let total = headers.len();
        self.import_songs = n;
        self.import_likes = m;
        self.import_done = 0;
        self.import_total = total;
        self.import_expected = n;
        self.import_shown = n;
        self.import_live.clear();
        self.import_finished.clear();
        self.import_gen += 1;
        let gen = self.import_gen;
        let mut tasks = self.reconcile_base(&staged, &headers);
        if total == 0 {
            self.login_busy = false;
            self.import_active = false;
            self.sync_active = false;
            self.mark_synced();
            self.status = self.language.import_done_line(kind, n, m);
            self.publish_mpris();
            return iced::Task::batch(tasks);
        }
        self.login_busy = true;
        self.import_active = true;
        self.sync_active = kind == ImportKind::Sync;
        self.status = self.language.import_fetching_line(kind, n, m, total);
        self.publish_mpris();
        let backend = staged.backend;
        let library_path = self.library_path.clone();
        tasks.extend(headers.into_iter().enumerate().map(|(i, h)| {
            Self::playlist_import_task(
                backend.clone(),
                library_path.clone(),
                kind,
                gen,
                h,
                i,
                total,
            )
        }));
        iced::Task::batch(tasks)
    }

    /// Base-stage reconcile (memory half; the disk half ran in
    /// [`Self::persist_base_pull`]): catalog register, server-wins
    /// like/dislike replace with still-pending intent re-applied, and
    /// pruning of server lists absent from clean headers. `headers` are
    /// the fetchable subset (pending-op lists filtered out).
    #[cfg(feature = "innertube")]
    pub(crate) fn reconcile_base(
        &mut self,
        staged: &StagedImport,
        headers: &[yutu_core::ServerPlaylist],
    ) -> Vec<iced::Task<Message>> {
        let tracks = self.register_tracks_mem(staged.tracks.clone());
        if staged.library_ok {
            self.liked = staged.liked_ids.iter().cloned().collect();
            self.liked_order = staged.liked_ids.clone();
            self.disliked = staged.disliked_ids.iter().cloned().collect();
            self.disliked_order = staged.disliked_ids.clone();
            // Still-pending pushes ride over the server sets (the disk
            // half in `apply_sync_pull` does the same).
            for (act, id) in self.pending_rate_ops() {
                match act.as_str() {
                    "like" => self.like_id(&id),
                    "unlike" => self.unlike_id(&id),
                    "dislike" => self.dislike_id(&id),
                    _ => self.undislike_id(&id),
                }
            }
        }
        if staged.headers_clean {
            let present: std::collections::HashSet<&str> =
                headers.iter().map(|h| h.id.as_str()).collect();
            let pruned: Vec<String> = self
                .user_playlists
                .iter()
                .filter(|p| {
                    Self::is_server_playlist(&p.id)
                        && !present.contains(p.id.as_str())
                        && !self.playlist_has_pending(&p.id)
                })
                .map(|p| p.id.clone())
                .collect();
            if !pruned.is_empty() {
                if let Ok(store) = LibraryStore::open(&self.library_path) {
                    for id in &pruned {
                        let _ = store.delete_playlist(id);
                    }
                }
                self.user_playlists.retain(|p| !pruned.contains(&p.id));
                if let Route::PlaylistDetail(id) = &self.route {
                    if pruned.contains(id) {
                        self.go(Route::Home);
                    }
                }
            }
        }
        self.artwork_tasks(&tracks)
    }

    /// Still-pending rate pushes as (op, track id) pairs (sync
    /// reconciliation re-applies them over server-wins sets).
    #[cfg(feature = "innertube")]
    pub(crate) fn pending_rate_ops(&self) -> Vec<(String, String)> {
        let Ok(store) = LibraryStore::open(&self.library_path) else {
            return Vec::new();
        };
        store
            .pending_ops()
            .unwrap_or_default()
            .into_iter()
            .filter(|o| matches!(o.op.as_str(), "like" | "unlike" | "dislike" | "undislike"))
            .filter_map(|o| o.track_id.map(|id| (o.op, id)))
            .collect()
    }

    /// True when a matching pending row exists (sync skips such lists).
    #[cfg(feature = "innertube")]
    pub(crate) fn has_pending_op(
        &self,
        op: &str,
        playlist_id: Option<&str>,
        track_id: Option<&str>,
    ) -> bool {
        let Ok(store) = LibraryStore::open(&self.library_path) else {
            return false;
        };
        store.pending_ops().unwrap_or_default().iter().any(|o| {
            o.op == op
                && o.playlist_id.as_deref() == playlist_id
                && o.track_id.as_deref() == track_id
        })
    }

    /// True when a playlist has pending add/remove rows (store-level
    /// check for background tasks, which have no `&self`).
    #[cfg(feature = "innertube")]
    pub(crate) fn playlist_has_pending_store(library_path: &str, playlist_id: &str) -> bool {
        let Ok(store) = LibraryStore::open(library_path) else {
            return false;
        };
        store.has_playlist_pending(playlist_id).unwrap_or(false)
    }

    /// True when a playlist has pending add/remove rows.
    #[cfg(feature = "innertube")]
    pub(crate) fn playlist_has_pending(&self, playlist_id: &str) -> bool {
        Self::playlist_has_pending_store(&self.library_path, playlist_id)
    }

    /// Record a not-yet-confirmed server write (see `PendingOp`).
    /// Offline builds: local state is the truth, nothing to record.
    #[cfg(feature = "innertube")]
    pub(crate) fn record_pending(
        &self,
        op: &str,
        playlist_id: Option<&str>,
        track_id: Option<&str>,
        title: Option<&str>,
    ) {
        let Ok(store) = LibraryStore::open(&self.library_path) else {
            return;
        };
        let _ = store.record_pending(op, playlist_id, track_id, title);
    }

    /// Offline builds: server sync is a no-op (local state is the truth).
    #[cfg(not(feature = "innertube"))]
    pub(crate) fn record_pending(
        &self,
        _op: &str,
        _playlist_id: Option<&str>,
        _t: Option<&str>,
        _ti: Option<&str>,
    ) {
    }

    /// Drop one confirmed rate push (exact-op match, so a stale
    /// in-flight task cannot clear a newer intent).
    #[cfg(feature = "innertube")]
    pub(crate) fn clear_pending_rate(&self, op: &str, track_id: &str) {
        let Ok(store) = LibraryStore::open(&self.library_path) else {
            return;
        };
        let _ = store.clear_pending(op, None, Some(track_id));
    }
}
