//! yutu-ui theme: split from lib.rs (no behavior change).

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

pub(crate) const ACCENT: iced::Color = iced::Color::from_rgb8(0xFF, 0x00, 0x33);
pub(crate) const ACCENT_HOVER: iced::Color = iced::Color::from_rgb8(0xFF, 0x33, 0x66);
pub(crate) const MUTED: iced::Color = iced::Color::from_rgb8(0xB3, 0xB3, 0xB3);
pub(crate) const CARD_BG: iced::Color = iced::Color::from_rgb8(0x18, 0x18, 0x18);
pub(crate) const CARD_BG_HOVER: iced::Color = iced::Color::from_rgb8(0x28, 0x28, 0x28);
pub(crate) const SIDEBAR_BG: iced::Color = iced::Color::from_rgb8(0x12, 0x12, 0x12);
pub(crate) const APP_BG: iced::Color = iced::Color::from_rgb8(0x0A, 0x0A, 0x0A);
pub(crate) const NAV_ACTIVE_BG: iced::Color = iced::Color::from_rgb8(0x2A, 0x2A, 0x2A);

pub(crate) fn app_bg(_: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(iced::Background::Color(APP_BG)),
        ..Default::default()
    }
}

pub(crate) fn sidebar_bg(_: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(iced::Background::Color(SIDEBAR_BG)),
        border: iced::Border::default().rounded(12),
        ..Default::default()
    }
}

pub(crate) fn card_bg(_: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(iced::Background::Color(CARD_BG)),
        border: iced::Border::default().rounded(8),
        ..Default::default()
    }
}

/// Modal backdrop: dims the app behind the dialog so it stands out.
/// Alpha-only (no layout), same overlay idiom as `art_dim`.
pub(crate) fn modal_dim(_: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(iced::Background::Color(iced::Color::from_rgba(
            0.0, 0.0, 0.0, 0.55,
        ))),
        ..Default::default()
    }
}

/// Collapsed rail thumb wrapper: red border for the live song, transparent
/// same-size border otherwise so the pitch never shifts on highlight.
pub(crate) fn rail_thumb_bg(
    current: bool,
) -> impl Fn(&iced::Theme) -> iced::widget::container::Style {
    move |_| iced::widget::container::Style {
        background: Some(iced::Background::Color(CARD_BG)),
        border: iced::Border {
            color: if current {
                ACCENT
            } else {
                iced::Color::TRANSPARENT
            },
            width: 2.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    }
}

/// Collapsed rail window: slice [start, end) of at most `capacity` items
/// keeping the live song pinned. Rules: first song sits 1st (no previous
/// to show), otherwise the live song stays 2nd while there is room to
/// scroll, and sinks to 3rd+ only when the tail cannot fill below it.
/// Pure helper so unit tests pin the anchoring without a window.
pub(crate) fn collapsed_rail_window(
    pos: Option<usize>,
    len: usize,
    capacity: usize,
) -> (usize, usize) {
    if len == 0 || capacity == 0 {
        return (0, 0);
    }
    let cap = capacity.min(len);
    if cap >= len {
        return (0, len);
    }
    let max_start = len - cap;
    let start = match pos {
        None => 0,
        Some(0) => 0,
        Some(p) => (p.saturating_sub(1)).min(max_start),
    };
    (start, start + cap)
}

/// Slim floating scrollbar shared by every scrollable (4px). Fully
/// transparent until the list is hovered, dragged, or recently
/// scrolled: Iced themes scrollbars by interaction status, and scroll
/// motion lights a per-list fade that the ticker eases out (see
/// `scroll_hint`), which is the available transition mechanism.
/// Floating scrollbars paint over content, so hiding never takes
/// layout space and revealing shifts nothing.
pub(crate) fn slim_bar() -> iced::widget::scrollable::Scrollbar {
    iced::widget::scrollable::Scrollbar::new()
        .width(4.0)
        .scroller_width(4.0)
        .margin(2.0)
}

/// Slim scrollbar style with a travelling scroll-fade opacity 0..1
/// from the ticking `scroll_hint` map: hover/drag always show, scroll
/// motion shows then eases back to invisible.
pub(crate) fn slim_scroll_alpha(
    theme: &iced::Theme,
    status: iced::widget::scrollable::Status,
    alpha: f32,
) -> iced::widget::scrollable::Style {
    use iced::widget::scrollable::Status;
    let mut base = iced::widget::scrollable::default(theme, status);
    let a = if matches!(status, Status::Active { .. }) {
        0.4 * alpha.clamp(0.0, 1.0)
    } else {
        0.4
    };
    let rail = iced::widget::scrollable::Rail {
        background: None,
        border: iced::Border::default(),
        scroller: iced::widget::scrollable::Scroller {
            background: iced::Background::Color(iced::Color::from_rgba(1.0, 1.0, 1.0, a)),
            border: iced::Border::default().rounded(2),
        },
    };
    base.vertical_rail = rail;
    base.horizontal_rail = rail;
    base
}

/// Search pill: the rounded-input look for the container that wraps the
/// icon + borderless input (the icon sits visually inside the field).
pub(crate) fn search_pill(_: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(iced::Background::Color(iced::Color::from_rgb8(
            0x1E, 0x1E, 0x1E,
        ))),
        border: iced::Border {
            color: iced::Color::from_rgb8(0x3A, 0x3A, 0x3A),
            width: 1.0,
            radius: 14.0.into(),
        },
        ..Default::default()
    }
}

/// Borderless input for inside the search pill (the pill owns the
/// background + border, so the field itself is transparent).
pub(crate) fn search_inner_input(
    _: &iced::Theme,
    _: iced::widget::text_input::Status,
) -> iced::widget::text_input::Style {
    iced::widget::text_input::Style {
        background: iced::Background::Color(iced::Color::TRANSPARENT),
        border: iced::Border::default(),
        icon: MUTED,
        placeholder: MUTED,
        value: iced::Color::WHITE,
        selection: ACCENT,
    }
}

pub(crate) fn player_bg(_: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(iced::Background::Color(SIDEBAR_BG)),
        border: iced::Border::default().rounded(8),
        ..Default::default()
    }
}

/// Accent circular primary action (red CTA button).
pub(crate) fn accent_button(
    _: &iced::Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let bg = match status {
        iced::widget::button::Status::Hovered => ACCENT_HOVER,
        _ => ACCENT,
    };
    iced::widget::button::Style {
        background: Some(iced::Background::Color(bg)),
        text_color: iced::Color::WHITE,
        border: iced::Border::default().rounded(20),
        ..Default::default()
    }
}

/// Sidebar nav: active gets a pill background, inactive is transparent.
pub(crate) fn nav_style(
    active: bool,
) -> impl Fn(&iced::Theme, iced::widget::button::Status) -> iced::widget::button::Style {
    move |_: &iced::Theme, status: iced::widget::button::Status| {
        let hovered = matches!(
            status,
            iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed
        );
        let background = if active || hovered {
            Some(iced::Background::Color(NAV_ACTIVE_BG))
        } else {
            None
        };
        let text_color = if active || hovered {
            iced::Color::WHITE
        } else {
            MUTED
        };
        iced::widget::button::Style {
            background,
            text_color,
            border: iced::Border::default().rounded(10),
            ..Default::default()
        }
    }
}
