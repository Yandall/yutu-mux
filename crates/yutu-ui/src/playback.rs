//! yutu-ui playback: queue advance, resolve and transport core (split from lib.rs, no behavior change).

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
    /// Hero art URL: large preferred, rewritten to 544px server-side so
    /// the 300px drawer hero stays crisp. Shared by display and fetch so
    /// the cache key never drifts.
    pub(crate) fn hero_art_url(t: &Track) -> Option<String> {
        t.thumbnail_large_url
            .clone()
            .or(t.thumbnail_url.clone())
            .map(|u| yutu_core::hires_thumb_url(&u, 544))
    }

    /// Catalog-aware hero URL: stale queue rows fall back to catalog art,
    /// same as thumb_sized, so the drawer never stays empty while the
    /// player bar shows art.
    pub(crate) fn hero_art_for(&self, t: &Track) -> Option<String> {
        let merged = self.catalog.get(&t.id).unwrap_or(t);
        Self::hero_art_url(merged)
    }

    /// Fetch the hero artwork unless cached/in-flight/cooled (drawer
    /// hero): the large URL first, falling back to the small one like
    /// the hero does. Catalog-aware so stale rows warm the same key
    /// they display.
    pub(crate) fn large_art_task(&mut self, t: &Track) -> Option<iced::Task<Message>> {
        let url = self.hero_art_for(t)?;
        if self.art_mem(&url).is_some()
            || self.artwork_loading.contains(&url)
            || artwork_file_exists(&url)
            || Self::cooled(&self.artwork_failed, &url)
        {
            return None;
        }
        self.artwork_loading.insert(url.clone());
        let url_for_msg = url.clone();
        Some(iced::Task::perform(
            async move { yutu_player::fetch_bytes(&url).await },
            move |result| Message::ArtworkReady {
                url: url_for_msg.clone(),
                result: result.map_err(|e| e.to_string()),
            },
        ))
    }

    /// Play whatever is current (no-op on an empty queue). Fast paths first:
    /// a prefetched URL or a fresh cached URL plays instantly with no
    /// fetch; otherwise a resolve task runs (mpv streams the URL, rodio
    /// fetches the bytes). Sets `loading_track` so stale arrivals drop.
    pub(crate) fn play_current_task(&mut self) -> iced::Task<Message> {
        let Some(t) = self.queue.current().cloned() else {
            return iced::Task::none();
        };
        // Every audible play funnels through here: log it now so
        // Next, Prev, jumps, auto-advances, repeats, skips and bulk
        // plays all land in Recent, not just click-to-play.
        self.log_play(&t);
        // Immediate feedback: silence the old song the instant a new load
        // starts (resolving takes ~1s; the old track must not play over it).
        if let Some(audio) = &self.audio {
            audio.stop();
        }
        if self.prefetched.as_ref().is_some_and(|(id, _)| id != &t.id) {
            self.prefetched = None;
        }
        self.prefetch_attempted = None;
        self.resolve_attempts = 0;
        self.heard_audio = false;
        self.stream_opened_at = None;
        self.loading_track = Some(t.id.clone());
        self.end_advanced = false;
        self.paused = false;
        self.paused_seek = false;
        self.seek_hold_until = None;
        self.position_secs = 0.0;
        // A new load cancels any pending resume seek (it belongs to a
        // previous track); the resume flow re-arms it afterwards.
        // A queued seek from the old track dies here too; seeks made
        // during this load re-arm it.
        self.pending_resume = None;
        self.pending_seek = None;
        self.autoload = false;
        self.restored_pending = false;
        // A new load also cancels any held seek drag (the preview
        // belongs to the old track); release afterwards is a no-op.
        self.seek_drag = None;
        self.seek_drag_at = None;
        self.seek_paused = false;
        // Lyrics page follows playback: swap the id in place (no back-stack
        // spam) and fetch the new song's lyrics unless cached.
        let mut follow_tasks: Vec<iced::Task<Message>> = Vec::new();
        if matches!(self.route, Route::Lyrics(_)) && self.route != Route::Lyrics(t.id.clone()) {
            self.route = Route::Lyrics(t.id.clone());
        }
        // Lyrics + hero warming on every track start, whatever the route.
        follow_tasks.extend(self.track_start_tasks(&t));
        // Prefetched URL: instant, unless its expiry passed while we
        // sat paused (a stale URL fails slow in mpv; re-resolve fast).
        if let Some((_, url)) = self.prefetched.take() {
            let fresh = url_expiry(&url).is_none_or(|exp| exp > now_unix() as u64 + 60);
            if fresh && self.try_play_url(&url, &t) {
                return iced::Task::batch(follow_tasks);
            }
        }
        // Fresh cached URL: instant (back/prev/replay never re-resolve).
        if let Some((url, expires)) = self.url_cache.get(&t.id).cloned() {
            if expires > now_unix() as u64 + 60 && self.try_play_url(&url, &t) {
                return iced::Task::batch(follow_tasks);
            }
        }
        self.status = self.language.loading_track(&t.title, &t.artist);
        let audio_task = match &self.audio {
            Some(a) if a.wants_stream() => Self::resolve_url_task(t.id),
            _ => Self::resolve_task(t.id),
        };
        follow_tasks.push(audio_task);
        iced::Task::batch(follow_tasks)
    }

    /// Startup autoload: buffer the restored track paused at the saved
    /// position so the first play/seek is instant (resolving takes ~1s).
    /// Unlike [`Self::play_current_task`] the display position is
    /// preserved (never flashes 00:00), the landing pauses instead
    /// of playing (see `autoload`: explicit transport clears it), and
    /// nothing is logged (buffering is not a play; the track is
    /// already the history tail from last session).
    pub(crate) fn autoload_current_task(&mut self) -> iced::Task<Message> {
        let Some(t) = self.queue.current().cloned() else {
            return iced::Task::none();
        };
        if let Some(audio) = &self.audio {
            audio.stop();
        }
        self.prefetch_attempted = None;
        self.resolve_attempts = 0;
        self.heard_audio = false;
        self.stream_opened_at = None;
        self.loading_track = Some(t.id.clone());
        self.end_advanced = false;
        self.paused = true;
        self.paused_seek = false;
        self.seek_hold_until = None;
        // Display stays at the saved position through the load (Tick
        // preserves it while loading); the backend seeks there on
        // landing via `pending_resume`.
        self.pending_resume = (self.position_secs > 5.0).then_some(self.position_secs);
        self.pending_seek = None;
        self.restored_pending = false;
        self.autoload = true;
        self.seek_drag = None;
        self.seek_drag_at = None;
        self.seek_paused = false;
        let mut follow_tasks = self.track_start_tasks(&t);
        self.status = self.language.loading_track(&t.title, &t.artist);
        let audio_task = match &self.audio {
            Some(a) if a.wants_stream() => Self::resolve_url_task(t.id),
            _ => Self::resolve_task(t.id),
        };
        follow_tasks.push(audio_task);
        iced::Task::batch(follow_tasks)
    }

    /// Resolve failure for the loading track: retry the same load up
    /// to [`MAX_RESOLVE_ATTEMPTS`] (transient yt-dlp/network flakes),
    /// then skip forward when a next song exists, else report honestly
    /// and hold. Never silently stuck, never an error loop (landing
    /// back on the same id, e.g. repeat loops, stops instead).
    pub(crate) fn retry_or_skip_failed(
        &mut self,
        failed_id: String,
        err: String,
    ) -> iced::Task<Message> {
        self.loading_track = None;
        self.pending_resume = None;
        // A queued seek survives retries of the same track (it is still
        // the user's target); skips clear it via `play_current_task`,
        // and the end-of-queue give-up below clears it outright.
        self.resolve_attempts += 1;
        if self.resolve_attempts < MAX_RESOLVE_ATTEMPTS {
            self.loading_track = Some(failed_id.clone());
            yutu_core::app_log::warn(
                "yutu-play",
                format_args!(
                    "resolve failed for {failed_id} ({err}); retrying ({}/{MAX_RESOLVE_ATTEMPTS})",
                    self.resolve_attempts
                ),
            );
            self.status = self
                .language
                .stream_retrying(self.resolve_attempts, MAX_RESOLVE_ATTEMPTS);
            self.publish_mpris();
            // Same backend choice as a fresh load (the queue never
            // moved, so no other state resets).
            return match &self.audio {
                Some(a) if a.wants_stream() => Self::resolve_url_task(failed_id),
                _ => Self::resolve_task(failed_id),
            };
        }
        let title = self
            .find_track(&failed_id)
            .map(|t| t.title.clone())
            .unwrap_or(failed_id.clone());
        self.log_error(format!(
            "Giving up on {failed_id} after {MAX_RESOLVE_ATTEMPTS} resolve attempts: {err}"
        ));
        if self.advance_queue() && !self.queue.current().is_some_and(|t| t.id == failed_id) {
            self.status = self.language.stream_skipped(&title, MAX_RESOLVE_ATTEMPTS);
            self.publish_mpris();
            // A startup autoload that skips stays paused (the user never
            // pressed play); play_current_task clears the flag, so carry
            // it over the track change.
            let autoload = self.autoload;
            let task = self.play_current_task();
            self.autoload = autoload;
            return task;
        }
        // Same song (repeat loop) or end of line: stop loudly instead
        // of cycling, and pin the end flag so Tick never retriggers.
        // (The give-up error above is the log entry for this stop too:
        // one entry per dead track, never per retry.)
        self.end_advanced = true;
        self.pending_seek = None;
        self.autoload = false;
        self.status = Self::failure_status(self.language.fail_playback_tail(&title), &err);
        self.publish_mpris();
        iced::Task::none()
    }

    /// Best-effort instant play of a stream URL. True on success.
    /// Failures stay quiet here (status only): the bytes fallback
    /// usually recovers, and the give-up path logs if nothing does.
    pub(crate) fn try_play_url(&mut self, url: &str, t: &Track) -> bool {
        match &self.audio {
            Some(audio) => match audio.play_stream_url(url) {
                Ok(()) => {
                    self.loading_track = None;
                    self.stream_opened_at = Some((std::time::Instant::now(), t.id.clone()));
                    self.status = self.language.playing_line(&t.title, &t.artist);
                    let held = self.seek_drag.is_some();
                    self.consume_deferred_seeks();
                    // A resolve landing mid-drag stays silent until
                    // release (the bar is held, not the backend).
                    if held {
                        if let Some(audio) = &self.audio {
                            audio.pause();
                        }
                    }
                    // A startup autoload buffers paused: silence the
                    // backend right after the load (the user never
                    // pressed play).
                    if self.autoload {
                        self.autoload = false;
                        if let Some(audio) = &self.audio {
                            audio.pause();
                        }
                        self.paused = true;
                        self.status = self.language.paused_line(&t.title, &t.artist);
                    }
                    true
                }
                Err(e) => {
                    self.pending_resume = None;
                    self.status = Self::failure_status(self.language.fail_playback(&t.title), &e);
                    false
                }
            },
            None => {
                self.pending_resume = None;
                self.pending_seek = None;
                self.status = self.language.stream_no_device();
                false
            }
        }
    }

    /// Flip shuffle. Turning on snapshots the order and physically shuffles
    /// the whole queue with the current track first (index 0); turning off
    /// restores the snapshot. Playback never skips: reorder retargets the
    /// current index by track id. Drags made while shuffled are absorbed by
    /// the id-restore (effectively undone on toggle-off).
    pub(crate) fn set_shuffle(&mut self, on: bool) {
        if self.queue.shuffle == on {
            return;
        }
        self.queue.shuffle = on;
        if !on {
            self.restore_shuffle();
            return;
        }
        self.pre_shuffle = Some(self.queue.items().iter().map(|t| t.id.clone()).collect());
        self.pre_shuffle_cur = self.queue.position().and_then(|cur| {
            self.queue.current().map(|t| {
                let rank = self.queue.items()[..cur]
                    .iter()
                    .filter(|x| x.id == t.id)
                    .count();
                (t.id.clone(), rank)
            })
        });
        let len = self.queue.len();
        if len <= 1 {
            return;
        }
        // Fisher-Yates over everything but current (xorshift, nanos seed);
        // the current track leads the new order at index 0.
        let mut seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as u64)
            .unwrap_or(0x2545F491)
            ^ 0x9E3779B97F4A7C15;
        if seed == 0 {
            seed = 0x2545F491;
        }
        match self.queue.position() {
            None => {
                // Nothing playing yet: full shuffle, index stays None.
                let mut order: Vec<usize> = (0..len).collect();
                for i in (1..order.len()).rev() {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    order.swap(i, (seed as usize) % (i + 1));
                }
                let _ = self.queue.reorder(&order);
            }
            Some(cur) => {
                let cur = cur.min(len - 1);
                let mut others: Vec<usize> = (0..len).filter(|&i| i != cur).collect();
                for i in (1..others.len()).rev() {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    others.swap(i, (seed as usize) % (i + 1));
                }
                let mut order = vec![cur];
                order.extend(others);
                let _ = self.queue.reorder(&order);
            }
        }
    }

    /// Fisher-Yates permutation of 0..len (xorshift, nanos seed).
    pub(crate) fn shuffled_order(len: usize) -> Vec<usize> {
        let mut seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as u64)
            .unwrap_or(0x2545F491)
            ^ 0x9E3779B97F4A7C15;
        if seed == 0 {
            seed = 0x2545F491;
        }
        let mut order: Vec<usize> = (0..len).collect();
        for i in (1..len).rev() {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            order.swap(i, (seed as usize) % (i + 1));
        }
        order
    }

    /// Fresh-context play with optional full shuffle: replaces the
    /// queue, snapshots the incoming order for toggle-off, shuffles the
    /// source list first (so the queued order - and its first fetch -
    /// is already shuffled, random head included), then queues. Always
    /// reshuffles, even when shuffle mode was already on.
    pub(crate) fn play_fresh(
        &mut self,
        tracks: Vec<Track>,
        label: &str,
        shuffle: bool,
    ) -> iced::Task<Message> {
        self.queue.clear();
        self.autoplay.clear();
        self.autoplay_for = None;
        self.drag = None;
        let mut tracks = tracks;
        if shuffle {
            self.pre_shuffle = Some(tracks.iter().map(|t| t.id.clone()).collect());
            let order = Self::shuffled_order(tracks.len());
            let mut shuffled = Vec::with_capacity(tracks.len());
            for i in order {
                shuffled.push(tracks[i].clone());
            }
            tracks = shuffled;
        }
        let task = self.queue_many(tracks, label);
        if shuffle {
            // First-occurrence seating if duplicates exist (see restore).
            self.pre_shuffle_cur = self.queue.current().map(|t| (t.id.clone(), 0));
            self.queue.shuffle = true;
        }
        self.publish_mpris();
        task
    }

    /// Restore the pre-shuffle order. Ids added while shuffled keep their
    /// relative order at the end; removed ids vanish with their rows.
    /// Duplicate ids are matched occurrence-by-occurrence (used-index set)
    /// so the result is always a valid permutation. The current track
    /// keeps playing: an unchanged current re-seats its snapshot
    /// occurrence, a moved current re-seats its restore-time occurrence
    /// (`reorder` alone retargets duplicates by first match). Pure index
    /// moves: playback never restarts.
    pub(crate) fn restore_shuffle(&mut self) {
        let Some(snap) = self.pre_shuffle.take() else {
            return;
        };
        let snap_cur = self.pre_shuffle_cur.take();
        let cur_fix: Option<(String, usize)> = self.queue.position().and_then(|cur| {
            self.queue.current().map(|t| {
                let rank = self.queue.items()[..cur]
                    .iter()
                    .filter(|x| x.id == t.id)
                    .count();
                // Unchanged since snapshot: the snapshot occurrence is
                // the track the user was on, not this first match.
                if snap_cur.as_ref().is_some_and(|(id, _)| id == &t.id) {
                    snap_cur.clone().expect("checked")
                } else {
                    (t.id.clone(), rank)
                }
            })
        });
        let items = self.queue.items();
        let mut used = vec![false; items.len()];
        let mut order: Vec<usize> = Vec::with_capacity(items.len());
        for id in &snap {
            if let Some(i) = items
                .iter()
                .enumerate()
                .filter(|(i, _)| !used[*i])
                .find(|(_, t)| &t.id == id)
                .map(|(i, _)| i)
            {
                used[i] = true;
                order.push(i);
            }
        }
        for (i, _) in items.iter().enumerate() {
            if !used[i] {
                order.push(i);
            }
        }
        let _ = self.queue.reorder(&order);
        // Re-seat the exact occurrence (see doc comment above).
        if let Some((id, rank)) = cur_fix {
            let mut seen = 0;
            for (i, t) in self.queue.items().iter().enumerate() {
                if t.id == id {
                    if seen == rank {
                        self.queue.jump_to(i);
                        break;
                    }
                    seen += 1;
                }
            }
        }
    }

    /// Next-song prefetch due check (pure for tests): the last 2s of
    /// a known-length song with nothing loading and nothing warmed.
    /// Fires once per track (see `prefetch_attempted`); failures stay
    /// silent until the advance, which retries with its own budget.
    pub(crate) fn prefetch_due(&self, position_secs: f64, duration_secs: f64) -> Option<String> {
        if self.paused
            || !duration_secs.is_finite()
            || duration_secs <= 2.0
            || self.prefetched.is_some()
            || self.loading_track.is_some()
            || position_secs < duration_secs - 2.0
        {
            return None;
        }
        self.peek_next_id()
    }

    /// Open-failure watchdog predicate (pure, Tick-tested): a stream
    /// URL handed to mpv at least [`STREAM_OPEN_GRACE`] ago, with the
    /// backend idle (no file) and nothing ever heard. The EOF net's
    /// mirror image: it needs heard audio, this needs none. Paused,
    /// loading, and drag-held states never qualify (a paused autoload
    /// that failed is caught on resume); rodio never reports idle.
    pub(crate) fn stream_open_failed(&self, now: std::time::Instant) -> bool {
        !self.paused
            && !self.heard_audio
            && self.loading_track.is_none()
            && self.seek_drag.is_none()
            && self.stream_opened_at.as_ref().is_some_and(|(at, id)| {
                now.saturating_duration_since(*at) >= STREAM_OPEN_GRACE
                    && self.queue.current().is_some_and(|t| &t.id == id)
            })
            && self
                .audio
                .as_ref()
                .is_some_and(|a| a.wants_stream() && a.is_idle())
    }

    /// EOF safety net predicate (pure, Tick-tested): true when the raw
    /// backend clock reads idle (0/0) after audible playback of a live
    /// load, or when the backend itself reports holding no file (mpv
    /// idle: the duration property can outlive the file, so 0/0 alone
    /// can miss the end). Paused, already-advanced, loading,
    /// never-heard, and empty states never qualify, so gaps, failures,
    /// and Stop stay put.
    pub(crate) fn eof_idle(&self, raw_pos: f64, raw_dur: f64) -> bool {
        !self.paused
            && !self.end_advanced
            && self.loading_track.is_none()
            && self.heard_audio
            && self.queue.current().is_some()
            && ((raw_pos <= 0.0 && raw_dur <= 1.0)
                || self.audio.as_ref().is_some_and(|a| a.is_idle()))
    }

    /// Id of the likely next track (repeat-all wraps), for prefetching.
    /// Repeat One needs none: the same song replays from the cache.
    pub(crate) fn peek_next_id(&self) -> Option<String> {
        if self.queue.repeat == yutu_player::RepeatMode::One {
            return None;
        }
        let items = self.queue.items();
        if items.is_empty() {
            return self.autoplay.first().map(|t| t.id.clone());
        }
        let pos = self.queue.position().unwrap_or(0);
        if pos + 1 < items.len() {
            Some(items[pos + 1].id.clone())
        } else if self.queue.repeat == yutu_player::RepeatMode::All {
            items.first().map(|t| t.id.clone())
        } else {
            self.autoplay.first().map(|t| t.id.clone())
        }
    }

    /// Same fetch without touching the status line (bulk actions keep theirs).
    /// Lyrics + hero art warming for a track start (every advance path
    /// funnels through play/fetch_current_task, so both call this).
    pub(crate) fn track_start_tasks(&mut self, t: &Track) -> Vec<iced::Task<Message>> {
        let mut tasks = Vec::new();
        // New song with the drawer open: back to the top (hero)
        // instead of keeping the old song's scroll depth; the follow
        // run re-arms from scratch (same rule as opening the drawer)
        // and centers the verse once playback passes its timestamp.
        // The 0.0 echo matches the ring, so no takeover fires.
        if self.drawer_open {
            self.drawer_scroll = 0.0;
            self.lyrics_follow = true;
            self.lyrics_follow_at = None;
            self.lyrics_cmds = [0.0, f32::NAN, f32::NAN];
            tasks.push(Self::lyrics_scroll_task(0.0));
        }
        if let Some(art) = self.large_art_task(t) {
            tasks.push(art);
        }
        // Warm rows ahead (current + next 5, thumbs): art lands without
        // waiting for another payload, and failures retry since
        // ArtworkReady clears the loading flag on error.
        if let Some(pos) = self.queue.position() {
            let ahead: Vec<Track> = self
                .queue
                .items()
                .iter()
                .skip(pos)
                .take(6)
                .cloned()
                .collect();
            tasks.extend(self.artwork_tasks(&ahead));
        }
        #[cfg(any(feature = "lyrics", feature = "innertube"))]
        {
            tasks.extend(self.start_lyrics_fetch(t.clone()));
        }
        tasks
    }

    pub(crate) fn fetch_current_task(&mut self) -> iced::Task<Message> {
        match self.queue.current().cloned() {
            None => iced::Task::none(),
            Some(t) => {
                // Same logging contract as `play_current_task`: every
                // audible play lands in Recent with durable metadata.
                self.log_play(&t);
                // Same cut-the-old-song feedback as the streaming path.
                if let Some(audio) = &self.audio {
                    audio.stop();
                }
                self.resolve_attempts = 0;
                self.heard_audio = false;
                self.stream_opened_at = None;
                self.prefetch_attempted = None;
                self.loading_track = Some(t.id.clone());
                self.end_advanced = false;
                self.paused = false;
                self.paused_seek = false;
                self.seek_hold_until = None;
                self.position_secs = 0.0;
                // New load cancels any pending resume seek (see above).
                // A queued seek from the old track dies here too.
                self.pending_resume = None;
                self.pending_seek = None;
                self.autoload = false;
                self.restored_pending = false;
                // ...and any held seek drag (preview belongs to the old
                // track); release afterwards is a no-op.
                self.seek_drag = None;
                self.seek_drag_at = None;
                self.seek_paused = false;
                let mut tasks = self.track_start_tasks(&t);
                tasks.push(Self::resolve_task(t.id));
                iced::Task::batch(tasks)
            }
        }
    }

    /// Manual queue first (shuffle/repeat apply there); when it truly ends
    /// and repeat is Off, the autoplay tail shifts in head-first so the
    /// single current-pointer (progress/lyrics/MPRIS) keeps working.
    /// True when current changed.
    pub(crate) fn advance_queue(&mut self) -> bool {
        self.advance_inner(false)
    }

    /// Manual Next path: Repeat One never replays here, only at the
    /// natural end of a song.
    pub(crate) fn advance_queue_manual(&mut self) -> bool {
        self.advance_inner(true)
    }

    /// Back button (bar, key, widget): past the first seconds the song
    /// restarts from 00:00 (instant seek, no reload); inside them the
    /// previous song plays. At the head with no previous song, restart
    /// in place instead of reloading (same sound, no network round
    /// trip). The clock read is the displayed position, matching what
    /// the progress bar shows.
    pub(crate) fn prev_task(&mut self) -> iced::Task<Message> {
        if self.position_secs > PREV_RESTART_SECS {
            self.seek_to_secs(0.0);
            self.publish_mpris();
            return iced::Task::none();
        }
        let before = self.queue.position();
        if self.queue.prev_track().is_some() {
            if self.queue.position() == before {
                self.seek_to_secs(0.0);
                self.publish_mpris();
                return iced::Task::none();
            }
            self.apply_volume();
            self.publish_mpris();
            return self.play_current_task();
        }
        iced::Task::none()
    }

    pub(crate) fn advance_inner(&mut self, manual: bool) -> bool {
        // Shuffle is a physical order (see set_shuffle), so the walk is
        // always linear: no repeats, a real end, then autoplay.
        let advanced = if manual {
            self.queue.next_manual().is_some()
        } else {
            self.queue.next_track().is_some()
        };
        if advanced {
            return true;
        }
        // Repeat All/One loop the manual queue by design (like
        // Spotify's repeat); only Off reaches autoplay.
        if self.queue.repeat != yutu_player::RepeatMode::Off {
            return false;
        }
        if self.autoplay.is_empty() {
            return false;
        }
        let t = self.autoplay.remove(0);
        self.queue.push(t);
        self.queue.jump_to(self.queue.len().saturating_sub(1));
        true
    }
}
