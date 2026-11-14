//! macOS Now Playing + media keys via souvlaki (`MPNowPlayingInfoCenter`
//! / `MPRemoteCommandCenter`, no accessibility permission needed).
//!
//! Thin shim over the shared [`crate::media`] driver: macOS needs no
//! owner window and no message pump, so the watch loop just sleeps.

use crate::mpris::SharedPlayer;

/// Best-effort macOS media integration on a helper thread (mirrors the
/// MPRIS spawn: failures log and the GUI runs regardless).
#[cfg(target_os = "macos")]
pub fn spawn_media_integration(state: SharedPlayer) {
    std::thread::spawn(move || crate::media::drive(state, None, "macos-media", std::thread::sleep));
}

/// Non-macOS builds have no driver here (the shared state still exists
/// for tests, but nothing drives it from this module).
#[cfg(not(target_os = "macos"))]
pub fn spawn_media_integration(_state: SharedPlayer) {}
