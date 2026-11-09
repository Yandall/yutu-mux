//! yutu-ui icons: split from lib.rs (no behavior change).

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

/// Lucide icon (ISC, pinned 0.469.0, `assets/icons/*.svg`), tinted to
/// `color`. Lucide strokes use `currentColor`, so single-color tinting
/// reproduces the design palette at any size.
/// Raw bytes for a Lucide icon (shared by `icon` and `btn_icon`).
pub(crate) fn icon_bytes(name: &str) -> &'static [u8] {
    match name {
        "play" => include_bytes!("../assets/icons/play.svg"),
        "pause" => include_bytes!("../assets/icons/pause.svg"),
        "skip-back" => include_bytes!("../assets/icons/skip-back.svg"),
        "skip-forward" => include_bytes!("../assets/icons/skip-forward.svg"),
        "shuffle" => include_bytes!("../assets/icons/shuffle.svg"),
        "repeat" => include_bytes!("../assets/icons/repeat.svg"),
        "repeat-1" => include_bytes!("../assets/icons/repeat-1.svg"),
        "heart" => include_bytes!("../assets/icons/heart.svg"),
        "search" => include_bytes!("../assets/icons/search.svg"),
        "house" => include_bytes!("../assets/icons/house.svg"),
        "library" => include_bytes!("../assets/icons/library.svg"),
        "plus" => include_bytes!("../assets/icons/plus.svg"),
        "x" => include_bytes!("../assets/icons/x.svg"),
        "volume" => include_bytes!("../assets/icons/volume-2.svg"),
        "volume-x" => include_bytes!("../assets/icons/volume-x.svg"),
        "mic" => include_bytes!("../assets/icons/mic.svg"),
        "user-round" => include_bytes!("../assets/icons/user-round.svg"),
        "radio" => include_bytes!("../assets/icons/radio.svg"),
        "settings" => include_bytes!("../assets/icons/settings.svg"),
        "queue" => include_bytes!("../assets/icons/list-music.svg"),
        "chevron-up" => include_bytes!("../assets/icons/chevron-up.svg"),
        "chevron-down" => include_bytes!("../assets/icons/chevron-down.svg"),
        "chevron-left" => include_bytes!("../assets/icons/chevron-left.svg"),
        "chevron-right" => include_bytes!("../assets/icons/chevron-right.svg"),
        "arrow-up" => include_bytes!("../assets/icons/arrow-up.svg"),
        "arrow-down" => include_bytes!("../assets/icons/arrow-down.svg"),
        "trash" => include_bytes!("../assets/icons/trash-2.svg"),
        "ellipsis" => include_bytes!("../assets/icons/ellipsis.svg"),
        "grip" => include_bytes!("../assets/icons/grip-vertical.svg"),
        "music" => include_bytes!("../assets/icons/music.svg"),
        "disc" => include_bytes!("../assets/icons/disc.svg"),
        "folder" => include_bytes!("../assets/icons/folder.svg"),
        "list-plus" => include_bytes!("../assets/icons/list-plus.svg"),
        "sparkles" => include_bytes!("../assets/icons/sparkles.svg"),
        "panel-left" => include_bytes!("../assets/icons/panel-left.svg"),
        "panel-right" => include_bytes!("../assets/icons/panel-right.svg"),
        "loader" => include_bytes!("../assets/icons/loader-circle.svg"),
        "thumbs-down" => include_bytes!("../assets/icons/thumbs-down.svg"),
        _ => include_bytes!("../assets/icons/music.svg"),
    }
}

pub(crate) fn icon(name: &str, size: f32, color: iced::Color) -> iced::Element<'static, Message> {
    use iced::widget::svg;
    svg(svg::Handle::from_memory(icon_bytes(name)))
        .width(size)
        .height(size)
        .style(move |_: &iced::Theme, _| svg::Style { color: Some(color) })
        .into()
}

/// Blank 24x24 frame (no paths) backing the idle remove slot: paints
/// zero pixels in any renderer while holding the exact armed box, so
/// rows never shift on hover. A transparent-tinted x is NOT a blank:
/// iced's svg tint keeps the source alpha and replaces RGB, so
/// TRANSPARENT (rgb 0,0,0) paints a solid black x on both the wgpu and
/// tiny-skia backends.
pub(crate) const EMPTY_SLOT_SVG: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"24\" height=\"24\" viewBox=\"0 0 24 24\" fill=\"none\"></svg>";

/// App logo mark (rings + M, own colors): shown beside the sidebar
/// wordmark. Unlike `icon`/`btn_icon`, no tint is applied: the style
/// color would repaint the red rings and white M.
pub(crate) fn logo_mark(size: f32) -> iced::Element<'static, Message> {
    use iced::widget::svg;
    pub(crate) static LOGO_HANDLE: std::sync::LazyLock<svg::Handle> =
        std::sync::LazyLock::new(|| {
            svg::Handle::from_memory(include_bytes!("../assets/logo.svg").as_slice())
        });
    svg(LOGO_HANDLE.clone())
        .width(size)
        .height(size)
        .style(|_: &iced::Theme, _| svg::Style { color: None })
        .into()
}

/// Button-weight stroke (2 to 2.5) applied to verbatim Lucide bytes.
/// Pure helper so tests pin the surgery (see `button_helpers_build`).
pub(crate) fn thicken_svg(bytes: &[u8]) -> Vec<u8> {
    String::from_utf8_lossy(bytes)
        .replace("stroke-width=\"2\"", "stroke-width=\"2.5\"")
        .into_bytes()
}

/// Optical centering for the play triangle (runtime surgery, the asset
/// stays verbatim): Lucide's play spans 5-19 (geometric center 12,
/// like pause) but its centroid sits ~2.3px left (triangle mass is
/// left-heavy), so it reads as shifted left in the red circle. Shifting
/// the polygon +2 right puts the visual mass on center (centroid
/// ~11.7, within 0.3 of 12). Same fill-flip pattern as [`heart_icon`].
pub(crate) fn optical_play(bytes: Vec<u8>) -> Vec<u8> {
    String::from_utf8_lossy(&bytes)
        .replace(
            "points=\"5 3 19 12 5 21 5 3\"",
            "points=\"7 3 21 12 7 21 7 3\"",
        )
        .into_bytes()
}

/// Button icon: the verbatim Lucide glyph with a slightly heavier
/// stroke (2 to 2.5), matching the bolder button labels. Runtime
/// surgery like the heart fill-flip; thickened handles are cached
/// per name so views never reparse.
pub(crate) fn btn_icon(
    name: &str,
    size: f32,
    color: iced::Color,
) -> iced::Element<'static, Message> {
    use iced::widget::svg;
    pub(crate) static THICK_ICONS: std::sync::LazyLock<
        std::sync::Mutex<std::collections::HashMap<String, svg::Handle>>,
    > = std::sync::LazyLock::new(|| {
        std::sync::Mutex::new(std::collections::HashMap::<String, svg::Handle>::new())
    });
    let handle = THICK_ICONS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(name.to_owned())
        .or_insert_with_key(|key| {
            let thick = thicken_svg(icon_bytes(key));
            let bytes = if key == "play" {
                optical_play(thick)
            } else {
                thick
            };
            svg::Handle::from_memory(bytes)
        })
        .clone();
    svg(handle)
        .width(size)
        .height(size)
        .style(move |_: &iced::Theme, _| svg::Style { color: Some(color) })
        .into()
}
