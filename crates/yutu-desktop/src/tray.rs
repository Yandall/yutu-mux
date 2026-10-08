//! System-tray model (pure data, no GUI toolkit).
//!
//! [`TrayState`] captures everything the UI needs to render a tray icon and
//! tooltip: whether playback is active and a one-line track label.
//! [`TrayState::tooltip`] builds the tooltip string from that state.
//!
//! # Why pure data?
//!
//! Iced (including the 0.14 series used by `yutu-ui`) ships no tray widget,
//! so this crate deliberately takes no dependency on any tray/GUI toolkit.
//! When a real icon is wanted, the recommended follow-ups are:
//!
//! * `tray-icon` (+ `tao` on some platforms) for a portable icon, context
//!   menu and click handling driven from the Iced event loop; or
//! * `statusnotifier` (StatusNotifierItem / libappindicator via D-Bus) for a
//!   desktop-native Linux tray entry.
//!
//! Either backend can be fed directly from [`TrayState`]: push a fresh
//! snapshot on every playback change (track switch, play/pause) using
//! [`TrayState::from_now_playing`], and mirror backend menu actions
//! (play/pause/next/quit) back into the Iced playback controller.
//!
//! For the macOS menu-bar / Now Playing equivalent, see
//! [`crate::macos_notes`].

use crate::NowPlaying;

/// Minimal tray snapshot: playback flag plus a display-ready track label.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TrayState {
    /// True while audio is playing (as opposed to paused/stopped).
    pub playing: bool,
    /// One-line label such as `"M83 · Midnight City"`; empty when idle.
    pub track_label: String,
}

impl TrayState {
    /// Idle state: not playing, no label.
    pub fn new() -> Self {
        Self::default()
    }

    /// Playing state with an explicit label.
    pub fn playing(label: impl Into<String>) -> Self {
        Self {
            playing: true,
            track_label: label.into(),
        }
    }

    /// Paused state with an explicit label.
    pub fn paused(label: impl Into<String>) -> Self {
        Self {
            playing: false,
            track_label: label.into(),
        }
    }

    /// Build a snapshot from player metadata. The label is
    /// `"artist · title"`, falling back to whichever part is non-empty, and
    /// empty when both are missing.
    pub fn from_now_playing(track: &NowPlaying, playing: bool) -> Self {
        Self {
            playing,
            track_label: Self::label_for(track),
        }
    }

    /// Replace the track label (e.g. on track change).
    pub fn set_track_label(&mut self, label: impl Into<String>) {
        self.track_label = label.into();
    }

    /// Update the playing flag (e.g. on play/pause).
    pub fn set_playing(&mut self, playing: bool) {
        self.playing = playing;
    }

    /// Clear the label and mark as not playing (e.g. on stop/queue end).
    pub fn clear_track(&mut self) {
        self.track_label.clear();
        self.playing = false;
    }

    /// Human-readable tooltip: `"yutu-mux: <label>"` while playing,
    /// `"yutu-mux (paused): <label>"` while paused, `"yutu-mux: idle"` with no
    /// label.
    pub fn tooltip(&self) -> String {
        if self.track_label.is_empty() {
            "yutu-mux: idle".to_owned()
        } else if self.playing {
            format!("yutu-mux: {}", self.track_label)
        } else {
            format!("yutu-mux (paused): {}", self.track_label)
        }
    }

    /// One-line `"artist · title"` label for a track.
    pub fn label_for(track: &NowPlaying) -> String {
        match (track.artist.is_empty(), track.title.is_empty()) {
            (false, false) => format!("{} · {}", track.artist, track.title),
            (false, true) => track.artist.clone(),
            (true, false) => track.title.clone(),
            (true, true) => String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> NowPlaying {
        NowPlaying {
            title: "Midnight City".to_owned(),
            artist: "M83".to_owned(),
            album: "Hurry Up, We're Dreaming".to_owned(),
            track_id: "m1".to_owned(),
            length_secs: 185,
        }
    }

    #[test]
    fn tooltip_while_playing() {
        let state = TrayState::playing("M83 · Midnight City");
        assert_eq!(state.tooltip(), "yutu-mux: M83 · Midnight City");
    }

    #[test]
    fn tooltip_while_paused() {
        let state = TrayState::paused("M83 · Midnight City");
        assert_eq!(state.tooltip(), "yutu-mux (paused): M83 · Midnight City");
    }

    #[test]
    fn tooltip_idle_without_label() {
        assert_eq!(TrayState::new().tooltip(), "yutu-mux: idle");
        assert_eq!(
            TrayState {
                playing: true,
                track_label: String::new(),
            }
            .tooltip(),
            "yutu-mux: idle"
        );
    }

    #[test]
    fn from_now_playing_builds_artist_title_label() {
        let state = TrayState::from_now_playing(&sample(), true);
        assert!(state.playing);
        assert_eq!(state.track_label, "M83 · Midnight City");
        assert_eq!(state.tooltip(), "yutu-mux: M83 · Midnight City");
    }

    #[test]
    fn label_falls_back_to_non_empty_part() {
        let mut track = sample();
        track.title.clear();
        assert_eq!(TrayState::label_for(&track), "M83");
        track.artist.clear();
        track.title = "Midnight City".to_owned();
        assert_eq!(TrayState::label_for(&track), "Midnight City");
        track.title.clear();
        assert_eq!(TrayState::label_for(&track), "");
    }

    #[test]
    fn mutations_and_clear() {
        let mut state = TrayState::new();
        state.set_track_label("M83 · Midnight City");
        state.set_playing(true);
        assert_eq!(state.tooltip(), "yutu-mux: M83 · Midnight City");
        state.set_playing(false);
        assert_eq!(state.tooltip(), "yutu-mux (paused): M83 · Midnight City");
        state.clear_track();
        assert_eq!(state, TrayState::new());
        assert_eq!(state.tooltip(), "yutu-mux: idle");
    }
}
