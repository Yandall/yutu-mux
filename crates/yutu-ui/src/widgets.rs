//! yutu-ui widgets: split from lib.rs (no behavior change).

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

/// Button label: one step bolder than body text (Medium), so labels
/// hold up at small sizes on pills and rows. Returns the `Text`
/// widget (not an element), so per-site chains like color, width,
/// and wrapping keep working after the call.
pub(crate) fn btn_text<'a>(
    label: impl iced::widget::text::IntoFragment<'a>,
    size: impl Into<iced::Pixels>,
) -> iced::widget::Text<'a, iced::Theme, iced::Renderer> {
    use iced::widget::text;
    text(label).size(size).font(medium_font())
}

/// Heart with hollow/filled states: outline when inactive, solid when
/// active. The filled glyph is the verbatim Lucide heart with its
/// `fill="none"` flipped at runtime (Lucide ships no filled heart), so
/// no custom drawing and no new asset file. Both states use the
/// button-weight stroke (see [`btn_icon`]).
pub(crate) fn heart_icon(
    liked: bool,
    size: f32,
    color: iced::Color,
) -> iced::Element<'static, Message> {
    use iced::widget::svg;
    if !liked {
        return btn_icon("heart", size, color);
    }
    pub(crate) const HEART: &str = include_str!("../assets/icons/heart.svg");
    // Parsed once: every liked row shares the handle.
    pub(crate) static FILLED_HEART: std::sync::LazyLock<svg::Handle> =
        std::sync::LazyLock::new(|| {
            let filled: Vec<u8> = HEART
                .replace("fill=\"none\"", "fill=\"currentColor\"")
                .replace("stroke-width=\"2\"", "stroke-width=\"2.5\"")
                .into_bytes();
            svg::Handle::from_memory(filled)
        });
    svg(FILLED_HEART.clone())
        .width(size)
        .height(size)
        .style(move |_: &iced::Theme, _| svg::Style { color: Some(color) })
        .into()
}

/// Thumbs-down with hollow/filled states (same fill-flip as
/// [`heart_icon`]): outline when the track is not disliked, solid when
/// "don't recommend" is active.
pub(crate) fn thumbs_down_icon(
    disliked: bool,
    size: f32,
    color: iced::Color,
) -> iced::Element<'static, Message> {
    use iced::widget::svg;
    if !disliked {
        return btn_icon("thumbs-down", size, color);
    }
    pub(crate) const THUMBS_DOWN: &str = include_str!("../assets/icons/thumbs-down.svg");
    pub(crate) static FILLED_THUMBS_DOWN: std::sync::LazyLock<svg::Handle> =
        std::sync::LazyLock::new(|| {
            let filled: Vec<u8> = THUMBS_DOWN
                .replace("fill=\"none\"", "fill=\"currentColor\"")
                .replace("stroke-width=\"2\"", "stroke-width=\"2.5\"")
                .into_bytes();
            svg::Handle::from_memory(filled)
        });
    svg(FILLED_THUMBS_DOWN.clone())
        .width(size)
        .height(size)
        .style(move |_: &iced::Theme, _| svg::Style { color: Some(color) })
        .into()
}

/// Rounded text input (search bar pill, filter fields): dark fill, soft
/// border, comfortable radius. One style for every input in the app.
pub(crate) fn rounded_input(
    _: &iced::Theme,
    _: iced::widget::text_input::Status,
) -> iced::widget::text_input::Style {
    iced::widget::text_input::Style {
        background: iced::Background::Color(iced::Color::from_rgb8(0x1E, 0x1E, 0x1E)),
        border: iced::Border {
            color: iced::Color::from_rgb8(0x3A, 0x3A, 0x3A),
            width: 1.0,
            radius: 14.0.into(),
        },
        icon: MUTED,
        placeholder: MUTED,
        value: iced::Color::WHITE,
        selection: ACCENT,
    }
}

