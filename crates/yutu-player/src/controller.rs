//! Backend-agnostic playback controller.
//!
//! Owns a [`Queue`] plus playback state, volume, and the active EQ preset
//! *name* (the actual coefficients live in [`crate::eq`]). No audio I/O
//! here, so there are no `Send`/thread issues; the future mpv backend can
//! observe this controller and drive mpv accordingly.

use crate::queue::Queue;
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
#[derive(Debug)]
pub struct Player {
    queue: Queue,
    state: PlaybackState,
    volume: f32,
    eq_preset_name: String,
}

impl Default for Player {
    fn default() -> Self {
        Self {
            queue: Queue::default(),
            state: PlaybackState::Stopped,
            volume: 1.0,
            eq_preset_name: "Flat".to_string(),
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
        }
    }

    /// Pause only out of `Playing`.
    pub fn pause(&mut self) {
        if self.state == PlaybackState::Playing {
            self.state = PlaybackState::Paused;
        }
    }

    /// Resume only out of `Paused`.
    pub fn resume(&mut self) {
        if self.state == PlaybackState::Paused {
            self.state = PlaybackState::Playing;
        }
    }

    /// Stop (keeps the queue position).
    pub fn stop(&mut self) {
        self.state = PlaybackState::Stopped;
    }

    /// Advance; `Playing` on success, `Stopped` at the end of queue.
    /// Returns `None` at the end of the queue (queue position is kept,
    /// so [`Player::now_playing`] still reports the last track).
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<&Track> {
        let advanced = self.queue.next_track().is_some();
        self.state = if advanced {
            PlaybackState::Playing
        } else {
            PlaybackState::Stopped
        };
        if advanced {
            self.queue.current()
        } else {
            None
        }
    }

    /// Go back; `Playing` on success (queue never returns `None` here
    /// unless empty, in which case state becomes `Stopped`).
    pub fn prev(&mut self) -> Option<&Track> {
        let has_prev = self.queue.prev_track().is_some();
        if has_prev {
            // If we were stopped with a track available, stepping counts
            // as (re)starting playback.
            if self.state == PlaybackState::Stopped {
                self.state = PlaybackState::Playing;
            }
        } else {
            self.state = PlaybackState::Stopped;
        }
        self.queue.current()
    }

    /// Set volume, clamped to `0.0..=1.0`.
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
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
}
