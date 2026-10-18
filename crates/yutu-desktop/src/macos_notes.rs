//! macOS Now Playing and media-key integration notes.
//!
//! Implemented in [`crate::macos`] (souvlaki driver over the shared
//! player state). This module keeps the raw design constraints so a
//! future native-`objc2` rewrite does not rediscover them.
//!
//! # Now Playing (Control Center / lock screen)
//!
//! * Publish track metadata through `MPNowPlayingInfoCenter`
//!   (`MediaPlayer.framework`): title (`MPMediaItemPropertyTitle`), artist,
//!   album, duration, and (later) artwork via `MPMediaItemArtwork`.
//! * The Iced layer must push a fresh info dictionary on every track change,
//!   mirroring what [`crate::mpris::MprisPlayer::update_track`] does for
//!   Linux, and clear it when the queue ends.
//! * Rust access requires Objective-C bridging (`objc2` + handwritten or
//!   generated `MediaPlayer` bindings); there is no maintained pure-Rust
//!   crate for `MPNowPlayingInfoCenter` today.
//!
//! # Remote commands / media keys
//!
//! * Subscribe to `MPRemoteCommandCenter` (`playCommand`, `pauseCommand`,
//!   `togglePlayPauseCommand`, `nextTrackCommand`, `previousTrackCommand`)
//!   and route the handlers back into the Iced playback controller: the
//!   same direction as MPRIS transport methods on Linux.
//! * Play/pause keys on their own do **not** need extra work once remote
//!   commands are registered; intercepting raw key events globally
//!   (`CGEventTap`) is a last resort that needs accessibility permission and
//!   should be avoided.
//! * `souvlaki` is the pragmatic third-party option: one small crate that
//!   covers media keys plus metadata on both macOS and Windows, at the cost
//!   of an extra dependency. Prefer the native `MediaPlayer` path if artwork
//!   and lock-screen progress matter.
//!
//! # Suggested build plan
//!
//! Shipped as [`crate::macos`]: one souvlaki driver over the shared
//! player state (no UI changes, target-gated dependency). The native
//! `objc2` + generated MediaPlayer bindings sketch stays the fallback
//! if souvlaki ever goes unmaintained.

#[cfg(test)]
mod tests {
    #[test]
    fn module_exists_as_documentation_anchor() {
        // No runtime behaviour: this module only carries integration notes.
        // The assertion below keeps the module referenced if docs are moved.
        assert!(module_path!().contains("macos_notes"));
    }
}
