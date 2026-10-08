//! Backend-agnostic playback controller.
//!
//! Owns a [`Queue`] plus playback state, volume, and the active EQ preset
//! *name* (the actual coefficients live in [`crate::eq`]). No audio I/O
//! here, so there are no `Send`/thread issues; the future mpv backend can
//! observe this controller and drive mpv accordingly.
//!
//! Playback position: the controller keeps a wall-clock mirror of the rodio
//! approach (`play_started_at` + accumulated paused time) as the UI-facing
//! source of truth for mock/rodio playback. mpv consumers should prefer
//! `MpvBackend::position_secs()` (the real `time-pos` property) instead.

use crate::queue::Queue;
use std::time::{Duration, Instant};
use yutu_core::Track;

/// Playback transport state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlaybackState {
    #[default]
    Stopped,
    Playing,
    Paused,
}

/// Controller owning queue + state + volume + EQ preset name.
///
/// The playback clock (`play_started_at` + `paused_accum`) mirrors the
/// [`crate::backend::RodioBackend`] wall-clock: `play_started_at` is `Some`
/// while time is accumulating and `None` while paused or stopped.
#[derive(Debug)]
pub struct Player {
    queue: Queue,
    state: PlaybackState,
    volume: f32,
    eq_preset_name: String,
    play_started_at: Option<Instant>,
    paused_accum: Duration,
}

impl Default for Player {
    fn default() -> Self {
        Self {
            queue: Queue::default(),
            state: PlaybackState::Stopped,
            volume: 1.0,
            eq_preset_name: "Flat".to_string(),
            play_started_at: None,
            paused_accum: Duration::ZERO,
        }
    }
}

impl Player {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn state(&self) -> PlaybackState {
        self.state
    }

    #[must_use]
    pub fn is_playing(&self) -> bool {
        self.state == PlaybackState::Playing
    }

    #[must_use]
    pub fn is_paused(&self) -> bool {
        self.state == PlaybackState::Paused
    }

    #[must_use]
    pub fn is_stopped(&self) -> bool {
        self.state == PlaybackState::Stopped
    }

    #[must_use]
    pub fn volume(&self) -> f32 {
        self.volume
    }

    #[must_use]
    pub fn eq_preset_name(&self) -> &str {
        &self.eq_preset_name
    }

    pub fn set_eq_preset(&mut self, name: impl Into<String>) {
        self.eq_preset_name = name.into();
    }

    #[must_use]
    pub fn queue(&self) -> &Queue {
        &self.queue
    }

    pub fn queue_mut(&mut self) -> &mut Queue {
        &mut self.queue
    }

    /// Convenience push into the owned queue.
    pub fn enqueue(&mut self, track: Track) {
        self.queue.push(track);
    }

    /// Currently selected track, if any.
    #[must_use]
    pub fn now_playing(&self) -> Option<&Track> {
        self.queue.current()
    }

    /// Start playback if the queue has a current track.
    pub fn play(&mut self) {
        if self.queue.current().is_some() {
            self.state = PlaybackState::Playing;
            self.start_clock();
        }
    }

    /// Pause only out of `Playing`.
    pub fn pause(&mut self) {
        if self.state == PlaybackState::Playing {
            self.state = PlaybackState::Paused;
            self.freeze_clock();
        }
    }

    /// Resume only out of `Paused`.
    pub fn resume(&mut self) {
        if self.state == PlaybackState::Paused {
            self.state = PlaybackState::Playing;
            self.continue_clock();
        }
    }

    /// Stop (keeps the queue position).
    pub fn stop(&mut self) {
        self.state = PlaybackState::Stopped;
        self.reset_clock();
    }

    /// Advance; `Playing` on success, `Stopped` at the end of queue.
    /// Returns `None` at the end of the queue (queue position is kept,
    /// so [`Player::now_playing`] still reports the last track).
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<&Track> {
        let advanced = self.queue.next_manual().is_some();
        if advanced {
            self.state = PlaybackState::Playing;
            self.start_clock();
        } else {
            self.state = PlaybackState::Stopped;
            self.reset_clock();
        }
        if advanced {
            self.queue.current()
        } else {
            None
        }
    }

