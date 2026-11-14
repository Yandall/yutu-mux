//! Keep the computer awake while music plays.
//!
//! Idle suspend stops playback mid-song, so while the shared player
//! reports `Playing` the app holds a system-sleep inhibitor. Only system
//! sleep is blocked: the screen may still blank, dim, or lock as usual.
//!
//! * Linux: systemd-logind `Inhibit("sleep", ..., "block")`. The returned
//!   file descriptor is the lock; dropping it releases it. GNOME, KDE,
//!   and logind's own `IdleAction` all honor it.
//! * macOS: IOKit `PreventUserIdleSystemSleep` power assertion (the
//!   display may still sleep).
//! * Windows: `SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED)`
//!   on the guard thread (no `ES_DISPLAY_REQUIRED`, so the screen may
//!   turn off).
//!
//! Rules: the hold is taken as soon as playback starts and released once
//! playback has been off for [`RELEASE_GRACE`] (track changes and buffering
//! briefly leave `Playing`; a short grace avoids flapping the lock, and an
//! overdue idle timer firing in that gap). Failures log once and the app
//! plays on without the guard.

use crate::mpris::{PlaybackStatus, SharedPlayer};
use std::time::{Duration, Instant};

/// How long playback must stay off before the sleep hold is dropped.
pub const RELEASE_GRACE: Duration = Duration::from_secs(15);

/// How often the guard thread samples the player status.
const POLL: Duration = Duration::from_secs(1);

/// Pure hold/release decision, separated from the OS calls for tests.
#[derive(Debug, Default)]
pub struct SleepPolicy {
    /// Last instant the player was seen `Playing`.
    last_playing: Option<Instant>,
}

impl SleepPolicy {
    /// Whether the sleep hold should be held right now.
    pub fn want_hold(&mut self, status: PlaybackStatus, now: Instant) -> bool {
        if status == PlaybackStatus::Playing {
            self.last_playing = Some(now);
            return true;
        }
        self.last_playing
            .is_some_and(|t| now.saturating_duration_since(t) < RELEASE_GRACE)
    }
}

/// Spawn the guard thread. Best effort: platforms without a backend
/// (or a missing bus) just log and play on.
pub fn spawn_sleep_guard(state: SharedPlayer) {
    let spawned = std::thread::Builder::new()
        .name("sleep-guard".into())
        .spawn(move || run(state));
    if let Err(e) = spawned {
        yutu_core::app_log::warn("sleep-guard", format_args!("thread: {e}"));
    }
}

