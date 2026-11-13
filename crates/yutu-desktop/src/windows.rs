//! Windows media keys + System Media Transport Controls via souvlaki.
//!
//! Shape mirrors the macOS side: the Iced layer keeps publishing into
//! the same shared [`crate::mpris::MprisPlayer`] state (no UI changes),
//! and this driver pushes that state to the OS on a 1 Hz watch loop
//! while hardware keys, the volume flyout, and the taskbar hover card
//! land in the same [`crate::mpris::PlayerCommand`] inbox the Tick
//! already drains. Everything is best-effort: any failure logs and the
//! GUI runs regardless, like the MPRIS spawn.
//!
//! Windows needs an `HWND` to own the SMTC session
//! (`ISystemMediaTransportControlsInterop::GetForWindow`), so the driver
//! creates one invisible top-level tool window on its helper thread and
//! keeps it alive for the process lifetime. The window is never shown,
//! never activates, and stays out of the taskbar and Alt-Tab.

#[cfg(target_os = "windows")]
use crate::macos::{meta_of, MediaMeta};
#[cfg(target_os = "windows")]
use crate::mpris::{PlaybackStatus, PlayerCommand, SharedPlayer};
#[cfg(target_os = "windows")]
use crate::MPRIS_IDENTITY;

/// Fallback seek step for undetermined `Seek` events (FastForward and
/// Rewind buttons report no amount, so a fixed 10s hop keeps them
/// useful and matches the keyboard seek step used elsewhere).
#[cfg(target_os = "windows")]
const UNDETERMINED_SEEK_SECS: f64 = 10.0;

/// Map one souvlaki remote event to our command inbox. Undetermined
/// seeks get the fixed fallback step, desktop-only actions
/// (open/raise/quit) have no mapping.
#[cfg(target_os = "windows")]
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
        E::Seek(dir) => match dir {
            SeekDirection::Forward => Some(PlayerCommand::Seek(UNDETERMINED_SEEK_SECS)),
            SeekDirection::Backward => Some(PlayerCommand::Seek(-UNDETERMINED_SEEK_SECS)),
        },
        E::SetPosition(pos) => Some(PlayerCommand::SetPosition(pos.0.as_secs_f64())),
        E::SetVolume(v) => Some(PlayerCommand::SetVolume(v)),
        E::OpenUri(_) | E::Raise | E::Quit => None,
    }
}

/// Invisible top-level window that owns the SMTC session. It must be a
/// real top-level window (message-only parents are rejected) and must
/// stay alive as long as the controls do, so `drive` holds one for the
/// process lifetime.
#[cfg(target_os = "windows")]
struct HiddenWindow {
    handle: windows::Win32::Foundation::HWND,
}

#[cfg(target_os = "windows")]
impl HiddenWindow {
    fn new() -> Result<Self, String> {
        use std::mem;
        use windows::core::PCWSTR;
        use windows::w;
        use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassExW, WINDOW_EX_STYLE,
            WINDOW_STYLE, WNDCLASSEXW,
        };

        unsafe extern "system" fn wnd_proc(
            hwnd: HWND,
            msg: u32,
            wparam: WPARAM,
            lparam: LPARAM,
        ) -> LRESULT {
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }

        unsafe {
            let instance =
                GetModuleHandleW(None).map_err(|e| format!("getting module handle failed: {e}"))?;
            let class_name = w!("YutuMuxSMTC");
            // Best effort: a previous attempt may have registered the
            // class already, so a register failure never blocks the
            // create below (its error is the one that matters).
            let _ = RegisterClassExW(&WNDCLASSEXW {
                cbSize: mem::size_of::<WNDCLASSEXW>() as u32,
                hInstance: instance,
                lpszClassName: PCWSTR::from(class_name),
                lpfnWndProc: Some(wnd_proc),
                ..Default::default()
            });
            let handle = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                class_name,
                w!("yutu-mux SMTC"),
                WINDOW_STYLE::default(),
                0,
                0,
                0,
                0,
                None,
                None,
                instance,
                None,
            );
            if handle.0 == 0 {
                Err(format!(
                    "hidden SMTC window creation failed: {}",
                    std::io::Error::last_os_error()
                ))
            } else {
                Ok(Self { handle })
            }
        }
    }
}

#[cfg(target_os = "windows")]
impl Drop for HiddenWindow {
    fn drop(&mut self) {
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::DestroyWindow;
            DestroyWindow(self.handle);
        }
    }
}

