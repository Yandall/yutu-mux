use std::sync::{Arc, Mutex};
use yutu_desktop::mpris::{MprisPlayer, PlaybackStatus, SharedPlayer};
use yutu_desktop::NowPlaying;

/// Best-effort MPRIS advertisement on a helper thread. Any failure
/// (headless, no bus) is a logged no-op: the GUI runs regardless.
/// The shared handle is also handed to the UI, which publishes the current
/// track/status/volume on every state change, so external clients
/// (`playerctl`, desktop widgets) observe live updates on property read.
/// `PropertiesChanged` signals on mutation are the remaining follow-up.
fn spawn_mpris(state: SharedPlayer) {
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                eprintln!("mpris: no async runtime: {e}");
                return;
            }
        };
        let conn = match rt.block_on(yutu_desktop::mpris::serve(state.clone())) {
            Ok(conn) => conn,
            Err(e) => {
                eprintln!("mpris unavailable: {e}");
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
    let state: SharedPlayer = Arc::new(Mutex::new(MprisPlayer::new(
        NowPlaying::default(),
        PlaybackStatus::Stopped,
        0.8,
    )));
    spawn_mpris(Arc::clone(&state));
    yutu_ui::run(Some(state))
}
