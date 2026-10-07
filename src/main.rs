use std::sync::{Arc, Mutex};
use yutu_desktop::mpris::{MprisPlayer, PlaybackStatus, SharedPlayer};
use yutu_desktop::NowPlaying;

/// Best-effort MPRIS advertisement on a helper thread. Any failure
/// (headless, no bus) is a logged no-op — the GUI runs regardless.
/// Live track/status propagation from the UI is the documented follow-up;
// the service advertises identity + initial Stopped state today.
fn spawn_mpris() {
    let state: SharedPlayer = Arc::new(Mutex::new(MprisPlayer::new(
        NowPlaying::default(),
        PlaybackStatus::Stopped,
        0.8,
    )));
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
        let conn = match rt.block_on(yutu_desktop::mpris::serve(state)) {
            Ok(conn) => conn,
            Err(e) => {
                eprintln!("mpris unavailable: {e}");
                return;
            }
        };
        // Keep the connection alive and serviced until the process exits.
        rt.block_on(async {
            let _conn = conn;
            let _ = tokio::signal::ctrl_c().await;
        });
    });
}

fn main() -> iced::Result {
    spawn_mpris();
    yutu_ui::run()
}