/// File-picker field: looks like a text input (dark fill, soft border)
/// but opens the system dialog on press. The hover border lightens so it
/// never reads as static text, and the red Import CTA stays distinct.
#[cfg(feature = "innertube")]
pub(crate) fn file_pick(
    _: &iced::Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let hovered = matches!(status, iced::widget::button::Status::Hovered);
    iced::widget::button::Style {
        background: Some(iced::Background::Color(iced::Color::from_rgb8(
            0x1E, 0x1E, 0x1E,
        ))),
        text_color: iced::Color::WHITE,
        border: iced::Border {
            color: if hovered {
                iced::Color::from_rgb8(0x6E, 0x6E, 0x6E)
            } else {
                iced::Color::from_rgb8(0x3A, 0x3A, 0x3A)
            },
            width: 1.0,
            radius: 14.0.into(),
        },
        ..Default::default()
    }
}

/// Shared cap for Settings inputs (text fields, pickers, sliders, the
/// cookie picker row): full width on narrow windows, aligned cap on wide
/// ones so no field stretches across the whole pane.
pub(crate) const SETTINGS_INPUT_W: f32 = 560.0;

/// Cap a Settings input at the shared width (pass-through element).
pub(crate) fn settings_input(content: iced::Element<'_, Message>) -> iced::Element<'_, Message> {
    iced::widget::container(content)
        .width(iced::Fill)
        .max_width(SETTINGS_INPUT_W)
        .into()
}

/// Centered fixed box for control glyphs: iced buttons hug content
/// (top-left placed, never centered), so uneven glyphs and badge text
/// read as off-center hover frames. Every box is the same size, so a
/// control row lines up instead of hugging each glyph.
pub(crate) fn ctrl_box(content: iced::Element<'_, Message>) -> iced::Element<'_, Message> {
    use iced::widget::container;
    container(content)
        .width(iced::Length::Fixed(18.0))
        .height(iced::Length::Fixed(18.0))
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center)
        .into()
}

/// Repeat transport button: icon only when `badge` is empty (Repeat
/// Off), icon plus badge text otherwise. An empty badge must not keep
/// its fixed-width box: the reserved space pushed the icon left of the
/// button center and widened the whole transport row, so Play sat left
/// of center. Collapsing the box when empty keeps every transport
/// button the same 18px content width and Play truly centered.
pub(crate) fn repeat_button<'a>(
    repeat_icon: &'a str,
    badge: &'a str,
    pad: impl Into<iced::Padding>,
) -> iced::Element<'a, Message> {
    use iced::widget::{button, container, row};
    if badge.is_empty() {
        button(ctrl_box(btn_icon(repeat_icon, 14.0, iced::Color::WHITE)))
            .style(hover_row)
            .on_press(Message::CycleRepeat)
            .padding(pad)
            .into()
    } else {
        button(
            row![
                ctrl_box(btn_icon(repeat_icon, 14.0, iced::Color::WHITE)),
                container(btn_text(badge, 11))
                    .width(iced::Length::Fixed(24.0))
                    .align_x(iced::alignment::Horizontal::Center),
            ]
            .spacing(3)
            .align_y(iced::alignment::Vertical::Center),
        )
        .style(hover_row)
        .on_press(Message::CycleRepeat)
        .padding(pad)
        .into()
    }
}

