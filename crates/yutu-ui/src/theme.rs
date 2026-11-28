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

/// Semantic theme colors: every view reads its colors from here (via
/// `tk()`), never from a literal, so a theme is just another `Tokens`
/// value. Field meanings live on `yutu_core::theme::Palette` (same
/// names); see `docs/THEMES.md`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Tokens {
    pub window: iced::Color,
    pub panel: iced::Color,
    pub surface: iced::Color,
    pub surface_hover: iced::Color,
    pub surface_active: iced::Color,
    pub inset: iced::Color,
    pub field: iced::Color,
    pub field_hover: iced::Color,
    pub menu: iced::Color,
    pub outline: iced::Color,
    pub outline_strong: iced::Color,
    pub text: iced::Color,
    pub text_body: iced::Color,
    pub text_muted: iced::Color,
    pub text_disabled: iced::Color,
    pub placeholder: iced::Color,
    pub accent: iced::Color,
    pub accent_hover: iced::Color,
    pub on_accent: iced::Color,
    pub handle: iced::Color,
    pub handle_edge: iced::Color,
    pub scrim: iced::Color,
    pub shadow: iced::Color,
    /// Light base (stock widget palette, art veils).
    pub light: bool,
}

/// Theme color to iced color (const so the default tokens stay a const).
pub(crate) const fn to_color(c: yutu_core::theme::Rgba) -> iced::Color {
    iced::Color::from_rgba8(c.r, c.g, c.b, c.a as f32 / 255.0)
}

impl Tokens {
    /// The default look (dark surfaces, vivid red accent).
    pub(crate) const DARK: Tokens = Tokens::from_palette(&yutu_core::theme::Palette::DARK, false);

    pub(crate) const fn from_palette(p: &yutu_core::theme::Palette, light: bool) -> Tokens {
        Tokens {
            window: to_color(p.window),
            panel: to_color(p.panel),
            surface: to_color(p.surface),
            surface_hover: to_color(p.surface_hover),
            surface_active: to_color(p.surface_active),
            inset: to_color(p.inset),
            field: to_color(p.field),
            field_hover: to_color(p.field_hover),
            menu: to_color(p.menu),
            outline: to_color(p.outline),
            outline_strong: to_color(p.outline_strong),
            text: to_color(p.text),
            text_body: to_color(p.text_body),
            text_muted: to_color(p.text_muted),
            text_disabled: to_color(p.text_disabled),
            placeholder: to_color(p.placeholder),
            accent: to_color(p.accent),
            accent_hover: to_color(p.accent_hover),
            on_accent: to_color(p.on_accent),
            handle: to_color(p.handle),
            handle_edge: to_color(p.handle_edge),
            scrim: to_color(p.scrim),
            shadow: to_color(p.shadow),
            light,
        }
    }

    /// Stock iced widgets (tooltips, un-colored text, defaults) follow
    /// the same colors through a custom iced theme.
    pub(crate) fn iced_theme(&self) -> iced::Theme {
        let base = if self.light {
            iced::theme::Palette::LIGHT
        } else {
            iced::theme::Palette::DARK
        };
        let palette = iced::theme::Palette {
            background: self.window,
            text: self.text_body,
            primary: self.accent,
            ..base
        };
        iced::Theme::custom("yutu-mux", palette)
    }
}

// The live theme. A `Copy` struct per UI thread (not inside
// `iced::Theme`, whose palette has too few slots): style fns read it
// with `tk()`, a theme switch swaps it with `set_tokens`. Thread-local
// because iced builds and draws views on the one UI thread, and so
// parallel tests never see each other's theme.
std::thread_local! {
    static TOKENS: std::cell::Cell<Tokens> = const { std::cell::Cell::new(Tokens::DARK) };
}

/// Current theme colors.
pub(crate) fn tk() -> Tokens {
    TOKENS.with(std::cell::Cell::get)
}

/// Swap the live theme (takes effect on the next frame).
pub(crate) fn set_tokens(tokens: Tokens) {
    TOKENS.with(|t| t.set(tokens));
}

/// Content drawn over cover art or video (play overlays, the mini
/// player's art glow): always white, whatever the theme, because the
/// art underneath is not a theme surface.
pub(crate) const ART_FG: iced::Color = iced::Color::WHITE;

pub(crate) fn app_bg(_: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(iced::Background::Color(tk().window)),
        ..Default::default()
    }
}

pub(crate) fn sidebar_bg(_: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(iced::Background::Color(tk().panel)),
        border: iced::Border::default().rounded(12),
        ..Default::default()
    }
}

pub(crate) fn card_bg(_: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(iced::Background::Color(tk().surface)),
        border: iced::Border::default().rounded(8),
        ..Default::default()
    }
}

/// Modal backdrop: dims the app behind the dialog so it stands out.
/// Alpha-only (no layout), same overlay idiom as `art_dim`.
pub(crate) fn modal_dim(_: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(iced::Background::Color(tk().scrim)),
        ..Default::default()
    }
}

/// Text contrast the backdrop veils keep over cover art (WCAG AAA).
pub(crate) const BACKDROP_TEXT_CONTRAST: f32 = 7.0;

/// Weakest veil that keeps the theme's text readable over art with the
/// given tones (light, dark) painted at `art_opacity` on `bg`. Light
/// text guards against the art's bright areas, dark text against its
/// dark ones. 0 (the design's own veil) when the tones are unknown.
pub(crate) fn backdrop_veil_floor(
    tones: Option<(yutu_core::theme::Rgba, yutu_core::theme::Rgba)>,
    text: yutu_core::theme::Rgba,
    bg: yutu_core::theme::Rgba,
    art_opacity: f32,
) -> f32 {
    let Some((light, dark)) = tones else {
        return 0.0;
    };
    let tone = if text.luminance() > bg.luminance() {
        light
    } else {
        dark
    };
    yutu_core::theme::veil_floor(text, bg, tone, art_opacity, BACKDROP_TEXT_CONTRAST)
}

