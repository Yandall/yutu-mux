//! yutu-ui queue_artwork: queue ops, artwork pipeline, home shelves and text (split from lib.rs, no behavior change).

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
    pub(crate) fn queue_many(
        &mut self,
        tracks: Vec<Track>,
        label: impl Into<String>,
    ) -> iced::Task<Message> {
        let label = label.into();
        let n = tracks.len();
        if n == 0 {
            self.status = self.language.nothing_to_queue(&label);
            return iced::Task::none();
        }
        // A bulk queue (radio/play-all) absorbs matching autoplay ids so
        // the two lists never show the same song twice.
        let incoming: std::collections::HashSet<&str> =
            tracks.iter().map(|t| t.id.as_str()).collect();
        self.autoplay.retain(|t| !incoming.contains(t.id.as_str()));
        let first_new = self.queue.len();
        // Warm art for the incoming head (thumbs, 12 tracks under the
        // task cap); the rest paints as the ahead-window reaches them
        // on advance. Deduped, so repeats cost nothing.
        let art_tasks = self.artwork_tasks(&tracks);
        // Resolve in memory first, persist once: per-track SQLite traffic
        // (open + fsync each) freezes the GUI on long playlists.
        let resolved: Vec<Track> = tracks
            .iter()
            .map(|t| self.resolve_for_play_mem(t))
            .collect();
        self.cache_tracks(&resolved);
        for rt in resolved {
            self.queue.push(rt);
        }
        self.queue.jump_to(first_new);
        self.status = self.language.queued_tracks(&label, n);
        self.apply_volume();
        let mut tasks = art_tasks;
        tasks.push(self.fetch_current_task());
        iced::Task::batch(tasks)
    }

    /// Play-all for the current detail view (artist/album/playlist/
    /// radio/discovery/search). With `shuffle`, the fresh queue plays in
    /// fully shuffled order (random first track, always reshuffled).
    pub(crate) fn play_all_tracks(&mut self, shuffle: bool) -> iced::Task<Message> {
        let (tracks, label) = match &self.route {
            Route::Artist(id) => {
                let name = self.resolve_artist_name(id);
                (
                    self.artist_display_tracks(id),
                    self.language.artist_queue_label(&name),
                )
            }
            Route::Album(id) => {
                let title = self.resolve_album_title(id);
                (
                    self.album_display_tracks(id),
                    self.language.album_queue_label(&title),
                )
            }
            Route::PlaylistDetail(id) => match self.find_playlist(id) {
                Some(p) => (
                    self.playlist_ordered_tracks(&p),
                    self.language.playlist_queue_label(&p.title),
                ),
                None => (Vec::new(), self.language.playlist_not_found_plain()),
            },
            Route::Radio(_) => (
                self.radio_tracks.clone(),
                self.language.radio_label().to_string(),
            ),
            Route::Discovery => (
                self.weekly_mix_tracks(),
                self.language.weekly_mix().to_string(),
            ),
            _ => (
                self.results.tracks.clone(),
                self.language.search_results_label().to_string(),
            ),
        };
        if tracks.is_empty() {
            self.status = self.language.nothing_to_queue(&label);
            return iced::Task::none();
        }
        // Fresh context (like PlayNow): the old queue and tail go away,
        // then the list plays (shuffled first when asked, so the first
        // fetch already targets the random head).
        self.play_fresh(tracks, &label, shuffle)
    }

    /// Shuffle-play a known track list (search hero actions): same fresh
    /// context as play-all, always shuffled, honest empty status.
    pub(crate) fn shuffle_tracks(
        &mut self,
        tracks: Vec<Track>,
        label: impl Into<String>,
    ) -> iced::Task<Message> {
        let label = label.into();
        if tracks.is_empty() {
            self.status = self.language.nothing_to_play_yet(&label);
            return iced::Task::none();
        }
        self.play_fresh(tracks, &label, true)
    }

    pub(crate) fn play_index(&mut self, i: usize) -> iced::Task<Message> {
        match self.results.tracks.get(i).cloned() {
            Some(t) => self.enqueue(t),
            None => {
                self.status = self.language.no_track_at(i);
                iced::Task::none()
            }
        }
    }

    pub(crate) fn enqueue_track_task(&mut self, id: &str) -> iced::Task<Message> {
        match self.find_track(id) {
            Some(t) => self.enqueue(t),
            None => self.unknown_track_task(id),
        }
    }

    /// Fetch missing artwork for freshly seen tracks (cap: 12 per payload,
    /// deduped by URL, never refetched once attempted). Row raster only:
    /// rows render it, and hero cards warm their own 544px key on
    /// demand, so warming both here would download every cover twice.
    pub(crate) fn artwork_tasks(&mut self, tracks: &[Track]) -> Vec<iced::Task<Message>> {
        let mut seen = std::collections::HashSet::new();
        let urls: Vec<String> = tracks
            .iter()
            .filter_map(|t| t.thumbnail_url.as_deref())
            .map(Self::row_art_url)
            .filter(|u| seen.insert(u.clone()))
            .collect();
        self.artwork_tasks_for_urls(&urls)
    }

    /// Prune the on-disk art cache (day-old covers swept, survivors
    /// LRU-capped by size), at most every interval: keeps browsing
    /// history from growing the cache without bound or pinning it
    /// past the TTL.
    pub(crate) fn maybe_prune_artwork(&mut self) {
        let now = std::time::Instant::now();
        if self
            .art_pruned_at
            .is_some_and(|at| now.duration_since(at).as_secs() < ART_PRUNE_MIN_SECS)
        {
            return;
        }
        self.art_pruned_at = Some(now);
        prune_artwork_dir_in(&artwork_dir(), ART_CACHE_MAX_BYTES);
    }

    /// Same fetch for any URL list (album covers, artist photos).
    /// Skips memory, in-flight, on-disk, and cooldown-failed URLs.
    pub(crate) fn artwork_tasks_for_urls(&mut self, urls: &[String]) -> Vec<iced::Task<Message>> {
        let mut tasks = Vec::new();
        for url in urls {
            if self.art_mem(url).is_none()
                && !self.artwork_loading.contains(url)
                && tasks.len() < 12
                && !artwork_file_exists(url)
                && !Self::cooled(&self.artwork_failed, url)
            {
                self.artwork_loading.insert(url.clone());
                let url_owned = url.clone();
                let url_for_msg = url_owned.clone();
                tasks.push(iced::Task::perform(
                    async move { yutu_player::fetch_bytes(&url_owned).await },
                    move |result| Message::ArtworkReady {
                        url: url_for_msg.clone(),
                        result: result.map_err(|e| e.to_string()),
                    },
                ));
            }
        }
        tasks
    }

    /// Fresh in-memory handle for `url`, if any: entries older than
    /// the TTL read as missing (stale handles linger until the count
    /// cap clears them, but nothing renders them).
    pub(crate) fn art_mem(&self, url: &str) -> Option<iced::widget::image::Handle> {
        self.artwork
            .get(url)
            .filter(|(_, at)| art_age_fresh(at.elapsed()))
            .map(|(handle, _)| handle.clone())
    }

    /// Cover/photo art by URL at any size (albums, artists).
    /// Memory first, then the on-disk art cache (`Handle::from_path`
    /// stays cached by iced, so repeated rows decode once).
    pub(crate) fn art_handle(&self, url: &str) -> Option<iced::widget::image::Handle> {
        if let Some(handle) = self.art_mem(url) {
            return Some(handle);
        }
        artwork_path_for(url).map(iced::widget::image::Handle::from_path)
    }

    /// Lazy image component (the only image renderer: every art site
    /// delegates here). Cache order is memory, then disk, then the
    /// fixed placeholder box (identical dimensions either way, so rows
    /// never jump when art lands). Fetching is visibility-driven and
    /// never triggered here: scroll position decides what is on
    /// screen (iced has no intersection observer), and sweeps fetch
    /// exactly that window plus overscan.
    pub(crate) fn lazy_image(&self, url: Option<&str>, size: f32) -> iced::Element<'_, Message> {
        use iced::widget::{container, image};
        match url.and_then(|u| self.art_handle(u)) {
            Some(handle) => container(image(handle.clone()).width(size).height(size))
                .style(card_bg)
                .padding(0)
                .width(iced::Length::Fixed(size))
                .height(iced::Length::Fixed(size))
                .align_x(iced::alignment::Horizontal::Center)
                .align_y(iced::alignment::Vertical::Center)
                .into(),
            None => container(icon("music", size * 0.4, MUTED))
                .style(card_bg)
                .padding(0)
                .width(iced::Length::Fixed(size))
                .height(iced::Length::Fixed(size))
                .align_x(iced::alignment::Horizontal::Center)
                .align_y(iced::alignment::Vertical::Center)
                .into(),
        }
    }

    /// Fetch art for what the viewport covers right now (plus
    /// overscan), on one scrollable. Scroll storms collapse: same
    /// bucket + minimum gap skips, and the fetch itself dedupes
    /// memory/in-flight/disk/cooldown. Forced sweeps (new data, nav)
    /// skip the gates but keep the fetch dedup.
    pub(crate) fn sweep_visible_art(
        &mut self,
        key: &'static str,
        force: bool,
    ) -> iced::Task<Message> {
        // Backpressure: completions chain the remainder, so a deep
        // backlog pauses instead of piling concurrent fetches.
        if self.artwork_loading.len() >= ART_INFLIGHT_CAP {
            return iced::Task::none();
        }
        let y = self.scroll_offsets.get(key).copied().unwrap_or(0.0);
        let bucket = (y / ART_BUCKET_PX) as i64;
        let now = std::time::Instant::now();
        if !force
            && self.art_buckets.get(key) == Some(&bucket)
            && self
                .art_sweep_at
                .get(key)
                .is_some_and(|at| now.duration_since(*at).as_millis() < ART_SWEEP_MIN_MS)
        {
            return iced::Task::none();
        }
        self.art_buckets.insert(key, bucket);
        self.art_sweep_at.insert(key, now);
        let urls = self.visible_art_urls(key);
        if urls.is_empty() {
            return iced::Task::none();
        }
        iced::Task::batch(self.artwork_tasks_for_urls(&urls))
    }

    /// Viewport height estimate (window minus header, player and
    /// padding). Overscan absorbs the slack, so this never needs to
    /// be exact.
    pub(crate) fn viewport_h(&self) -> f32 {
        self.window
            .map(|s| (s.height - 240.0).max(400.0))
            .unwrap_or(600.0)
    }

    /// Card columns for wrapping grids at the current center width.
    pub(crate) fn grid_cols(&self) -> usize {
        let ww = self.window.map(|s| s.width).unwrap_or(1280.0);
        let avail = (ww - 16.0 - self.sidebar_w - 8.0 - self.right_w - 8.0 - 40.0).min(1110.0);
        ((avail / (HOME_CARD_W + 10.0)) as usize).max(1)
    }

    /// Art URLs currently in the viewport window (plus overscan) for a
    /// scrollable key, deduped in display order and capped.
    pub(crate) fn visible_art_urls(&self, key: &str) -> Vec<String> {
        let y = self.scroll_offsets.get(key).copied().unwrap_or(0.0);
        let top = (y - ART_OVERSCAN_PX).max(0.0);
        let bottom = y + self.viewport_h() + ART_OVERSCAN_PX;
        let mut seen = std::collections::HashSet::new();
        let mut urls = Vec::new();
        for (url, yy) in self.art_slots(key) {
            if urls.len() >= ART_SWEEP_CAP {
                break;
            }
            if yy >= top && yy <= bottom && seen.insert(url.clone()) {
                urls.push(url);
            }
        }
        urls
    }

    /// Home "Jump back in" picks (single source for the grid view and
    /// the art visibility slots): recent history, else liked tracks.
    pub(crate) fn home_pick_tracks(&self) -> Vec<Track> {
        let mut seen = std::collections::HashSet::new();
        let mut recent: Vec<Track> = Vec::new();
        for id in self.history.iter().rev() {
            if recent.len() >= 8 {
                break;
            }
            if seen.insert(id.clone()) {
                if let Some(t) = self.find_track(id) {
                    recent.push(t);
                }
            }
        }
        // Liked tracks fill an empty history so the grid is alive
        // (add order, deterministic even past 8).
        if recent.is_empty() {
            recent = self.favorite_tracks().into_iter().take(8).collect();
        }
        recent
    }

    /// Play counts per track id from the timestamped log (newest
    /// first). Drives the Home shelves; History stays in charge of
    /// the raw log itself.
    pub(crate) fn play_counts(&self) -> std::collections::HashMap<&str, usize> {
        let mut counts = std::collections::HashMap::new();
        for e in &self.history_full {
            *counts.entry(e.track_id.as_str()).or_insert(0) += 1;
        }
        counts
    }

    /// Quick picks: the heavy-rotation head (most plays first,
    /// ties broken by recency), skipping disliked tracks. Falls back
    /// to the Jump-back-in order when nothing has played yet.
    pub(crate) fn quick_pick_tracks(&self, n: usize) -> Vec<Track> {
        let counts = self.play_counts();
        if counts.is_empty() {
            return self.home_pick_tracks().into_iter().take(n).collect();
        }
        let mut ranked: Vec<(&str, usize, usize)> = Vec::new();
        let mut first_seen: std::collections::HashMap<&str, usize> =
            std::collections::HashMap::new();
        // Newest occurrence wins ties (history_full is newest-first,
        // so the first insert per id is its most recent play).
        for (i, e) in self.history_full.iter().enumerate() {
            first_seen.entry(e.track_id.as_str()).or_insert(i);
        }
        for (id, c) in &counts {
            if self.disliked.contains(*id) {
                continue;
            }
            let recency = first_seen.get(id).copied().unwrap_or(usize::MAX);
            ranked.push((*id, *c, recency));
        }
        // Most plays first; a muted track never outranks a heard one,
        // and newer first-plays win ties (deterministic).
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.2.cmp(&b.2)));
        ranked
            .into_iter()
            .filter_map(|(id, _, _)| self.find_track(id))
            .take(n)
            .collect()
    }

    /// Most-played artist name, if anything has played (drives the
    /// "More from" shelf; ties break toward the most recent play).
    pub(crate) fn top_artist(&self) -> Option<String> {
        // (plays, most-recent log index): newest-first log, so the
        // smallest index is the most recent play.
        let mut by_artist: std::collections::HashMap<String, (usize, usize)> =
            std::collections::HashMap::new();
        for (i, e) in self.history_full.iter().enumerate() {
            if let Some(t) = self.find_track(&e.track_id) {
                let slot = by_artist.entry(t.artist.clone()).or_insert((0, usize::MAX));
                slot.0 += 1;
                slot.1 = slot.1.min(i);
            }
        }
        by_artist
            .into_iter()
            .max_by(|a, b| {
                let (ap, ar) = a.1;
                let (bp, br) = b.1;
                ap.cmp(&bp).then_with(|| br.cmp(&ar))
            })
            .map(|(a, _)| a)
    }

    /// Catalog tracks by one artist, most-played first (ties keep
    /// catalog order, so the shelf is deterministic).
    pub(crate) fn more_from_artist(&self, artist: &str, n: usize) -> Vec<Track> {
        let counts = self.play_counts();
        let catalog = self.catalog_tracks();
        let mut tracks: Vec<(&Track, usize)> = catalog
            .iter()
            .filter(|t| t.artist == artist)
            .map(|t| (t, counts.get(t.id.as_str()).copied().unwrap_or(0)))
            .collect();
        tracks.sort_by_key(|a| std::cmp::Reverse(a.1));
        tracks.into_iter().map(|(t, _)| t.clone()).take(n).collect()
    }

    /// Recommended albums: catalog tracks grouped by (album, artist),
    /// scored by summed plays, most-played group first. Only groups
    /// with artwork render as cards (the art is the draw).
    pub(crate) fn recommended_albums(&self, n: usize) -> Vec<AlbumGroup> {
        let counts = self.play_counts();
        let catalog = self.catalog_tracks();
        let mut groups: std::collections::HashMap<(String, String), Vec<Track>> =
            std::collections::HashMap::new();
        for t in catalog {
            if t.album.is_none() {
                continue;
            }
            if t.thumbnail_url.is_none() && t.thumbnail_large_url.is_none() {
                continue;
            }
            groups
                .entry((t.album.clone().unwrap_or_default(), t.artist.clone()))
                .or_default()
                .push(t);
        }
        let mut scored: Vec<AlbumGroup> = groups
            .into_iter()
            .map(|((album, artist), mut tracks)| {
                let plays: usize = tracks
                    .iter()
                    .map(|t| counts.get(t.id.as_str()).copied().unwrap_or(0))
                    .sum();
                tracks.sort_by(|a, b| a.id.cmp(&b.id));
                AlbumGroup {
                    title: album,
                    artist,
                    plays,
                    tracks,
                }
            })
            .collect();
        // Most-played group first; bigger groups then titles break
        // ties so the shelf order never shuffles between frames.
        scored.sort_by(|a, b| {
            b.plays
                .cmp(&a.plays)
                .then_with(|| b.tracks.len().cmp(&a.tracks.len()))
                .then_with(|| a.title.cmp(&b.title))
        });
        scored.into_iter().take(n).collect()
    }

    /// Tracks for one recommended-album group, in card order (id
    /// order, same as the shelf). Shared by the card play action and
    /// the card right-click menu so both queue the same list.
    pub(crate) fn home_album_tracks(&self, album: &str, artist: &str) -> Vec<Track> {
        self.recommended_albums(usize::MAX)
            .into_iter()
            .find(|g| g.title == album && g.artist == artist)
            .map(|g| g.tracks)
            .unwrap_or_default()
    }

    /// Recompute the memoized Home/Discovery shelves (quick picks,
    /// albums, more-from, weekly mix, forgotten). Ranking scans the
    /// whole catalog plus a history query, so views read the cache
    /// instead of paying that on every frame. Called on navigation
    /// and on Tick while the shelves are visible.
    pub(crate) fn refresh_home_shelves(&mut self) {
        let more_artist = self.top_artist();
        let more = more_artist
            .as_deref()
            .map(|a| self.more_from_artist(a, 8))
            .unwrap_or_default();
        self.home_shelves = HomeShelves {
            quick: self.quick_pick_tracks(8),
            albums: self.recommended_albums(10),
            more_artist,
            more,
            weekly: self.weekly_mix_tracks(),
            forgotten: self.forgotten_tracks(),
        };
    }

    /// User playlists for the Home library strip, in creation order,
    /// capped: every card renders a 544px hero, so an unbounded strip
    /// would pin one big texture per playlist.
    pub(crate) fn home_strip_playlists(&self) -> Vec<&Playlist> {
        const STRIP_CAP: usize = 12;
        self.user_playlists
            .iter()
            .filter(|p| !p.tracks.is_empty())
            .take(STRIP_CAP)
            .collect()
    }

    /// Uniform rows of tracks as art slots (`pitch` includes spacing
    /// and dividers): one entry per track (the row raster rows render;
    /// heroes warm on demand via [`large_art_task`]).
    pub(crate) fn track_row_slots(tracks: &[Track], y0: f32, pitch: f32) -> Vec<(String, f32)> {
        tracks
            .iter()
            .enumerate()
            .filter_map(|(i, t)| {
                t.thumbnail_url
                    .as_deref()
                    .map(|u| (Self::row_art_url(u), y0 + i as f32 * pitch))
            })
            .collect()
    }

    /// Estimated text block height (line height ~1.35): mixed-page
    /// header math only, overscan absorbs the slack.
    pub(crate) fn text_h(size: f32) -> f32 {
        size * 1.35
    }

    /// Art slots for a scrollable key: (url, y-top) in display order.
    /// Uniform lists use exact pitches; mixed pages stack fixed header
    /// estimates (all heroes/cards/rows are fixed-size) under the same
    /// overscan, so estimate error only prefetches neighbors. Sources
    /// mirror the view builders exactly (same vecs, same order).
    pub(crate) fn art_slots(&self, key: &str) -> Vec<(String, f32)> {
        match key {
            "queue" => {
                let mut slots =
                    Self::track_row_slots(self.queue.items(), 0.0, QUEUE_ROW_H + DIVIDER_PX);
                let n = self.queue.len();
                let upcoming_h = if n > 0 {
                    n as f32 * (QUEUE_ROW_H + DIVIDER_PX) - DIVIDER_PX
                } else {
                    0.0
                };
                let auto_y0 = upcoming_h + 2.0 + Self::text_h(13.0) + 2.0;
                slots.extend(Self::track_row_slots(&self.autoplay, auto_y0, TRACK_ROW_H));
                slots
            }
            "body" => self.body_art_slots(),
            // Horizontal strip + flyout need no sweeps (warmed on open).
            _ => Vec::new(),
        }
    }

    /// Art slots for the center body scrollable, per route. Header
    /// heights are fixed-size estimates (see constants above); rows
    /// and grids are exact.
    pub(crate) fn body_art_slots(&self) -> Vec<(String, f32)> {
        const CHIPS_H: f32 = 34.0;
        const HERO_H: f32 = 124.0;
        const SECTION_H: f32 = 32.0;
        match &self.route {
            Route::Home => {
                // Vertical page scrolls under the "body" key (the "home"
                // key belongs to the horizontal strip, which needs no
                // sweep: its cards warm on nav).
                let mut y = BODY_PAD + Self::text_h(24.0) + 12.0;
                let mut slots = Vec::new();
                // Picks grid (same order as the view): the cards render
                // the hero key (`thumb_hero`), so only it is warmed.
                // New shelves above: quick-pick + more-from rows use
                // exact pitches; both card strips warm on nav (same
                // 544px key), so the y-flow just steps past them.
                let quick = &self.home_shelves.quick;
                if !quick.is_empty() {
                    y += Self::text_h(19.0) + 10.0;
                    for (k, pair) in quick.chunks(2).enumerate() {
                        for t in pair {
                            if let Some(url) = t.thumbnail_url.as_deref().map(Self::row_art_url) {
                                slots.push((url, y + k as f32 * ROW_PITCH));
                            }
                        }
                    }
                    y += quick.len().div_ceil(2) as f32 * ROW_PITCH + 12.0;
                }
                let lib_shown = self.user_playlists.iter().any(|p| !p.tracks.is_empty());
                if lib_shown {
                    y += Self::text_h(19.0) + 10.0 + HOME_CARD_H + HOME_STRIP_GAP + 12.0;
                }
                let albums = &self.home_shelves.albums;
                if !albums.is_empty() {
                    y += Self::text_h(19.0) + 10.0 + HOME_CARD_H + HOME_STRIP_GAP + 12.0;
                }
                let more = &self.home_shelves.more;
                if !more.is_empty() {
                    y += Self::text_h(19.0) + 10.0;
                    slots.extend(Self::track_row_slots(more, y, ROW_PITCH));
                    y += more.len() as f32 * ROW_PITCH + 12.0;
                }
                y += Self::text_h(19.0) + 12.0;
                y += Self::text_h(19.0) + 12.0;
                let picks = self.home_pick_tracks();
                let cols = self.grid_cols();
                for (i, t) in picks.iter().enumerate() {
                    let gy = y + (i / cols) as f32 * GRID_PITCH;
                    if let Some(url) = Self::hero_art_url(t) {
                        slots.push((url, gy));
                    }
                }
                slots
            }
            Route::Search => {
                let mut slots = Vec::new();
                match self.search_filter {
                    SearchFilter::All => {
                        // Hero art, then one entry per mixed row.
                        if let Some(hero) = top_hero(&self.all_results) {
                            let y = BODY_PAD + CHIPS_H + 10.0;
                            match hero {
                                SearchHit::Song(t) => {
                                    // The hero card renders the 544px key.
                                    if let Some(url) = Self::hero_art_url(t) {
                                        slots.push((url, y));
                                    }
                                }
                                SearchHit::Album(a) => {
                                    if let Some(url) = self.album_cover(a) {
                                        slots.push((url, y));
                                    }
                                }
                                SearchHit::Artist(a) => {
                                    if let Some(url) = a
                                        .thumbnail_url
                                        .as_deref()
                                        .map(|u| yutu_core::hires_thumb_url(u, 544))
                                    {
                                        slots.push((url, y));
                                    }
                                }
                                SearchHit::Playlist(p) => {
                                    if let Some(url) = p
                                        .thumbnail_url
                                        .as_deref()
                                        .map(|u| yutu_core::hires_thumb_url(u, 544))
                                    {
                                        slots.push((url, y));
                                    }
                                }
                            }
                        }
                        for (i, hit) in order_all_hits(&self.all_results).iter().enumerate() {
                            let y =
                                BODY_PAD + CHIPS_H + 10.0 + HERO_H + 10.0 + i as f32 * ROW_PITCH;
                            match hit {
                                SearchHit::Song(t) => {
                                    // Mixed rows render the row raster.
                                    if let Some(url) =
                                        t.thumbnail_url.as_deref().map(Self::row_art_url)
                                    {
                                        slots.push((url, y));
                                    }
                                }
                                SearchHit::Album(a) => {
                                    if let Some(url) = self.album_row_cover(a) {
                                        slots.push((url, y));
                                    }
                                }
                                SearchHit::Artist(a) => {
                                    if let Some(url) =
                                        a.thumbnail_url.as_deref().map(Self::row_art_url)
                                    {
                                        slots.push((url, y));
                                    }
                                }
                                SearchHit::Playlist(p) => {
                                    if let Some(url) = Self::playlist_row_art(p) {
                                        slots.push((url, y));
                                    }
                                }
                            }
                        }
                    }
                    SearchFilter::Songs => slots.extend(Self::track_row_slots(
                        &self.results.tracks,
                        BODY_PAD + CHIPS_H + 10.0,
                        ROW_PITCH,
                    )),
                    SearchFilter::Albums => {
                        slots.extend(
                            self.grid_art_slots(
                                BODY_PAD + CHIPS_H + 10.0,
                                self.results
                                    .albums
                                    .iter()
                                    .filter_map(|a| self.album_cover(a))
                                    .collect(),
                            ),
                        );
                    }
                    SearchFilter::Artists => {
                        slots.extend(
                            self.grid_art_slots(
                                BODY_PAD + CHIPS_H + 10.0,
                                self.results
                                    .artists
                                    .iter()
                                    .filter_map(|a| {
                                        a.thumbnail_url
                                            .as_deref()
                                            .map(|u| yutu_core::hires_thumb_url(u, 544))
                                    })
                                    .collect(),
                            ),
                        );
                    }
                    SearchFilter::Playlists => {
                        // Playlist rows (same order as the view).
                        for (i, p) in self.results.playlists.iter().enumerate() {
                            let y = BODY_PAD + CHIPS_H + 10.0 + i as f32 * ROW_PITCH;
                            if let Some(url) = Self::playlist_row_art(p) {
                                slots.push((url, y));
                            }
                        }
                    }
                }
                slots
            }
            Route::PlaylistDetail(id) => {
                let y0 =
                    BODY_PAD + Self::text_h(24.0) + 2.0 + Self::text_h(12.0) + 10.0 + 32.0 + 10.0;
                match self.find_playlist(id) {
                    None => Vec::new(),
                    Some(p) => {
                        let ordered = self.playlist_ordered_tracks(&p);
                        let tracks: Vec<Track> = ordered
                            .iter()
                            .map(|t| self.find_track(&t.id).unwrap_or_else(|| t.clone()))
                            .collect();
                        Self::track_row_slots(&tracks, y0, ROW_PITCH)
                    }
                }
            }
            Route::Artist(id) => {
                let name = self.resolve_artist_name(id);
                let mut slots = Vec::new();
                // Header photo (top of page).
                if let Some(url) = self.artist_photo(&name) {
                    slots.push((url, BODY_PAD));
                }
                let popular: Vec<Track> = self
                    .artist_display_tracks(id)
                    .into_iter()
                    .take(10)
                    .collect();
                let pop_y0 = BODY_PAD + 160.0 + 12.0 + SECTION_H;
                slots.extend(Self::track_row_slots(&popular, pop_y0, ROW_PITCH));
                let grid_y0 = pop_y0 + popular.len() as f32 * ROW_PITCH + 12.0 + SECTION_H;
                slots.extend(
                    self.grid_art_slots(
                        grid_y0,
                        self.artist_albums(&name)
                            .into_iter()
                            .take(5)
                            .filter_map(|a| self.album_cover(&a))
                            .collect(),
                    ),
                );
                slots
            }
            Route::Album(id) => {
                let rows = self.album_display_tracks(id);
                Self::track_row_slots(&rows, BODY_PAD + 180.0 + 12.0, ROW_PITCH)
            }
            Route::Discovery => {
                let weekly = &self.home_shelves.weekly;
                let forgotten = &self.home_shelves.forgotten;
                let mut slots = Self::track_row_slots(
                    weekly,
                    BODY_PAD + Self::text_h(22.0) + 10.0 + 32.0 + 10.0,
                    ROW_PITCH,
                );
                let fy0 = BODY_PAD
                    + Self::text_h(22.0)
                    + 10.0
                    + 32.0
                    + 10.0
                    + weekly.len() as f32 * ROW_PITCH
                    + 10.0
                    + 32.0
                    + 10.0;
                slots.extend(Self::track_row_slots(forgotten, fy0, ROW_PITCH));
                slots
            }
            Route::Radio(_) => Self::track_row_slots(
                &self.radio_tracks,
                BODY_PAD + Self::text_h(24.0) + 10.0 + Self::text_h(12.0) + 10.0 + 32.0 + 10.0,
                ROW_PITCH,
            ),
            Route::History => {
                // Mirrors `history_view`: title + count + filter input,
                // then one section column per month (header + uniform
                // rows). Overscan absorbs the input-height estimate.
                const FILTER_H: f32 = 42.0;
                let mut y = BODY_PAD
                    + Self::text_h(22.0)
                    + 10.0
                    + Self::text_h(12.0)
                    + 10.0
                    + FILTER_H
                    + 10.0;
                let mut slots = Vec::new();
                let (groups, _) = self.history_groups(now_unix(), self.history_limit);
                for (_, rows) in &groups {
                    y += Self::text_h(19.0) + 6.0;
                    slots.extend(Self::track_row_slots(rows, y, ROW_PITCH));
                    y += rows.len() as f32 * ROW_PITCH + 10.0;
                }
                slots
            }
            _ => Vec::new(),
        }
    }

    /// Card-grid art slots: row-major at the wrapped column count.
    pub(crate) fn grid_art_slots(&self, y0: f32, urls: Vec<String>) -> Vec<(String, f32)> {
        let cols = self.grid_cols();
        urls.into_iter()
            .enumerate()
            .map(|(i, url)| (url, y0 + (i / cols) as f32 * GRID_PITCH))
            .collect()
    }

    /// Cover/photo art by URL at any size (albums, artists).
    pub(crate) fn art_thumb(&self, url: Option<&str>, size: f32) -> iced::Element<'_, Message> {
        self.lazy_image(url, size)
    }

    /// 40px artwork for rows/panel, music-note placeholder until loaded.
    pub(crate) fn thumb(&self, track: &Track) -> iced::Element<'_, Message> {
        self.thumb_sized(track, 40.0)
    }

    /// Two-line song label for the narrow queue sidebar: the title
    /// wraps to at most two lines (40 chars + ellipsis), the artist is
    /// always a single truncated line below it, and the compact block
    /// is vertically centered by the fixed-height row container. The
    /// column clips, so a narrower window cuts text instead of painting
    /// over the duration/remove controls.
    pub(crate) fn track_text<'a>(
        title: &str,
        artist: &str,
        title_size: u16,
        color: iced::Color,
    ) -> iced::Element<'a, Message> {
        use iced::widget::text::{LineHeight, Wrapping};
        use iced::widget::{column, text};
        use iced::Fill;
        // Explicit 1.25 line height keeps 1- and 2-line titles compact.
        const LH: f32 = 1.25;
        let artist_px = f32::from(title_size.saturating_sub(2).max(11));
        column![
            text(truncate_title(title, 40))
                .size(f32::from(title_size))
                .color(color)
                .width(Fill)
                .line_height(LineHeight::Relative(LH)),
            text(truncate_title(artist, 32))
                .size(artist_px)
                .color(MUTED)
                .width(Fill)
                .wrapping(Wrapping::None)
                .line_height(LineHeight::Relative(LH)),
        ]
        .spacing(2)
        .width(Fill)
        .clip(true)
        .into()
    }

    /// Single-line song label for wide lists (search, library, radio,
    /// queue page): title and artist both truncate with an ellipsis and
    /// never wrap. The column clips, so in a narrow window the text cuts
    /// instead of painting over the duration/action controls, and every
    /// row shares one fixed height.
    pub(crate) fn track_text_single<'a>(
        title: &str,
        artist: &str,
        title_size: u16,
        color: iced::Color,
    ) -> iced::Element<'a, Message> {
        Self::track_text_weighted(title, artist, title_size, color, false)
    }

    /// Button-row variant of [`track_text_single`]: same two-line
    /// layout, button-weight type (queue rows sit inside buttons).
    pub(crate) fn btn_track_text_single<'a>(
        title: &str,
        artist: &str,
        title_size: u16,
        color: iced::Color,
    ) -> iced::Element<'a, Message> {
        Self::track_text_weighted(title, artist, title_size, color, true)
    }

    /// Two-line title/artist cell shared by both variants above.
    pub(crate) fn track_text_weighted<'a>(
        title: &str,
        artist: &str,
        title_size: u16,
        color: iced::Color,
        bold: bool,
    ) -> iced::Element<'a, Message> {
        use iced::widget::text::Wrapping;
        use iced::widget::{column, text};
        use iced::Fill;
        let title = text(truncate_title(title, 45))
            .size(f32::from(title_size))
            .color(color)
            .width(Fill)
            .wrapping(Wrapping::None);
        let title = if bold {
            title.font(medium_font())
        } else {
            title
        };
        let artist = text(truncate_title(artist, 40))
            .size(f32::from(title_size.saturating_sub(2).max(11)))
            .color(MUTED)
            .width(Fill)
            .wrapping(Wrapping::None);
        let artist = if bold {
            artist.font(medium_font())
        } else {
            artist
        };
        column![title, artist,]
            .spacing(2)
            .width(Fill)
            .clip(true)
            .into()
    }

    /// Idle remove-slot glyph: the blank frame at button-icon size.
    /// Parsed once (same precedent as the heart fill-flip); no tint
    /// needed since there is nothing to paint.
    pub(crate) fn empty_slot_icon(size: f32) -> iced::Element<'static, Message> {
        use iced::widget::svg;
        static EMPTY_HANDLE: std::sync::LazyLock<svg::Handle> =
            std::sync::LazyLock::new(|| svg::Handle::from_memory(EMPTY_SLOT_SVG.as_bytes()));
        svg(EMPTY_HANDLE.clone())
            .width(size)
            .height(size)
            .style(|_: &iced::Theme, _| svg::Style { color: None })
            .into()
    }
}