    /// Go back; `Playing` on success (queue never returns `None` here
    /// unless empty, in which case state becomes `Stopped`).
    ///
    /// A successful step resets the clock to the start of the new track;
    /// the clock runs only when the resulting state is `Playing` (so a
    /// step taken while `Paused` leaves the fresh position frozen at zero).
    pub fn prev(&mut self) -> Option<&Track> {
        let has_prev = self.queue.prev_track().is_some();
        if has_prev {
            // If we were stopped with a track available, stepping counts
            // as (re)starting playback.
            if self.state == PlaybackState::Stopped {
                self.state = PlaybackState::Playing;
            }
            self.reset_clock();
            if self.state == PlaybackState::Playing {
                self.continue_clock();
            }
        } else {
            self.state = PlaybackState::Stopped;
            self.reset_clock();
        }
        self.queue.current()
    }

    /// Set volume, clamped to `0.0..=1.0`.
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
    }

    /// UI-facing playback position in seconds (>= 0.0): accumulated wall
    /// time since the current track started. Frozen while paused, zero
    /// after [`Player::stop`] or before any playback.
    #[must_use]
    pub fn position_secs(&self) -> f64 {
        let extra = self
            .play_started_at
            .map(|t| t.elapsed())
            .unwrap_or(Duration::ZERO);
        (self.paused_accum + extra).as_secs_f64().max(0.0)
    }

    /// Reset the playback clock to zero and stop counting (used by
    /// [`Player::stop`] and track transitions).
    pub fn reset_clock(&mut self) {
        self.paused_accum = Duration::ZERO;
        self.play_started_at = None;
    }

    /// Reset to zero and start counting from now (new track / restart).
    fn start_clock(&mut self) {
        self.paused_accum = Duration::ZERO;
        self.play_started_at = Some(Instant::now());
    }

    /// Freeze accumulation at the current position (pause).
    fn freeze_clock(&mut self) {
        if let Some(since) = self.play_started_at.take() {
            self.paused_accum += since.elapsed();
        }
    }

    /// Continue counting from the frozen position (resume).
    fn continue_clock(&mut self) {
        if self.play_started_at.is_none() {
            self.play_started_at = Some(Instant::now());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yutu_core::MockBackend;

    fn player_with(n: usize) -> Player {
        let mut p = Player::new();
        for t in MockBackend::fixture_tracks().into_iter().take(n) {
            p.enqueue(t);
        }
        p
    }

    #[test]
    fn defaults_are_sane() {
        let p = Player::new();
        assert_eq!(p.state(), PlaybackState::Stopped);
        assert_eq!(p.volume(), 1.0);
        assert_eq!(p.eq_preset_name(), "Flat");
        assert!(p.now_playing().is_none());
    }

    #[test]
    fn play_pause_resume_stop_transitions() {
        let mut p = player_with(2);
        // Play with tracks -> Playing.
        p.play();
        assert_eq!(p.state(), PlaybackState::Playing);

        p.pause();
        assert_eq!(p.state(), PlaybackState::Paused);

        // Pause is idempotent outside Playing.
        p.pause();
        assert_eq!(p.state(), PlaybackState::Paused);

        p.resume();
        assert_eq!(p.state(), PlaybackState::Playing);

        // Resume is a no-op unless Paused.
        p.resume();
        assert_eq!(p.state(), PlaybackState::Playing);

        p.stop();
        assert_eq!(p.state(), PlaybackState::Stopped);

        // Resume from Stopped stays Stopped; play restarts.
        p.resume();
        assert_eq!(p.state(), PlaybackState::Stopped);
        p.play();
        assert_eq!(p.state(), PlaybackState::Playing);
    }

    #[test]
    fn play_with_empty_queue_stays_stopped() {
        let mut p = Player::new();
        p.play();
        assert_eq!(p.state(), PlaybackState::Stopped);
    }

    #[test]
    fn next_prev_update_state_and_now_playing() {
        let mut p = player_with(3);
        p.play();
        assert_eq!(p.now_playing().unwrap().id, "m1");

        assert_eq!(p.next().unwrap().id, "m2");
        assert!(p.is_playing());
        assert_eq!(p.next().unwrap().id, "m3");
        // End of queue (RepeatMode::Off): None + Stopped.
        assert!(p.next().is_none());
        assert_eq!(p.state(), PlaybackState::Stopped);

        // Stepping back from Stopped restarts playback.
        assert_eq!(p.prev().unwrap().id, "m2");
        assert_eq!(p.state(), PlaybackState::Playing);
    }

    #[test]
    fn next_under_repeat_one_advances_then_stops() {
        use crate::queue::RepeatMode;
        let mut p = player_with(2);
        p.queue.repeat = RepeatMode::One;
        p.play();
        assert_eq!(p.next().unwrap().id, "m2");
        assert!(p.is_playing());
        // Tail under One: manual next ends (no replay, no autoplay here).
        assert!(p.next().is_none());
        assert_eq!(p.state(), PlaybackState::Stopped);
    }

    #[test]
    fn next_on_empty_queue_is_none_and_stopped() {
        let mut p = Player::new();
        assert!(p.next().is_none());
        assert!(p.is_stopped());
        assert!(p.prev().is_none());
    }

    #[test]
    fn volume_is_clamped() {
        let mut p = Player::new();
        p.set_volume(0.5);
        assert_eq!(p.volume(), 0.5);
        p.set_volume(2.0);
        assert_eq!(p.volume(), 1.0);
        p.set_volume(-1.0);
        assert_eq!(p.volume(), 0.0);
    }

    #[test]
    fn eq_preset_name_roundtrip() {
        let mut p = Player::new();
        p.set_eq_preset("Rock");
        assert_eq!(p.eq_preset_name(), "Rock");
    }

    #[test]
    fn clock_freezes_on_pause_and_resets_on_stop() {
        let mut p = player_with(2);
        assert_eq!(p.position_secs(), 0.0);

        p.play();
        assert!(p.position_secs() >= 0.0);

        // Freeze: two immediate reads are exactly equal (no running part,
        // no sleeps needed).
        p.pause();
        let frozen_a = p.position_secs();
        let frozen_b = p.position_secs();
        assert_eq!(frozen_a, frozen_b);

        // Resume continues from the frozen position.
        p.resume();
        assert!(p.position_secs() >= frozen_b);

        // Stop resets to zero.
        p.stop();
        assert_eq!(p.position_secs(), 0.0);
    }

    #[test]
    fn clock_resets_on_play_and_next() {
        let mut p = player_with(3);
        p.play();
        std::thread::sleep(std::time::Duration::from_millis(30));
        let before = p.position_secs();
        assert!(before > 0.0, "expected elapsed time, got {before}");

        // Re-playing restarts the clock.
        p.play();
        let restarted = p.position_secs();
        assert!(
            restarted < before,
            "play should reset the clock: {before} -> {restarted}"
        );

        std::thread::sleep(std::time::Duration::from_millis(30));
        let before_next = p.position_secs();
        assert!(before_next > restarted);

        // Advancing to the next track resets the clock.
        assert!(p.next().is_some());
        let after_next = p.position_secs();
        assert!(
            after_next < before_next,
            "next should reset the clock: {before_next} -> {after_next}"
        );

        // Running to the end of the queue stops and resets the clock.
        assert!(p.next().is_some());
        assert!(p.next().is_none());
        assert_eq!(p.state(), PlaybackState::Stopped);
        assert_eq!(p.position_secs(), 0.0);
    }
}
