//! yutu-ui app: split from lib.rs (no behavior change).

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use pointing_slider::pointing_slider;
#[allow(unused_imports)]
use row_button::row_button;
#[allow(unused_imports)]
use std::collections::{HashMap, HashSet};
#[allow(unused_imports)]
use yutu_core::{
    forgotten_favorites, history_section, should_scrobble, weekly_mix, Album, Artist, HistoryEntry,
    HistorySection, LibraryStore, LyricsProvider, LyricsResult, Playlist, PlaylistHit,
    QueueSnapshot, SearchAll, SearchResults, TopKind, Track,
};
#[cfg(feature = "mpv")]
#[allow(unused_imports)]
use yutu_player::MpvBackend;
#[allow(unused_imports)]
use yutu_player::{EqPreset, Queue, RodioBackend, BAND_COUNT, MAX_GAIN_DB, MIN_GAIN_DB};

/// Window identity for taskbar/dock matching: the application id must
/// equal the .desktop basename (`yutu-mux`) so the running window picks
/// up the installed icon on X11 (WM_CLASS) and Wayland (app_id). Iced
/// leaves it empty by default, which strands Wayland windows with a
/// generic icon. Other platforms keep the default settings.
pub(crate) fn window_settings() -> iced::window::Settings {
    // Title-bar/taskbar icon (the Windows caption row, X11 hints): the
    // exe-embedded .ico only brands Explorer entries, the live window
    // needs its own icon or the OS shows the generic placeholder.
    // Shared source with the Linux icon theme entries (no forked art);
    // a corrupt PNG falls back to the OS default icon, never a panic.
    // Format is guessed from the PNG magic bytes, so no extra decoder
    // dependency is needed.
    let icon = iced::window::icon::from_file_data(
        include_bytes!("../../../packaging/linux/icons/hicolor/64x64/apps/yutu-mux.png"),
        None,
    )
    .ok();
    #[cfg(target_os = "linux")]
    {
        iced::window::Settings {
            icon,
            platform_specific: iced::window::settings::PlatformSpecific {
                application_id: "yutu-mux".into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        iced::window::Settings {
            icon,
            ..Default::default()
        }
    }
}

pub fn run(mpris: Option<yutu_desktop::SharedPlayer>) -> iced::Result {
    // Display startup: the bundled Adwaita Sans variable face always
    // registers (the System pick just leaves it unused) and the stored
    // family picks the renderer default. Scale reads live state, so
    // the slider applies instantly; the family is startup-fixed.
    let startup_family = LibraryStore::open(&default_library_path())
        .ok()
        .and_then(|store| store.get_pref("font_family").unwrap_or(None))
        .map(|raw| FontFamily::parse(&raw))
        .unwrap_or(FontFamily::Adwaita);
    let _ = EFFECTIVE_FONT.set(startup_family);
    iced::application(
        move || {
            let (mut state, task) = AppState::new();
            state.mpris = mpris.clone();
            state.publish_mpris();
            (state, task)
        },
        AppState::update,
        AppState::view,
    )
    .title(|state: &AppState| state.window_title())
    .window(window_settings())
    .font(include_bytes!("../assets/fonts/AdwaitaSans-Regular.ttf").as_slice())
    .default_font(startup_family.to_iced_font())
    .scale_factor(|state: &AppState| clamp_scale(state.ui_scale))
    .theme(AppState::theme)
    .subscription(AppState::subscription)
    .run()
}
