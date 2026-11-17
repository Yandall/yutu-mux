//! Windows window tweaks for the mini player.
//!
//! Rules:
//! - Windows only (the module is target-gated in `lib.rs`).
//! - The mini window is opaque there (iced's DX12 surface cannot be
//!   transparent), so it is the capsule itself. Windows 11 draws its
//!   corners rounded when asked through DWM; earlier versions ignore
//!   the request (square corners, no error shown to the user).

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
    DWM_WINDOW_CORNER_PREFERENCE,
};

/// Ask DWM for rounded corners on this window. Returns whether DWM
/// accepted it (false before Windows 11).
pub fn round_corners<W: HasWindowHandle + ?Sized>(window: &W) -> bool {
    let Ok(handle) = window.window_handle() else {
        return false;
    };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return false;
    };
    let hwnd = HWND(win32.hwnd.get());
    let pref: DWM_WINDOW_CORNER_PREFERENCE = DWMWCP_ROUND;
    // SAFETY: `hwnd` is a live window for as long as the handle is
    // borrowed; the attribute buffer is a valid value of its size.
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            (&pref as *const DWM_WINDOW_CORNER_PREFERENCE).cast(),
            std::mem::size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
        )
        .is_ok()
    }
}