/// Flow preset names into rows that fit `body_w` (like flex-wrap):
/// greedy fill per row, so a short preset joins the first row even when
/// the full strip cannot. Every row holds at least one segment, and an
/// empty input yields no rows. The page scrolls vertically only, so a
/// strip must never clip.
pub(crate) fn eq_flow_rows(names: &[String], body_w: f32) -> Vec<Vec<String>> {
    pub(crate) const SEG_PAD: f32 = 16.0;
    pub(crate) const GAP: f32 = 2.0;
    pub(crate) const GROUP_PAD: f32 = 8.0;
    pub(crate) const PX_PER_CHAR: f32 = 7.0;
    let inner = (body_w - GROUP_PAD).max(0.0);
    let mut rows: Vec<Vec<String>> = vec![Vec::new()];
    let mut used = 0.0;
    for n in names {
        let w = n.chars().count() as f32 * PX_PER_CHAR + SEG_PAD;
        let cur_empty = rows.last().is_none_or(|r| r.is_empty());
        if !cur_empty && used + GAP + w > inner {
            rows.push(Vec::new());
            used = 0.0;
        }
        // Trailing gap overcounts by 2px: conservative, breaks earlier.
        used += w + GAP;
        rows.last_mut().expect("at least one row").push(n.clone());
    }
    rows.into_iter().filter(|r| !r.is_empty()).collect()
}

/// Current local hour via `localtime_r` (libc, always present on Linux
/// and macOS): std only, no chrono. Falls back to 12 (afternoon) when the
/// clock is unreadable.
pub(crate) fn local_hour() -> u8 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as std::os::raw::c_long)
        .unwrap_or(0);
    #[repr(C, align(8))]
    struct LocalTm {
        tm_sec: i32,
        tm_min: i32,
        tm_hour: i32,
        _rest: [u8; 64],
    }
    extern "C" {
        pub(crate) fn localtime_r(
            timep: *const std::os::raw::c_long,
            result: *mut LocalTm,
        ) -> *mut LocalTm;
    }
    let mut tm = LocalTm {
        tm_sec: 0,
        tm_min: 0,
        tm_hour: 12,
        _rest: [0; 64],
    };
    let ok = unsafe { !localtime_r(&secs, &mut tm).is_null() };
    if ok {
        tm.tm_hour.clamp(0, 23) as u8
    } else {
        12
    }
}

/// Slider in theme red: red filled rail, pale track, white round handle.
pub(crate) fn themed_slider(
    _: &iced::Theme,
    _: iced::widget::slider::Status,
) -> iced::widget::slider::Style {
    iced::widget::slider::Style {
        rail: iced::widget::slider::Rail {
            backgrounds: (
                iced::Background::Color(ACCENT),
                iced::Background::Color(iced::Color::from_rgb8(0x3A, 0x3A, 0x3A)),
            ),
            width: 4.0,
            border: iced::Border::default().rounded(2),
        },
        handle: iced::widget::slider::Handle {
            shape: iced::widget::slider::HandleShape::Circle { radius: 8.0 },
            background: iced::Background::Color(iced::Color::WHITE),
            border_width: 0.0,
            border_color: iced::Color::TRANSPARENT,
        },
    }
}

/// EQ bypass toggle in theme red: red track when on, dark track when
/// off, white knob either way (glyphs on red are always white).
pub(crate) fn red_toggle(
    _: &iced::Theme,
    status: iced::widget::toggler::Status,
) -> iced::widget::toggler::Style {
    use iced::widget::toggler::Status;
    let on = match status {
        Status::Active { is_toggled }
        | Status::Hovered { is_toggled }
        | Status::Disabled { is_toggled } => is_toggled,
    };
    iced::widget::toggler::Style {
        background: iced::Background::Color(if on {
            ACCENT
        } else {
            iced::Color::from_rgb8(0x3A, 0x3A, 0x3A)
        }),
        background_border_width: 0.0,
        background_border_color: iced::Color::TRANSPARENT,
        foreground: iced::Background::Color(iced::Color::WHITE),
        foreground_border_width: 0.0,
        foreground_border_color: iced::Color::TRANSPARENT,
        text_color: None,
        border_radius: None,
        padding_ratio: 0.1,
    }
}

/// Import progress bar in theme red: red fill on a dark track.
#[cfg(feature = "innertube")]
pub(crate) fn import_bar(_: &iced::Theme) -> iced::widget::progress_bar::Style {
    iced::widget::progress_bar::Style {
        background: iced::Background::Color(iced::Color::from_rgb8(0x3A, 0x3A, 0x3A)),
        bar: iced::Background::Color(ACCENT),
        border: iced::Border::default().rounded(4),
    }
}

