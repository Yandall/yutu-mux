//! macOS Now Playing + media keys via souvlaki (`MPNowPlayingInfoCenter`
//! / `MPRemoteCommandCenter`, no accessibility permission needed).
//!
//! Shape mirrors the Linux side: the Iced layer keeps publishing into
//! the same shared [`MprisPlayer`] state (no UI changes), and this
//! driver pushes that state to the OS on a 1 Hz watch loop while media
//! keys / Control Center commands land in the same [`PlayerCommand`]
//! inbox the Tick already drains. Everything is best-effort: any failure
//! logs and the GUI runs regardless, like the MPRIS spawn.
//!
//! The souvlaki dependency (and the OS calls) only exist on macOS:
//! other platforms compile just the pure snapshot helpers, which double
//! as the unit-test surface (the driver thread itself needs a Mac).

use crate::mpris::{MprisPlayer, SharedPlayer};
#[cfg(target_os = "macos")]
use crate::mpris::{PlaybackStatus, PlayerCommand};
use crate::NowPlaying;
#[cfg(target_os = "macos")]
use crate::MPRIS_IDENTITY;

/// Track-shaped half of the OS state: pushed only when it changes, so
/// Control Center is not spammed with identical dictionaries.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MediaMeta {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub track_id: String,
    pub length_secs: u32,
    pub art_url: String,
}

/// Read the track-shaped half out of the shared player state.
#[must_use]
pub fn meta_of(player: &MprisPlayer) -> MediaMeta {
    let t: &NowPlaying = player.now_playing();
    MediaMeta {
        title: t.title.clone(),
        artist: t.artist.clone(),
        album: t.album.clone(),
        track_id: t.track_id.clone(),
        length_secs: t.length_secs,
        art_url: t.art_url.clone(),
    }
}

/// Map one souvlaki remote event to our command inbox. Undetermined
/// seeks and desktop-only actions (raise/quit/open) have no mapping.
/// (Upstream gap: skip-forward/backward commands are never registered
/// by the macOS backend, so headset skip-interval buttons stay silent.)
#[cfg(target_os = "macos")]
fn command_from(event: souvlaki::MediaControlEvent) -> Option<PlayerCommand> {
    use souvlaki::{MediaControlEvent as E, SeekDirection};
    match event {
        E::Play => Some(PlayerCommand::Play),
        E::Pause => Some(PlayerCommand::Pause),
        E::Toggle => Some(PlayerCommand::PlayPause),
        E::Next => Some(PlayerCommand::Next),
        E::Previous => Some(PlayerCommand::Previous),
        E::Stop => Some(PlayerCommand::Stop),
        E::SeekBy(dir, amount) => {
            let secs = amount.as_secs_f64();
            match dir {
                SeekDirection::Forward => Some(PlayerCommand::Seek(secs)),
                SeekDirection::Backward => Some(PlayerCommand::Seek(-secs)),
            }
        }
        E::SetPosition(pos) => Some(PlayerCommand::SetPosition(pos.0.as_secs_f64())),
        E::SetVolume(v) => Some(PlayerCommand::SetVolume(v)),
        E::Seek(_) | E::OpenUri(_) | E::Raise | E::Quit => None,
    }
}