/// Now Playing backdrop veil: the card surface fading in from light
/// at the top (cover colors show) to heavy at the bottom (lyrics read
/// on a near-plain surface). `floor` (from `backdrop_veil_floor`)
/// thickens it where the cover would wash out the text.
pub(crate) fn np_backdrop_veil(
    floor: f32,
) -> impl Fn(&iced::Theme) -> iced::widget::container::Style {
    move |_| {
        let top = iced::Color {
            a: floor.max(0.25),
            ..tk().surface
        };
        let bottom = iced::Color {
            a: floor.max(0.85),
            ..tk().surface
        };
        iced::widget::container::Style {
            background: Some(iced::Background::Gradient(iced::Gradient::Linear(
                iced::gradient::Linear::new(iced::Radians(std::f32::consts::PI))
                    .add_stop(0.0, top)
                    .add_stop(1.0, bottom),
            ))),
            border: iced::Border::default().rounded(8),
            ..Default::default()
        }
    }
}

/// Collection page backdrop veil: light at the top so the cover's
/// colors show, ending in exactly the app background so the band melts
/// into the page with no visible edge. `floor` as for Now Playing.
pub(crate) fn page_backdrop_veil(
    floor: f32,
) -> impl Fn(&iced::Theme) -> iced::widget::container::Style {
    move |_| {
        let at = |a: f32| iced::Color {
            a: floor.max(a),
            ..tk().window
        };
        iced::widget::container::Style {
            background: Some(iced::Background::Gradient(iced::Gradient::Linear(
                iced::gradient::Linear::new(iced::Radians(std::f32::consts::PI))
                    .add_stop(0.0, at(0.30))
                    .add_stop(0.4, at(0.60))
                    .add_stop(0.7, at(0.90))
                    .add_stop(0.88, tk().window)
                    .add_stop(1.0, tk().window),
            ))),
            // Window-level band: square edges, the panels cover the sides.
            ..Default::default()
        }
    }
}

/// Floating side panel (Medium hover expand, Narrow drawer): the panel
/// surface plus a soft shadow so it reads as above the content.
pub(crate) fn floating_panel_bg(_: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(iced::Background::Color(tk().panel)),
        border: iced::Border::default().rounded(12),
        shadow: iced::Shadow {
            color: tk().shadow.scale_alpha(0.6),
            offset: iced::Vector::new(0.0, 0.0),
            blur_radius: 28.0,
        },
        ..Default::default()
    }
}

/// Narrow drawer scrim: the modal dim, rounded like the panels it
/// covers so its corners never stick out square.
pub(crate) fn drawer_scrim(_: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(iced::Background::Color(tk().scrim)),
        border: iced::Border::default().rounded(12),
        ..Default::default()
    }
}

/// Collapsed rail thumb wrapper: red border for the live song, transparent
/// same-size border otherwise so the pitch never shifts on highlight.
pub(crate) fn rail_thumb_bg(
    current: bool,
) -> impl Fn(&iced::Theme) -> iced::widget::container::Style {
    move |_| iced::widget::container::Style {
        background: Some(iced::Background::Color(tk().surface)),
        border: iced::Border {
            color: if current {
                tk().accent
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
            background: iced::Background::Color(tk().text.scale_alpha(a)),
            border: iced::Border::default().rounded(2),
        },
    };
    base.vertical_rail = rail;
    base.horizontal_rail = rail;
    base
}

/// Shared field look (every text input, the top search pill, the file
/// picker and select faces): one fill, border, radius and placeholder,
/// with a lighter fill and border while hovered or focused.
pub(crate) const FIELD_RADIUS: f32 = 20.0;

/// Field fill + border for the active (hovered/focused) or resting state.
pub(crate) fn field_look(active: bool) -> (iced::Background, iced::Border) {
    (
        iced::Background::Color(if active {
            tk().surface_active
        } else {
            tk().field
        }),
        iced::Border {
            color: if active {
                tk().outline_strong
            } else {
                tk().outline
            },
            width: 1.0,
            radius: FIELD_RADIUS.into(),
        },
    )
}

/// Top search pill: the shared field look on the container that wraps
/// the icon + borderless input (the icon sits visually inside the
/// field); active while the input is focused.
pub(crate) fn search_pill(active: bool) -> impl Fn(&iced::Theme) -> iced::widget::container::Style {
    move |_| {
        let (background, border) = field_look(active);
        iced::widget::container::Style {
            background: Some(background),
            border,
            ..Default::default()
        }
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
        icon: tk().text_muted,
        placeholder: tk().placeholder,
        value: tk().text,
        selection: tk().accent,
    }
}

pub(crate) fn player_bg(_: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(iced::Background::Color(tk().panel)),
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
        iced::widget::button::Status::Hovered => tk().accent_hover,
        _ => tk().accent,
    };
    iced::widget::button::Style {
        background: Some(iced::Background::Color(bg)),
        text_color: tk().on_accent,
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
            Some(iced::Background::Color(tk().surface_active))
        } else {
            None
        };
        let text_color = if active || hovered {
            tk().text
        } else {
            tk().text_muted
        };
        iced::widget::button::Style {
            background,
            text_color,
            border: iced::Border::default().rounded(10),
            ..Default::default()
        }
    }
}