/// Row action button: transparent at rest, soft card hover with rounded
/// corners (every clickable row/item gets hover feedback).
/// Browser arrows: always transparent, softly rounded; the icon (not a
/// background) carries the enabled state.
pub(crate) fn nav_arrow(
    _: &iced::Theme,
    _: iced::widget::button::Status,
) -> iced::widget::button::Style {
    iced::widget::button::Style {
        background: None,
        text_color: iced::Color::WHITE,
        border: iced::Border::default().rounded(12),
        ..Default::default()
    }
}

pub(crate) fn hover_row(
    _: &iced::Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let hovered = matches!(
        status,
        iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed
    );
    iced::widget::button::Style {
        background: hovered.then_some(iced::Background::Color(CARD_BG_HOVER)),
        text_color: iced::Color::WHITE,
        border: iced::Border::default().rounded(8),
        ..Default::default()
    }
}

/// Secondary button: visible dark fill + border at rest, so standalone
/// text actions never read as plain text. The red `accent_button` stays
/// the CTA; menus and rows keep `hover_row` (hover feedback only).
pub(crate) fn ghost_button(
    _: &iced::Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let hovered = matches!(
        status,
        iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed
    );
    iced::widget::button::Style {
        background: Some(iced::Background::Color(if hovered {
            iced::Color::from_rgb8(0x33, 0x33, 0x33)
        } else {
            iced::Color::from_rgb8(0x22, 0x22, 0x22)
        })),
        text_color: iced::Color::WHITE,
        border: iced::Border {
            color: if hovered {
                iced::Color::from_rgb8(0x6E, 0x6E, 0x6E)
            } else {
                iced::Color::from_rgb8(0x3A, 0x3A, 0x3A)
            },
            width: 1.0,
            radius: 20.0.into(),
        },
        ..Default::default()
    }
}

/// Segmented group box: one bordered shell holding the EQ preset
/// segments, so the presets read as a single picker.
pub(crate) fn seg_group(_: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(iced::Background::Color(iced::Color::from_rgb8(
            0x16, 0x16, 0x16,
        ))),
        border: iced::Border {
            color: iced::Color::from_rgb8(0x3A, 0x3A, 0x3A),
            width: 1.0,
            radius: 14.0.into(),
        },
        ..Default::default()
    }
}

/// One EQ preset segment: red fill when active, transparent otherwise
/// (hover lifts inactive segments). Glyphs on red are always white.
pub(crate) fn seg_button(
    active: bool,
) -> impl Fn(&iced::Theme, iced::widget::button::Status) -> iced::widget::button::Style {
    move |_: &iced::Theme, status: iced::widget::button::Status| {
        let hovered = matches!(
            status,
            iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed
        );
        iced::widget::button::Style {
            background: if active {
                Some(iced::Background::Color(if hovered {
                    ACCENT_HOVER
                } else {
                    ACCENT
                }))
            } else if hovered {
                Some(iced::Background::Color(CARD_BG_HOVER))
            } else {
                None
            },
            text_color: iced::Color::WHITE,
            border: iced::Border::default().rounded(10),
            ..Default::default()
        }
    }
}

/// Latched-selected row (flyout toggle while its submenu is open).
pub(crate) fn selected_row(
    _: &iced::Theme,
    _: iced::widget::button::Status,
) -> iced::widget::button::Style {
    iced::widget::button::Style {
        background: Some(iced::Background::Color(CARD_BG_HOVER)),
        text_color: iced::Color::WHITE,
        border: iced::Border::default().rounded(8),
        ..Default::default()
    }
}

