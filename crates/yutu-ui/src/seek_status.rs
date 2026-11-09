//! yutu-ui seek_status: seek, MPRIS, status, EQ and scrobbling (split from lib.rs, no behavior change).

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
    /// Playlist cover: first resolvable track art, else the stored
    /// row's own art, else a fixed music placeholder (same box either
    /// way, rows never shift).
    pub(crate) fn playlist_cover(&self, pl: &Playlist, size: f32) -> iced::Element<'_, Message> {
        if let Some(t) = pl.tracks.first().and_then(|t| self.find_track(&t.id)) {
            return self.thumb_sized(&t, size);
        }
        let url = pl
            .tracks
            .first()
            .and_then(|t| t.thumbnail_url.clone())
            .map(|u| Self::cover_url_for(&u, size));
        self.lazy_image(url.as_deref(), size)
    }

    /// Display size at/above which track art uses the 544px hero key
    /// instead of the row raster (song rows stay low-res). Focal
    /// sub-64 headers (home picks 52, menu headers 56) deliberately
    /// use thumb_hero anyway: they are card headers, not rows, and the
    /// hero key is already warmed, so the extra fetch pays once.
    pub(crate) const HIRES_MIN_PX: f32 = 64.0;
    /// Row art raster width: rows render at 32-58px, so a 120px server
    /// raster stays crisp on hidpi while decoding to ~57KB RGBA (a
    /// 544px hero costs ~1.2MB and upstream thumbs can be larger
    /// still). One rule for every row site, so fetch keys match
    /// display keys and the same cover is never held at two sizes.
    pub(crate) const ROW_ART_W: u32 = 120;

    /// Row art key for a raw thumbnail URL (pass-through when the URL
    /// carries no YTM size suffix).
    pub(crate) fn row_art_url(url: &str) -> String {
        yutu_core::hires_thumb_url(url, Self::ROW_ART_W)
    }

    /// Art URL for a display size: catalog merged, hero at/above the
    /// threshold, row raster below it.
    pub(crate) fn art_url_for(&self, t: &Track, display_px: f32) -> Option<String> {
        let merged = self.catalog.get(&t.id).unwrap_or(t);
        if display_px >= Self::HIRES_MIN_PX {
            Self::hero_art_url(merged)
        } else {
            merged.thumbnail_url.clone().map(|u| Self::row_art_url(&u))
        }
    }

    /// Hero art element for focal headers (context menu, playlist menu,
    /// search hero): always the 544px key.
    pub(crate) fn thumb_hero(&self, track: &Track, size: f32) -> iced::Element<'_, Message> {
        let url = self.hero_art_for(track);
        self.lazy_image(url.as_deref(), size)
    }

    /// Artwork at any square size (home cards use 52, rows use 40).
    pub(crate) fn thumb_sized(&self, track: &Track, size: f32) -> iced::Element<'_, Message> {
        // Catalog wins on art: queued/stored rows may predate the fetch.
        // At/above HIRES_MIN_PX the hero key is used (same key the
        // fetch warms), below it the row raster.
        let url = self.art_url_for(track, size);
        self.lazy_image(url.as_deref(), size)
    }

    /// Record a playback/fetch failure for the Settings error log (capped),
    /// the log file, and stderr (same event, so journald, the file, and
    /// the in-app log agree; file/stderr lines carry `[time] [LEVEL]`
    /// prefixes the panel entry omits).
    pub(crate) fn log_error(&mut self, msg: String) {
        yutu_core::app_log::error("yutu-play", &msg);
        if self.error_log.len() >= 30 {
            self.error_log.remove(0);
        }
        self.error_log.push(msg);
    }

    /// Apply a pending resume seek once audio has started (restored
    /// queue): seeks when past 5s, otherwise starts from the top.
    pub(crate) fn consume_resume(&mut self) {
        // Pending resume seek after audio starts (restored queue):
        // applied once, dropped on failure paths by the callers.
        if let Some(pos) = self.pending_resume.take() {
            if pos > 5.0 {
                self.seek_to_secs(pos);
            }
        }
    }

    /// First play after a session restore: load the saved track and arm
    /// the saved-position seek. Some(task) when it fired (callers return
    /// it); any other load clears the flag instead.
    pub(crate) fn take_restored_play(&mut self) -> Option<iced::Task<Message>> {
        if self.restored_pending && self.queue.current().is_some() {
            self.restored_pending = false;
            // An explicit seek made before this first play (queued while
            // there was no file) wins over the saved position and is
            // honored even under 5s; otherwise the saved position seeks
            // when past 5s, else the track starts from the top.
            let explicit = self.pending_seek.take();
            let resume = explicit.or((self.position_secs > 5.0).then_some(self.position_secs));
            let task = self.play_current_task();
            // play_current_task resets the display to 00:00; put the
            // seek target back immediately so the bar never flashes it
            // mid-load (Tick preserves it while loading, the landing
            // seeks the backend there).
            if let Some(target) = resume {
                self.position_secs = target;
                if explicit.is_some() {
                    self.pending_seek = Some(target);
                } else {
                    self.pending_resume = Some(target);
                }
            }
            Some(task)
        } else {
            self.restored_pending = false;
            None
        }
    }

    /// Seek to an absolute position: backend first, manual clock +
    /// anti-flicker hold on success. On mpv the seek is never a hard
    /// error: when the backend has no file yet (resolving) or the
    /// stream has not demuxed enough to accept the seek (mpv raw -12),
    /// the position is remembered and retried until it lands instead
    /// of erroring. Rodio has no seek at all, so it still fails
    /// honestly below.
    pub(crate) fn seek_to_secs(&mut self, pos: f64) {
        let pos = pos.max(0.0);
        // Deferred seek: the resolver has not handed mpv a file yet, so
        // the seek command would fail. Queue it and show it; the Tick
        // freezes the manual position while loading, and the load paths
        // apply it via `consume_deferred_seeks`. Rodio has no seek at
        // all, so it still fails honestly below.
        if self.loading_track.is_some() && self.audio.as_ref().is_some_and(|a| a.wants_stream()) {
            self.pending_seek = Some(pos);
            self.position_secs = pos;
            self.lyrics_follow = true;
            return;
        }
        match &self.audio {
            None => {
                self.status = self.language.no_audio_device();
            }
            Some(audio) => match audio.seek_secs(pos) {
                Ok(()) => {
                    self.pending_seek = None;
                    self.position_secs = pos;
                    // A landed seek re-engages lyrics follow (slider,
                    // verse tap, widget, media keys): the glide lands
                    // on the new verse. Failed seeks leave follow alone.
                    self.lyrics_follow = true;
                    // Hold the manual clock ~1.2s: the backend still
                    // reports the pre-seek position for a tick or two.
                    self.seek_hold_until =
                        Some(std::time::Instant::now() + std::time::Duration::from_millis(1200));
                    // While paused the backend may never catch up, so the
                    // hold lasts until resume (kills the paused flicker).
                    self.paused_seek = self.paused;
                }
                Err(e) => {
                    // mpv reports MPV_ERROR_COMMAND (raw -12) when there
                    // is no file yet or the network stream has not
                    // demuxed enough to seek: queue and retry instead of
                    // erroring, so scrubbing a fresh song just works.
                    if self.audio.as_ref().is_some_and(|a| a.wants_stream()) {
                        self.pending_seek = Some(pos);
                        self.position_secs = pos;
                        self.lyrics_follow = true;
                        return;
                    }
                    let msg = format!("Seek failed ({e}).");
                    self.log_error(msg.clone());
                    self.status = msg;
                }
            },
        }
    }

    /// Apply deferred seeks once audio has started: an explicit queued
    /// seek wins over a restored-queue resume (the resume is dropped
    /// without an extra backend seek).
    pub(crate) fn consume_deferred_seeks(&mut self) {
        if self.pending_seek.is_some() {
            self.pending_resume = None;
            if let Some(pos) = self.pending_seek.take() {
                self.seek_to_secs(pos);
            }
        } else {
            self.consume_resume();
        }
    }

    /// Retry a queued mpv seek once the backend can accept it. Called
    /// from Tick (loading is None and no drag is held there): on
    /// success the queue clears with the usual hold; while the backend
    /// is still not ready (idle, no duration yet) the queue is kept
    /// silently; only a failure against a ready backend surfaces as
    /// an honest error.
    pub(crate) fn flush_pending_seek(&mut self) {
        let Some(pos) = self.pending_seek else {
            return;
        };
        let Some(audio) = self.audio.as_ref() else {
            self.pending_seek = None;
            return;
        };
        if !audio.wants_stream() {
            self.pending_seek = None;
            return;
        }
        match audio.seek_secs(pos) {
            Ok(()) => {
                self.pending_seek = None;
                self.position_secs = pos;
                self.lyrics_follow = true;
                self.seek_hold_until =
                    Some(std::time::Instant::now() + std::time::Duration::from_millis(1200));
                self.paused_seek = self.paused;
            }
            Err(e) => {
                // Not ready yet (no file, idle, no duration): keep the
                // queued target and the manual display, retry next Tick.
                let ready = !audio.is_idle() && audio.duration_secs(0.0) > 1.0;
                if ready {
                    self.pending_seek = None;
                    let msg = format!("Seek failed ({e}).");
                    self.log_error(msg.clone());
                    self.status = msg;
                } else {
                    self.position_secs = pos;
                    self.lyrics_follow = true;
                }
            }
        }
    }

    /// Seek-bar drag preview: display only (no backend seek, so the
    /// advance cannot fire mid-drag). The first preview pauses live
    /// playback; release resumes it only in that case.
    pub(crate) fn preview_seek(&mut self, pos: f32) {
        let pos = pos.max(0.0);
        if self.seek_drag.is_none() && !self.paused {
            // Device-gated: with no audio backend there is nothing to
            // pause, so no resume debt either (display previews anyway).
            if let Some(audio) = &self.audio {
                audio.pause();
                self.paused = true;
                self.seek_paused = true;
            }
        }
        self.seek_drag = Some(pos);
        self.seek_drag_at = Some(std::time::Instant::now());
        self.position_secs = f64::from(pos);
    }

    /// Seek-bar release: commit the preview to the backend, then resume
    /// when the drag itself paused playback (a pre-paused song stays
    /// paused, mirroring `TogglePause`).
    pub(crate) fn release_seek(&mut self) {
        let Some(preview) = self.seek_drag.take() else {
            return;
        };
        self.seek_drag_at = None;
        self.seek_to_secs(f64::from(preview));
        if self.seek_paused {
            self.seek_paused = false;
            if let Some(audio) = &self.audio {
                audio.resume();
            }
            self.paused = false;
            self.paused_seek = false;
        }
    }

    /// Position intent (keys, taps, desktop): a held drag turns it into
    /// preview so backend and display never diverge; otherwise it
    /// commits at once like before.
    pub(crate) fn seek_or_preview(&mut self, pos: f64) {
        if self.seek_drag.is_some() {
            self.preview_seek(pos as f32);
        } else {
            self.seek_to_secs(pos);
        }
    }

    /// Apply one desktop transport command (MPRIS widget / media keys).
    /// Next/Previous return their real play tasks (dropping them would
    /// move the pointer without ever fetching audio).
    pub(crate) fn apply_player_command(
        &mut self,
        cmd: yutu_desktop::PlayerCommand,
    ) -> iced::Task<Message> {
        use yutu_desktop::PlayerCommand as C;
        match cmd {
            C::Play => {
                // Explicit transport takes over any startup autoload.
                self.autoload = false;
                if let Some(task) = self.take_restored_play() {
                    return task;
                }
                if self.paused {
                    if let Some(a) = &self.audio {
                        a.resume();
                    }
                    self.paused = false;
                    self.paused_seek = false;
                    self.status = self.language.desktop_resumed();
                }
            }
            C::Pause => {
                if let Some(a) = &self.audio {
                    a.pause();
                }
                self.paused = true;
                // Explicit headset pause wins over the drag: release
                // must not resume behind the user's back.
                if self.seek_drag.is_some() {
                    self.seek_paused = false;
                }
                self.status = self.language.desktop_paused();
            }
            C::PlayPause => {
                // Explicit transport takes over any startup autoload.
                self.autoload = false;
                if let Some(task) = self.take_restored_play() {
                    return task;
                }
                if self.paused {
                    if let Some(a) = &self.audio {
                        a.resume();
                    }
                    self.paused = false;
                    self.paused_seek = false;
                    self.status = self.language.desktop_resumed();
                } else {
                    if let Some(a) = &self.audio {
                        a.pause();
                    }
                    self.paused = true;
                    self.status = self.language.desktop_paused();
                }
            }
            C::Stop => {
                if let Some(a) = &self.audio {
                    a.stop();
                }
                // Stopped is not finished (same disarm as the UI Stop:
                // a later resume on the empty backend must not skip).
                self.heard_audio = false;
                self.stream_opened_at = None;
                self.autoload = false;
                self.paused = true;
                self.status = self.language.desktop_stopped();
            }
            C::Next => {
                if self.advance_queue_manual() {
                    self.apply_volume();
                    return self.play_current_task();
                }
            }
            C::Previous => {
                return self.prev_task();
            }
            C::Seek(offset) => {
                self.seek_or_preview(self.position_secs + offset);
            }
            C::SetPosition(pos) => {
                self.seek_or_preview(pos);
            }
            C::SetVolume(v) => {
                self.volume = (v as f32).clamp(0.0, 1.0);
                self.muted = false;
                self.apply_volume();
            }
            C::SetShuffle(shuffle) => {
                if self.queue.shuffle != shuffle {
                    self.set_shuffle(shuffle);
                    self.status = if shuffle {
                        "Shuffle on (desktop).".into()
                    } else {
                        "Shuffle off (desktop).".into()
                    };
                }
            }
            C::SetLoop(loop_status) => {
                let mode = match loop_status {
                    yutu_desktop::LoopStatus::None => yutu_player::RepeatMode::Off,
                    yutu_desktop::LoopStatus::Track => yutu_player::RepeatMode::One,
                    yutu_desktop::LoopStatus::Playlist => yutu_player::RepeatMode::All,
                };
                self.queue.repeat = mode;
                self.status = self.language.desktop_repeat(self.queue.repeat);
            }
        }
        iced::Task::none()
    }

    /// Drain desktop transport commands queued since the last tick.
    pub(crate) fn drain_player_commands(&mut self) -> iced::Task<Message> {
        let cmds: Vec<yutu_desktop::PlayerCommand> = self
            .mpris
            .as_ref()
            .map(|h| {
                h.lock()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .drain_commands()
            })
            .unwrap_or_default();
        if cmds.is_empty() {
            return iced::Task::none();
        }
        let mut tasks = Vec::new();
        for cmd in cmds {
            tasks.push(self.apply_player_command(cmd));
        }
        iced::Task::batch(tasks)
    }

    /// EMA update for the FPS meter: the instant rate blended in 20% per
    /// frame, so the readout glides instead of flickering. Non-positive
    /// or non-finite intervals keep the previous value (out-of-order
    /// frames must never spike or NaN the meter).
    pub(crate) fn fps_ema(prev: f32, dt_secs: f32) -> f32 {
        if !dt_secs.is_finite() || dt_secs <= 0.0 {
            return prev;
        }
        // Sub-millisecond intervals are timer noise, not 100000 fps:
        // floor the interval so the meter tops out at a sane 1000.
        let inst = 1.0 / dt_secs.max(1.0 / 1000.0);
        if prev <= 0.0 || !prev.is_finite() {
            inst
        } else {
            prev + (inst - prev) * 0.2
        }
    }

    /// Live FPS meter (`debug-panel` feature only):
    /// smoothed frame rate plus last view-build cost, next to the
    /// status line. Builds without the flag render (and subscribe
    /// to) nothing.
    pub(crate) fn fps_text(&self) -> iced::Element<'_, Message> {
        #[cfg(not(feature = "debug-panel"))]
        {
            iced::widget::text("").size(1).into()
        }
        #[cfg(feature = "debug-panel")]
        {
            use iced::widget::text;
            if self.fps <= 0.0 {
                return text("").size(1).into();
            }
            text(format!(
                "{:.0} fps · {:.1}ms build",
                self.fps,
                self.view_build_ms.get()
            ))
            .size(11)
            .color(MUTED)
            .into()
        }
    }

    /// Bottom-bar failure text: full `label: detail` with the
    /// `debug-panel` feature, the human `label` alone without it.
    /// Every call site logs `detail` to the app log, so builds without
    /// the flag never surface provider internals (URLs, status codes,
    /// raw I/O errors) while diagnosis loses nothing.
    pub(crate) fn failure_status(label: String, detail: impl std::fmt::Display) -> String {
        #[cfg(feature = "debug-panel")]
        {
            format!("{label}: {detail}")
        }
        #[cfg(not(feature = "debug-panel"))]
        {
            drop(detail);
            label
        }
    }

    /// Release status filter: only failures surface (no action
    /// chatter, no provider internals). The marker is language-aware
    /// so both languages classify identically (see
    /// [`Language::is_failure_line`).
    pub(crate) fn status_text(&self) -> String {
        #[cfg(feature = "debug-panel")]
        {
            self.status.clone()
        }
        #[cfg(not(feature = "debug-panel"))]
        {
            if self.language.is_failure_line(&self.status) {
                self.status.clone()
            } else {
                String::new()
            }
        }
    }

    /// Status + FPS row for the player bar, or `None` when both are
    /// empty so the bar reclaims the line instead of holding a gap
    /// (release builds hide chatter and the meter, leaving nothing).
    pub(crate) fn status_row(&self) -> Option<iced::Element<'_, Message>> {
        use iced::widget::{row, text};
        let status = self.status_text();
        #[cfg(feature = "debug-panel")]
        {
            if status.is_empty() && self.fps <= 0.0 {
                return None;
            }
        }
        #[cfg(not(feature = "debug-panel"))]
        {
            if status.is_empty() {
                return None;
            }
        }
        Some(
            row![text(status).size(12).color(MUTED), self.fps_text(),]
                .spacing(6)
                .into(),
        )
    }

    /// Publish current track/status/volume to the MPRIS handle (if any) so
    /// desktop clients observe live state on property read. No-op headless.
    /// Poison-tolerant: a poisoned mutex still publishes via the inner guard.
    pub(crate) fn publish_mpris(&self) {
        let Some(handle) = self.mpris.as_ref() else {
            return;
        };
        let mut player = handle.lock().unwrap_or_else(|poison| poison.into_inner());
        if let Some(t) = self.queue.current() {
            // Crisp hero URL so external widgets get cover art (https URI
            // is valid MPRIS artUrl; no file download lifecycle needed).
            let art_url = self.hero_art_for(t).unwrap_or_default();
            player.update_track(yutu_desktop::NowPlaying {
                title: t.title.clone(),
                artist: t.artist.clone(),
                album: t.album.clone().unwrap_or_default(),
                track_id: t.id.clone(),
                length_secs: t.duration_secs,
                art_url,
            });
        }
        player.set_shuffle(self.queue.shuffle);
        player.set_loop_status(match self.queue.repeat {
            yutu_player::RepeatMode::Off => yutu_desktop::LoopStatus::None,
            yutu_player::RepeatMode::One => yutu_desktop::LoopStatus::Track,
            yutu_player::RepeatMode::All => yutu_desktop::LoopStatus::Playlist,
        });
        player.set_status(if self.queue.current().is_none() {
            yutu_desktop::PlaybackStatus::Stopped
        } else if self.paused {
            yutu_desktop::PlaybackStatus::Paused
        } else {
            yutu_desktop::PlaybackStatus::Playing
        });
        player.set_volume(f64::from(if self.muted { 0.0 } else { self.volume }));
        player.set_position(self.position_secs);
    }

    /// Validate + select an EQ preset (persisted via the Tick saver).
    /// Honest about backends: rodio has no filter stage, so selecting a
    /// preset there records the choice and says so (it takes effect on mpv).
    /// Band sliders stay in sync: presets overwrite all ten gains.
    pub(crate) fn select_eq_preset(&mut self, name: &str) {
        match EqPreset::preset_by_name(name) {
            Some(p) => match p.validate() {
                Ok(()) => {
                    self.eq_preset = p.name.clone();
                    self.eq_gains = p.bands;
                    // Discrete click: applies at once, superseding any
                    // pending drag flush (unless bypassed: the curve is
                    // kept, nothing pushes).
                    self.eq_dirty = false;
                    if self.eq_enabled {
                        self.apply_eq_curve();
                    } else {
                        self.status = self.language.eq_saved(&p.name);
                    }
                }
                Err(e) => {
                    self.status = self.language.eq_preset_invalid(&e);
                }
            },
            None => {
                self.status = self.language.eq_preset_unknown(name);
            }
        }
    }

    /// Nudge one EQ band from its slider: clamp, relabel (the matching
    /// built-in curve, or "Custom"), and push the curve live. Drags are
    /// throttled: the first nudge pushes at once, later ones inside the
    /// 600 ms cooldown set `eq_dirty` for the Tick flush (an `af` push
    /// rebuilds mpv's whole filter graph, so per-tick pushes stutter).
    pub(crate) fn set_eq_band(&mut self, index: usize, gain: f32) {
        if !gain.is_finite() {
            return;
        }
        let Some(slot) = self.eq_gains.get_mut(index) else {
            return;
        };
        *slot = ((gain * 2.0).round() / 2.0).clamp(MIN_GAIN_DB, MAX_GAIN_DB);
        self.eq_preset =
            EqPreset::builtin_name_for(&self.eq_gains).unwrap_or_else(|| "Custom".to_string());
        if !self.eq_enabled {
            self.status = self.language.eq_saved(&self.eq_preset);
            return;
        }
        let now = std::time::Instant::now();
        if self.eq_cooldown_until.is_none_or(|until| now >= until) {
            self.eq_cooldown_until = Some(now + std::time::Duration::from_millis(600));
            self.apply_eq_curve();
        } else {
            self.eq_dirty = true;
            self.status = self.language.eq_adjusting(&self.eq_preset);
        }
    }

    /// Push the current `eq_gains` to the backend under the current
    /// `eq_preset` name, with the same honest reporting as preset
    /// selection (rodio records the curve and says so).
    pub(crate) fn apply_eq_curve(&mut self) {
        let name = self.eq_preset.clone();
        let preset = EqPreset {
            name: name.clone(),
            bands: self.eq_gains,
        };
        match &self.audio {
            None => {
                self.status = self.language.eq_no_device_name(&name);
            }
            Some(audio) => match audio.apply_eq(&preset) {
                Ok(()) => {
                    self.status = self.language.eq_active(&name, audio.backend_name());
                }
                Err(e) => {
                    self.status = self.language.eq_failed(&name, &e);
                }
            },
        }
    }

    /// Bypass toggle: off pushes a flat chain (the stored curve + labels
    /// survive untouched), on re-pushes the stored curve. Either push
    /// arms the drag cooldown (back-to-back `af` rebuilds stutter).
    pub(crate) fn toggle_eq(&mut self, on: bool) {
        self.eq_enabled = on;
        self.eq_dirty = false;
        self.eq_cooldown_until =
            Some(std::time::Instant::now() + std::time::Duration::from_millis(600));
        if on {
            self.apply_eq_curve();
        } else {
            let flat = EqPreset::flat();
            match &self.audio {
                None => {
                    self.status = self.language.eq_off_no_device();
                }
                Some(audio) => match audio.apply_eq(&flat) {
                    Ok(()) => {
                        self.status = self.language.eq_off_skipped(audio.backend_name());
                    }
                    Err(e) => {
                        self.status = self.language.eq_off_failed(&e);
                    }
                },
            }
        }
        self.persist_eq();
    }

    /// Persist the EQ curve + bypass (gains JSON, enabled flag; the
    /// name re-derives at load, so it is never stored). The Tick saver
    /// calls this on change, so drags settle without write storms;
    /// toggles land at once. Saved-state updates even when the store
    /// fails (a dead store retries nothing every tick).
    pub(crate) fn persist_eq(&mut self) -> bool {
        let ok = LibraryStore::open(&self.library_path)
            .map(|store| {
                store
                    .set_pref("eq_gains", &EqPreset::gains_to_json(&self.eq_gains))
                    .is_ok()
                    && store
                        .set_pref("eq_enabled", if self.eq_enabled { "1" } else { "0" })
                        .is_ok()
            })
            .unwrap_or(false);
        self.eq_saved = Some((self.eq_preset.clone(), self.eq_gains, self.eq_enabled));
        ok
    }

    /// One-line EQ backend note for Settings (rodio/no-device only: mpv
    /// builds show nothing, so the capped layout stays clean there).
    pub(crate) fn eq_note(&self) -> String {
        match &self.audio {
            Some(a) if a.supports_eq() => String::new(),
            Some(_) => {
                "EQ needs the mpv backend: install libmpv / rebuild with --features mpv.".into()
            }
            None => "EQ: no audio device.".into(),
        }
    }

    /// Persist the default lyrics provider pref (best effort, same as EQ).
    pub(crate) fn persist_lyrics_provider(&self) {
        if let Ok(store) = LibraryStore::open(&self.library_path) {
            let _ = store.set_pref("lyrics_provider", self.lyrics_provider.as_pref());
        }
    }

    /// Persisted play history for discovery (up to 200 entries).
    pub(crate) fn discovery_history(&self) -> Vec<HistoryEntry> {
        LibraryStore::open(&self.library_path)
            .and_then(|s| s.history(200))
            .unwrap_or_default()
    }
}
