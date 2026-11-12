//! yutu-ui persist: library persistence, likes, lyrics cache and history (split from lib.rs, no behavior change).

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
    /// Current playback session as a snapshot (queue ids in order plus
    /// index, position, shuffle/repeat/volume/mute).
    pub(crate) fn queue_snapshot(&self) -> QueueSnapshot {
        QueueSnapshot {
            ids: self.queue.items().iter().map(|t| t.id.clone()).collect(),
            index: self.queue.position(),
            position_secs: self.position_secs,
            shuffle: self.queue.shuffle,
            repeat: self.language.repeat_mode(self.queue.repeat),
            volume: self.volume,
            muted: self.muted,
        }
    }

    /// Write the session snapshot (or clear it when the queue is empty).
    /// Ids only, one shot: restarts resolve rows via the track cache.
    /// Reports success so the relaunch path can cancel on a dead store
    /// instead of exiting into a lost session.
    pub(crate) fn persist_queue(&mut self) -> bool {
        let Ok(store) = LibraryStore::open(&self.library_path) else {
            return false;
        };
        if self.queue.is_empty() {
            let ok = store.clear_queue().is_ok();
            self.queue_saved = None;
            ok
        } else {
            let snap = self.queue_snapshot();
            if store.save_queue(&snap).is_ok() {
                self.queue_saved = Some(snap);
                self.queue_saved_at = Some(std::time::Instant::now());
                true
            } else {
                false
            }
        }
    }

    /// Tick saver: queue identity changes (ids, index, shuffle, repeat,
    /// volume, mute) persist on the next tick; the playing position refreshes
    /// at most every 30s so Ticks stay write-free while paused.
    pub(crate) fn maybe_persist_queue(&mut self, now: std::time::Instant) {
        let snap = self.queue_snapshot();
        let identity_changed = self.queue_saved.as_ref().is_none_or(|s| {
            s.ids != snap.ids
                || s.index != snap.index
                || s.shuffle != snap.shuffle
                || s.repeat != snap.repeat
                || s.muted != snap.muted
                || (s.volume - snap.volume).abs() > f32::EPSILON
        });
        if identity_changed {
            self.persist_queue();
            return;
        }
        let position_stale = (self
            .queue_saved
            .as_ref()
            .map(|s| s.position_secs)
            .unwrap_or(0.0)
            - snap.position_secs)
            .abs()
            > 5.0;
        let due = self
            .queue_saved_at
            .is_none_or(|at| now.duration_since(at).as_secs() >= 30);
        if !self.queue.is_empty() && !self.paused && position_stale && due {
            self.persist_queue();
        }
    }

    /// Rebuild the queue from the persisted snapshot (unknown ids drop:
    /// the track was never cached). Starts paused at the saved position
    /// with repeat/volume/mute restored. Shuffle always starts off (the
    /// saved order is kept as-is): a fresh launch never opens shuffled.
    pub(crate) fn restore_queue(&mut self) {
        let Ok(store) = LibraryStore::open(&self.library_path) else {
            return;
        };
        let snap = match store.load_queue() {
            Ok(Some(snap)) => snap,
            _ => return,
        };
        let items: Vec<Track> = snap
            .ids
            .iter()
            .filter_map(|id| self.find_track(id))
            .collect();
        if items.is_empty() {
            return;
        }
        let dropped = snap.ids.len() - items.len();
        self.queue.restore(items, snap.index);
        self.queue.shuffle = false;
        self.queue.repeat = Self::repeat_parse(&snap.repeat);
        self.volume = snap.volume;
        self.muted = snap.muted;
        self.apply_volume();
        self.position_secs = snap.position_secs;
        self.paused = true;
        self.queue_saved = Some(snap);
        // Mirror the forced shuffle-off so the first Tick does not see
        // a phantom identity change (one redundant rewrite per launch).
        if let Some(saved) = self.queue_saved.as_mut() {
            saved.shuffle = false;
        }
        self.queue_saved_at = Some(std::time::Instant::now());
        // The next play loads the saved track (never resumes an empty
        // backend); any other load clears the flag instead.
        self.restored_pending = true;
        self.status = if dropped == 0 {
            format!("Restored queue ({} songs).", self.queue.len())
        } else {
            format!(
                "Restored queue ({} songs, {dropped} not cached).",
                self.queue.len()
            )
        };
    }

    pub(crate) fn persist_like(&self, id: &str, liked: bool) {
        let Ok(store) = LibraryStore::open(&self.library_path) else {
            return;
        };
        if liked {
            // Cache the full row when we know it: Favorites then survives
            // restarts with title/art, not just a bare id that needs a
            // search to become visible again.
            if let Some(t) = self.catalog.get(id) {
                let _ = store.like_track(t);
            } else {
                let _ = store.like(id);
            }
        } else {
            let _ = store.unlike(id);
        }
    }

    /// Like with an explicit track (callers that already hold the full
    /// row): guarantees the metadata cache is warmed even if the catalog
    /// has not registered it yet.
    pub(crate) fn persist_like_track(&self, track: &Track, liked: bool) {
        let Ok(store) = LibraryStore::open(&self.library_path) else {
            return;
        };
        let _ = if liked {
            store.like_track(track)
        } else {
            store.unlike(&track.id)
        };
    }

    pub(crate) fn persist_dislike(&self, id: &str, disliked: bool) {
        let Ok(store) = LibraryStore::open(&self.library_path) else {
            return;
        };
        if disliked {
            let _ = store.dislike(id);
        } else {
            let _ = store.undislike(id);
        }
    }

    pub(crate) fn persist_play(&self, id: &str) {
        let Ok(store) = LibraryStore::open(&self.library_path) else {
            return;
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let _ = store.record_play(id, now);
    }

    pub(crate) fn persist_playlist_created(&self, id: &str, title: &str) {
        let Ok(store) = LibraryStore::open(&self.library_path) else {
            return;
        };
        let _ = store.create_playlist(id, title);
    }

    pub(crate) fn persist_playlist_deleted(&self, id: &str) {
        let Ok(store) = LibraryStore::open(&self.library_path) else {
            return;
        };
        let _ = store.delete_playlist(id);
    }

    pub(crate) fn persist_playlist_renamed(&self, id: &str, title: &str) {
        let Ok(store) = LibraryStore::open(&self.library_path) else {
            return;
        };
        let _ = store.rename_playlist(id, title);
    }

    pub(crate) fn persist_playlist_added(&self, playlist_id: &str, track: &Track) {
        let Ok(store) = LibraryStore::open(&self.library_path) else {
            return;
        };
        let _ = store.add_to_playlist(playlist_id, track);
        // Keep the metadata cache in step so the same song in Favorites
        // or history still renders if its playlist is later deleted.
        let _ = store.upsert_track(track);
    }

    pub(crate) fn persist_playlist_removed(&self, playlist_id: &str, track_id: &str) {
        let Ok(store) = LibraryStore::open(&self.library_path) else {
            return;
        };
        let _ = store.remove_from_playlist(playlist_id, track_id);
    }

    /// Persist a manual reorder (positions only, `added_at` untouched).
    pub(crate) fn persist_playlist_reordered(&self, playlist_id: &str, ordered_ids: &[String]) {
        let Ok(store) = LibraryStore::open(&self.library_path) else {
            return;
        };
        let _ = store.reorder_playlist(playlist_id, ordered_ids);
    }

    /// Refresh the cached insertion times for one playlist after an
    /// add/remove/sync (single-row query, no full rescan).
    pub(crate) fn refresh_playlist_added(&mut self, playlist_id: &str) {
        if let Ok(store) = LibraryStore::open(&self.library_path) {
            if let Ok(map) = store.playlist_added_map(playlist_id) {
                self.playlist_added.insert(playlist_id.to_string(), map);
            }
        }
    }

    /// Cache fetched lyrics, capped: entries are KBs of text, but a
    /// very long session must not pin every verse ever viewed. A dropped
    /// half refetches on demand, never in a loop (plays fetch once per
    /// need behind the `lyrics_loading` guard). The playing track and
    /// the open lyrics page are never evicted (dropping what is on
    /// screen would blank it until the next navigation).
    pub(crate) fn cache_lyrics(
        &mut self,
        id: String,
        lyrics: yutu_core::LyricsResult,
        provider: LyricsProvider,
    ) {
        if self.lyrics_cache.len() >= LYRICS_CACHE_CAP {
            let protect_playing = self.queue.current().map(|t| t.id.clone());
            let protect_page = match &self.route {
                Route::Lyrics(page) => Some(page.clone()),
                _ => None,
            };
            let drop: Vec<(String, LyricsProvider)> = self
                .lyrics_cache
                .keys()
                .filter(|(track_id, _)| {
                    Some(track_id.as_str()) != protect_playing.as_deref()
                        && Some(track_id.as_str()) != protect_page.as_deref()
                })
                .take(LYRICS_CACHE_CAP / 2)
                .cloned()
                .collect();
            for key in &drop {
                self.lyrics_cache.remove(key);
            }
        }
        self.lyrics_cache.insert((id, provider), lyrics);
    }

    /// Any lyrics fetch in flight for `id` (either provider).
    pub(crate) fn lyrics_fetch_pending(&self, id: &str) -> bool {
        self.lyrics_loading.iter().any(|(t, _)| t == id)
    }

    /// The entry to display for `id`: the default provider's result when
    /// cached; while the default fetch is still in flight, nothing (the
    /// view shows loading, never a flash of the other provider); once
    /// the default has settled without a result, any other cached
    /// provider's result as automatic fallback — unless the track was
    /// manually pinned by the stepper, which shows only the default
    /// (loading, or the empty state when it has none). `None` renders
    /// loading or empty states.
    pub(crate) fn lyrics_display_entry(&self, id: &str) -> Option<(LyricsProvider, &LyricsResult)> {
        let default_key = (id.to_string(), self.lyrics_provider);
        if let Some(cached) = self.lyrics_cache.get(&default_key) {
            return Some((self.lyrics_provider, cached));
        }
        if self.lyrics_loading.contains(&default_key) {
            return None;
        }
        if self.lyrics_manual.contains(id) {
            return None;
        }
        for provider in LyricsProvider::all() {
            if provider != self.lyrics_provider {
                if let Some(cached) = self.lyrics_cache.get(&(id.to_string(), provider)) {
                    return Some((provider, cached));
                }
            }
        }
        None
    }

    /// The provider actually on screen for `id`: the displayed entry's
    /// source when lyrics show, else the default (loading and empty
    /// states). The stepper label reads through here so it never
    /// credits the wrong provider.
    pub(crate) fn lyrics_shown_provider(&self, id: &str) -> LyricsProvider {
        self.lyrics_display_entry(id)
            .map(|(provider, _)| provider)
            .unwrap_or(self.lyrics_provider)
    }

    /// Start lyrics fetches for `t`, one parallel task per provider
    /// missing from the cache and not already in flight. Disk hits land
    /// synchronously (tagged LRCLIB, legacy read path). Returns the
    /// tasks to run (empty when everything is cached or in flight).
    #[cfg(any(feature = "lyrics", feature = "innertube"))]
    pub(crate) fn start_lyrics_fetch(&mut self, t: Track) -> Vec<iced::Task<Message>> {
        let mut tasks = Vec::new();
        #[cfg(feature = "lyrics")]
        if !self.lyrics_cache.keys().any(|(id, _)| id == &t.id) && !self.lyrics_fetch_pending(&t.id)
        {
            if let Some(cached) = load_lyrics(&t.id) {
                self.cache_lyrics(t.id.clone(), cached, LyricsProvider::Lrclib);
            }
        }
        // Default first so its arrival (and any error log) reads as the
        // primary; the other rides along for instant stepper switches.
        let order = [self.lyrics_provider, self.lyrics_provider.next()];
        for provider in order {
            let key = (t.id.clone(), provider);
            if self.lyrics_cache.contains_key(&key) || self.lyrics_loading.contains(&key) {
                continue;
            }
            self.lyrics_loading.insert(key);
            tasks.push(Self::lyrics_task(t.clone(), provider));
        }
        tasks
    }

    /// Record a play in history, capped: Home picks read the last 8,
    /// so anything past the cap is dead weight in long sessions.
    pub(crate) fn push_history(&mut self, id: String) {
        self.history.push(id);
        if self.history.len() > HISTORY_CAP {
            self.history.drain(..self.history.len() - HISTORY_CAP);
        }
    }

    /// Record a track whose load just started: in-memory history
    /// (Recent tab, Home picks), a durable play row, and a metadata
    /// upsert so the row still renders days later after restarts.
    /// Every audible play funnels through `play_current_task` /
    /// `fetch_current_task`, so logging here captures Next, Prev,
    /// jumps, auto-advances, repeats, skips and bulk plays too, not
    /// just click-to-play. Autoload buffers without playing and stays
    /// out (its track is already the history tail).
    pub(crate) fn log_play(&mut self, t: &Track) {
        self.push_history(t.id.clone());
        self.persist_play(&t.id);
        self.cache_tracks(std::slice::from_ref(t));
        let played_at_unix = now_unix();
        self.history_full.insert(
            0,
            HistoryEntry {
                track_id: t.id.clone(),
                played_at_unix,
            },
        );
        if self.history_full.len() > HISTORY_FULL_CAP {
            self.history_full.truncate(HISTORY_FULL_CAP);
        }
    }

    /// Click-to-play: queue the track, jump to it, and fetch real audio.
    /// The fetch task completes in `StreamResolved`; the tone is gone.
    pub(crate) fn enqueue(&mut self, track: Track) -> iced::Task<Message> {
        let track = self.resolve_for_play(&track);
        self.absorb_autoplay(&track.id);
        self.queue.push(track);
        self.queue.jump_to(self.queue.len().saturating_sub(1));
        self.apply_volume();
        let play = self.play_current_task();
        let just_queued = self.queue.current().cloned();
        let scrobble = just_queued
            .map(|t| self.maybe_scrobble_task(&t))
            .unwrap_or_else(iced::Task::none);
        if self.audio.is_none() {
            self.status.push_str(" (no audio device)");
            return scrobble;
        }
        iced::Task::batch([scrobble, play])
    }
}