/// Push the shared state to the OS: metadata on change, playback state
/// plus position every pass (this keeps lock-screen progress live).
/// Runs until the process exits; any OS error is logged, never fatal.
#[cfg(target_os = "macos")]
fn drive(state: SharedPlayer) {
    use souvlaki::{MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig};
    use std::time::Duration;

    let mut controls = match souvlaki::MediaControls::new(PlatformConfig {
        display_name: MPRIS_IDENTITY,
        dbus_name: MPRIS_IDENTITY,
        hwnd: None,
    }) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("macos media: controls unavailable: {e}");
            return;
        }
    };
    let inbox = state.clone();
    if controls
        .attach(move |event| {
            if let Some(cmd) = command_from(event) {
                inbox
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .push_command(cmd);
            }
        })
        .is_err()
    {
        eprintln!("macos media: remote commands unavailable");
        return;
    }
    let mut last_meta = MediaMeta::default();
    let mut last_play: Option<(PlaybackStatus, f64)> = None;
    loop {
        std::thread::sleep(Duration::from_secs(1));
        let (meta, status, position_secs) = match state.lock() {
            Ok(player) => (meta_of(&player), player.status(), player.position_secs()),
            Err(poison) => {
                let player = poison.into_inner();
                (meta_of(&player), player.status(), player.position_secs())
            }
        };
        // Cleared queues keep the last track card (souvlaki has no
        // clear API): a blank entry would be worse than the just-played
        // one sitting at Stopped.
        if meta != last_meta && !meta.track_id.is_empty() {
            last_meta = meta.clone();
            let metadata = MediaMetadata {
                title: Some(meta.title.as_str()),
                artist: Some(meta.artist.as_str()),
                album: Some(meta.album.as_str()),
                cover_url: (!meta.art_url.is_empty()).then_some(meta.art_url.as_str()),
                duration: Some(Duration::from_secs(u64::from(meta.length_secs))),
            };
            if controls.set_metadata(metadata).is_err() {
                eprintln!("macos media: metadata push failed");
            }
        }
        // Playback re-pushes only on real change (frozen pause/stop
        // costs nothing; playing ticks keep progress live). The clock
        // is sanitized: `from_secs_f64` panics on infinite/huge input,
        // and a corrupt value must never kill the driver thread.
        let position_secs = if position_secs.is_finite() {
            position_secs.clamp(0.0, 86_400.0)
        } else {
            0.0
        };
        if last_play.is_none_or(|(s, p)| s != status || p != position_secs) {
            last_play = Some((status, position_secs));
            let progress = Some(MediaPosition(Duration::from_secs_f64(position_secs)));
            let playback = match status {
                PlaybackStatus::Playing => MediaPlayback::Playing { progress },
                PlaybackStatus::Paused => MediaPlayback::Paused { progress },
                PlaybackStatus::Stopped => MediaPlayback::Stopped,
            };
            if controls.set_playback(playback).is_err() {
                eprintln!("macos media: playback push failed");
            }
        }
    }
}

/// Best-effort macOS media integration on a helper thread (mirrors the
/// MPRIS spawn: failures log and the GUI runs regardless).
#[cfg(target_os = "macos")]
pub fn spawn_media_integration(state: SharedPlayer) {
    std::thread::spawn(move || drive(state));
}

/// Non-macOS builds have no OS media service (the shared state still
/// exists for tests, but nothing drives it).
#[cfg(not(target_os = "macos"))]
pub fn spawn_media_integration(_state: SharedPlayer) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mpris::{MprisPlayer, PlaybackStatus};

    fn player() -> MprisPlayer {
        let mut p = MprisPlayer::new(NowPlaying::default(), PlaybackStatus::Stopped, 0.8);
        p.update_track(NowPlaying {
            title: "Song".into(),
            artist: "Band".into(),
            album: "Album".into(),
            track_id: "abc".into(),
            length_secs: 200,
            art_url: "https://example.invalid/x".into(),
        });
        p
    }

    #[test]
    fn meta_snapshot_mirrors_shared_state() {
        let meta = meta_of(&player());
        assert_eq!(meta.title, "Song");
        assert_eq!(meta.artist, "Band");
        assert_eq!(meta.album, "Album");
        assert_eq!(meta.track_id, "abc");
        assert_eq!(meta.length_secs, 200);
        assert_eq!(meta.art_url, "https://example.invalid/x");
    }

    #[test]
    fn unchanged_meta_compares_equal() {
        let (a, b) = (player(), player());
        assert_eq!(meta_of(&a), meta_of(&b));
        let mut c = player();
        c.update_track(NowPlaying {
            title: "Other".into(),
            ..NowPlaying::default()
        });
        assert_ne!(meta_of(&a), meta_of(&c));
    }

    #[test]
    fn empty_art_url_stays_empty() {
        let mut p = player();
        p.update_track(NowPlaying::default());
        assert_eq!(meta_of(&p).art_url, "");
    }

    /// Remote-event mapping (macOS only: souvlaki types do not exist on
    /// other targets, so this runs under the mac CI job).
    #[cfg(target_os = "macos")]
    #[test]
    fn remote_events_map_to_commands() {
        use souvlaki::{MediaControlEvent as E, MediaPosition, SeekDirection};
        use std::time::Duration;
        assert_eq!(command_from(E::Play), Some(PlayerCommand::Play));
        assert_eq!(command_from(E::Toggle), Some(PlayerCommand::PlayPause));
        assert_eq!(
            command_from(E::SeekBy(SeekDirection::Backward, Duration::from_secs(10))),
            Some(PlayerCommand::Seek(-10.0))
        );
        assert_eq!(
            command_from(E::SetPosition(MediaPosition(Duration::from_secs(42)))),
            Some(PlayerCommand::SetPosition(42.0))
        );
        assert_eq!(command_from(E::Seek(SeekDirection::Forward)), None);
        assert_eq!(command_from(E::Raise), None);
        assert_eq!(command_from(E::Quit), None);
    }
}
