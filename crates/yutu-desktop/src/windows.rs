//! Windows media keys + System Media Transport Controls via souvlaki.
//!
//! Thin shim over the shared [`crate::media`] driver: hardware keys,
//! the volume flyout media card, and the taskbar hover card land in
//! the same [`crate::mpris::PlayerCommand`] inbox the Tick drains.
//!
//! Windows needs an `HWND` to own the SMTC session
//! (`ISystemMediaTransportControlsInterop::GetForWindow`), so the driver
//! creates one invisible top-level window on its helper thread and
//! keeps it alive for the process lifetime. It is never shown (no
//! `WS_VISIBLE`), so it never activates and stays out of the taskbar
//! and Alt-Tab. Its message queue is pumped while the watch loop waits,
//! so broadcast senders are never left blocked on it.

use crate::mpris::SharedPlayer;

/// Invisible top-level window that owns the SMTC session. It must be a
/// real top-level window (message-only windows are rejected) and must
/// outlive the controls, so the driver thread holds it until exit.
#[cfg(target_os = "windows")]
struct HiddenWindow {
    handle: windows::Win32::Foundation::HWND,
}

#[cfg(target_os = "windows")]
impl HiddenWindow {
    fn new() -> Result<Self, String> {
        use windows::w;
        use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, RegisterClassExW, WINDOW_EX_STYLE, WINDOW_STYLE,
            WNDCLASSEXW,
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
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                hInstance: instance,
                lpszClassName: class_name,
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
            windows::Win32::UI::WindowsAndMessaging::DestroyWindow(self.handle);
        }
    }
}

/// Drain every pending message for this thread's hidden window.
#[cfg(target_os = "windows")]
fn pump_messages() {
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
    };
    unsafe {
        let mut msg = MSG::default();
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Wait out one watch interval while keeping the window responsive:
/// sleep in the message wait, wake on any input, drain, repeat until
/// the deadline.
#[cfg(target_os = "windows")]
fn wait_pumping(interval: std::time::Duration) {
    use windows::Win32::UI::WindowsAndMessaging::{MsgWaitForMultipleObjects, QS_ALLINPUT};
    let deadline = std::time::Instant::now() + interval;
    loop {
        pump_messages();
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            return;
        }
        let ms = u32::try_from(left.as_millis()).unwrap_or(u32::MAX).max(1);
        unsafe {
            MsgWaitForMultipleObjects(None, false, ms, QS_ALLINPUT);
        }
    }
}

/// Best-effort Windows media integration on a helper thread (mirrors
/// the MPRIS spawn: failures log and the GUI runs regardless).
#[cfg(target_os = "windows")]
pub fn spawn_media_integration(state: SharedPlayer) {
    std::thread::spawn(move || {
        // Window and controls live on this thread: the driver never
        // returns while healthy, so the window outlives the controls.
        let window = match HiddenWindow::new() {
            Ok(window) => window,
            Err(e) => {
                yutu_core::app_log::error("windows-media", format_args!("hidden window: {e}"));
                return;
            }
        };
        let hwnd = window.handle.0 as *mut std::ffi::c_void;
        crate::media::drive(state, Some(hwnd), "windows-media", wait_pumping);
        drop(window);
    });
}

/// Non-Windows builds have no driver here (the shared state still
/// exists for tests, but nothing drives it from this module).
#[cfg(not(target_os = "windows"))]
pub fn spawn_media_integration(_state: SharedPlayer) {}
