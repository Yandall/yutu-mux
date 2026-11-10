use std::sync::{Arc, Mutex};
use yutu_desktop::mpris::{MprisPlayer, PlaybackStatus, SharedPlayer};
use yutu_desktop::NowPlaying;

/// Process-wide allocator: returns freed pages to the OS promptly, so
/// image/audio churn does not linger as retained footprint (the macOS
/// "Memory" column counts retained + compressed pages, not just the
/// resident set in "Real Memory").
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// Best-effort MPRIS advertisement on a helper thread. Any failure
/// (headless, no bus) is a logged no-op: the GUI runs regardless.
/// The shared handle is also handed to the UI, which publishes the current
/// track/status/volume on every state change, so external clients
/// (`playerctl`, desktop widgets) observe live updates on property read.
/// `PropertiesChanged` signals on mutation are the remaining follow-up.
/// macOS uses the Now Playing driver instead: this spawn is Linux-only
/// (Windows has no D-Bus session bus, so MPRIS never applies there).
#[cfg(target_os = "linux")]
fn spawn_mpris(state: SharedPlayer) {
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                yutu_core::app_log::warn("mpris", format_args!("no async runtime: {e}"));
                return;
            }
        };
        let conn = match rt.block_on(yutu_desktop::mpris::serve(state.clone())) {
            Ok(conn) => conn,
            Err(e) => {
                yutu_core::app_log::warn("mpris", format_args!("unavailable: {e}"));
                return;
            }
        };
        // Property-change signals (timeline/track widgets) alongside the
        // connection keeper until the process exits.
        rt.block_on(async {
            let watch = tokio::spawn(yutu_desktop::mpris::watch_and_signal(
                conn.clone(),
                state.clone(),
            ));
            let _ = tokio::signal::ctrl_c().await;
            watch.abort();
        });
    });
}

fn main() -> iced::Result {
    // File logging first (everything below lands in today's log, kept
    // a week): panics are chained into it too, so a wedge or crash
    // always leaves a trail even when stderr is lost (Finder launch).
    yutu_core::app_log::init(&format!("{}/logs", yutu_ui::data_dir()));
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        yutu_core::app_log::log_panic("panic", &info.to_string());
        default_hook(info);
    }));
    let state: SharedPlayer = Arc::new(Mutex::new(MprisPlayer::new(
        NowPlaying::default(),
        PlaybackStatus::Stopped,
        0.8,
    )));
    // Platform media service on a helper thread (MPRIS on Linux,
    // Now Playing + media keys on macOS): failures log, GUI runs on.
    #[cfg(target_os = "macos")]
    yutu_desktop::macos::spawn_media_integration(Arc::clone(&state));
    #[cfg(target_os = "linux")]
    spawn_mpris(Arc::clone(&state));
    // Windows: pin the GPU backend to DX12 unless the user chose one.
    // wgpu's default backend probe hangs on machines with a broken
    // Vulkan/GL stack (no window ever appears, no error either); DX12 is
    // the native API and works where the default does not. Explicit user
    // choice wins: only fill the default when WGPU_BACKEND is unset.
    #[cfg(target_os = "windows")]
    if std::env::var_os("WGPU_BACKEND").is_none() {
        std::env::set_var("WGPU_BACKEND", "dx12");
    }
    yutu_ui::run(Some(state))
}
