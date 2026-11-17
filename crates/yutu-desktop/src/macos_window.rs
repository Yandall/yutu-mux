//! macOS window tweaks for the mini player.
//!
//! Rules:
//! - macOS only (the module is target-gated in `lib.rs`).
//! - wgpu draws into a `CAMetalLayer` sublayer of the window's view and
//!   leaves Core Animation's default `contentsGravity` (resize). While
//!   the window resizes, macOS then stretches the last frame to the new
//!   size until the app draws at the new size: the mini player's
//!   lyrics toggle flickers as if its content stretched vertically.
//!   Pinning the layers' contents to the top-left keeps the last frame
//!   unscaled instead (wgpu-hal itself keeps this line for "debugging
//!   resize issues").
//! - Must run on the main thread (AppKit), e.g. inside
//!   `iced::window::run`.

use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_quartz_core::{kCAGravityTopLeft, CALayer};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// Pin the window view's layer and its sublayers (wgpu's Metal layer)
/// to top-left contents gravity. Returns whether a view was found.
pub fn pin_contents_top_left<W: HasWindowHandle + ?Sized>(window: &W) -> bool {
    let Ok(handle) = window.window_handle() else {
        return false;
    };
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return false;
    };
    let view = appkit.ns_view.as_ptr().cast::<AnyObject>();
    // SAFETY: `ns_view` is a live NSView for as long as the handle is
    // borrowed, and this runs on the main thread (AppKit's requirement).
    unsafe {
        let layer: Option<Retained<CALayer>> = msg_send![view, layer];
        let Some(layer) = layer else {
            return false;
        };
        layer.setContentsGravity(kCAGravityTopLeft);
        if let Some(subs) = layer.sublayers() {
            for i in 0..subs.count() {
                subs.objectAtIndex(i).setContentsGravity(kCAGravityTopLeft);
            }
        }
    }
    true
}