/// Card-as-button (playlist / mix tiles).
pub(crate) fn card_button(
    _: &iced::Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let bg = match status {
        iced::widget::button::Status::Hovered => CARD_BG_HOVER,
        _ => CARD_BG,
    };
    iced::widget::button::Style {
        background: Some(iced::Background::Color(bg)),
        text_color: iced::Color::WHITE,
        border: iced::Border::default().rounded(8),
        ..Default::default()
    }
}

/// Ghost chip: the held block while dragging (card + accent border,
/// deliberately more compact than a full row).
/// Accent bar marking the armed insertion slot.
pub(crate) fn drop_bar(_: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(iced::Background::Color(ACCENT)),
        border: iced::Border::default().rounded(2),
        ..Default::default()
    }
}

/// Hairline divider between queue rows (visible on dark and hover).
pub(crate) fn hairline(_: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(iced::Background::Color(NAV_ACTIVE_BG)),
        ..Default::default()
    }
}

pub(crate) fn hairline_rule() -> iced::Element<'static, Message> {
    iced::widget::container(
        iced::widget::container(
            iced::widget::Space::new()
                .height(iced::Length::Fixed(1.0))
                .width(iced::Length::Fill),
        )
        .style(hairline),
    )
    // Total layout 1 plus 2 top plus 2 bottom (5px); only the inner
    // 1px paints, the rest is transparent breathing room.
    .padding(iced::Padding {
        top: 2.0,
        bottom: 2.0,
        left: 0.0,
        right: 0.0,
    })
    .into()
}

/// Interleave hairline dividers between rows (never trailing).
pub(crate) fn with_dividers<'a>(
    rows: Vec<iced::Element<'a, Message>>,
) -> Vec<iced::Element<'a, Message>> {
    let mut out = Vec::with_capacity(rows.len() * 2);
    let mut first = true;
    for row in rows {
        if !first {
            out.push(hairline_rule());
        }
        first = false;
        out.push(row);
    }
    out
}
/// Row click target: transparent in every state (the row container owns
/// the hover highlight, so fast moves never leave a title-only pill).
pub(crate) fn row_click(
    _: &iced::Theme,
    _: iced::widget::button::Status,
) -> iced::widget::button::Style {
    iced::widget::button::Style {
        background: None,
        text_color: iced::Color::WHITE,
        border: iced::Border::default().rounded(8),
        ..Default::default()
    }
}
/// Full-row hover background: transparent at rest, soft card highlight when
/// the cursor is anywhere over the row (the row container owns this; the
/// inner buttons keep their own `hover_row` press targets).
pub(crate) fn row_bg(hovered: bool) -> impl Fn(&iced::Theme) -> iced::widget::container::Style {
    move |_: &iced::Theme| iced::widget::container::Style {
        background: Some(iced::Background::Color(if hovered {
            CARD_BG_HOVER
        } else {
            iced::Color::TRANSPARENT
        })),
        border: iced::Border::default().rounded(8),
        ..Default::default()
    }
}
/// One context-menu row: icon + label, full-width hover press target.
pub(crate) fn menu_item(
    icon_name: &'static str,
    label: impl Into<String>,
    msg: Message,
) -> iced::Element<'static, Message> {
    use iced::widget::row;
    use iced::Fill;
    row_button(
        row![
            btn_icon(icon_name, 14.0, iced::Color::WHITE),
            btn_text(label.into(), 14),
        ]
        .spacing(8)
        .align_y(iced::alignment::Vertical::Center)
        .width(Fill),
    )
    .style(hover_row)
    .on_press(msg)
    .padding(8)
    .width(Fill)
    .into()
}

/// Dim backdrop over art for the hover play overlay: darkens the
/// image so the white play icon reads on top.
pub(crate) fn art_dim(_: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(iced::Background::Color(iced::Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.45,
        })),
        ..Default::default()
    }
}

/// Row art size (matches `thumb`) + hidden action width (icon 15 +
/// padding 6 each side): spacers keep rows steady when actions hide.
pub(crate) const ROW_ART: f32 = 40.0;
pub(crate) const ROW_ACTION_W: f32 = 27.0;