fn run(state: SharedPlayer) {
    let mut policy = SleepPolicy::default();
    let mut backend = platform::Backend::new();
    let mut hold: Option<platform::Hold> = None;
    let mut warned = false;
    loop {
        let status = state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .status();
        let want = policy.want_hold(status, Instant::now());
        match (want, hold.is_some()) {
            (true, false) => match backend.acquire() {
                Ok(h) => {
                    hold = Some(h);
                    warned = false;
                }
                Err(e) => {
                    // One warning per failure streak, not one per second.
                    if !warned {
                        yutu_core::app_log::warn(
                            "sleep-guard",
                            format_args!("cannot block system sleep: {e}"),
                        );
                        warned = true;
                    }
                }
            },
            (false, true) => hold = None,
            _ => {}
        }
        std::thread::sleep(POLL);
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use zbus::blocking::Connection;
    use zbus::zvariant::OwnedFd;

    /// Cached system-bus connection (reconnects after a failure).
    #[derive(Default)]
    pub struct Backend {
        conn: Option<Connection>,
    }

    /// logind inhibitor lock: closing the fd releases it.
    pub struct Hold {
        _fd: OwnedFd,
    }

    impl Backend {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn acquire(&mut self) -> Result<Hold, String> {
            let conn = match &self.conn {
                Some(conn) => conn.clone(),
                None => {
                    let conn = Connection::system().map_err(|e| e.to_string())?;
                    self.conn = Some(conn.clone());
                    conn
                }
            };
            let reply = conn
                .call_method(
                    Some("org.freedesktop.login1"),
                    "/org/freedesktop/login1",
                    Some("org.freedesktop.login1.Manager"),
                    "Inhibit",
                    &("sleep", crate::MPRIS_IDENTITY, "Playing music", "block"),
                )
                .map_err(|e| {
                    self.conn = None;
                    e.to_string()
                })?;
            let fd: OwnedFd = reply.body().deserialize().map_err(|e| e.to_string())?;
            Ok(Hold { _fd: fd })
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::ffi::{c_char, c_void, CStr};

    type CFStringRef = *const c_void;

    const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
    const K_IOPM_ASSERTION_LEVEL_ON: u32 = 255;
    const K_IO_RETURN_SUCCESS: i32 = 0;

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFStringCreateWithCString(
            alloc: *const c_void,
            c_str: *const c_char,
            encoding: u32,
        ) -> CFStringRef;
        fn CFRelease(cf: *const c_void);
    }

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOPMAssertionCreateWithName(
            assertion_type: CFStringRef,
            level: u32,
            name: CFStringRef,
            id: *mut u32,
        ) -> i32;
        fn IOPMAssertionRelease(id: u32) -> i32;
    }

    pub struct Backend;

    /// IOKit power assertion, released on drop.
    pub struct Hold {
        id: u32,
    }

    impl Drop for Hold {
        fn drop(&mut self) {
            // SAFETY: `id` came from a successful IOPMAssertionCreateWithName
            // and is released exactly once here.
            unsafe {
                IOPMAssertionRelease(self.id);
            }
        }
    }

    fn cf_string(s: &CStr) -> Result<CFStringRef, String> {
        // SAFETY: `s` is a valid NUL-terminated string for the call.
        let r = unsafe {
            CFStringCreateWithCString(std::ptr::null(), s.as_ptr(), K_CF_STRING_ENCODING_UTF8)
        };
        if r.is_null() {
            Err("CFStringCreateWithCString failed".into())
        } else {
            Ok(r)
        }
    }

    impl Backend {
        pub fn new() -> Self {
            Backend
        }

        pub fn acquire(&mut self) -> Result<Hold, String> {
            let kind = cf_string(c"PreventUserIdleSystemSleep")?;
            let name = match cf_string(c"yutu-mux: playing music") {
                Ok(name) => name,
                Err(e) => {
                    // SAFETY: `kind` is a live CFString we own.
                    unsafe { CFRelease(kind) };
                    return Err(e);
                }
            };
            let mut id = 0u32;
            // SAFETY: both CFStrings are live; `id` is a valid out pointer.
            // The strings are released right after (IOKit retains them).
            let rc = unsafe {
                let rc =
                    IOPMAssertionCreateWithName(kind, K_IOPM_ASSERTION_LEVEL_ON, name, &mut id);
                CFRelease(kind);
                CFRelease(name);
                rc
            };
            if rc == K_IO_RETURN_SUCCESS {
                Ok(Hold { id })
            } else {
                Err(format!("IOPMAssertionCreateWithName returned {rc:#x}"))
            }
        }
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use windows::Win32::System::Power::{
        SetThreadExecutionState, ES_CONTINUOUS, ES_SYSTEM_REQUIRED,
    };

    pub struct Backend;

    /// Thread execution state; cleared on drop. Acquire and drop both run
    /// on the guard thread, which is what the per-thread flag requires.
    pub struct Hold;

    impl Drop for Hold {
        fn drop(&mut self) {
            // SAFETY: plain Win32 call with valid flags.
            unsafe {
                SetThreadExecutionState(ES_CONTINUOUS);
            }
        }
    }

    impl Backend {
        pub fn new() -> Self {
            Backend
        }

        pub fn acquire(&mut self) -> Result<Hold, String> {
            // SAFETY: plain Win32 call with valid flags.
            let prev = unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) };
            if prev.0 == 0 {
                Err("SetThreadExecutionState failed".into())
            } else {
                Ok(Hold)
            }
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod platform {
    pub struct Backend;

    pub struct Hold;

    impl Backend {
        pub fn new() -> Self {
            Backend
        }

        pub fn acquire(&mut self) -> Result<Hold, String> {
            Err("not supported on this platform".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holds_while_playing_and_through_the_grace() {
        let mut p = SleepPolicy::default();
        let t0 = Instant::now();
        assert!(!p.want_hold(PlaybackStatus::Stopped, t0));
        assert!(p.want_hold(PlaybackStatus::Playing, t0));
        // A brief gap (track change, buffering) keeps the hold.
        assert!(p.want_hold(PlaybackStatus::Stopped, t0 + Duration::from_secs(3)));
        assert!(p.want_hold(PlaybackStatus::Paused, t0 + RELEASE_GRACE / 2));
    }

    #[test]
    fn releases_after_the_grace() {
        let mut p = SleepPolicy::default();
        let t0 = Instant::now();
        assert!(p.want_hold(PlaybackStatus::Playing, t0));
        assert!(!p.want_hold(PlaybackStatus::Paused, t0 + RELEASE_GRACE));
        // Resuming takes the hold again at once.
        assert!(p.want_hold(PlaybackStatus::Playing, t0 + RELEASE_GRACE * 2));
    }

    #[test]
    fn never_holds_without_playback() {
        let mut p = SleepPolicy::default();
        let t0 = Instant::now();
        assert!(!p.want_hold(PlaybackStatus::Paused, t0));
        assert!(!p.want_hold(PlaybackStatus::Stopped, t0 + Duration::from_secs(60)));
    }
}

#[cfg(all(test, target_os = "linux"))]
mod live {
    #[test]
    #[ignore = "needs a system bus with logind"]
    fn logind_grants_a_sleep_inhibitor() {
        let _hold = super::platform::Backend::new().acquire().unwrap();
        let out = std::process::Command::new("systemd-inhibit")
            .arg("--list")
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&out.stdout).contains("Playing music"));
    }
}
