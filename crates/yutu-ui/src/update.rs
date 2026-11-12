//! yutu-ui update: Elm update dispatcher and sync update (split from lib.rs, no behavior change).

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
    /// Synchronous state updates (plus stream-resolve tasks, which are
    /// Iced-side async, not scrobble submits). Publishes MPRIS before
    /// returning; the `PlayStreamId` early return publishes explicitly.
    pub(crate) fn update_sync(&mut self, message: Message) -> iced::Task<Message> {
        match message {
            Message::PlayTrack(_) | Message::EnqueueTrack(_) => {
                unreachable!("play actions are dispatched in update()")
            }
            Message::ScrobbleSubmitted { .. } => {
                unreachable!("scrobble results are dispatched in update()")
            }
            Message::Navigate(r) => {
                self.navigate(r);
                self.publish_mpris();
                return self.ensure_visible_tasks();
            }
            Message::OpenUrl(url) => {
                // Best effort: a missing opener only logs (stderr
                // keeps the detail, the status line stays quiet).
                if let Err(e) = open::that(&url) {
                    yutu_core::app_log::warn("yutu-ui", format!("open {url} failed: {e}"));
                }
            }
            Message::CheckForUpdates => {
                // One probe at a time (button spam collapses into the
                // in-flight check).
                if self.update_checking {
                    return iced::Task::none();
                }
                #[cfg(feature = "update")]
                {
                    self.update_manual = true;
                    self.update_checking = true;
                    self.status = self.language.checking_for_updates();
                    self.publish_mpris();
                }
                #[cfg(not(feature = "update"))]
                {
                    self.status = self.language.needs_update_check();
                    self.publish_mpris();
                }
                // Offline builds: no HTTP client, the task is a no-op.
                return Self::update_check_task(crate::APP_VERSION.to_string());
            }
            Message::UpdateCheckCompleted { result } => {
                self.update_checking = false;
                self.update_checked = true;
                let manual = std::mem::replace(&mut self.update_manual, false);
                match result {
                    Ok(info) => {
                        if info.newer {
                            self.update_available = Some(info.clone());
                            self.status = self.language.update_available(&info.latest_tag);
                            self.publish_mpris();
                        } else {
                            self.update_available = None;
                            // The silent startup probe stays out of the
                            // way when there is nothing to report; only
                            // manual checks confirm up-to-date.
                            if manual {
                                self.status = self.language.up_to_date(crate::APP_VERSION);
                                self.publish_mpris();
                            }
                        }
                    }
                    Err(e) => {
                        self.update_available = None;
                        yutu_core::app_log::warn("yutu-update", format!("check: {e}"));
                        // Same silence rule as up-to-date: startup stays
                        // on Ready, manual checks report honestly.
                        if manual {
                            self.status =
                                Self::failure_status(self.language.fail_update_check(), &e);
                            self.publish_mpris();
                        }
                    }
                }
            }
            Message::NavBack => {
                // Side buttons never disturb a drag (Esc owns cancel);
                // menus and the popup dismiss first, second press goes.
                // Header buttons share this message, so they inherit the
                // same dismiss-first order by design (one code path).
                if self.drag.is_some() {
                    return iced::Task::none();
                }
                if self.context_menu.is_some()
                    || self.pl_menu.is_some()
                    || self.coll_menu.is_some()
                    || self.sort_menu.is_some()
                {
                    self.close_menu();
                    return iced::Task::none();
                }
                if self.show_suggest {
                    self.show_suggest = false;
                    return iced::Task::none();
                }
                self.go_back();
                self.publish_mpris();
                return self.ensure_visible_tasks();
            }
            Message::NavForward => {
                if self.drag.is_some() {
                    return iced::Task::none();
                }
                if self.context_menu.is_some()
                    || self.pl_menu.is_some()
                    || self.coll_menu.is_some()
                    || self.sort_menu.is_some()
                {
                    self.close_menu();
                    return iced::Task::none();
                }
                if self.show_suggest {
                    self.show_suggest = false;
                    return iced::Task::none();
                }
                self.go_forward();
                self.publish_mpris();
                return self.ensure_visible_tasks();
            }
            Message::SetRightTab(t) => {
                self.right_tab = t;
            }
            Message::SetSearchFilter(f) => {
                self.search_filter = f;
            }
            Message::HomeSearchChanged(q) => {
                return self.apply_search_input(q, true);
            }
            Message::PickSuggestion(q) => {
                return self.apply_search_input(q, false);
            }
            Message::SuggestMoveUp => {
                self.move_suggest(false);
            }
            Message::SuggestMoveDown => {
                self.move_suggest(true);
            }
            Message::SuggestPick => {
                return self.pick_suggest();
            }
            Message::SuggestCompleted { gen, result } => {
                if gen != self.suggest_gen {
                    return iced::Task::none();
                }
                match result {
                    Ok(texts) => {
                        self.suggest = texts.into_iter().take(6).collect();
                    }
                    Err(_) => {
                        self.suggest.clear();
                    }
                }
                self.clamp_suggest();
                let tasks = self.popup_art_tasks();
                self.publish_mpris();
                return iced::Task::batch(tasks);
            }
            Message::SearchDebounced { gen, query } => {
                // Stale keystroke (user kept typing): drop silently.
                if gen != self.search_gen {
                    return iced::Task::none();
                }
                let q = query.trim().to_string();
                if q.is_empty() || q.len() < 2 {
                    return iced::Task::none();
                }
                // Query changed while debouncing (Enter fired first): skip.
                if q != self.search_query.trim() {
                    return iced::Task::none();
                }
                #[cfg(feature = "innertube")]
                {
                    self.status = self.language.searching_for(&q);
                    self.searching = true;
                    self.publish_mpris();
                    // Repeat query (retype, Enter-after-popup): paint the
                    // cached results with no network round trip.
                    if let Some(task) = self.search_cache_hit_task(&q) {
                        return task;
                    }
                    return Self::search_task(q);
                }
                #[cfg(not(feature = "innertube"))]
                {
                    // Cached queries still paint offline.
                    if let Some(task) = self.search_cache_hit_task(&q) {
                        return task;
                    }
                    self.status = self.language.needs_innertube_search_offline();
                }
            }
            Message::SearchStay => {
                self.show_suggest = false;
                // Cancel any pending keystroke debounce: the explicit
                // submit below owns this query, so the timer must not
                // fire a duplicate search for it.
                self.search_gen = self.search_gen.wrapping_add(1);
                self.go(Route::Search);
                let q = self.search_query.trim().to_string();
                #[cfg(feature = "innertube")]
                {
                    if q.is_empty() {
                        self.status = self.language.type_to_search();
                    } else {
                        self.status = self.language.searching_for(&q);
                        self.searching = true;
                        self.publish_mpris();
                        if let Some(task) = self.search_cache_hit_task(&q) {
                            return task;
                        }
                        return Self::search_task(q);
                    }
                }
                #[cfg(not(feature = "innertube"))]
                {
                    if let Some(task) = self.search_cache_hit_task(&q) {
                        return task;
                    }
                    self.status = self.language.needs_innertube_search_offline();
                }
            }
            Message::SearchChanged(q) => {
                self.search_query = q.clone();
                self.results = filter_results(&self.search_base, &q);
                self.all_results = filter_all(&self.search_all_base, &q);
            }
            Message::Search => {
                // Same debounce cancel as SearchStay: the explicit submit
                // owns this query.
                self.search_gen = self.search_gen.wrapping_add(1);
                let q = self.search_query.trim().to_string();
                #[cfg(feature = "innertube")]
                {
                    if q.is_empty() {
                        self.status = self.language.type_to_search();
                    } else {
                        self.status = self.language.searching_for(&q);
                        self.searching = true;
                        self.publish_mpris();
                        if let Some(task) = self.search_cache_hit_task(&q) {
                            return task;
                        }
                        return Self::search_task(q);
                    }
                }
                #[cfg(not(feature = "innertube"))]
                {
                    if let Some(task) = self.search_cache_hit_task(&q) {
                        return task;
                    }
                    self.status = self.language.needs_innertube_search_offline();
                    self.go(Route::Search);
                }
            }
            Message::SearchCompleted { query, result } => {
                self.searching = false;
                self.clamp_suggest();
                match result {
                    Ok(search) => {
                        self.search_cache_put(&query, (*search).clone());
                        // Out-of-order arrival (user typed on): keep the
                        // tracks in the catalog, but never overwrite the
                        // newer query's results.
                        if query.trim() == self.search_query.trim() {
                            return self.apply_search_hit(&query, *search);
                        }
                        let (_, tasks) = self.stage_search(*search);
                        self.publish_mpris();
                        return iced::Task::batch(tasks);
                    }
                    Err(e) => {
                        // Stale-query errors stay silent; current ones report.
                        if query.trim() == self.search_query.trim() {
                            yutu_core::app_log::warn("yutu-search", format!("{query}: {e}"));
                            self.status = Self::failure_status(self.language.fail_search(), &e);
                        }
                    }
                }
            }
            Message::NewPlaylistNameChanged(n) => {
                self.new_playlist_name = n;
            }
            Message::Pause => {
                if let Some(a) = &self.audio {
                    a.pause();
                    self.paused = true;
                    self.status = self.language.paused_word();
                }
            }
            Message::TogglePause => {
                // Explicit transport takes over any startup autoload
                // (its landing must play, not buffer paused).
                self.autoload = false;
                if let Some(task) = self.take_restored_play() {
                    return task;
                }
                if let Some(a) = &self.audio {
                    if self.paused {
                        a.resume();
                        self.paused = false;
                        self.paused_seek = false;
                        self.status = self.language.resumed_word();
                    } else {
                        a.pause();
                        self.paused = true;
                        self.status = self.language.paused_word();
                    }
                } else {
                    self.paused = !self.paused;
                    self.status = self.language.no_audio_device();
                }
            }
            Message::Stop => {
                // Stopped is not finished: disarm the EOF-idle net (the
                // backend is silent with no next song coming), device or
                // no device. A held seek drag dies with the load too.
                self.heard_audio = false;
                self.stream_opened_at = None;
                self.seek_drag = None;
                self.seek_drag_at = None;
                self.seek_paused = false;
                self.pending_seek = None;
                self.autoload = false;
                if let Some(a) = &self.audio {
                    a.stop();
                    self.paused = false;
                    self.status = self.language.stopped_word();
                }
            }
            Message::Next => {
                if self.advance_queue_manual() {
                    self.apply_volume();
                    self.publish_mpris();
                    return self.play_current_task();
                }
            }
            Message::Prev => {
                return self.prev_task();
            }
            Message::Like(id) => {
                // Local state applies first; the server push (if signed
                // in) only ever reports failures, so the row never lies.
                // Like and dislike are one status server-side: setting one
                // clears the other locally too.
                let liked: bool;
                if self.liked.contains(&id) {
                    self.unlike_id(&id);
                    self.persist_like(&id, false);
                    self.status = self.language.removed_favorite(&id);
                    liked = false;
                } else {
                    let known = self.find_track(&id).is_some();
                    self.like_id(&id);
                    self.persist_like(&id, true);
                    if self.disliked.contains(&id) {
                        self.undislike_id(&id);
                        self.persist_dislike(&id, false);
                    }
                    liked = true;
                    if known {
                        self.status = self.language.added_favorite(&id);
                    } else {
                        // Bare id with no metadata yet (older installs only
                        // stored ids): keep the like, fetch the info so the
                        // row appears without a manual search.
                        self.status = self.language.added_favorite_fetching(&id);
                        self.publish_mpris();
                        #[cfg(feature = "innertube")]
                        {
                            return iced::Task::batch(vec![
                                Self::hydrate_task(vec![id.clone()]),
                                Self::like_sync_task(id, true),
                            ]);
                        }
                        #[cfg(not(feature = "innertube"))]
                        {
                            return Self::hydrate_task(vec![id]);
                        }
                    }
                }
                self.publish_mpris();
                self.record_pending(if liked { "like" } else { "unlike" }, None, Some(&id), None);
                return Self::like_sync_task(id, liked);
            }
            Message::Dislike(id) => {
                // "Don't recommend": local dislike first, server push only
                // reports failures. Mutual exclusion with likes (see Like).
                let disliked: bool = if self.disliked.contains(&id) {
                    self.undislike_id(&id);
                    self.persist_dislike(&id, false);
                    self.status = self.language.undislike_removed(&id);
                    false
                } else {
                    self.dislike_id(&id);
                    self.persist_dislike(&id, true);
                    if self.liked.contains(&id) {
                        self.unlike_id(&id);
                        self.persist_like(&id, false);
                    }
                    self.status = self.language.disliked(&id);
                    true
                };
                self.publish_mpris();
                self.record_pending(
                    if disliked { "dislike" } else { "undislike" },
                    None,
                    Some(&id),
                    None,
                );
                return Self::dislike_sync_task(id, disliked);
            }
            Message::SetVolume(v) => {
                self.volume = v.clamp(0.0, 1.0);
                // Dragging the slider always unmutes (standard mixer
                // behavior); the mute button owns the muted flag.
                self.muted = false;
                self.apply_volume();
            }
            Message::ToggleMute => {
                self.muted = !self.muted;
                self.apply_volume();
                self.publish_mpris();
            }
            Message::QueueClear => {
                self.cancel_drag();
                self.queue.clear();
                self.status = if self.autoplay.is_empty() {
                    "Queue cleared.".into()
                } else {
                    format!("Queue cleared ({} autoplay kept).", self.autoplay.len())
                };
            }
            Message::OpenArtist(id) => {
                self.close_menu();
                return self.open_artist(id);
            }
            Message::OpenAlbum(id) => {
                self.close_menu();
                return self.open_album(id);
            }
            Message::OpenPlaylist(id) => {
                // Canonicalise: pre-migration sessions may hand a browse id.
                let id = yutu_core::canonical_playlist_id(&id);
                // Preload the sort pick so the view (immutable) sees it
                // on first frame (the ordered helper reads the cache).
                let _ = self.playlist_sort(&id);
                self.cancel_pl_drag();
                self.pl_hover = None;
                self.pl_action_hover = None;
                self.go(Route::PlaylistDetail(id));
                self.publish_mpris();
                return self.ensure_visible_tasks();
            }
            Message::OpenLyrics(id) => {
                self.close_menu();
                self.go(Route::Lyrics(id.clone()));
                #[cfg(any(feature = "lyrics", feature = "innertube"))]
                {
                    match self.find_track(&id) {
                        None => {
                            // Unknown id with nothing cached: hydrate
                            // metadata first (same as before).
                            if self.lyrics_display_entry(&id).is_none()
                                && !self.lyrics_fetch_pending(&id)
                            {
                                self.status = self.language.fetching_info(&id);
                                self.publish_mpris();
                                return Self::hydrate_task(vec![id]);
                            }
                        }
                        Some(t) => {
                            let tasks = self.start_lyrics_fetch(t);
                            if !tasks.is_empty() {
                                self.publish_mpris();
                                return iced::Task::batch(tasks);
                            }
                        }
                    }
                }
            }
            Message::LyricsReady {
                track_id,
                provider,
                result,
            } => {
                // Per-key removal: a late arrival never clears a newer
                // in-flight fetch for another provider or track.
                self.lyrics_loading.remove(&(track_id.clone(), provider));
                match result {
                    Ok(lyrics) => {
                        self.cache_lyrics(track_id, lyrics, provider);
                    }
                    Err(e) => {
                        // A lyrics miss is routine (no record on this
                        // provider, a network flake after retries): it
                        // always lands in the log, but the bottom bar only
                        // shows it with the `debug-panel` flag. Without it
                        // the bar stays clean: the song plays fine
                        // without lyrics.
                        yutu_core::app_log::warn(
                            "yutu-lyrics",
                            format!("{track_id} ({}): {e}", provider.label()),
                        );
                        #[cfg(feature = "debug-panel")]
                        {
                            self.status = self.language.lyrics_failed(&e);
                        }
                    }
                }
            }
            Message::SetLyricsProvider(provider) => {
                self.sort_menu = None;
                if self.lyrics_provider != provider {
                    self.lyrics_provider = provider;
                    self.persist_lyrics_provider();
                    self.publish_mpris();
                }
                // Both providers preload at song start, so switching is
                // instant when cached; fetch here only tops up whatever
                // is still missing (e.g. an earlier failure).
                #[cfg(any(feature = "lyrics", feature = "innertube"))]
                {
                    let target = match &self.route {
                        Route::Lyrics(id) => Some(id.clone()),
                        _ => self.queue.current().map(|t| t.id.clone()),
                    };
                    if let Some(id) = target {
                        if let Some(t) = self.find_track(&id) {
                            let tasks = self.start_lyrics_fetch(t);
                            if !tasks.is_empty() {
                                self.status = self.language.fetching_lyrics(provider.label());
                                return iced::Task::batch(tasks);
                            }
                        }
                    }
                }
            }
            Message::CycleLyricsProvider(dir) => {
                let next = if dir >= 0 {
                    self.lyrics_provider.next()
                } else {
                    self.lyrics_provider.prev()
                };
                // Manual pick pins this track to strict display (no
                // automatic fallback); fresh song loads stay automatic.
                // The pin records the viewed track, not the provider:
                // picking already makes it the default.
                let target = match &self.route {
                    Route::Lyrics(id) => Some(id.clone()),
                    _ => self.queue.current().map(|t| t.id.clone()),
                };
                if let Some(id) = target {
                    self.lyrics_manual.insert(id);
                    if self.lyrics_manual.len() > LYRICS_MANUAL_CAP {
                        self.lyrics_manual.clear();
                    }
                }
                return self.update(Message::SetLyricsProvider(next));
            }
            Message::ArtworkReady { url, result } => {
                self.artwork_loading.remove(&url);
                match &result {
                    Ok(bytes) if bytes.len() > 512 => {
                        Self::note_success(&mut self.artwork_failed, &url);
                    }
                    _ => {
                        // Tiny/errored bodies never decode: cool down so
                        // chained visible scans do not refetch in a loop.
                        Self::note_failed(&mut self.artwork_failed, url.clone());
                    }
                }
                if let Ok(bytes) = result {
                    if bytes.len() > 512 {
                        // The disk cache holds every fetched cover (TTL
                        // swept, size capped), so rows render from disk
                        // when the in-memory handle is gone, online or off.
                        self.maybe_prune_artwork();
                        save_artwork(&url, &bytes);
                        // The file is the source of truth from here: the
                        // handle points at it instead of pinning a copy
                        // of the encoded bytes in RAM (iced decodes once
                        // and caches the texture per path).
                        let handle = artwork_path_for(&url)
                            .map(iced::widget::image::Handle::from_path)
                            .unwrap_or_else(|| iced::widget::image::Handle::from_bytes(bytes));
                        if self.artwork.len() >= ART_MEM_CAP {
                            // Drop expired covers first: a browsing burst
                            // must not nuke hot entries while stale ones
                            // linger.
                            self.artwork
                                .retain(|_, (_, at)| art_age_fresh(at.elapsed()));
                            if self.artwork.len() >= ART_MEM_CAP {
                                // Oldest half goes (not everything: a full
                                // clear would refetch the viewport at once
                                // and spike back to the cap immediately).
                                let mut oldest: Vec<(std::time::Instant, String)> = self
                                    .artwork
                                    .iter()
                                    .map(|(k, (_, at))| (*at, k.clone()))
                                    .collect();
                                oldest.sort();
                                let drop_n =
                                    (self.artwork.len() - ART_MEM_CAP + 1).max(ART_MEM_CAP / 2);
                                for (_, key) in oldest.into_iter().take(drop_n) {
                                    self.artwork.remove(&key);
                                }
                            }
                        }
                        self.artwork
                            .insert(url, (handle, std::time::Instant::now()));
                    }
                }
                // Cover the rest of the screenful: the batch cap (12 tasks)
                // leaves remainder that only an arrival can pick up.
                // Self-terminating: memory, in-flight, disk, and cooldown
                // gates make this a no-op when done.
                self.publish_mpris();
                return self.ensure_visible_tasks();
            }
            Message::StartRadio(seed) => {
                self.close_menu();
                match self.find_track(&seed) {
                    None => {
                        self.status = self.language.fetching_info(&seed);
                        self.publish_mpris();
                        return Self::hydrate_task(vec![seed]);
                    }
                    Some(t) => {
                        let artist = t.artist.clone();
                        #[cfg(feature = "innertube")]
                        {
                            self.status = self.language.starting_radio(&t.title, &t.artist);
                            self.go(Route::Radio(seed.clone()));
                            self.publish_mpris();
                            return Self::radio_task(seed, artist);
                        }
                        #[cfg(not(feature = "innertube"))]
                        {
                            let _ = artist;
                            self.status = self.language.needs_innertube_radio();
                        }
                    }
                }
            }
            Message::RadioReady { seed, result } => match result {
                Ok(tracks) => {
                    let tracks: Vec<Track> = self
                        .register_tracks(tracks)
                        .into_iter()
                        .filter(|t| !self.disliked.contains(&t.id))
                        .collect();
                    let n = tracks.len();
                    self.radio_tracks = tracks.clone();
                    self.go(Route::Radio(seed.clone()));
                    let mut tasks = self.artwork_tasks(&tracks);
                    tasks.push(self.queue_many(tracks, self.language.radio_seed_label(&seed)));
                    tasks.push(self.sweep_visible_art("body", true));
                    if n == 0 {
                        self.status = self.language.radio_no_results(&seed);
                    }
                    self.publish_mpris();
                    return iced::Task::batch(tasks);
                }
                Err(e) => {
                    yutu_core::app_log::warn("yutu-radio", format!("{seed}: {e}"));
                    self.status = Self::failure_status(self.language.fail_radio(), &e);
                }
            },
            Message::QueueTrack(id) => {
                self.show_suggest = false;
                self.close_menu();
                self.cancel_drag();
                match self.find_track(&id) {
                    None => {
                        return self.unknown_track_task(&id);
                    }
                    Some(t) => {
                        self.absorb_autoplay(&t.id);
                        let was_empty = self.queue.is_empty();
                        self.queue.push(t.clone());
                        self.status = self.language.queued_confirm(&t.title, &t.artist, was_empty);
                    }
                }
            }
            Message::QueueJump(i) => {
                // Jumping to the live song is a no-op (never restarts it).
                if self.queue.position() == Some(i) {
                    return iced::Task::none();
                }
                // A reorder shares its release click with the row: eat it.
                if self
                    .just_reordered_at
                    .is_some_and(|at| at.elapsed() < std::time::Duration::from_millis(150))
                {
                    self.just_reordered_at = None;
                    return iced::Task::none();
                }
                self.just_reordered_at = None;
                if self.queue.jump_to(i) {
                    self.apply_volume();
                    self.publish_mpris();
                    return self.play_current_task();
                }
            }
            Message::ToggleShuffle => {
                let on = !self.queue.shuffle;
                self.cancel_drag();
                self.set_shuffle(on);
                self.status = if on {
                    "Shuffle on: queue reordered.".into()
                } else {
                    "Shuffle off: original order back.".into()
                };
            }
            Message::CycleRepeat => {
                let mode = self.queue.cycle_repeat();
                self.status = self.language.repeat_line(mode);
            }
            Message::PlayAll => return self.play_all_tracks(false),
            Message::PlayAllShuffled => return self.play_all_tracks(true),
            Message::PlayPlaylistAt {
                playlist_id,
                index,
                track_id,
            } => {
                self.close_menu();
                // Same-song no-op (app contract): clicking the live
                // track's own row never restarts audio.
                if self.queue.current().is_some_and(|t| t.id == track_id) {
                    return iced::Task::none();
                }
                self.cancel_drag();
                self.cancel_pl_drag();
                let Some(p) = self.find_playlist(&playlist_id) else {
                    self.status = self.language.playlist_not_found_plain();
                    return iced::Task::none();
                };
                // Same suffix rule as artist/album pages: the displayed
                // (sorted) list plays from the clicked song on. The
                // clicked id anchors the head: adds/reorders between
                // render and click fall back to the index.
                let display = self.playlist_ordered_tracks(&p);
                let start = display
                    .iter()
                    .position(|t| t.id == track_id)
                    .unwrap_or(index.min(display.len()));
                let label = self.language.playlist_queue_label(&p.title);
                let tracks: Vec<Track> = display.into_iter().skip(start).collect();
                if tracks.is_empty() {
                    self.status = self.language.nothing_to_play(&label);
                    return iced::Task::none();
                }
                // Fresh context like PlayNow: the old queue and tail go
                // away, then the playlist plays from the clicked song on.
                self.queue.clear();
                self.autoplay.clear();
                self.autoplay_for = None;
                self.drag = None;
                return self.queue_many(tracks, &label);
            }
            Message::PlayArtistAt {
                artist_id,
                index,
                track_id,
            } => {
                self.close_menu();
                // Same-song no-op (app contract): clicking the live
                // track's own row never restarts audio.
                if self.queue.current().is_some_and(|t| t.id == track_id) {
                    return iced::Task::none();
                }
                self.cancel_drag();
                // Same suffix rule as playlists: the displayed list
                // (live rank when fetched) plays from the clicked song.
                // The clicked id anchors the head: catalog inserts
                // between render and click fall back to the index.
                // Label from the queued songs (never a stale resolve).
                let display = self.artist_display_tracks(&artist_id);
                let start = display
                    .iter()
                    .position(|t| t.id == track_id)
                    .unwrap_or(index.min(display.len()));
                let tracks: Vec<Track> = display.into_iter().skip(start).collect();
                let artist_name = tracks
                    .first()
                    .map(|t| t.artist.clone())
                    .unwrap_or_else(|| self.resolve_artist_name(&artist_id));
                let label = self.language.artist_queue_label(&artist_name);
                if tracks.is_empty() {
                    self.status = self.language.nothing_to_play(&label);
                    return iced::Task::none();
                }
                self.queue.clear();
                self.autoplay.clear();
                self.autoplay_for = None;
                return self.queue_many(tracks, &label);
            }
            Message::PlayAlbumAt {
                album_id,
                index,
                track_id,
            } => {
                self.close_menu();
                // Same-song no-op (app contract): clicking the live
                // track's own row never restarts audio.
                if self.queue.current().is_some_and(|t| t.id == track_id) {
                    return iced::Task::none();
                }
                self.cancel_drag();
                // Same suffix rule as playlists: the displayed tracklist
                // (`track_no` order when live) plays from the click.
                let display = self.album_display_tracks(&album_id);
                let start = display
                    .iter()
                    .position(|t| t.id == track_id)
                    .unwrap_or(index.min(display.len()));
                let tracks: Vec<Track> = display.into_iter().skip(start).collect();
                let album_title = tracks
                    .first()
                    .and_then(|t| t.album.clone())
                    .unwrap_or_else(|| self.resolve_album_title(&album_id));
                let label = self.language.album_queue_label(&album_title);
                if tracks.is_empty() {
                    self.status = self.language.nothing_to_play(&label);
                    return iced::Task::none();
                }
                self.queue.clear();
                self.autoplay.clear();
                self.autoplay_for = None;
                return self.queue_many(tracks, &label);
            }
            Message::PlayArtistShuffled(name) => {
                self.close_menu();
                // Same list as the page shows (live rank when fetched).
                let tracks = self.artist_display_tracks(&name);
                return self.shuffle_tracks(tracks, self.language.artist_queue_label(&name));
            }
            Message::PlayAlbumShuffled(id) => {
                self.close_menu();
                let title = self.resolve_album_title(&id);
                let tracks = self.album_display_tracks(&id);
                return self.shuffle_tracks(tracks, self.language.album_queue_label(&title));
            }
            Message::PlaySearchPlaylist(id) => {
                return self.play_search_playlist(id, false);
            }
            Message::PlaySearchPlaylistShuffled(id) => {
                return self.play_search_playlist(id, true);
            }
            Message::SearchPlaylistReady {
                id,
                label,
                shuffle,
                result,
            } => match result {
                Ok(tracks) => {
                    let tracks = self.register_tracks(tracks);
                    if tracks.is_empty() {
                        self.status = self.language.playlist_nothing_playable(&label);
                        return iced::Task::none();
                    }
                    return self.play_fresh(
                        tracks,
                        &self.language.playlist_queue_label(&label),
                        shuffle,
                    );
                }
                Err(e) => {
                    self.status = self.language.playlist_detail_error(&label, &id, &e);
                }
            },
            Message::CreatePlaylist(title) => {
                let title = title.trim().to_string();
                if title.is_empty() {
                    self.status = self.language.playlist_name_empty();
                } else {
                    self.playlist_seq += 1;
                    let id = format!("u{}", self.playlist_seq);
                    self.user_playlists.push(Playlist {
                        id: id.clone(),
                        title: title.clone(),
                        tracks: Vec::new(),
                    });
                    self.persist_playlist_created(&id, &title);
                    self.record_pending("create", Some(&id), None, Some(&title));
                    self.new_playlist_name.clear();
                    self.status = self.language.playlist_created(&title, &id);
                    self.publish_mpris();
                    // Signed in: mirror on YouTube Music, migrating the
                    // local shell to the server id on success.
                    return Self::playlist_create_sync_task(id, title);
                }
            }
            Message::AddToPlaylist {
                playlist_id,
                track_id,
            } => match self.find_track(&track_id) {
                None => {
                    return self.unknown_track_task(&track_id);
                }
                Some(track) => {
                    // Accept either stored form: older sessions kept browse
                    // (`VL...`) ids, storage is canonical (edit form) now.
                    let playlist_id = yutu_core::canonical_playlist_id(&playlist_id);
                    // Virtual Favorites maps to likes, never SQLite.
                    if playlist_id == Self::FAVORITES_ID {
                        if self.liked.contains(&track.id) {
                            self.status = self.language.already_in_favorites(&track.title);
                            self.publish_mpris();
                            return iced::Task::none();
                        }
                        self.like_id(&track.id);
                        self.persist_like_track(&track, true);
                        self.record_pending("like", None, Some(&track.id), None);
                        self.status = self.language.added_to_favorites(&track.title);
                        self.publish_mpris();
                        return Self::like_sync_task(track.id.clone(), true);
                    }
                    match self.user_playlists.iter().position(|p| p.id == playlist_id) {
                        None => {
                            self.status = self.language.playlist_readonly(&playlist_id);
                        }
                        Some(i) => {
                            if self.user_playlists[i]
                                .tracks
                                .iter()
                                .any(|t| t.id == track.id)
                            {
                                self.status = self.language.already_in_playlist(
                                    &track.title,
                                    &self.user_playlists[i].title,
                                );
                            } else {
                                let title = self.user_playlists[i].title.clone();
                                self.user_playlists[i].tracks.push(track.clone());
                                self.persist_playlist_added(&playlist_id, &track);
                                self.refresh_playlist_added(&playlist_id);
                                if Self::is_server_playlist(&playlist_id) {
                                    self.record_pending(
                                        "add",
                                        Some(&playlist_id),
                                        Some(&track.id),
                                        None,
                                    );
                                }
                                self.status = self.language.added_to_playlist(&track.title, &title);
                                self.publish_mpris();
                                // Server lists mirror on YouTube Music;
                                // local shells stay local-only.
                                return Self::playlist_add_sync_task(playlist_id, track.id.clone());
                            }
                        }
                    }
                }
            },
            Message::RemoveFromPlaylist {
                playlist_id,
                track_id,
            } => {
                // Same either-form rule as adds (see AddToPlaylist).
                let playlist_id = yutu_core::canonical_playlist_id(&playlist_id);
                // Virtual Favorites maps to unlikes, never SQLite.
                if playlist_id == Self::FAVORITES_ID {
                    if !self.liked.contains(&track_id) {
                        self.status = self.language.track_not_in_favorites(&track_id);
                        self.publish_mpris();
                        return iced::Task::none();
                    }
                    self.unlike_id(&track_id);
                    self.persist_like(&track_id, false);
                    self.record_pending("unlike", None, Some(&track_id), None);
                    self.status = self.language.removed_track_favorites(&track_id);
                    self.publish_mpris();
                    return Self::like_sync_task(track_id, false);
                }
                match self.user_playlists.iter().position(|p| p.id == playlist_id) {
                    None => {
                        self.status = self.language.playlist_readonly(&playlist_id);
                    }
                    Some(i) => {
                        let title = self.user_playlists[i].title.clone();
                        self.user_playlists[i].tracks.retain(|t| t.id != track_id);
                        self.persist_playlist_removed(&playlist_id, &track_id);
                        self.refresh_playlist_added(&playlist_id);
                        if Self::is_server_playlist(&playlist_id) {
                            self.record_pending(
                                "remove",
                                Some(&playlist_id),
                                Some(&track_id),
                                None,
                            );
                        }
                        self.status = self.language.removed_track_playlist(&track_id, &title);
                        self.publish_mpris();
                        // Server lists mirror the removal on YouTube Music
                        // (setVideoId resolved from raw browse JSON);
                        // local shells stay local-only.
                        return Self::playlist_remove_sync_task(playlist_id, track_id);
                    }
                }
            }
            Message::RequestDeletePlaylist(id) => {
                self.close_menu();
                // Same either-form rule as adds (see AddToPlaylist).
                let id = yutu_core::canonical_playlist_id(&id);
                if id == Self::FAVORITES_ID {
                    self.status = self.language.favorites_locked();
                    return iced::Task::none();
                }
                if self.find_playlist(&id).is_none() {
                    self.status = self.language.playlist_locked(&id);
                    return iced::Task::none();
                }
                // Restart the entrance only when nothing is open: the
                // ticker is shared, so resetting under a settled dialog
                // would make its card jump down and re-rise.
                if self.modal_target() == 0.0 {
                    self.modal_anim = 0.0;
                }
                self.pending_playlist_rename = None;
                self.rename_playlist_name.clear();
                self.pending_playlist_delete = Some(id);
            }
            Message::CancelDeletePlaylist => {
                self.pending_playlist_delete = None;
            }
            Message::RequestRenamePlaylist(id) => {
                self.close_menu();
                // Same either-form rule as adds (see AddToPlaylist).
                let id = yutu_core::canonical_playlist_id(&id);
                if id == Self::FAVORITES_ID {
                    self.status = self.language.favorites_locked();
                    return iced::Task::none();
                }
                let Some(current) = self.find_playlist(&id).map(|p| p.title.clone()) else {
                    self.status = self.language.playlist_locked(&id);
                    return iced::Task::none();
                };
                // Restart the entrance only when nothing is open: the
                // ticker is shared, so resetting under a settled dialog
                // would make its card jump down and re-rise.
                if self.modal_target() == 0.0 {
                    self.modal_anim = 0.0;
                }
                self.pending_playlist_delete = None;
                self.pending_playlist_rename = Some(id);
                self.rename_playlist_name = current;
            }
            Message::RenamePlaylistNameChanged(name) => {
                self.rename_playlist_name = name;
            }
            Message::CancelRenamePlaylist => {
                self.pending_playlist_rename = None;
                self.rename_playlist_name.clear();
            }
            Message::RenamePlaylist { playlist_id, title } => {
                self.close_menu();
                let pending = self.pending_playlist_rename.clone();
                self.pending_playlist_rename = None;
                self.rename_playlist_name.clear();
                // Same either-form rule as adds (see AddToPlaylist).
                let id = yutu_core::canonical_playlist_id(&playlist_id);
                if id == Self::FAVORITES_ID {
                    self.status = self.language.favorites_locked();
                    self.publish_mpris();
                    return iced::Task::none();
                }
                let title = title.trim().to_string();
                if title.is_empty() {
                    self.status = self.language.playlist_name_empty();
                    self.publish_mpris();
                    return iced::Task::none();
                }
                let Some(pos) = self.user_playlists.iter().position(|p| p.id == id) else {
                    self.status = self.language.playlist_locked(&id);
                    self.publish_mpris();
                    return iced::Task::none();
                };
                if self.user_playlists[pos].title == title {
                    // Same name: still close the dialog, but only when it
                    // was the armed one (a stale confirm never clobbers).
                    if pending.as_deref() != Some(id.as_str()) {
                        self.status = self.language.playlist_locked(&id);
                    }
                    return iced::Task::none();
                }
                self.user_playlists[pos].title = title.clone();
                self.persist_playlist_renamed(&id, &title);
                if Self::is_server_playlist(&id) {
                    self.record_pending("rename", Some(&id), None, Some(&title));
                } else {
                    // Local shells have no server row yet: the pending
                    // create carries the title, so refresh it instead of
                    // recording a rename the server could never apply.
                    self.record_pending("create", Some(&id), None, Some(&title));
                }
                self.status = self.language.renamed_playlist(&title);
                self.publish_mpris();
                // Server lists mirror the rename on YouTube Music;
                // local shells stay local-only.
                return Self::playlist_rename_sync_task(id, title);
            }
            Message::DeletePlaylist(id) => {
                self.close_menu();
                self.pending_playlist_delete = None;
                self.pending_playlist_rename = None;
                self.rename_playlist_name.clear();
                // Same either-form rule as adds (see AddToPlaylist).
                let id = yutu_core::canonical_playlist_id(&id);
                if id == Self::FAVORITES_ID {
                    self.status = self.language.favorites_locked();
                    self.publish_mpris();
                    return iced::Task::none();
                }
                let before = self.user_playlists.len();
                self.user_playlists.retain(|p| p.id != id);
                if self.user_playlists.len() < before {
                    self.persist_playlist_deleted(&id);
                    self.playlist_sorts.remove(&id);
                    self.playlist_added.remove(&id);
                    if let Ok(store) = LibraryStore::open(&self.library_path) {
                        let _ = store.set_pref(&Self::playlist_sort_key(&id), "");
                    }
                    self.cancel_pl_drag();
                    self.pl_hover = None;
                    self.pl_action_hover = None;
                    if Self::is_server_playlist(&id) {
                        self.record_pending("delete", Some(&id), None, None);
                    }
                    if self.route == Route::PlaylistDetail(id.clone()) {
                        self.go(Route::Home);
                    }
                    self.status = self.language.deleted_playlist(&id);
                    self.publish_mpris();
                    // Server lists mirror the delete on YouTube Music.
                    return Self::playlist_delete_sync_task(id);
                } else {
                    self.status = self.language.playlist_locked(&id);
                }
            }
            Message::SelectEqPreset(name) => self.select_eq_preset(&name),
            Message::SetEqBand(index, gain) => self.set_eq_band(index, gain),
            Message::EqToggled(on) => self.toggle_eq(on),
            Message::ScrobbleToggled(on) => {
                self.scrobble_enabled = on;
                if let Ok(store) = LibraryStore::open(&self.library_path) {
                    let _ = store.set_pref("scrobble_enabled", if on { "1" } else { "0" });
                }
                self.status = if on {
                    "Scrobbling enabled (submits when credentials are set).".into()
                } else {
                    "Scrobbling disabled.".into()
                };
            }
            Message::OpenScrobbleModal => {
                // Restart the entrance only when nothing is open: the
                // ticker is shared, so resetting under a settled dialog
                // would make its card jump down and re-rise.
                #[cfg(feature = "innertube")]
                let cookie_open = self.pending_cookie_cleanup.is_some();
                #[cfg(not(feature = "innertube"))]
                let cookie_open = false;
                if !self.scrobble_modal_open && !cookie_open {
                    self.modal_anim = 0.0;
                }
                self.scrobble_modal_open = true;
            }
            Message::CloseScrobbleModal => {
                self.scrobble_modal_open = false;
                self.persist_scrobble_secrets();
            }
            Message::ScrobbleUserChanged(u) => {
                self.scrobble_user = u.clone();
                if let Ok(store) = LibraryStore::open(&self.library_path) {
                    let _ = store.set_pref("scrobble_user", &u);
                }
            }
            Message::ScrobbleLbTokenChanged(t) => {
                self.scrobble_lb_token = t;
            }
            Message::ScrobbleFmKeyChanged(k) => {
                self.scrobble_fm_key = k;
            }
            Message::ScrobbleFmSecretChanged(s) => {
                self.scrobble_fm_secret = s;
            }
            Message::ScrobbleFmSkChanged(sk) => {
                self.scrobble_fm_sk = sk;
            }
            Message::PlayWeeklyMix => {
                let tracks = self.weekly_mix_tracks();
                if tracks.is_empty() {
                    self.status = self.language.weekly_empty();
                } else {
                    let task = self.queue_many(tracks, self.language.weekly_mix());
                    self.publish_mpris();
                    return task;
                }
            }
            Message::PlayForgotten => {
                let tracks = self.forgotten_tracks();
                if tracks.is_empty() {
                    self.status = self.language.forgotten_empty();
                } else {
                    let task = self.queue_many(tracks, self.language.forgotten());
                    self.publish_mpris();
                    return task;
                }
            }
            Message::PlayQuickPicks => {
                let tracks = self.quick_pick_tracks(8);
                if tracks.is_empty() {
                    self.status = self.language.quick_empty();
                } else {
                    let task = self.queue_many(tracks, self.language.quick_picks());
                    self.publish_mpris();
                    return task;
                }
            }
            Message::PlayHomeAlbum { album, artist } => {
                let tracks = self.home_album_tracks(&album, &artist);
                if tracks.is_empty() {
                    self.status = self.language.album_not_cached(&album);
                } else {
                    let task = self.queue_many(tracks, self.language.album_queue_label(&album));
                    self.publish_mpris();
                    return task;
                }
            }
            Message::OpenHomeAlbumMenu { album, artist } => {
                self.open_coll_for(CollectionMenu::HomeAlbum {
                    album: album.clone(),
                    artist: artist.clone(),
                });
                // Same hero-header contract as playlist menus: warm
                // the first track's hero so the menu never
                // placeholders for its lifetime.
                if let Some(task) = self
                    .home_album_tracks(&album, &artist)
                    .first()
                    .and_then(|t| self.find_track(&t.id))
                    .and_then(|t| self.large_art_task(&t))
                {
                    return task;
                }
            }
            Message::QueueHomeAlbum { album, artist } => {
                self.close_menu();
                self.show_suggest = false;
                let tracks = self.home_album_tracks(&album, &artist);
                if tracks.is_empty() {
                    self.status = self.language.album_not_cached(&album);
                } else {
                    // Append-only like QueuePlaylist: never hijacks
                    // playback, absorbs matching autoplay ids.
                    let label = self.language.album_queue_label(&album);
                    let n = tracks.len();
                    let was_empty = self.queue.is_empty();
                    let incoming: std::collections::HashSet<&str> =
                        tracks.iter().map(|t| t.id.as_str()).collect();
                    self.autoplay.retain(|t| !incoming.contains(t.id.as_str()));
                    let resolved: Vec<Track> = tracks
                        .iter()
                        .map(|t| self.resolve_for_play_mem(t))
                        .collect();
                    self.cache_tracks(&resolved);
                    for rt in resolved {
                        self.queue.push(rt);
                    }
                    self.status = self.language.queued_tracks(&label, n);
                    self.publish_mpris();
                    if was_empty {
                        self.queue.jump_to(0);
                        self.apply_volume();
                        return self.play_current_task();
                    }
                }
            }
            Message::ShuffleHomeAlbum { album, artist } => {
                self.close_menu();
                self.show_suggest = false;
                let tracks = self.home_album_tracks(&album, &artist);
                if tracks.is_empty() {
                    self.status = self.language.album_not_cached(&album);
                } else {
                    // Same path as the playlist Shuffle pill: fully
                    // shuffled with a random head, always reshuffled.
                    self.cancel_drag();
                    return self.play_fresh(tracks, &self.language.album_queue_label(&album), true);
                }
            }
            Message::PlayNow(id) => {
                self.close_menu();
                self.show_suggest = false;
                // Re-clicking the live song is a no-op (never restarts it).
                if self.queue.current().is_some_and(|t| t.id == id) {
                    return iced::Task::none();
                }
                self.cancel_drag();
                match self.find_track(&id) {
                    None => {
                        return self.unknown_track_task(&id);
                    }
                    Some(t) => {
                        let artist = t.artist.clone();
                        self.queue.clear();
                        self.queue.push(t);
                        self.queue.jump_to(0);
                        self.autoplay_for = Some(id.clone());
                        self.autoplay.clear();
                        self.apply_volume();
                        self.publish_mpris();
                        let play = self.play_current_task();
                        #[cfg(feature = "innertube")]
                        {
                            let auto = Self::autoplay_task(id, artist);
                            return iced::Task::batch([play, auto]);
                        }
                        #[cfg(not(feature = "innertube"))]
                        {
                            let _ = artist;
                            return play;
                        }
                    }
                }
            }
            Message::AutoplayReady { seed, result } => {
                match result {
                    Err(e) => {
                        // Quiet: the tail is best-effort, but keep the log.
                        self.log_error(format!("Autoplay for {seed} failed: {e}"));
                    }
                    Ok(tracks) => {
                        let tracks = self.register_tracks(tracks);
                        // Stale (user moved on): catalog only, no append.
                        if self.autoplay_for.as_deref() != Some(seed.as_str()) {
                            return iced::Task::none();
                        }
                        let known: std::collections::HashSet<String> = self
                            .queue
                            .items()
                            .iter()
                            .chain(self.autoplay.iter())
                            .map(|t| t.id.clone())
                            .collect();
                        let fresh: Vec<Track> = tracks
                            .into_iter()
                            .filter(|t| {
                                t.id != seed
                                    && !known.contains(&t.id)
                                    && !self.disliked.contains(&t.id)
                            })
                            .take(10)
                            .collect();
                        if fresh.is_empty() {
                            return iced::Task::none();
                        }
                        let art = self.artwork_tasks(&fresh);
                        self.autoplay.extend(fresh);
                        self.publish_mpris();
                        return iced::Task::batch(art);
                    }
                }
            }
            Message::PlayNext(id) => {
                self.close_menu();
                self.show_suggest = false;
                self.cancel_drag();
                match self.find_track(&id) {
                    None => {
                        return self.unknown_track_task(&id);
                    }
                    Some(t) => {
                        self.absorb_autoplay(&t.id);
                        if self.queue.is_empty() {
                            self.queue.push(t);
                            self.queue.jump_to(0);
                            self.apply_volume();
                            self.publish_mpris();
                            return self.play_current_task();
                        }
                        let at = self.queue.insert_next(t.clone());
                        self.status = self.language.queue_next(&t.title, &t.artist);
                        let _ = at;
                    }
                }
            }
            Message::PlayAutoplay(i) => {
                self.close_menu();
                self.cancel_drag();
                match self.autoplay.get(i).cloned() {
                    None => {
                        self.status = self.language.no_autoplay_at(i);
                    }
                    Some(t) => {
                        self.autoplay.remove(i);
                        self.queue.push(t);
                        self.queue.jump_to(self.queue.len().saturating_sub(1));
                        self.apply_volume();
                        self.publish_mpris();
                        return self.play_current_task();
                    }
                }
            }
            Message::RemoveFromQueue(id) => {
                self.close_menu();
                self.cancel_drag();
                let was_current = self.queue.current().is_some_and(|t| t.id == id);
                if self.queue.remove_id(&id) {
                    self.status = self.language.removed_from_queue(&id);
                    if was_current {
                        // The ghost must not keep playing: move on, or stop.
                        if self.advance_queue() {
                            self.apply_volume();
                            self.publish_mpris();
                            return self.play_current_task();
                        }
                        if let Some(a) = &self.audio {
                            a.stop();
                        }
                        // Stopped, not finished: disarm both idle nets
                        // (the index fell back to the previous song).
                        self.heard_audio = false;
                        self.stream_opened_at = None;
                        self.position_secs = 0.0;
                        self.loading_track = None;
                    }
                } else {
                    self.status = self.language.not_in_queue(&id);
                }
            }
            Message::RemoveAutoplay(i) => {
                self.close_menu();
                if i < self.autoplay.len() {
                    let t = self.autoplay.remove(i);
                    self.status = self.language.removed_from_autoplay(&t.title);
                } else {
                    self.status = self.language.no_autoplay_at(i);
                }
            }
            Message::QueueExit(i) => {
                // Match-guarded: a stale exit (enter(new) already
                // latched before this exit in the same event batch) must
                // never wipe the live hover. Other families are already
                // None here (every enter clears them), so only our own
                // field needs the guard.
                if self.hovered_queue == Some(i) {
                    self.hovered_queue = None;
                }
            }
            Message::HistoryHover(h) => {
                // Modal open: rows behind scrims never highlight.
                if self.context_menu.is_some()
                    || self.pl_menu.is_some()
                    || self.coll_menu.is_some()
                    || self.sort_menu.is_some()
                    || self.show_suggest
                {
                    self.hovered_history = None;
                } else {
                    self.hovered_history = h;
                }
                self.hovered_auto = None;
                self.hovered_album = None;
                self.hovered_artist = None;
                self.hovered_search_playlist = None;
                self.hovered_recent = None;
            }
            Message::HistoryExit(i) => {
                if self.hovered_history == Some(i) {
                    self.hovered_history = None;
                }
            }
            Message::TrackHover(h) => {
                if self.context_menu.is_some()
                    || self.pl_menu.is_some()
                    || self.coll_menu.is_some()
                    || self.sort_menu.is_some()
                    || self.show_suggest
                {
                    self.hovered_track = None;
                    self.pl_hover = None;
                    self.pl_action_hover = None;
                } else {
                    self.hovered_track = h.clone();
                    // Playlist detail highlight follows the same hover:
                    // derive the ordered index so manual rows light up
                    // without a second enter message.
                    if let (Some(id), Route::PlaylistDetail(pid)) = (h.as_deref(), &self.route) {
                        if let Some(p) = self.find_playlist(pid) {
                            let ordered = self.playlist_ordered_tracks(&p);
                            self.pl_hover = ordered.iter().position(|t| t.id == id);
                        } else {
                            self.pl_hover = None;
                        }
                    } else {
                        self.pl_hover = None;
                    }
                }
                // Entering a non-queue row drops stale queue hovers so a
                // press there never arms a drag off old state.
                self.hovered_queue = None;
                self.hovered_history = None;
                self.hovered_auto = None;
                self.hovered_album = None;
                self.hovered_artist = None;
                self.hovered_search_playlist = None;
                self.hovered_recent = None;
            }
            Message::TrackExit(id) => {
                if self.hovered_track.as_deref() == Some(id.as_str()) {
                    self.hovered_track = None;
                }
                // Clear the derived playlist hover when its row exits.
                if let Route::PlaylistDetail(pid) = &self.route {
                    if let Some(p) = self.find_playlist(pid) {
                        let ordered = self.playlist_ordered_tracks(&p);
                        if self
                            .pl_hover
                            .and_then(|i| ordered.get(i))
                            .is_some_and(|t| t.id == id)
                        {
                            self.pl_hover = None;
                        }
                    } else {
                        self.pl_hover = None;
                    }
                } else if self.pl_hover.is_some() {
                    self.pl_hover = None;
                }
            }
            Message::RecentHover(h) => {
                if self.context_menu.is_some()
                    || self.pl_menu.is_some()
                    || self.coll_menu.is_some()
                    || self.sort_menu.is_some()
                    || self.show_suggest
                {
                    self.hovered_recent = None;
                } else {
                    self.hovered_recent = h;
                }
                // Same cross-list clearing as track rows: a Recent enter
                // drops the center highlight for the same track id, so
                // only the hovered component lights up.
                self.hovered_track = None;
                self.hovered_queue = None;
                self.hovered_history = None;
                self.hovered_auto = None;
                self.hovered_album = None;
                self.hovered_artist = None;
                self.hovered_search_playlist = None;
                self.hovered_playlist = None;
                self.hover_remove = None;
            }
            Message::RecentExit(id) => {
                if self.hovered_recent.as_deref() == Some(id.as_str()) {
                    self.hovered_recent = None;
                }
            }
            Message::AlbumHover(h) => {
                self.hovered_album = self.coll_hover(h);
                self.hovered_track = None;
                self.hovered_recent = None;
                self.hovered_artist = None;
                self.hovered_search_playlist = None;
                self.hovered_playlist = None;
                self.hovered_queue = None;
                self.hovered_auto = None;
                self.hover_remove = None;
            }
            Message::ArtistHover(h) => {
                self.hovered_artist = self.coll_hover(h);
                self.hovered_track = None;
                self.hovered_recent = None;
                self.hovered_album = None;
                self.hovered_search_playlist = None;
                self.hovered_playlist = None;
                self.hovered_queue = None;
                self.hovered_auto = None;
                self.hover_remove = None;
            }
            Message::SearchPlaylistHover(h) => {
                self.hovered_search_playlist = self.coll_hover(h);
                self.hovered_track = None;
                self.hovered_recent = None;
                self.hovered_album = None;
                self.hovered_artist = None;
                self.hovered_playlist = None;
                self.hovered_queue = None;
                self.hovered_auto = None;
                self.hover_remove = None;
            }
            Message::AlbumExit(id) => {
                if self.hovered_album.as_deref() == Some(id.as_str()) {
                    self.hovered_album = None;
                }
            }
            Message::ArtistExit(id) => {
                if self.hovered_artist.as_deref() == Some(id.as_str()) {
                    self.hovered_artist = None;
                }
            }
            Message::SearchPlaylistExit(id) => {
                if self.hovered_search_playlist.as_deref() == Some(id.as_str()) {
                    self.hovered_search_playlist = None;
                }
            }
            Message::ClearHovers => {
                self.hovered_track = None;
                self.hovered_recent = None;
                self.hovered_album = None;
                self.hovered_artist = None;
                self.hovered_search_playlist = None;
                self.hovered_playlist = None;
                self.hovered_queue = None;
                self.hovered_history = None;
                self.hovered_auto = None;
                self.hover_remove = None;
                self.pl_hover = None;
                self.pl_action_hover = None;
            }
            Message::RemoveHover(h) => {
                self.hover_remove = h;
            }
            Message::RemoveExit(i) => {
                if self.hover_remove == Some(i) {
                    self.hover_remove = None;
                }
            }
            Message::PlActionHover(h) => {
                self.pl_action_hover = h;
            }
            Message::PlActionExit(i) => {
                if self.pl_action_hover == Some(i) {
                    self.pl_action_hover = None;
                }
            }
            Message::PlaylistHover(h) => {
                if self.context_menu.is_some()
                    || self.pl_menu.is_some()
                    || self.coll_menu.is_some()
                    || self.sort_menu.is_some()
                    || self.show_suggest
                {
                    self.hovered_playlist = None;
                } else {
                    self.hovered_playlist = h;
                }
                self.hovered_queue = None;
                self.hovered_auto = None;
                self.hovered_album = None;
                self.hovered_artist = None;
                self.hovered_search_playlist = None;
                self.hovered_recent = None;
            }
            Message::PlaylistExit(id) => {
                if self.hovered_playlist.as_deref() == Some(id.as_str()) {
                    self.hovered_playlist = None;
                }
            }
            Message::AutoHover(h) => {
                if self.context_menu.is_some()
                    || self.pl_menu.is_some()
                    || self.coll_menu.is_some()
                    || self.sort_menu.is_some()
                    || self.show_suggest
                {
                    self.hovered_auto = None;
                } else {
                    self.hovered_auto = h;
                }
                self.hovered_queue = None;
                self.hovered_album = None;
                self.hovered_artist = None;
                self.hovered_search_playlist = None;
                self.hovered_recent = None;
            }
            Message::AutoExit(i) => {
                if self.hovered_auto == Some(i) {
                    self.hovered_auto = None;
                }
            }
            Message::PanelHover(i) => {
                if self.context_menu.is_some()
                    || self.pl_menu.is_some()
                    || self.coll_menu.is_some()
                    || self.sort_menu.is_some()
                    || self.show_suggest
                {
                    self.hovered_queue = None;
                } else {
                    self.hovered_queue = Some(i);
                }
                self.hovered_auto = None;
                self.hovered_album = None;
                self.hovered_artist = None;
                self.hovered_search_playlist = None;
                self.hovered_recent = None;
            }
            Message::DragStart(from) => {
                // Two-row lists reorder via the trailing edge slot, so
                // drags start at two rows and up. Never from the remove
                // button: slipping off delete must not reorder. Arming
                // is invisible: lanes appear only after the cursor moves
                // past click slop, so a plain click never shifts rows.
                if from < self.queue.len()
                    && self.queue.len() >= 2
                    && self.hover_remove != Some(from)
                {
                    self.drag = Some(DragState {
                        from,
                        slot: None,
                        live: false,
                    });
                    self.press_snap = Some(PressSnap {
                        from,
                        pos: self.cursor,
                        id: self
                            .queue
                            .items()
                            .get(from)
                            .map(|t| t.id.clone())
                            .unwrap_or_default(),
                        len: self.queue.len(),
                    });
                }
            }
            Message::GlobalPress => {
                // Arm from the hovered queue row: one press path for text,
                // art, and gaps (only the remove button owns its press).
                // Row buttons coexist order-independently (a shared
                // release either jumps once or no-ops). Armed, not live:
                // no layout change until the cursor actually moves.
                if self.drag.is_none() && self.queue.len() >= 2 {
                    if let Some(i) = self.hovered_queue {
                        if i < self.queue.len() && self.hover_remove != Some(i) {
                            self.drag = Some(DragState {
                                from: i,
                                slot: None,
                                live: false,
                            });
                            self.press_snap = Some(PressSnap {
                                from: i,
                                pos: self.cursor,
                                id: self
                                    .queue
                                    .items()
                                    .get(i)
                                    .map(|t| t.id.clone())
                                    .unwrap_or_default(),
                                len: self.queue.len(),
                            });
                        }
                    }
                }
            }
            Message::DragSlot(k) => {
                // Slot lanes only exist while live (edges included);
                // anything else is a stale hover (no-op). Latching a slot
                // goes live: in tests the hover is sent without a cursor
                // move, and in the app the lane cannot be hovered before
                // the move made it visible.
                if let Some(drag) = self.drag.as_mut() {
                    if k <= self.queue.len() {
                        drag.slot = Some(k);
                        drag.live = true;
                    }
                }
            }
            Message::DragEnd => {
                let snap = self.press_snap.take();
                if let Some(drag) = self.drag.take() {
                    // No move plus a clean press/release over the same
                    // unmutated row: the whole row clicks (plays it).
                    // Snapshot-guarded on both paths: a queue mutated
                    // mid-press (remove, play-all, mix) never jumps or
                    // reorders off stale indices.
                    let snap_ok = snap.as_ref().is_some_and(|s| {
                        s.from == drag.from
                            && self.queue.len() == s.len
                            && self.queue.items().get(s.from).is_some_and(|t| t.id == s.id)
                    });
                    let mut committed = false;
                    // Only a live drag (moved past slop, lanes visible)
                    // can reorder: an armed press released in place is a
                    // click even if a stale slot hover arrived.
                    if snap_ok && drag.live {
                        if let Some(slot) = drag.slot {
                            if let Some(to) = slot_commit(self.queue.len(), drag.from, slot) {
                                if self.queue.move_to(drag.from, to) {
                                    self.status = self.language.queue_reordered();
                                    self.just_reordered_at = Some(std::time::Instant::now());
                                    committed = true;
                                }
                            }
                        }
                    }
                    let modal = self.context_menu.is_some()
                        || self.pl_menu.is_some()
                        || self.coll_menu.is_some()
                        || self.sort_menu.is_some()
                        || self.show_suggest;
                    let click = !committed
                        && !modal
                        && snap.as_ref().is_some_and(|s| {
                            snap_ok
                                && !self.just_reordered_at.is_some_and(|at| {
                                    at.elapsed() < std::time::Duration::from_millis(150)
                                })
                                && release_is_click_at(s.pos, self.cursor)
                                && self.hovered_queue == Some(s.from)
                                && self.queue.position() != Some(s.from)
                        });
                    if click {
                        let from = snap.as_ref().map(|s| s.from).unwrap_or(0);
                        if self.queue.jump_to(from) {
                            self.apply_volume();
                            self.publish_mpris();
                            return self.play_current_task();
                        }
                    }
                }
                // Release-anywhere also settles a manual playlist press
                // (the subscription only sends DragEnd): a move to a
                // slot reorders, a clean press/release plays the row.
                if self.pl_drag.is_some() {
                    return self.pl_release_task();
                }
            }
            Message::PlaylistSortSelected { playlist_id, sort } => {
                self.persist_playlist_sort(&playlist_id, sort);
                self.sort_menu = None;
                self.cancel_pl_drag();
                self.pl_hover = None;
                self.pl_action_hover = None;
            }
            Message::PlRowHover(i) => {
                if self.context_menu.is_some()
                    || self.pl_menu.is_some()
                    || self.coll_menu.is_some()
                    || self.sort_menu.is_some()
                    || self.show_suggest
                {
                    self.pl_hover = None;
                } else {
                    self.pl_hover = Some(i);
                }
            }
            Message::PlRowExit(i) => {
                if self.pl_hover == Some(i) {
                    self.pl_hover = None;
                }
            }
            Message::PlDragStart { playlist_id, index } => {
                // Manual mode only, two rows and up (single rows have no
                // order to change). Never from an action button: slipping
                // off like/menu must not reorder. Arming is invisible:
                // lanes appear only after the cursor moves past click
                // slop, so a plain click never shifts rows.
                if self.pl_action_hover == Some(index) {
                    return iced::Task::none();
                }
                let sort = self
                    .playlist_sorts
                    .get(&playlist_id)
                    .copied()
                    .unwrap_or_default();
                if sort != PlaylistSort::Manual {
                    return iced::Task::none();
                }
                let len = self
                    .find_playlist(&playlist_id)
                    .map(|p| p.tracks.len())
                    .unwrap_or(0);
                if index >= len || len < 2 {
                    return iced::Task::none();
                }
                let id = self
                    .find_playlist(&playlist_id)
                    .and_then(|p| {
                        self.playlist_ordered_tracks(&p)
                            .get(index)
                            .map(|t| t.id.clone())
                    })
                    .unwrap_or_default();
                self.pl_drag = Some(DragState {
                    from: index,
                    slot: None,
                    live: false,
                });
                self.pl_drag_id = Some(playlist_id);
                self.pl_press_snap = Some(PressSnap {
                    from: index,
                    pos: self.cursor,
                    id,
                    len,
                });
            }
            Message::PlDragSlot(k) => {
                // Length from the drag's own playlist (navigating away
                // changes the detail page, stale slots never latch).
                let len = self
                    .pl_drag_id
                    .as_deref()
                    .and_then(|id| self.find_playlist(id))
                    .map(|p| p.tracks.len())
                    .unwrap_or(0);
                if let Some(drag) = self.pl_drag.as_mut() {
                    if k <= len {
                        drag.slot = Some(k);
                        drag.live = true;
                    }
                }
            }
            Message::PlDragEnd => {
                return self.pl_release_task();
            }
            Message::Animate => {
                if !self.animations_enabled {
                    self.sidebar_w = self.sidebar_target();
                    self.right_w = self.right_target();
                    self.drawer_anim = if self.drawer_open { 1.0 } else { 0.0 };
                    self.modal_anim = self.modal_target();
                    self.drag_gap = self.drag_gap_target();
                    for (_, o) in self.scroll_hint.values_mut() {
                        *o = 0.0;
                    }
                } else {
                    // Duration-based steps with a 1px floor: each glide
                    // finishes near the slider ms, and short nudges still
                    // move at least a pixel per tick.
                    let (sidebar_step, right_step, unit_step) = anim_steps(self.anim_ms);
                    self.sidebar_w =
                        ease_toward(self.sidebar_w, self.sidebar_target(), sidebar_step);
                    self.right_w = ease_toward(self.right_w, self.right_target(), right_step);
                    let target = if self.drawer_open { 1.0 } else { 0.0 };
                    self.drawer_anim = ease_toward(self.drawer_anim, target, unit_step);
                    self.modal_anim = ease_toward(self.modal_anim, self.modal_target(), unit_step);
                    self.drag_gap = ease_toward(self.drag_gap, self.drag_gap_target(), unit_step);
                    // Scrollbar fades ease out after their idle delay (a
                    // real transition: bars light on motion, then fade).
                    for (at, o) in self.scroll_hint.values_mut() {
                        if *o > 0.0
                            && at.elapsed() > std::time::Duration::from_millis(SCROLL_FADE_DELAY_MS)
                        {
                            *o = (*o - SCROLL_FADE_STEP).max(0.0);
                        }
                    }
                    // Spinners advance while the pipeline runs or the
                    // artist full shelf loads (the frame drives the
                    // rotated glyphs; nothing else moves).
                    #[cfg(feature = "innertube")]
                    if self.pipeline_busy() || self.artist_full_loading.is_some() {
                        self.spin_frame = self.spin_frame.wrapping_add(1);
                    }
                    // Lyrics follow glide: ease toward the verse target
                    // one step per tick (smooth follow instead of a jump).
                    if let Some(glide) = self.lyrics_glide_task() {
                        self.publish_mpris();
                        return glide;
                    }
                }
            }
            Message::ToggleDrawer => {
                self.drawer_open = !self.drawer_open;
                if self.drawer_open {
                    self.drawer_scroll = 0.0;
                    // Fresh follow run: the next tick centers the verse
                    // as the drawer glides open (no commands yet, so the
                    // initial echo cannot trip the takeover check).
                    self.lyrics_follow = true;
                    self.lyrics_follow_at = None;
                    self.lyrics_cmds = [f32::NAN; 3];
                }
                // Warm the hi-res hero and the lyrics on open: the drawer
                // is the lyrics home, but no fetch trigger fires for it.
                if self.drawer_open {
                    if let Some(t) = self.queue.current().cloned() {
                        let tasks = self.track_start_tasks(&t);
                        if !tasks.is_empty() {
                            return iced::Task::batch(tasks);
                        }
                    }
                }
            }
            Message::DrawerScrolled(y) => {
                self.drawer_scroll = y.max(0.0);
                self.lit_scroll("drawer");
                // Follow takeover: anything but a recent command echo
                // is a real user scroll (echoes match the ring exactly,
                // even lagged ones; end-of-song clamping may trip this,
                // which is harmless).
                if self.lyrics_follow && !self.lyrics_cmds.contains(&self.drawer_scroll) {
                    self.lyrics_follow = false;
                }
            }
            Message::Scrolled(key, y) => {
                self.lit_scroll(key);
                self.scroll_offsets.insert(key, y.max(0.0));
                return self.sweep_visible_art(key, false);
            }
            Message::OpenContext(id) => {
                self.show_suggest = false;
                self.open_menu(id.clone());
                // The menu header renders the 544px hero while rows only
                // warm the row raster (menus never scroll-sweep): warm it
                // here so the header never placeholders for its lifetime.
                if let Some(t) = self.find_track(&id) {
                    if let Some(task) = self.large_art_task(&t) {
                        return task;
                    }
                }
            }
            Message::TogglePlaylistSubmenu(id) => {
                // Click-toggle kept as a dismiss path (hover opens, so a
                // click on the open trigger closes). Only the open
                // menu's own track can own the flyout.
                if self.context_menu.as_deref() != Some(id.as_str()) {
                    return iced::Task::none();
                }
                if self.submenu_playlists_for.as_deref() == Some(id.as_str()) {
                    self.close_playlist_submenu();
                } else {
                    self.submenu_playlists_for = Some(id);
                    self.submenu_trigger_hot = true;
                    self.submenu_hot_at = Some(std::time::Instant::now());
                    // Flyout covers are always on screen when open.
                    let covers = self.sidebar_cover_tracks();
                    let tasks = self.artwork_tasks(&covers);
                    if !tasks.is_empty() {
                        return iced::Task::batch(tasks);
                    }
                }
            }
            Message::SubmenuTriggerHot(hover) => {
                match hover {
                    Some(id) => {
                        // Hover opens (no click needed); a foreign id
                        // can never steal the open menu's flyout.
                        if self.context_menu.as_deref() == Some(id.as_str()) {
                            let fresh = self.submenu_playlists_for.as_deref() != Some(id.as_str());
                            self.submenu_playlists_for = Some(id);
                            self.submenu_trigger_hot = true;
                            self.submenu_hot_at = Some(std::time::Instant::now());
                            if fresh {
                                let covers = self.sidebar_cover_tracks();
                                let tasks = self.artwork_tasks(&covers);
                                if !tasks.is_empty() {
                                    return iced::Task::batch(tasks);
                                }
                            }
                        }
                    }
                    None => {
                        self.submenu_trigger_hot = false;
                    }
                }
            }
            Message::SubmenuFlyHot(hot) => {
                self.submenu_fly_hot = hot;
                if hot {
                    self.submenu_hot_at = Some(std::time::Instant::now());
                }
            }
            Message::OpenPlaylistMenu(id) => {
                self.open_pl_menu(id.clone());
                // Same hero-header contract as track menus: warm the
                // first resolved track's hero (the unknown-track
                // fallback reads the row raster, already warmed).
                if let Some(task) = self
                    .find_playlist(&id)
                    .and_then(|p| p.tracks.first().cloned())
                    .and_then(|t| self.find_track(&t.id))
                    .and_then(|t| self.large_art_task(&t))
                {
                    return task;
                }
            }
            Message::OpenSelectMenu(menu) => {
                self.open_select_menu(menu);
            }
            Message::OpenAlbumMenu(id) => {
                self.open_coll_for(CollectionMenu::Album(id.clone()));
                // Menu headers render the 544 key (rows warm the small
                // raster now, and menus never sweep): warm it on open.
                if let Some(url) = self
                    .results
                    .albums
                    .iter()
                    .find(|a| a.id == id)
                    .and_then(|a| self.album_cover(a))
                {
                    self.publish_mpris();
                    return iced::Task::batch(self.artwork_tasks_for_urls(&[url]));
                }
            }
            Message::OpenArtistMenu(id) => {
                self.open_coll_for(CollectionMenu::Artist(id.clone()));
                // Same contract (the menu reads the hit thumb at 544).
                if let Some(url) = self
                    .results
                    .artists
                    .iter()
                    .find(|a| a.id == id)
                    .and_then(|a| a.thumbnail_url.clone())
                    .map(|u| yutu_core::hires_thumb_url(&u, 544))
                {
                    self.publish_mpris();
                    return iced::Task::batch(self.artwork_tasks_for_urls(&[url]));
                }
            }
            Message::OpenSearchPlaylistMenu(id) => {
                self.open_coll_for(CollectionMenu::Playlist(id.clone()));
                // Same contract (`playlist_art` at menu size).
                if let Some(url) = self
                    .results
                    .playlists
                    .iter()
                    .find(|p| p.id == id)
                    .and_then(|p| p.thumbnail_url.clone())
                    .map(|u| yutu_core::hires_thumb_url(&u, 544))
                {
                    self.publish_mpris();
                    return iced::Task::batch(self.artwork_tasks_for_urls(&[url]));
                }
            }
            Message::PlayPlaylist(id) => {
                self.close_menu();
                self.show_suggest = false;
                let Some(p) = self.find_playlist(&id) else {
                    self.status = self.language.playlist_not_found(&id);
                    return iced::Task::none();
                };
                if p.tracks.is_empty() {
                    self.status = self.language.playlist_nothing_to_queue(&p.title);
                    return iced::Task::none();
                }
                self.queue.clear();
                self.autoplay.clear();
                self.autoplay_for = None;
                self.cancel_drag();
                self.cancel_pl_drag();
                let title = p.title.clone();
                let ordered = self.playlist_ordered_tracks(&p);
                // queue_many resolves each row against the catalog itself.
                let task = self.queue_many(ordered, self.language.playlist_queue_label(&title));
                self.publish_mpris();
                return task;
            }
            Message::QueuePlaylist(id) => {
                self.close_menu();
                self.show_suggest = false;
                let Some(p) = self.find_playlist(&id) else {
                    self.status = self.language.playlist_not_found(&id);
                    return iced::Task::none();
                };
                if p.tracks.is_empty() {
                    self.status = self.language.playlist_nothing_to_queue(&p.title);
                    return iced::Task::none();
                }
                let title = p.title.clone();
                let ordered = self.playlist_ordered_tracks(&p);
                let n = ordered.len();
                let was_empty = self.queue.is_empty();
                let incoming: std::collections::HashSet<&str> =
                    ordered.iter().map(|t| t.id.as_str()).collect();
                self.autoplay.retain(|t| !incoming.contains(t.id.as_str()));
                // Resolve in memory first, persist once: per-track SQLite
                // traffic freezes the GUI on long playlists.
                let resolved: Vec<Track> = ordered
                    .iter()
                    .map(|t| self.resolve_for_play_mem(t))
                    .collect();
                self.cache_tracks(&resolved);
                for rt in resolved {
                    self.queue.push(rt);
                }
                self.status = self.language.playlist_queued(&title, n);
                self.publish_mpris();
                if was_empty {
                    self.queue.jump_to(0);
                    self.apply_volume();
                    return self.play_current_task();
                }
                return iced::Task::none();
            }
            Message::ShufflePlaylist(id) => {
                self.close_menu();
                self.show_suggest = false;
                let Some(p) = self.find_playlist(&id) else {
                    self.status = self.language.playlist_not_found(&id);
                    return iced::Task::none();
                };
                if p.tracks.is_empty() {
                    self.status = self.language.playlist_nothing_to_queue(&p.title);
                    return iced::Task::none();
                }
                let title = p.title.clone();
                // Same path as the page Shuffle pill: fully shuffled with
                // a random head, always reshuffled.
                self.cancel_drag();
                self.cancel_pl_drag();
                // Shuffle ignores display order (always reshuffled).
                return self.play_fresh(
                    p.tracks.clone(),
                    &self.language.playlist_queue_label(&title),
                    true,
                );
            }
            Message::CursorMoved(p) => {
                self.cursor = Some(p);
                // Press moved past click slop: the armed drag goes live
                // and the slot lanes appear. Until then the layout stays
                // exactly as at rest, so a plain click never shifts rows
                // under the cursor.
                if let Some(drag) = self.drag.as_mut() {
                    if !drag.live {
                        let moved = match self.press_snap.as_ref().and_then(|s| s.pos) {
                            Some(a) => {
                                let dx = a.x - p.x;
                                let dy = a.y - p.y;
                                dx * dx + dy * dy >= CLICK_SLOP_PX * CLICK_SLOP_PX
                            }
                            // No press position (cursor never tracked):
                            // any motion counts as a drag.
                            None => true,
                        };
                        if moved {
                            drag.live = true;
                        }
                    }
                }
                if let Some(drag) = self.pl_drag.as_mut() {
                    if !drag.live {
                        let moved = match self.pl_press_snap.as_ref().and_then(|s| s.pos) {
                            Some(a) => {
                                let dx = a.x - p.x;
                                let dy = a.y - p.y;
                                dx * dx + dy * dy >= CLICK_SLOP_PX * CLICK_SLOP_PX
                            }
                            None => true,
                        };
                        if moved {
                            drag.live = true;
                        }
                    }
                }
                // Modal freeze: hover latch never glides behind menus.
                let modal = self.context_menu.is_some()
                    || self.pl_menu.is_some()
                    || self.coll_menu.is_some()
                    || self.sort_menu.is_some()
                    || self.show_suggest;
                // Latched hover hysteresis: expand on rail/edge entry, stay
                // open anywhere inside panel + grace, collapse only outside.
                // Y-gated above the player zone so the bottom strip never
                // expands the rails.
                let wh = self.window.map(|s| s.height).unwrap_or(800.0);
                let above_player = p.y < wh - self.player_zone_h();
                if !self.sidebar_pinned && !modal {
                    if p.x < 72.0 && above_player {
                        self.sidebar_latched = true;
                    } else if p.x >= self.sidebar_w + SIDEBAR_GRACE || !above_player {
                        self.sidebar_latched = false;
                    }
                }
                if !self.right_pinned && !modal {
                    let ww = self.window.map(|s| s.width).unwrap_or(1280.0);
                    if p.x > ww - RIGHT_RAIL_W - RIGHT_EXPAND_GRACE && above_player {
                        self.right_latched = true;
                    } else if p.x <= ww - self.right_w - RIGHT_GRACE || !above_player {
                        self.right_latched = false;
                    }
                }
                return iced::Task::none();
            }
            Message::WindowResized(sz) => {
                self.window = Some(sz);
                // Geometry changed: re-snap the followed verse once
                // (unless the user took over scrolling).
                if self.drawer_open && self.lyrics_follow {
                    self.lyrics_follow_at = None;
                }
                return iced::Task::none();
            }
            Message::CloseContext => {
                // Topmost first: the confirm dialogs render above the
                // credentials dialog, so Esc dismisses them first.
                // Scrim/Esc keeps the playlist / stays signed in.
                if self.pending_playlist_delete.is_some() {
                    self.pending_playlist_delete = None;
                    return iced::Task::none();
                }
                if self.pending_playlist_rename.is_some() {
                    self.pending_playlist_rename = None;
                    self.rename_playlist_name.clear();
                    return iced::Task::none();
                }
                #[cfg(feature = "innertube")]
                if self.pending_signout {
                    self.pending_signout = false;
                    return iced::Task::none();
                }
                // Topmost first: the scrobbling dialog renders above the
                // cookie modal (pushed later), so Esc closes it first.
                // Scrim/Esc closes it with edits kept.
                if self.scrobble_modal_open {
                    self.scrobble_modal_open = false;
                    self.persist_scrobble_secrets();
                    return iced::Task::none();
                }
                // Topmost first: cookie cleanup modal, menus, popup,
                // drag, drawer. Esc keeps the export file (safe default).
                #[cfg(feature = "innertube")]
                if self.pending_cookie_cleanup.is_some() {
                    self.pending_cookie_cleanup = None;
                    self.cookie_path.clear();
                    if !self.import_active {
                        self.status =
                            "Signed in. Export file kept (delete it manually when ready).".into();
                    }
                    return iced::Task::none();
                }
                // Topmost first: menus, popup, drag, drawer. Menus sit
                // above the popup in the layer stack.
                if self.context_menu.is_some()
                    || self.pl_menu.is_some()
                    || self.coll_menu.is_some()
                    || self.sort_menu.is_some()
                {
                    self.close_menu();
                } else if self.show_suggest {
                    self.show_suggest = false;
                    self.suggest_idx = None;
                } else if self.drag.is_some() {
                    // Esc cancels an in-progress drag first (it owns the
                    // pointer); the drawer goes on the next Esc.
                    self.cancel_drag();
                } else if self.pl_drag.is_some() {
                    self.cancel_pl_drag();
                } else if self.drawer_open {
                    self.drawer_open = false;
                }
            }
            Message::CloseSuggest => {
                self.show_suggest = false;
                self.suggest_idx = None;
            }
            Message::OpenTrackAlbum(id) => {
                self.close_menu();
                match self.find_track(&id) {
                    Some(t) => match t.album.clone() {
                        Some(album) => return self.open_album(album),
                        None => {
                            self.status = self.language.no_album_for(&t.title, &t.artist);
                        }
                    },
                    None => {
                        return self.unknown_track_task(&id);
                    }
                }
            }
            Message::OpenTrackArtist(id) => {
                self.close_menu();
                match self.find_track(&id) {
                    Some(t) => return self.open_artist(t.artist.clone()),
                    None => {
                        return self.unknown_track_task(&id);
                    }
                }
            }
            Message::ToggleSidebarPin => {
                self.sidebar_pinned = !self.sidebar_pinned;
                // Recompute the latch from the current cursor so unpinning
                // with the cursor far away collapses immediately. Y-gated
                // like CursorMoved so the player strip never expands.
                let zone = self.player_zone_h();
                let wh = self.window.map(|s| s.height).unwrap_or(800.0);
                self.sidebar_latched = self.sidebar_pinned
                    || self.cursor.is_some_and(|p| p.x < 72.0 && p.y < wh - zone);
            }
            Message::ToggleRightPin => {
                self.right_pinned = !self.right_pinned;
                let ww = self.window.map(|s| s.width).unwrap_or(1280.0);
                let wh = self.window.map(|s| s.height).unwrap_or(800.0);
                let zone = self.player_zone_h();
                self.right_latched = self.right_pinned
                    || self.cursor.is_some_and(|p| {
                        p.x > ww - RIGHT_RAIL_W - RIGHT_EXPAND_GRACE && p.y < wh - zone
                    });
            }
            Message::AnimationsToggled(on) => {
                self.animations_enabled = on;
                // Killing animations mid-glide would strand the verse:
                // snap to the live target instead of waiting for a change.
                if !on && self.drawer_open && self.lyrics_follow {
                    if let Some(t) = self.queue.current() {
                        let id = t.id.clone();
                        if let Some(at) = self.lyric_cursor_at(&id) {
                            if let Some(y) = self.lyric_follow_y(&id, at) {
                                self.lyrics_target = y;
                                self.lyrics_follow_at = Some((id, at));
                                self.lyrics_cmds = [y, f32::NAN, f32::NAN];
                                self.publish_mpris();
                                return Self::lyrics_scroll_task(y);
                            }
                        }
                    }
                }
            }
            Message::AnimMsChanged(ms) => {
                let ms = (ms as u64).clamp(ANIM_MIN_MS, ANIM_MAX_MS);
                self.anim_ms = ms;
                if let Ok(store) = LibraryStore::open(&self.library_path) {
                    let _ = store.set_pref("anim_ms", &ms.to_string());
                }
            }
            Message::FontFamilyChanged(family) => {
                self.sort_menu = None;
                self.font_family = family;
                if let Ok(store) = LibraryStore::open(&self.library_path) {
                    let _ = store.set_pref("font_family", &family.stored());
                }
                // The renderer font is fixed at startup: a changed pick
                // notes the relaunch, a re-pick of the live one leaves
                // the status alone (release builds show failures only,
                // so a no-op must never erase a visible failure).
                if let Some(note) = font_pick_status(family, effective_font()) {
                    self.status = note;
                }
            }
            Message::UiScaleChanged(scale) => {
                let scale = clamp_scale(scale);
                self.ui_scale = scale;
                if let Ok(store) = LibraryStore::open(&self.library_path) {
                    let _ = store.set_pref("ui_scale", &scale.to_string());
                }
            }
            Message::LanguageChanged(language) => {
                self.sort_menu = None;
                self.language = language;
                if let Ok(store) = LibraryStore::open(&self.library_path) {
                    let _ = store.set_pref("language", language.stored());
                }
            }
            Message::SystemFontsLoaded(mut names) => {
                // Belt and braces (the task already sorts): picker rows
                // stay ordered and unique whatever arrived.
                names.sort();
                names.dedup();
                self.system_font_rows = names;
                self.font_rows = font_option_rows(&self.system_font_rows);
            }
            Message::ArtistTopLoaded { channel_id, result } => {
                // The keyed cache makes staleness a non-issue (a revisit
                // reuses it, no refetch); the marker only scopes the
                // failure status to the page still awaiting it.
                let watching = self.artist_loading.as_deref() == Some(channel_id.as_str());
                if watching {
                    self.artist_loading = None;
                }
                match result {
                    Ok(tracks) => {
                        yutu_core::app_log::info(
                            "live-detail",
                            format_args!(
                                "artist top arrived: {channel_id} ({} songs)",
                                tracks.len()
                            ),
                        );
                        let ids = self.merge_live_tracks(tracks);
                        // Empty answers stay uncached: the next visit
                        // retries instead of locking in the fallback.
                        // A late head never clobbers a landed full shelf.
                        if !ids.is_empty() && !self.artist_full.contains(&channel_id) {
                            if let Some(evicted) = Self::capped_detail_insert(
                                &mut self.artist_top,
                                &mut self.artist_top_order,
                                channel_id,
                                ids,
                            ) {
                                self.artist_full.remove(&evicted);
                            }
                        }
                    }
                    // "failed" keeps it visible in release builds
                    // (status shows failures only); the catalog
                    // fallback stays on screen regardless. Stays on
                    // the page that launched it.
                    Err(e) => {
                        yutu_core::app_log::warn(
                            "live-detail",
                            format_args!("artist top failed: {channel_id} ({e})"),
                        );
                        if watching && self.route_shows_browse(&channel_id, true) {
                            self.status = Self::failure_status(self.language.fail_live_top(), &e);
                        }
                    }
                }
            }
            Message::ArtistFullLoaded { channel_id, result } => {
                let watching = self.artist_full_loading.as_deref() == Some(channel_id.as_str());
                if watching {
                    self.artist_full_loading = None;
                }
                match result {
                    Ok(tracks) => {
                        yutu_core::app_log::info(
                            "live-detail",
                            format_args!(
                                "artist full arrived: {channel_id} ({} songs)",
                                tracks.len()
                            ),
                        );
                        let ids = self.merge_live_tracks(tracks);
                        // Empty answers stay uncached (same retry rule
                        // as the head fetch).
                        if !ids.is_empty() {
                            if let Some(evicted) = Self::capped_detail_insert(
                                &mut self.artist_top,
                                &mut self.artist_top_order,
                                channel_id.clone(),
                                ids,
                            ) {
                                self.artist_full.remove(&evicted);
                            }
                            self.artist_full.insert(channel_id);
                        }
                    }
                    Err(e) => {
                        yutu_core::app_log::warn(
                            "live-detail",
                            format_args!("artist full failed: {channel_id} ({e})"),
                        );
                        if watching && self.route_shows_browse(&channel_id, true) {
                            self.status = Self::failure_status(self.language.fail_live_top(), &e);
                        }
                    }
                }
            }
            Message::AlbumLiveLoaded { album_id, result } => {
                let watching = self.album_loading.as_deref() == Some(album_id.as_str());
                if watching {
                    self.album_loading = None;
                }
                match result {
                    Ok(tracks) => {
                        yutu_core::app_log::info(
                            "live-detail",
                            format_args!(
                                "album tracklist arrived: {album_id} ({} songs)",
                                tracks.len()
                            ),
                        );
                        let ids = self.merge_live_tracks(tracks);
                        if !ids.is_empty() {
                            let _ = Self::capped_detail_insert(
                                &mut self.album_live,
                                &mut self.album_live_order,
                                album_id,
                                ids,
                            );
                        }
                    }
                    Err(e) => {
                        yutu_core::app_log::warn(
                            "live-detail",
                            format_args!("album tracklist failed: {album_id} ({e})"),
                        );
                        if watching && self.route_shows_browse(&album_id, false) {
                            self.status = Self::failure_status(self.language.fail_live_album(), &e);
                        }
                    }
                }
            }
            Message::ArtistChannelResolved {
                route_id,
                name,
                result,
            } => {
                let watching = self.artist_resolving.as_deref() == Some(route_id.as_str());
                if watching {
                    self.artist_resolving = None;
                }
                // Statuses stay on the page that launched them.
                let showing = matches!(&self.route, Route::Artist(rid) if rid == &route_id);
                match result {
                    Ok(channel) => {
                        yutu_core::app_log::info(
                            "live-detail",
                            format_args!("artist resolved: {name} -> {channel}"),
                        );
                        Self::alias_insert(&mut self.artist_alias, name, channel.clone());
                        Self::alias_insert(&mut self.artist_alias, route_id, channel.clone());
                        if !self.artist_top.contains_key(&channel) {
                            self.artist_loading = Some(channel.clone());
                            #[cfg(feature = "innertube")]
                            {
                                return Self::artist_top_task(channel);
                            }
                        }
                    }
                    Err(e) => {
                        yutu_core::app_log::warn(
                            "live-detail",
                            format_args!("artist resolve failed: {name} ({e})"),
                        );
                        if watching && showing {
                            self.status =
                                Self::failure_status(self.language.fail_artist_lookup(), &e);
                        }
                    }
                }
            }
            Message::AlbumBrowseResolved {
                route_id,
                title,
                result,
            } => {
                let watching = self.album_resolving.as_deref() == Some(route_id.as_str());
                if watching {
                    self.album_resolving = None;
                }
                let showing = matches!(&self.route, Route::Album(rid) if rid == &route_id);
                match result {
                    Ok(browse) => {
                        yutu_core::app_log::info(
                            "live-detail",
                            format_args!("album resolved: {title} -> {browse}"),
                        );
                        Self::alias_insert(&mut self.album_alias, route_id, browse.clone());
                        Self::alias_insert(&mut self.album_alias, title, browse.clone());
                        if !self.album_live.contains_key(&browse) {
                            self.album_loading = Some(browse.clone());
                            #[cfg(feature = "innertube")]
                            {
                                return Self::album_live_task(browse);
                            }
                        }
                    }
                    Err(e) => {
                        yutu_core::app_log::warn(
                            "live-detail",
                            format_args!("album resolve failed: {title} ({e})"),
                        );
                        if watching && showing {
                            self.status =
                                Self::failure_status(self.language.fail_album_lookup(), &e);
                        }
                    }
                }
            }
            Message::ArtistTopExpand => {
                self.artist_expanded = true;
                #[cfg(feature = "innertube")]
                {
                    // Full shelf on demand (head already paints):
                    // cached full lists expand instantly, otherwise the
                    // raw shelf fetches behind a spinner beside Show
                    // more.
                    let channel = match &self.route {
                        Route::Artist(rid) => self.artist_channel_id(rid),
                        _ => None,
                    };
                    if let Some(channel) = channel {
                        if !self.artist_full.contains(&channel)
                            && self.artist_full_loading.as_deref() != Some(channel.as_str())
                        {
                            self.artist_full_loading = Some(channel.clone());
                            yutu_core::app_log::info(
                                "live-detail",
                                format_args!("artist full fetch: {channel}"),
                            );
                            return Self::artist_full_task(channel);
                        }
                    }
                }
            }
            Message::RelaunchNow => {
                // One-click restart for the pending font pick: the font
                // pref is already stored. Identity + EQ flush first (a
                // manual quit relies on the same savers, so this keeps
                // parity with it); a failed save cancels instead of
                // exiting into a lost session. Deliberately untestable
                // (the success path diverges).
                if !(self.persist_eq() && self.persist_queue()) {
                    self.status = self.language.relaunch_cancelled();
                    yutu_core::app_log::warn(
                        "relaunch",
                        format_args!("session save failed, staying put"),
                    );
                } else {
                    match std::env::current_exe() {
                        Ok(exe) => match std::process::Command::new(&exe).spawn() {
                            Ok(_) => std::process::exit(0),
                            Err(e) => {
                                self.status =
                                    "Relaunch failed: quit and start the app manually.".into();
                                yutu_core::app_log::warn(
                                    "relaunch",
                                    format_args!("spawn {} failed: {e}", exe.display()),
                                );
                            }
                        },
                        Err(e) => {
                            self.status =
                                "Relaunch failed: quit and start the app manually.".into();
                            yutu_core::app_log::warn(
                                "relaunch",
                                format_args!("current_exe failed: {e}"),
                            );
                        }
                    }
                }
            }
            Message::SidebarFilterChanged(f) => {
                self.sidebar_filter = f;
            }
            Message::HistoryFilterChanged(f) => {
                self.history_filter = f;
                // A new query restarts the render window (and any stale
                // row hover dies with the old indices).
                self.history_limit = HISTORY_PAGE_SIZE;
                self.hovered_history = None;
            }
            Message::HistoryMore => {
                self.history_limit = self.history_limit.saturating_add(HISTORY_PAGE_SIZE);
            }
            Message::CookiePathChanged(p) => {
                self.cookie_path = p;
            }
            #[cfg(feature = "innertube")]
            Message::CookieBrowse => {
                return Self::cookie_browse_task();
            }
            #[cfg(feature = "innertube")]
            Message::CookieFilePicked(picked) => {
                // Cancelled dialogs leave the previous pick alone.
                if let Some(p) = picked {
                    if !p.trim().is_empty() {
                        self.cookie_path = p;
                    }
                }
            }
            Message::CookieImport => {
                let path = self.cookie_path.trim().to_string();
                #[cfg(feature = "innertube")]
                {
                    if path.is_empty() {
                        self.status =
                            "Choose your exported cookies file first (see the note below).".into();
                    } else {
                        self.login_busy = true;
                        self.import_active = true;
                        self.sync_active = false;
                        self.status = self.language.reading_cookies();
                        // Capture the source now: the modal decision later
                        // must name the file actually imported, not
                        // whatever the field holds when stages finish.
                        self.pending_cookie_source = Some(path.clone());
                        self.pending_cookie_cleanup = None;
                        self.publish_mpris();
                        return Self::cookie_import_task(path, self.library_path.clone());
                    }
                }
                #[cfg(not(feature = "innertube"))]
                {
                    let _ = path;
                    self.status = self.language.needs_innertube_import_offline();
                }
            }
            #[cfg(feature = "innertube")]
            Message::ImportStageReady { kind, result } => match result {
                Ok(staged) => {
                    // Connection succeeded and the session is stored:
                    // offer to delete the plaintext export now, without
                    // waiting for the playlist stages behind it.
                    if kind == ImportKind::Cookie {
                        self.arm_cookie_cleanup();
                    }
                    return self.begin_playlist_stages(kind, staged);
                }
                Err(e) => {
                    self.login_busy = false;
                    self.import_active = false;
                    self.sync_active = false;
                    // A failed fresh import owns no session: drop its
                    // source so no modal can name it later.
                    if kind == ImportKind::Cookie {
                        self.pending_cookie_source = None;
                    }
                    // A dead saved session must not keep posing as signed
                    // in: drop it so the panel goes back to the file picker.
                    if matches!(kind, ImportKind::Cookie | ImportKind::Sync)
                        && e.contains("saved cookie session")
                    {
                        let _ = delete_cookie_session_secure();
                        self.cookie_logged_in = false;
                    }
                    self.status = if kind == ImportKind::Sync {
                        yutu_core::app_log::warn("yutu-import", format!("sync: {e}"));
                        Self::failure_status(self.language.fail_sync(), &e)
                    } else {
                        yutu_core::app_log::warn("yutu-import", format!("{}: {e}", kind.label()));
                        Self::failure_status(self.language.import_failed(kind), &e)
                    };
                }
            },
            #[cfg(feature = "innertube")]
            Message::PlaylistImported {
                kind,
                gen,
                title,
                index,
                total,
                count,
                result,
            } => {
                // Stragglers from an older run (re-import, sign-out):
                // ignore, never merge, never touch the counters.
                if gen != self.import_gen {
                    return iced::Task::none();
                }
                // A newer run re-staged while this list was in flight:
                // same treatment (totals belong to the new run).
                if total != self.import_total {
                    return iced::Task::none();
                }
                let _ = index;
                self.import_done += 1;
                let done = self.import_done;
                // This list leaves the live set: its songs now count via
                // Completed songs already count via `import_songs`;
                // failures drop their share out of the expected total
                // (so the bar can still reach 100%). A Done that
                // beat its ticks here still carries the details count, so
                // the expected total stays exact in every ordering.
                if let Some(live) = self.import_live.remove(&index) {
                    if result.is_err() {
                        if let Some(c) = live.count {
                            self.import_expected = self.import_expected.saturating_sub(c);
                        }
                    }
                } else if let Some(c) = count {
                    self.import_expected += c;
                }
                self.import_finished.insert(index);
                let finish = |s: &mut Self| {
                    s.login_busy = false;
                    s.import_active = false;
                    s.sync_active = false;
                    s.mark_synced();
                    if kind == ImportKind::Cookie {
                        s.cookie_logged_in = cookie_session_exists();
                    }
                };
                match result {
                    Ok(playlist) => {
                        let n = playlist.tracks.len();
                        let pid = playlist.id.clone();
                        self.import_songs += n;
                        let tasks = self.merge_imported(Vec::new(), Vec::new(), vec![playlist]);
                        self.refresh_playlist_added(&pid);
                        if done >= total {
                            finish(self);
                            self.status = self.language.import_done_lists(
                                kind,
                                self.import_songs,
                                self.import_likes,
                                total,
                            );
                        } else {
                            self.status = self
                                .language
                                .playlist_fetch_progress(&title, n, done, total);
                        }
                        self.publish_mpris();
                        return iced::Task::batch(tasks);
                    }
                    Err(e) => {
                        if done >= total {
                            finish(self);
                            // "failures" keeps the release status filter
                            // (`status_text` shows only lines with "fail").
                            self.status = self.language.import_done_lists_errors(
                                kind,
                                self.import_songs,
                                self.import_likes,
                                done,
                                total,
                            );
                        } else {
                            self.status =
                                self.language.playlist_fetch_failed(&title, &e, done, total);
                        }
                        self.publish_mpris();
                    }
                }
            }
            #[cfg(feature = "innertube")]
            Message::PlaylistTick {
                gen,
                index,
                arrived,
                count,
            } => {
                // Quarantine stale or already-finished arrivals; ticks only
                // move banner counters, never merge data (no publish: the
                // status text is untouched, so widgets see no change).
                if gen != self.import_gen || self.import_finished.contains(&index) {
                    return iced::Task::none();
                }
                match self.import_live.get_mut(&index) {
                    Some(live) => {
                        live.arrived = arrived;
                        if live.count.is_none() {
                            live.count = count;
                        }
                    }
                    None => {
                        yutu_core::app_log::info(
                            "yutu-import",
                            format_args!("playlist {index}: {arrived} songs arrived"),
                        );
                        if let Some(c) = count {
                            self.import_expected += c;
                        }
                        self.import_live
                            .insert(index, PlaylistLive { arrived, count });
                    }
                }
            }
            #[cfg(feature = "innertube")]
            Message::CookieSessionImport => {
                if !cookie_session_exists() {
                    self.status = self.language.cookie_session_lost();
                } else {
                    self.login_busy = true;
                    self.import_active = true;
                    self.sync_active = false;
                    self.status = self.language.importing_library();
                    self.publish_mpris();
                    return Self::cookie_session_import_task(self.library_path.clone());
                }
            }
            #[cfg(feature = "innertube")]
            Message::RequestSignOut => {
                // Restart the entrance only when nothing is open: the
                // ticker is shared, so resetting under a settled dialog
                // would make its card jump down and re-rise.
                if self.modal_target() == 0.0 {
                    self.modal_anim = 0.0;
                }
                self.pending_signout = true;
            }
            #[cfg(feature = "innertube")]
            Message::CancelSignOut => {
                self.pending_signout = false;
            }
            #[cfg(feature = "innertube")]
            Message::CookieLogout => {
                self.pending_signout = false;
                // Orphan in-flight playlist arrivals (generation bump).
                self.import_gen += 1;
                self.import_done = 0;
                self.import_total = 0;
                self.import_active = false;
                self.sync_active = false;
                self.import_expected = 0;
                self.import_shown = 0;
                self.import_live.clear();
                self.import_finished.clear();
                self.pending_cookie_cleanup = None;
                self.pending_cookie_source = None;
                if cookie_session_exists() {
                    match delete_cookie_session_secure() {
                        Ok(()) => {
                            self.cookie_logged_in = false;
                            self.status = self.language.cookie_cleared();
                        }
                        Err(e) => {
                            yutu_core::app_log::warn("yutu-auth", format!("sign-out: {e}"));
                            self.status = Self::failure_status(self.language.fail_signout(), &e);
                        }
                    }
                } else {
                    self.cookie_logged_in = false;
                    self.status = self.language.cookie_cleared();
                }
            }
            #[cfg(feature = "innertube")]
            Message::CookieCleanupDelete => {
                if let Some(src) = self.pending_cookie_cleanup.take() {
                    self.cookie_path.clear();
                    match std::fs::remove_file(&src) {
                        Ok(()) => {
                            yutu_core::app_log::info(
                                "yutu-auth",
                                format_args!("export file deleted"),
                            );
                            // Progress owns the status while playlists
                            // load; confirm only once idle.
                            if !self.import_active {
                                self.status = self.language.export_deleted();
                            }
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                            if !self.import_active {
                                self.status = self.language.export_already_gone();
                            }
                        }
                        Err(e) => {
                            yutu_core::app_log::warn(
                                "yutu-auth",
                                format!("export delete failed: {e}"),
                            );
                            self.status =
                                Self::failure_status(self.language.fail_export_delete(), e);
                            // Re-arm so the user can retry (file kept).
                            self.pending_cookie_cleanup = Some(src);
                        }
                    }
                }
            }
            #[cfg(feature = "innertube")]
            Message::CookieCleanupKeep => {
                self.pending_cookie_cleanup = None;
                self.cookie_path.clear();
                if !self.import_active {
                    self.status =
                        "Signed in. Export file kept (delete it manually when ready).".into();
                }
            }
            #[cfg(feature = "innertube")]
            Message::LikeSynced {
                track_id,
                liked,
                result,
            } => match result {
                Ok(()) => self.clear_pending_rate(if liked { "like" } else { "unlike" }, &track_id),
                Err(e) => {
                    let verb = if liked { "like" } else { "unlike" };
                    yutu_core::app_log::warn("yutu-sync", format!("{verb} {track_id}: {e}"));
                    self.status =
                        Self::failure_status(self.language.fail_youtube_op(verb, &track_id), &e);
                }
            },
            #[cfg(feature = "innertube")]
            Message::DislikeSynced {
                track_id,
                disliked,
                result,
            } => match result {
                Ok(()) => self
                    .clear_pending_rate(if disliked { "dislike" } else { "undislike" }, &track_id),
                Err(e) => {
                    let verb = if disliked {
                        "don't-recommend"
                    } else {
                        "clear-dislike"
                    };
                    yutu_core::app_log::warn("yutu-sync", format!("{verb} {track_id}: {e}"));
                    self.status =
                        Self::failure_status(self.language.fail_youtube_op(verb, &track_id), &e);
                }
            },
            #[cfg(feature = "innertube")]
            Message::ServerPlaylistCreated {
                local_id,
                title,
                result,
            } => match result {
                Ok(server_id) => {
                    // The create row clears first (by shell id), then any
                    // add/remove rows recorded mid-flight move shell id to
                    // server id for the flush below.
                    if let Ok(store) = LibraryStore::open(&self.library_path) {
                        let _ = store.clear_pending("create", Some(&local_id), None);
                        let _ = store.repoint_pending(&local_id, &server_id);
                    }
                    // Migrate the local shell to the real server id, in
                    // memory and in SQLite, so later edits sync. Tracks
                    // added while the create was in flight ride along:
                    // they flush to the server id below.
                    let Some(pos) = self.user_playlists.iter().position(|p| p.id == local_id)
                    else {
                        self.status = self.language.created_on_youtube(&title, &server_id);
                        return iced::Task::none();
                    };
                    let pl = self.user_playlists.remove(pos);
                    if self.route == Route::PlaylistDetail(local_id.clone()) {
                        self.go(Route::PlaylistDetail(server_id.clone()));
                    }
                    // Import race: fold the shell rows into the existing
                    // server list instead of duplicating it.
                    let merged = match self.user_playlists.iter().find(|p| p.id == server_id) {
                        Some(existing) => {
                            let mut m = existing.clone();
                            for t in &pl.tracks {
                                if !m.tracks.iter().any(|x| x.id == t.id) {
                                    m.tracks.push(t.clone());
                                }
                            }
                            m
                        }
                        None => Playlist {
                            id: server_id.clone(),
                            title: pl.title.clone(),
                            tracks: pl.tracks,
                        },
                    };
                    if let Ok(store) = LibraryStore::open(&self.library_path) {
                        let _ =
                            store.sync_server_playlist(&server_id, &merged.title, &merged.tracks);
                        let _ = store.delete_playlist(&local_id);
                    }
                    // Carry the sort pick + insertion times to the server
                    // id (the shell id dies here): existing server picks
                    // win on import races, otherwise the shell pick moves.
                    if let Some(sort) = self.playlist_sorts.remove(&local_id) {
                        if !self.playlist_sorts.contains_key(&server_id) {
                            self.persist_playlist_sort(&server_id, sort);
                        }
                        if let Ok(store) = LibraryStore::open(&self.library_path) {
                            let _ = store.set_pref(&Self::playlist_sort_key(&local_id), "");
                        }
                    }
                    self.playlist_added.remove(&local_id);
                    self.refresh_playlist_added(&server_id);
                    if self.pl_drag_id.as_deref() == Some(local_id.as_str()) {
                        self.cancel_pl_drag();
                    }
                    self.pl_hover = None;
                    self.pl_action_hover = None;
                    match self.user_playlists.iter().position(|p| p.id == server_id) {
                        Some(i) => self.user_playlists[i] = merged.clone(),
                        None => self
                            .user_playlists
                            .insert(pos.min(self.user_playlists.len()), merged.clone()),
                    }
                    self.status = self.language.created_synced(&merged.title);
                    self.publish_mpris();
                    // Flush any in-flight rows to the server id (failures
                    // report per track, the local rows are kept).
                    return iced::Task::batch(
                        merged
                            .tracks
                            .iter()
                            .map(|t| Self::playlist_add_sync_task(server_id.clone(), t.id.clone()))
                            .collect::<Vec<_>>(),
                    );
                }
                Err(e) => {
                    yutu_core::app_log::warn("yutu-sync", format!("create {title}: {e}"));
                    self.status =
                        Self::failure_status(self.language.fail_created_local(&title), &e);
                }
            },
            #[cfg(feature = "innertube")]
            Message::ServerPlaylistSynced { detail, op, result } => match result {
                Ok(()) => match &op {
                    PlaylistSyncOp::Add {
                        playlist_id,
                        track_id,
                    } => {
                        if let Ok(store) = LibraryStore::open(&self.library_path) {
                            let _ = store.clear_pending(
                                "add",
                                Some(playlist_id.as_str()),
                                Some(track_id.as_str()),
                            );
                        }
                    }
                    PlaylistSyncOp::Remove {
                        playlist_id,
                        track_id,
                    } => {
                        if let Ok(store) = LibraryStore::open(&self.library_path) {
                            let _ = store.clear_pending(
                                "remove",
                                Some(playlist_id.as_str()),
                                Some(track_id.as_str()),
                            );
                        }
                    }
                    PlaylistSyncOp::Delete { playlist_id } => {
                        if let Ok(store) = LibraryStore::open(&self.library_path) {
                            let _ = store.clear_pending("delete", Some(playlist_id.as_str()), None);
                        }
                    }
                    PlaylistSyncOp::Rename { playlist_id, title } => {
                        if let Ok(store) = LibraryStore::open(&self.library_path) {
                            let _ = store.clear_pending_rename(playlist_id, title);
                        }
                    }
                },
                Err(e) => {
                    yutu_core::app_log::warn("yutu-sync", format!("{detail}: {e}"));
                    self.status =
                        Self::failure_status(self.language.fail_kept_locally(&detail), &e);
                }
            },
            #[cfg(feature = "innertube")]
            Message::GoogleLoginStart => {
                self.login_busy = true;
                self.status = self.language.google_device_code();
                self.publish_mpris();
                return Self::google_begin_task();
            }
            #[cfg(feature = "innertube")]
            Message::GoogleLoginBegan { result } => {
                self.login_busy = false;
                match result {
                    Ok(login) => {
                        // Terminal copy: the URL is clickable there; the
                        // Settings panel shows it as text (Iced copy TBA).
                        yutu_core::app_log::info(
                            "yutu-login",
                            format_args!(
                                "open {} and enter {}",
                                login.verification_url, login.user_code
                            ),
                        );
                        self.google_login = Some(login);
                        self.status =
                            "Approve in your browser, then press “I’ve approved: check”.".into();
                    }
                    Err(e) => {
                        yutu_core::app_log::warn("yutu-login", format!("begin: {e}"));
                        self.status = Self::failure_status(self.language.fail_signin(), &e);
                    }
                }
            }
            #[cfg(feature = "innertube")]
            Message::GoogleLoginPoll => match self.google_login.clone() {
                None => {
                    self.status = self.language.google_signin_first();
                }
                Some(login) => {
                    self.login_busy = true;
                    self.status = self.language.checking_approval();
                    self.publish_mpris();
                    return Self::google_poll_task(login);
                }
            },
            #[cfg(feature = "innertube")]
            Message::GoogleLoginPolled { result } => {
                self.login_busy = false;
                match result {
                    Ok(Some(persist)) => {
                        match persist.save_to_file(std::path::Path::new(&oauth_path())) {
                            Ok(()) => {
                                self.logged_in = true;
                                self.google_login = None;
                                self.login_busy = true;
                                self.import_active = true;
                                self.sync_active = false;
                                self.status = self.language.signed_in_importing();
                                self.publish_mpris();
                                return Self::library_import_task(self.library_path.clone());
                            }
                            Err(e) => {
                                yutu_core::app_log::warn(
                                    "yutu-login",
                                    format!("save session: {e}"),
                                );
                                self.status = Self::failure_status(self.language.fail_signin(), &e);
                            }
                        }
                    }
                    Ok(None) => {
                        self.status =
                            "Still waiting: approve in your browser, then press Check again."
                                .into();
                    }
                    Err(e) => {
                        if e.contains("expired") {
                            self.google_login = None;
                            self.status =
                                "Code expired: press “Sign in with Google” to restart.".into();
                        } else {
                            yutu_core::app_log::warn("yutu-login", format!("poll: {e}"));
                            self.status = Self::failure_status(self.language.fail_check(), &e);
                        }
                    }
                }
            }
            #[cfg(feature = "innertube")]
            Message::GoogleLogout => {
                let _ = std::fs::remove_file(oauth_path());
                // Orphan in-flight playlist arrivals (generation bump).
                self.import_gen += 1;
                self.import_done = 0;
                self.import_total = 0;
                self.import_active = false;
                self.sync_active = false;
                self.import_expected = 0;
                self.import_shown = 0;
                self.import_live.clear();
                self.import_finished.clear();
                self.logged_in = false;
                self.google_login = None;
                self.status = self.language.signed_out_token();
            }
            #[cfg(feature = "innertube")]
            Message::LibraryImport => {
                if !self.logged_in {
                    self.status = self.language.google_signin_needed();
                } else {
                    self.login_busy = true;
                    self.import_active = true;
                    self.sync_active = false;
                    self.status = self.language.reading_library();
                    self.publish_mpris();
                    return Self::library_import_task(self.library_path.clone());
                }
            }
            #[cfg(feature = "innertube")]
            Message::SyncNow => {
                return self.start_sync(false);
            }
            #[cfg(feature = "innertube")]
            Message::AutoSyncToggled(on) => {
                self.auto_sync = on;
                if let Ok(store) = LibraryStore::open(&self.library_path) {
                    let _ = store.set_pref("auto_sync", if on { "1" } else { "0" });
                }
                self.status = if on {
                    "Auto-sync on (every 15 min while signed in).".into()
                } else {
                    "Auto-sync off.".into()
                };
            }
            Message::Frame(at) => {
                // Debug FPS meter: fold the frame interval into the EMA.
                // The subscription only exists where the readout does
                // (debug builds / `debug-panel`), so release builds
                // never wake up just to count.
                if let Some(last) = self.last_frame {
                    let dt = at.saturating_duration_since(last).as_secs_f32();
                    self.fps = Self::fps_ema(self.fps, dt);
                }
                self.last_frame = Some(at);
                return iced::Task::none();
            }
            Message::Tick(now) => {
                // Desktop transport commands first (widget / media keys).
                let mut tasks = vec![self.drain_player_commands()];
                // First frame after launch: lazily fetch only the Home
                // screenful (cached titles + disk covers already painted).
                // Startup itself does zero network so the window opens fast.
                if !self.lazy_init_done {
                    self.lazy_init_done = true;
                    tasks.push(self.ensure_visible_tasks());
                }
                // Home/Discovery shelves rank the whole catalog on every
                // view build otherwise; refresh the memo at tick cadence
                // instead so frames stay cheap while data stays fresh.
                if matches!(self.route, Route::Home | Route::Discovery) {
                    self.refresh_home_shelves();
                }
                // Position clock: manual advance while a user seek settles
                // (the backend reports the stale pre-seek position for a
                // tick or two: that stale tick is the lyrics flicker).
                let dt = self
                    .last_tick
                    .map(|last| now.duration_since(last).as_secs_f64())
                    .unwrap_or(0.0)
                    .clamp(0.0, 2.0);
                self.last_tick = Some(now);
                // Session saver: queue changes land within a tick, the
                // playing position refreshes every 30s.
                self.maybe_persist_queue(now);
                // EQ trailing edge: slider nudges deferred by the drag
                // throttle land here (at most one extra `af` push).
                // Bypassed EQ never pushes (the curve waits for the
                // toggle).
                if self.eq_enabled
                    && self.eq_dirty
                    && self.eq_cooldown_until.is_none_or(|until| now >= until)
                {
                    self.eq_dirty = false;
                    self.eq_cooldown_until = Some(now + std::time::Duration::from_millis(600));
                    self.apply_eq_curve();
                }
                // EQ curve saver: gains/bypass persist on change (drags
                // settle within a tick, no write storm).
                if self.eq_saved.as_ref().is_none_or(|(n, g, e)| {
                    *n != self.eq_preset || *g != self.eq_gains || *e != self.eq_enabled
                }) {
                    self.persist_eq();
                }
                // Playlist flyout hover grace: open on trigger hover,
                // close once the cursor has left both the trigger and
                // the panel past the grace (crossing the gap exits one
                // before entering the other, so an immediate close
                // would flicker mid-transit).
                if self.submenu_playlists_for.is_some()
                    && !self.submenu_trigger_hot
                    && !self.submenu_fly_hot
                    && self
                        .submenu_hot_at
                        .is_some_and(|at| now.duration_since(at) > SUBMENU_HOVER_GRACE)
                {
                    self.close_playlist_submenu();
                }
                if self.seek_drag.is_some() {
                    // Drag preview holds the display: no backend reads
                    // (paused) and no clock math until release commits.
                    // Orphaned drags (canvas flag lost, e.g. a layout
                    // switch mid-gesture) expire after 10s motionless:
                    // cancel, resuming a drag-pause, instead of gating
                    // the advance forever.
                    if let Some(at) = self.seek_drag_at {
                        if now.duration_since(at).as_secs() >= 10 {
                            self.seek_drag = None;
                            self.seek_drag_at = None;
                            if self.seek_paused {
                                self.seek_paused = false;
                                if let Some(audio) = &self.audio {
                                    audio.resume();
                                }
                                self.paused = false;
                                self.paused_seek = false;
                            }
                        }
                    }
                } else if self.loading_track.is_some() {
                    // A new load stopped the backend (0.0) while the
                    // URL/bytes resolve: keep the manual position (0.0
                    // or a queued seek) instead of syncing the stopped
                    // clock, or the bar snaps back until audio lands.
                } else if self.pending_seek.is_some() {
                    // A seek the backend could not take yet (fresh
                    // stream still demuxing): retry until it lands and
                    // keep showing the target meanwhile, or the bar
                    // snaps back to the stale clock.
                    self.flush_pending_seek();
                } else if self.seek_hold_until.is_some_and(|until| now < until) {
                    if !self.paused {
                        self.position_secs = (self.position_secs + dt).max(0.0);
                    }
                    // Paused: freeze the user-set position either way.
                } else if self.paused && self.paused_seek {
                    // Seek landed while paused and the backend never caught
                    // up: keep the manual position until resume.
                } else {
                    self.seek_hold_until = None;
                    self.paused_seek = false;
                    self.position_secs = self.audio_position();
                }
                let duration = self.audio_duration();
                // Raw backend clock (no metadata fallback): mpv reports
                // 0/0 when idle, which is exactly the EOF signature.
                let raw_pos = self
                    .audio
                    .as_ref()
                    .map(|a| a.position_secs())
                    .unwrap_or(0.0)
                    .max(0.0);
                let raw_dur = self
                    .audio
                    .as_ref()
                    .map(|a| a.duration_secs(0.0))
                    .unwrap_or(0.0)
                    .max(0.0);
                // Audible playback seen for this load: any nonzero raw
                // position means the demuxer produced timestamps (past
                // the silent load gap), so the net below cannot mistake
                // a slow start (or an instant failure, which never
                // leaves 0.0) for a finished song.
                if raw_pos > 0.0 {
                    self.heard_audio = true;
                    self.stream_opened_at = None;
                }
                // Stream that never opened (edge refused the URL): mpv
                // went idle with nothing heard. Drop the dead URL and
                // take the normal retry/skip path (fresh resolve).
                if self.stream_open_failed(now) {
                    if let Some((_, id)) = self.stream_opened_at.take() {
                        self.url_cache.remove(&id);
                        if self.prefetched.as_ref().is_some_and(|(p, _)| p == &id) {
                            self.prefetched = None;
                        }
                        tasks.push(self.retry_or_skip_failed(
                            id,
                            "the stream never started (server refused the URL)".into(),
                        ));
                    }
                }
                // Prefetch the next song 2s before the current one ends,
                // so the advance lands on a warm URL. One attempt per
                // track: failures stay silent until the advance, which
                // retries with its own budget.
                if let Some(id) = self.prefetch_due(self.position_secs, duration) {
                    if self.prefetch_attempted.as_deref() != Some(id.as_str()) {
                        self.prefetch_attempted = Some(id.clone());
                        tasks.push(Self::prefetch_task(id));
                    }
                }
                // Auto-advance once per track end. The threshold sits 1s
                // before the reported duration: mpv's time-pos can hold just
                // short of it, and the rodio wall-clock can overshoot, so
                // both cross (duration - 1) exactly once per song. Held
                // drags never advance (the preview is not a position);
                // neither does an unapplied queued seek (aspirational,
                // not real: applying it near the end must not skip).
                if !self.paused
                    && !self.end_advanced
                    && self.seek_drag.is_none()
                    && self.pending_seek.is_none()
                    && duration > 2.0
                    && self.position_secs >= duration - 1.0
                    && self.advance_queue()
                {
                    yutu_core::app_log::info("yutu-play", "track reached its end: advancing");
                    self.end_advanced = true;
                    self.publish_mpris();
                    tasks.push(self.play_current_task());
                }
                // EOF safety net: at end-of-file the backend goes idle
                // (mpv time-pos errors to 0.0) while the metadata
                // duration survives, so the threshold above can never
                // fire when the last live read sat below it (frozen
                // tail, short reads). A backend gone silent after
                // audible playback is a finished track: advance the
                // same way (a mid-play stream death skips rather than
                // wedging the queue; the give-up path still logs). Held
                // drags pin it too (release re-arms from the new spot),
                // as does an unapplied queued seek.
                if self.seek_drag.is_none()
                    && self.pending_seek.is_none()
                    && self.eof_idle(raw_pos, raw_dur)
                    && self.advance_queue()
                {
                    yutu_core::app_log::info(
                        "yutu-play",
                        "backend went idle after audible playback: advancing",
                    );
                    self.end_advanced = true;
                    self.publish_mpris();
                    tasks.push(self.play_current_task());
                }
                // Import banner smoothing toward the true arrived count.
                #[cfg(feature = "innertube")]
                {
                    self.ease_import_shown();
                }
                // Drawer lyrics follow: centers the verse on change.
                tasks.push(self.lyric_follow_task());
                // Scheduled sync: web changes arrive on their own while
                // signed in (plus Sync now in Settings). Idle only: never
                // while a pull is already in flight.
                #[cfg(feature = "innertube")]
                {
                    if sync_due(self.auto_sync, self.last_sync, self.started_at, now)
                        && Self::has_session()
                    {
                        tasks.push(self.start_sync(true));
                    }
                }
                self.publish_mpris();
                return iced::Task::batch(tasks);
            }
            Message::SeekTo(pos) => {
                self.seek_or_preview(pos as f64);
                // Retarget like a verse tap (the ticker glides there).
                self.lyrics_follow_at = None;
            }
            Message::SeekPreview(pos) => self.preview_seek(pos),
            Message::SeekRelease => self.release_seek(),
            Message::SeekToMs(ms) => {
                self.seek_or_preview(ms as f64 / 1000.0);
                // Force the snap even inside the live verse (equality
                // would otherwise suppress it while off-verse).
                self.lyrics_follow_at = None;
                self.publish_mpris();
                return self.lyric_follow_task();
            }
            Message::UrlResolved { track_id, result } => {
                // Only the in-flight track may report; anything else is stale.
                if self.loading_track.as_deref() != Some(track_id.as_str()) {
                    return iced::Task::none();
                }
                self.loading_track = None;
                match result {
                    Err(e) => {
                        return self.retry_or_skip_failed(track_id, e);
                    }
                    Ok(url) => {
                        if let Some(expires) = url_expiry(&url) {
                            // Sweep dead entries first (URLs expire in
                            // hours), then cap: unbounded sessions must
                            // not pin a URL per song ever played.
                            let now = now_unix() as u64;
                            self.url_cache.retain(|_, (_, exp)| *exp > now);
                            if self.url_cache.len() >= URL_CACHE_CAP {
                                if let Some(key) = self.url_cache.keys().next().cloned() {
                                    self.url_cache.remove(&key);
                                }
                            }
                            self.url_cache
                                .insert(track_id.clone(), (url.clone(), expires));
                        }
                        if let Some(t) = self.queue.current().cloned() {
                            if t.id == track_id && !self.try_play_url(&url, &t) {
                                // Stream play failed: fall back to bytes.
                                self.loading_track = Some(t.id.clone());
                                self.publish_mpris();
                                return Self::resolve_task(t.id);
                            }
                        }
                    }
                }
            }
            Message::UrlPrefetched { track_id, result } => {
                // Keep it only if it is still the upcoming track.
                if self.peek_next_id().as_deref() == Some(track_id.as_str()) {
                    if let Ok(url) = result {
                        self.prefetched = Some((track_id, url));
                    }
                }
            }
            Message::StreamResolved { video_id, result } => {
                // Stale errors (user moved on mid-fetch) must not retry
                // or skip the new song: only the loader may report.
                if result.is_err() && self.loading_track.as_deref() != Some(video_id.as_str()) {
                    return iced::Task::none();
                }
                match result {
                    Err(e) => {
                        return self.retry_or_skip_failed(video_id, e);
                    }
                    Ok(bytes) => {
                        // Stale arrival (user moved on): drop without
                        // touching the new load's flag, or its resolve
                        // would be orphaned and the song never starts.
                        if self.loading_track.as_deref() != Some(video_id.as_str()) {
                            return iced::Task::none();
                        }
                        self.loading_track = None;
                        // Jumped away mid-fetch: drop, keep playing.
                        let current_id = self
                            .queue
                            .current()
                            .map(|t| t.id.clone())
                            .unwrap_or_default();
                        if video_id != current_id {
                            return iced::Task::none();
                        }
                        match &self.audio {
                            None => {
                                self.pending_resume = None;
                                self.pending_seek = None;
                                self.status = self.language.stream_fetched_no_device(&video_id);
                            }
                            Some(audio) => match audio.play_track_bytes(bytes) {
                                Ok(()) => {
                                    self.paused = false;
                                    self.end_advanced = false;
                                    self.position_secs = 0.0;
                                    if let Some(t) = self.queue.current() {
                                        self.status =
                                            self.language.playing_line(&t.title, &t.artist);
                                    }
                                    self.consume_deferred_seeks();
                                    // A startup autoload buffers paused.
                                    if self.autoload {
                                        self.autoload = false;
                                        if let Some(audio) = &self.audio {
                                            audio.pause();
                                        }
                                        self.paused = true;
                                        if let Some(t) = self.queue.current() {
                                            self.status =
                                                self.language.paused_line(&t.title, &t.artist);
                                        }
                                    }
                                }
                                Err(e) => {
                                    // Fresh bytes that will not play: treat
                                    // like a failed fetch (retry, then skip).
                                    return self.retry_or_skip_failed(video_id, e.to_string());
                                }
                            },
                        }
                    }
                }
            }
            Message::TracksHydrated { requested, tracks } => {
                // Probes landed (or failed): clear the in-flight marks so a
                // later navigation can retry what is still visible.
                for id in &requested {
                    self.hydrating.remove(id);
                }
                if tracks.is_empty() {
                    // Offline or removed videos: cool down so chained
                    // visible scans do not re-probe in a loop. Explicit
                    // navigation retries after the cooldown expires.
                    for id in requested {
                        Self::note_failed(&mut self.hydrate_cooldown, id);
                    }
                    return iced::Task::none();
                }
                let registered = self.register_tracks(tracks);
                let tasks = self.artwork_tasks(&registered);
                // Still-missing ids are unresolvable right now: cool them
                // down instead of letting art arrivals re-probe at once.
                let still_missing: Vec<String> = requested
                    .into_iter()
                    .filter(|id| !self.catalog.contains_key(id))
                    .collect();
                if !still_missing.is_empty() {
                    yutu_core::app_log::warn(
                        "yutu-hydrate",
                        format_args!("{} still missing after batch", still_missing.len()),
                    );
                    for id in still_missing {
                        Self::note_failed(&mut self.hydrate_cooldown, id);
                    }
                }
                let n = registered.len();
                yutu_core::app_log::info("yutu-hydrate", format_args!("restored {n} track(s)"));
                // Nudge the status only when Favorites was empty before:
                // otherwise this is a silent background heal.
                if self.favorite_tracks().len() <= n {
                    self.status = self.language.restored_songs(n);
                }
                self.publish_mpris();
                return iced::Task::batch(tasks);
            }
        }
        self.publish_mpris();
        iced::Task::none()
    }

    /// Resolve + fetch (memory-only) task for a video ID. Runs on iced's tokio executor.
    pub(crate) fn resolve_task(video_id: String) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let result = match tokio::time::timeout(
                    std::time::Duration::from_secs(RESOLVE_BYTES_TIMEOUT_SECS),
                    resolve_and_fetch(&video_id),
                )
                .await
                {
                    Ok(r) => r,
                    Err(_) => Err(format!(
                        "fetch timed out after {RESOLVE_BYTES_TIMEOUT_SECS}s"
                    )),
                };
                (video_id, result)
            },
            |(video_id, result)| Message::StreamResolved { video_id, result },
        )
    }
}