/// Pump the thread message queue so the hidden window stays responsive.
/// WinRT button callbacks do not strictly need this, but the owning
/// thread created a window, so draining keeps it well-behaved.
#[cfg(target_os = "windows")]
fn pump_event_queue() {
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetAncestor, IsDialogMessageW, PeekMessageW, TranslateMessage, GA_ROOT,
        MSG, PM_REMOVE, WM_QUIT,
    };
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        let mut has_message = PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool();
        while msg.message != WM_QUIT && has_message {
            if !IsDialogMessageW(GetAncestor(msg.hwnd, GA_ROOT), &msg).as_bool() {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            has_message = PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool();
        }
    }
}

/// Push the shared state to SMTC: metadata on change, playback state
/// plus position every pass (this keeps the flyout progress live).
/// Runs until the process exits; any OS error is logged, never fatal.
#[cfg(target_os = "windows")]
fn drive(state: SharedPlayer) {
    use souvlaki::{MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig};
    use std::time::Duration;

    let window = match HiddenWindow::new() {
        Ok(window) => window,
        Err(e) => {
            yutu_core::app_log::error("windows-media", format_args!("hidden window: {e}"));
            return;
        }
    };
    let mut controls = match souvlaki::MediaControls::new(PlatformConfig {
        display_name: MPRIS_IDENTITY,
        dbus_name: MPRIS_IDENTITY,
        hwnd: Some(window.handle.0 as *mut std::ffi::c_void),
    }) {
        Ok(controls) => controls,
        Err(e) => {
            yutu_core::app_log::error("windows-media", format_args!("controls unavailable: {e:?}"));
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
        yutu_core::app_log::error("windows-media", "remote commands unavailable");
        return;
    }
    // Held for the process lifetime: dropping it would revoke SMTC.
    let _window = window;
    let mut last_meta = MediaMeta::default();
    let mut last_play: Option<(PlaybackStatus, f64)> = None;
    loop {
        std::thread::sleep(Duration::from_secs(1));
        pump_event_queue();
        let (meta, status, position_secs) = match state.lock() {
            Ok(player) => (meta_of(&player), player.status(), player.position_secs()),
            Err(poison) => {
                let player = poison.into_inner();
                (meta_of(&player), player.status(), player.position_secs())
            }
        };
        // Cleared queues keep the last track card (no clear API): a
        // blank entry would be worse than the just-played one sitting
        // at Stopped.
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
                yutu_core::app_log::error("windows-media", "metadata push failed");
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
                yutu_core::app_log::error("windows-media", "playback push failed");
            }
        }
    }
}

/// Best-effort Windows media integration on a helper thread (mirrors
/// the MPRIS spawn: failures log and the GUI runs regardless).
#[cfg(target_os = "windows")]
pub fn spawn_media_integration(state: SharedPlayer) {
    std::thread::spawn(move || drive(state));
}

/// Non-Windows builds have no SMTC driver here (the shared state still
/// exists for tests, but nothing drives it from this module).
#[cfg(not(target_os = "windows"))]
pub fn spawn_media_integration(_state: crate::mpris::SharedPlayer) {}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;
    use crate::mpris::MprisPlayer;
    use crate::NowPlaying;

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
        assert_eq!(meta.track_id, "abc");
    }

    #[test]
    fn remote_events_map_to_commands() {
        use souvlaki::{MediaControlEvent as E, MediaPosition, SeekDirection};
        use std::time::Duration;
        assert_eq!(command_from(E::Play), Some(PlayerCommand::Play));
        assert_eq!(command_from(E::Pause), Some(PlayerCommand::Pause));
        assert_eq!(command_from(E::Toggle), Some(PlayerCommand::PlayPause));
        assert_eq!(command_from(E::Next), Some(PlayerCommand::Next));
        assert_eq!(command_from(E::Previous), Some(PlayerCommand::Previous));
        assert_eq!(command_from(E::Stop), Some(PlayerCommand::Stop));
        assert_eq!(
            command_from(E::SeekBy(SeekDirection::Backward, Duration::from_secs(10))),
            Some(PlayerCommand::Seek(-10.0))
        );
        assert_eq!(
            command_from(E::Seek(SeekDirection::Forward)),
            Some(PlayerCommand::Seek(UNDETERMINED_SEEK_SECS))
        );
        assert_eq!(
            command_from(E::Seek(SeekDirection::Backward)),
            Some(PlayerCommand::Seek(-UNDETERMINED_SEEK_SECS))
        );
        assert_eq!(
            command_from(E::SetPosition(MediaPosition(Duration::from_secs(42)))),
            Some(PlayerCommand::SetPosition(42.0))
        );
        assert_eq!(command_from(E::Raise), None);
        assert_eq!(command_from(E::Quit), None);
    }
}
