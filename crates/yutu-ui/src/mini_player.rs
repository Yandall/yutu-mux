//! Picture-in-picture mini player (design "Pill · Glow"): a second,
//! frameless, transparent, always-on-top window holding only the live
//! song and the transport. The capsule glows with the cover's colors
//! (the same 8x8 downscale as the Now Playing glow), progress is a
//! ring around the round art, and a lyrics toggle grows the window
//! downward into a five-line synced ticker.
//!
//! Rules:
//!   - The main window keeps running untouched: opening the mini
//!     player never minimizes or hides it (Wayland cannot restore a
//!     minimized window from the app, so nothing would bring it back).
//!   - Window actions (Open, Close) float above the capsule only while
//!     the pointer is over the mini window. Wayland only: the leave the
//!     compositor's move grab sends as a drag starts is ignored.
//!   - Platform workarounds are gated on [`MiniPlatform`] (detected at
//!     open), never applied everywhere: Hyprland `hyprctl` placement and
//!     focus, Wayland size-hint re-pin and grab leave, client positions
//!     on X11, macOS and Windows, macOS layer gravity (no stretched
//!     frame while the lyrics toggle resizes the window).
//!   - The art and title are the drag handle (no title bar).
//!   - Sizes are logical units times the UI scale, so the window always
//!     fits its content at any scale.
//!   - Its own app id ([`MINI_APP_ID`], Linux) so compositor rules can
//!     target it (README: Hyprland rule).
//!   - Position: reopens where the user left it (`mini_pos` pref) where
//!     the platform reports moves (X11, macOS, Windows). Wayland clients
//!     cannot read or set their position, so under Hyprland the app
//!     asks `hyprctl` instead (also to keep the top edge fixed when the
//!     lyrics grow, since Hyprland keeps a resized window's center);
//!     other Wayland compositors place it themselves.
//!   - The synced ticker is a wheel picker: the current line sits on
//!     the center band and the wheel glides (ease-out, 450ms) to the
//!     next line; far seeks, new songs and disabled animations jump.
//!   - The synced ticker never cuts the current verse: a long one wraps
//!     whole, at a smaller size and with fewer neighbor lines when
//!     needed (`MINI_TICKER_LAYOUTS`). Neighbor lines are one ellipsized
//!     line each. Unsynced lyrics show
//!     in full instead, wrapped and scrollable by hand.

use super::*;

/// Pref remembering whether the ticker was open ("1") last time.
pub(crate) const MINI_LYRICS_PREF: &str = "mini_lyrics";
/// Pref remembering where the mini window was last left ("x,y").
pub(crate) const MINI_POS_PREF: &str = "mini_pos";
/// Linux app id (Wayland) / class of the mini window, for compositor
/// rules. Equal to `yutu_desktop::hyprland::MINI_CLASS` (a test pins
/// it); spelled out here because that module is Linux-only.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) const MINI_APP_ID: &str = "yutu-mux-mini";
/// Capsule width (content units).
pub(crate) const MINI_W: f32 = 360.0;
/// Capsule height when collapsed (the header row when expanded).
pub(crate) const MINI_PILL_H: f32 = 60.0;
/// Lyrics ticker height below the header row.
pub(crate) const MINI_LYRICS_H: f32 = 202.0;
/// Hover chip row above the capsule (reserved even when hidden, so
/// the capsule never jumps when the chips appear).
pub(crate) const MINI_CHIPS_H: f32 = 26.0;
pub(crate) const MINI_CHIPS_GAP: f32 = 6.0;
/// Transparent room around the capsule for its drop shadow.
pub(crate) const MINI_MARGIN: f32 = 10.0;
/// Round art diameter and the progress ring box around it.
const MINI_ART: f32 = 40.0;
const MINI_RING: f32 = 52.0;
/// Ring stroke width.
const MINI_RING_W: f32 = 3.0;
/// Ring cover raster: the 544px hero downscaled once with a Triangle
/// filter. Drawing the hero itself at 40px (bilinear, no mipmaps)
/// aliases, and the 120px row raster upscales video thumbnails; 160px
/// stays sharp at 2x hidpi with room for a larger UI scale.
pub(crate) const MINI_COVER_PX: u32 = 160;
/// Char budget for the title/artist beside the controls.
const MINI_TITLE_CHARS: usize = 18;
/// Ticker padding (vertical, horizontal) and gap between lines.
const MINI_TICKER_PAD: (f32, f32) = (16.0, 24.0);
const MINI_TICKER_GAP: f32 = 10.0;
/// Synced ticker layouts, best first: (current verse size, how far
/// the neighbor lines reach). A long verse is never cut: it steps down
/// in size, then gives up neighbor lines, until it fits whole.
const MINI_TICKER_LAYOUTS: [(f32, u32); 8] = [
    (21.0, 2),
    (16.0, 2),
    (14.0, 2),
    (16.0, 1),
    (14.0, 1),
    (16.0, 0),
    (14.0, 0),
    (13.0, 0),
];
/// Glow strength over the base surface, then a dark veil on top so
/// white text always reads (same idea as the Now Playing sheet).
const MINI_GLOW_OPACITY: f32 = 0.9;
const MINI_VEIL: iced::Color = iced::Color::from_rgba(0.04, 0.02, 0.03, 0.45);
/// Base capsule surface (shows alone until the glow is decoded).
const MINI_SURFACE: iced::Color = iced::Color::from_rgb8(0x1A, 0x14, 0x17);

/// Live mini player window state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MiniPlayer {
    pub(crate) id: iced::window::Id,
    /// Lyrics ticker open (window grown downward).
    pub(crate) lyrics: bool,
    /// Pointer over the mini window (shows the Open/Close chips).
    pub(crate) hovered: bool,
    /// When the last drag started (Wayland only): a leave right after
    /// it is the compositor's move grab, not the pointer leaving.
    pub(crate) drag_hold: Option<std::time::Instant>,
    /// Detected when the window opened; gates every workaround.
    pub(crate) platform: MiniPlatform,
}

/// Wayland only: how long after a drag starts a leave still counts as
/// the move grab's (it lands within a few frames of the press).
pub(crate) const MINI_GRAB_LEAVE_MS: u128 = 400;

/// Parse the `mini_pos` pref ("x,y", finite numbers).
pub(crate) fn parse_mini_pos(s: &str) -> Option<iced::Point> {
    let (x, y) = s.split_once(',')?;
    let (x, y): (f32, f32) = (x.trim().parse().ok()?, y.trim().parse().ok()?);
    (x.is_finite() && y.is_finite()).then(|| iced::Point::new(x, y))
}

/// Format a position for the `mini_pos` pref (whole pixels).
pub(crate) fn format_mini_pos(at: iced::Point) -> String {
    format!("{},{}", at.x.round(), at.y.round())
}

/// Windows: corner radius of the opaque capsule window (matches the
/// rounded corners Windows 11 draws on it, so no square edge shows).
pub(crate) const MINI_WIN_RADIUS: f32 = 8.0;

/// Content size in UI units: capsule plus chip row plus shadow margin,
/// or only the capsule where the window cannot be transparent.
pub(crate) fn mini_content_size(lyrics: bool, platform: MiniPlatform) -> iced::Size {
    let body = if lyrics {
        MINI_PILL_H + MINI_LYRICS_H
    } else {
        MINI_PILL_H
    };
    if platform.opaque_window() {
        return iced::Size::new(MINI_W, body);
    }
    iced::Size::new(
        MINI_W + 2.0 * MINI_MARGIN,
        MINI_CHIPS_H + MINI_CHIPS_GAP + body + 2.0 * MINI_MARGIN,
    )
}

/// Logical window size: the content scaled by the UI scale, so the
/// layout never clips when the user enlarges the interface.
pub(crate) fn mini_window_size(lyrics: bool, scale: f32, platform: MiniPlatform) -> iced::Size {
    let c = mini_content_size(lyrics, platform);
    iced::Size::new((c.width * scale).ceil(), (c.height * scale).ceil())
}

/// First placement: top-right corner, so the lyrics grow downward into
/// open screen instead of off the bottom edge. Compositors that place
/// windows themselves (Wayland) ignore this.
fn mini_start_position(window: iced::Size, monitor: iced::Size) -> iced::Point {
    iced::Point::new((monitor.width - window.width - 24.0).max(0.0), 56.0)
}

/// Window settings for the mini player: frameless, transparent (the
/// capsule is the visible shape), not resizable, kept above other
/// windows where the platform allows it, at the saved spot when there
/// is one.
pub(crate) fn mini_window_settings(
    lyrics: bool,
    scale: f32,
    saved: Option<iced::Point>,
    platform: MiniPlatform,
) -> iced::window::Settings {
    let base = window_settings();
    #[cfg(target_os = "linux")]
    let platform_specific = iced::window::settings::PlatformSpecific {
        application_id: MINI_APP_ID.into(),
        ..base.platform_specific.clone()
    };
    #[cfg(not(target_os = "linux"))]
    let platform_specific = base.platform_specific.clone();
    iced::window::Settings {
        size: mini_window_size(lyrics, scale, platform),
        min_size: None,
        position: match saved {
            Some(at) => iced::window::Position::Specific(at),
            None => iced::window::Position::SpecificWith(mini_start_position),
        },
        resizable: false,
        decorations: false,
        transparent: !platform.opaque_window(),
        minimizable: false,
        level: iced::window::Level::AlwaysOnTop,
        platform_specific,
        ..base
    }
}

/// Resize the (non-resizable) mini window. On Wayland a non-resizable
/// window pins min = max = its first size (winit stores those hints
/// and restores them even when unlocked), so a bare resize is clamped
/// by the compositor: re-pin both hints at the new size, then resize
/// (max cleared first so the new min never exceeds the old max;
/// chained so the steps land in order). Elsewhere a plain resize.
fn mini_resize(
    platform: MiniPlatform,
    id: iced::window::Id,
    size: iced::Size,
) -> iced::Task<Message> {
    if !platform.pinned_size_hints() {
        return iced::window::resize(id, size);
    }
    iced::window::set_max_size(id, None)
        .chain(iced::window::set_min_size(id, Some(size)))
        .chain(iced::window::set_max_size(id, Some(size)))
        .chain(iced::window::resize(id, size))
}

/// macOS only: pin the window's layers to top-left contents gravity, so
/// a resize never shows the last frame stretched (see
/// `yutu_desktop::macos_window`). Runs on the main thread via iced.
#[cfg(target_os = "macos")]
fn macos_pin_layers(id: iced::window::Id) -> iced::Task<Message> {
    iced::window::run(id, |w| {
        yutu_desktop::macos_window::pin_contents_top_left(w);
    })
    .discard()
}

/// Windows only: ask DWM for rounded corners on the opaque capsule
/// window (Windows 11; older versions keep square corners).
#[cfg(target_os = "windows")]
fn windows_round_corners(id: iced::window::Id) -> iced::Task<Message> {
    iced::window::run(id, |w| {
        yutu_desktop::windows_window::round_corners(w);
    })
    .discard()
}

/// Run a blocking helper (`hyprctl`) off the UI thread.
#[cfg(target_os = "linux")]
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    tokio::task::spawn_blocking(f).await.ok()
}

/// The windowing platform the mini player runs on, detected once when
/// it opens. Every platform workaround is gated on it, never applied
/// everywhere: a Wayland fix can break macOS and the other way round.
// Each OS builds only its own variants outside tests.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MiniPlatform {
    /// Wayland under Hyprland (`hyprctl` reachable).
    Hyprland,
    /// Any other Wayland compositor.
    Wayland,
    /// X11 (or an unknown Unix backend).
    X11,
    MacOs,
    Windows,
}

impl MiniPlatform {
    pub(crate) fn detect() -> Self {
        #[cfg(target_os = "macos")]
        {
            Self::MacOs
        }
        #[cfg(target_os = "windows")]
        {
            Self::Windows
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            // winit prefers Wayland whenever a Wayland display is set,
            // unless the user forces X11.
            let var = |k: &str| std::env::var(k).unwrap_or_default();
            let forced_x11 = var("WINIT_UNIX_BACKEND").eq_ignore_ascii_case("x11");
            if forced_x11 || var("WAYLAND_DISPLAY").is_empty() {
                Self::X11
            } else if Self::hyprland_available() {
                Self::Hyprland
            } else {
                Self::Wayland
            }
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    fn hyprland_available() -> bool {
        #[cfg(target_os = "linux")]
        {
            yutu_desktop::hyprland::available()
        }
        #[cfg(not(target_os = "linux"))]
        {
            false
        }
    }

    /// Wayland: the compositor's move grab sends a leave as a drag
    /// starts, while the pointer is still on the window.
    pub(crate) fn grab_leave(self) -> bool {
        matches!(self, Self::Hyprland | Self::Wayland)
    }

    /// Wayland: a non-resizable window keeps min = max at its first
    /// size, so a resize must re-pin both hints first.
    pub(crate) fn pinned_size_hints(self) -> bool {
        matches!(self, Self::Hyprland | Self::Wayland)
    }

    /// Windows: the window cannot be transparent (iced's wgpu DX12
    /// surface is opaque-only, so clear pixels show black). The window
    /// is the capsule alone, with Windows 11 rounded corners, and the
    /// Open/Close chips replace the title in the header row on hover.
    pub(crate) fn opaque_window(self) -> bool {
        self == Self::Windows
    }

    /// macOS: the Metal layer must not stretch the last frame while
    /// the window resizes (pinned to top-left contents gravity).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) fn pins_layer_gravity(self) -> bool {
        self == Self::MacOs
    }

    /// Hyprland: position, focus and the top edge go through `hyprctl`.
    pub(crate) fn hyprland(self) -> bool {
        self == Self::Hyprland
    }

    /// Where `mini_pos` comes from: window Moved events here. Wayland
    /// clients never learn their position (Hyprland reads it through
    /// `hyprctl`, in layout coordinates, never a client position).
    pub(crate) fn client_positions(self) -> bool {
        matches!(self, Self::X11 | Self::MacOs | Self::Windows)
    }
}

/// Hyprland: the mini window's top-left in layout coordinates.
#[cfg(target_os = "linux")]
fn hypr_mini_window() -> iced::Task<Option<yutu_desktop::hyprland::HyprWindow>> {
    iced::Task::perform(
        blocking(|| yutu_desktop::hyprland::class_window(MINI_APP_ID).ok()),
        Option::flatten,
    )
}

/// Text size of a neighbor line `distance` lines from the current
/// verse (always one line, ellipsized).
pub(crate) fn ticker_neighbor_size(distance: i32) -> f32 {
    if distance.unsigned_abs() <= 1 {
        14.0
    } else {
        12.0
    }
}

/// Text width inside the ticker.
pub(crate) fn ticker_text_width() -> f32 {
    MINI_W - 2.0 * MINI_TICKER_PAD.1
}

/// Height the ticker content may take (inside its padding).
pub(crate) fn ticker_budget() -> f32 {
    MINI_LYRICS_H - 2.0 * MINI_TICKER_PAD.0
}

fn line_h(size: f32) -> f32 {
    iced::widget::text::LineHeight::default()
        .to_absolute(iced::Pixels(size))
        .0
}

/// Measured height of drawn ticker lines (the current verse wrapped
/// in bold, neighbors one line each) plus the gaps between them.
pub(crate) fn ticker_content_height(lines: &[(String, f32, i32)]) -> f32 {
    let width = ticker_text_width();
    let rows: f32 = lines
        .iter()
        .map(|(text, size, d)| {
            if *d == 0 {
                wrapped_height(text, width, *size, bold_font())
            } else {
                line_h(*size)
            }
        })
        .sum();
    rows + MINI_TICKER_GAP * lines.len().saturating_sub(1) as f32
}

/// Synced ticker lines ready to draw: `(text, size, distance)`. The
/// current verse is always whole (wrapped, bold); the first layout of
/// `MINI_TICKER_LAYOUTS` that fits the ticker picks its size and how
/// many neighbor lines stay. Neighbors are one ellipsized line each.
/// Measured with the real font, so the fit matches what is drawn.
pub(crate) fn ticker_lines(visible: &[&str], at: Option<usize>) -> Vec<(String, f32, i32)> {
    let width = ticker_text_width();
    let window = ticker_window(visible.len(), at);
    let neighbor = |i: usize, d: i32| {
        let size = ticker_neighbor_size(d);
        (fit_lines(visible[i], width, size, 1), size, d)
    };
    let Some(&(cur, _)) = window.iter().find(|(_, d)| *d == 0) else {
        // Before the first verse: upcoming lines only.
        return window.into_iter().map(|(i, d)| neighbor(i, d)).collect();
    };
    let layout = |(size, reach): (f32, u32)| -> Vec<(String, f32, i32)> {
        window
            .iter()
            .filter(|(_, d)| d.unsigned_abs() <= reach)
            .map(|&(i, d)| {
                if d == 0 {
                    (visible[i].to_string(), size, 0)
                } else {
                    neighbor(i, d)
                }
            })
            .collect()
    };
    let budget = ticker_budget();
    MINI_TICKER_LAYOUTS
        .into_iter()
        .map(layout)
        .find(|lines| ticker_content_height(lines) <= budget)
        // Longer than the ticker even alone at the smallest size: the
        // verse still shows whole, centered (rare; the clip trims).
        .unwrap_or_else(|| vec![(visible[cur].to_string(), 13.0, 0)])
}

/// Synced lyric line look by its (continuous) distance from the
/// current line: text size, white opacity, bold. One scale for every
/// lyrics view (mini player wheel, Now Playing, Lyrics page): the
/// current line 21px bold, its neighbors 14px at 72%, the rest 12px
/// at 50%, blended in between while a glide runs.
pub(crate) fn verse_style(distance: f32) -> (f32, f32, bool) {
    let x = distance.abs();
    let mix = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let (size, alpha) = if x <= 1.0 {
        (mix(21.0, 14.0, x), mix(1.0, 0.72, x))
    } else if x <= 2.0 {
        (mix(14.0, 12.0, x - 1.0), mix(0.72, 0.5, x - 1.0))
    } else {
        (12.0, 0.5)
    };
    (size, alpha, x < 0.5)
}

/// How long the synced ticker glides from one line to the next.
pub(crate) const MINI_WHEEL_MS: u128 = 450;

/// iOS-picker-like deceleration: fast start, soft landing.
pub(crate) fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

/// A glide of the synced ticker between two current lines (`None` =
/// before the first verse).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TickerGlide {
    pub(crate) from: Option<usize>,
    pub(crate) to: Option<usize>,
    pub(crate) started: std::time::Instant,
}

impl TickerGlide {
    /// Eased progress 0..=1 (1 once finished).
    pub(crate) fn progress(&self) -> f32 {
        ease_out_cubic(self.started.elapsed().as_millis() as f32 / MINI_WHEEL_MS as f32)
    }

    pub(crate) fn running(&self) -> bool {
        self.started.elapsed().as_millis() < MINI_WHEEL_MS
    }
}

/// One placed line of the synced ticker (a wheel slot).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WheelLine {
    /// Index among the visible (non-empty) lyric lines.
    pub(crate) index: usize,
    pub(crate) text: String,
    pub(crate) size: f32,
    pub(crate) alpha: f32,
    pub(crate) bold: bool,
    /// Top edge inside the ticker (0 = its top).
    pub(crate) top: f32,
    pub(crate) height: f32,
}

/// Static wheel for one current line: `ticker_lines` placed so the
/// current verse's center sits on the ticker's center (the picker's
/// selection band), kept inside the ticker padding when it fits. With
/// no current line yet, the upcoming lines are centered as a block.
pub(crate) fn wheel_layout(visible: &[&str], at: Option<usize>) -> Vec<WheelLine> {
    let width = ticker_text_width();
    let mut lines: Vec<WheelLine> = ticker_lines(visible, at)
        .into_iter()
        .map(|(text, size, d)| {
            let index = match at {
                Some(c) => (c as i64 + d as i64).max(0) as usize,
                None => (d - 1).max(0) as usize,
            };
            let current = at.is_some() && d == 0;
            let height = if current {
                wrapped_height(&text, width, size, bold_font())
            } else {
                line_h(size)
            };
            let alpha = match (current, d.unsigned_abs()) {
                (true, _) => 1.0,
                (false, 1) => 0.72,
                _ => 0.5,
            };
            WheelLine {
                index,
                text,
                size,
                alpha,
                bold: current,
                top: 0.0,
                height,
            }
        })
        .collect();
    let mut y = 0.0;
    for l in lines.iter_mut() {
        l.top = y;
        y += l.height + MINI_TICKER_GAP;
    }
    let block = (y - MINI_TICKER_GAP).max(0.0);
    let center = MINI_LYRICS_H / 2.0;
    let mut offset = match lines.iter().find(|l| l.bold) {
        Some(cur) => center - (cur.top + cur.height / 2.0),
        None => center - block / 2.0,
    };
    // Lopsided wheels (first lines of a song) stay inside the padding
    // when the whole block fits.
    let pad = MINI_TICKER_PAD.0;
    if block <= MINI_LYRICS_H - 2.0 * pad {
        offset = offset.clamp(pad, MINI_LYRICS_H - pad - block);
    }
    for l in lines.iter_mut() {
        l.top += offset;
    }
    lines
}

/// One frame of a glide from wheel `a` to wheel `b` at eased progress
/// `p`: every line moves, resizes and fades between its two slots.
/// Lines only in `a` roll out (fading to 0) with the wheel's motion,
/// lines only in `b` roll in from it.
pub(crate) fn wheel_frame(a: &[WheelLine], b: &[WheelLine], p: f32) -> Vec<WheelLine> {
    let p = p.clamp(0.0, 1.0);
    let lerp = |x: f32, y: f32| x + (y - x) * p;
    // How far the wheel turned: the shift of a line both wheels show
    // (the incoming current line when it is in both).
    let shift = b
        .iter()
        .filter(|l| l.bold)
        .chain(b.iter())
        .find_map(|lb| {
            a.iter()
                .find(|la| la.index == lb.index)
                .map(|la| lb.top - la.top)
        })
        .unwrap_or(0.0);
    let mut out: Vec<WheelLine> = Vec::new();
    for la in a {
        match b.iter().find(|lb| lb.index == la.index) {
            Some(lb) => out.push(WheelLine {
                index: lb.index,
                text: lb.text.clone(),
                size: lerp(la.size, lb.size),
                alpha: lerp(la.alpha, lb.alpha),
                bold: if p < 0.5 { la.bold } else { lb.bold },
                top: lerp(la.top, lb.top),
                height: lerp(la.height, lb.height),
            }),
            None => out.push(WheelLine {
                top: lerp(la.top, la.top + shift),
                alpha: lerp(la.alpha, 0.0),
                ..la.clone()
            }),
        }
    }
    for lb in b {
        if a.iter().all(|la| la.index != lb.index) {
            out.push(WheelLine {
                top: lerp(lb.top - shift, lb.top),
                alpha: lerp(0.0, lb.alpha),
                ..lb.clone()
            });
        }
    }
    out.sort_by_key(|l| l.index);
    out
}

/// Mini window theme: the dark theme with a transparent window
/// background, so only the capsule paints (the capsule surface where
/// the window is opaque).
pub(crate) fn mini_theme(platform: MiniPlatform) -> iced::Theme {
    let palette = iced::theme::Palette {
        background: if platform.opaque_window() {
            MINI_SURFACE
        } else {
            iced::Color::TRANSPARENT
        },
        ..iced::Theme::Dark.palette()
    };
    iced::Theme::custom("yutu-mux mini", palette)
}

/// Lyric lines to show in the ticker: `(visible line index, distance
/// from the current line)`, at most two above and two below the
/// current verse. Before the first timestamp nothing is current, so
/// the opening lines show as upcoming (distance 1, 2, 3).
pub(crate) fn ticker_window(len: usize, at: Option<usize>) -> Vec<(usize, i32)> {
    match at {
        Some(i) if i < len => {
            let lo = i.saturating_sub(2);
            let hi = (i + 2).min(len - 1);
            (lo..=hi).map(|j| (j, j as i32 - i as i32)).collect()
        }
        _ => (0..len.min(3)).map(|j| (j, j as i32 + 1)).collect(),
    }
}

/// Progress ring around the round art (white arc on a faint track,
/// starting at 12 o'clock).
struct ProgressRing {
    frac: f32,
}

impl iced::widget::canvas::Program<Message> for ProgressRing {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: iced::Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<iced::widget::canvas::Geometry> {
        use iced::widget::canvas::{path::Arc, Frame, LineCap, Path, Stroke};
        let mut frame = Frame::new(renderer, bounds.size());
        let center = frame.center();
        let radius = bounds.width.min(bounds.height) / 2.0 - MINI_RING_W / 2.0;
        frame.stroke(
            &Path::circle(center, radius),
            Stroke::default()
                .with_width(MINI_RING_W)
                .with_color(iced::Color::from_rgba(1.0, 1.0, 1.0, 0.18)),
        );
        let frac = self.frac.clamp(0.0, 1.0);
        if frac > 0.0 {
            let start = -std::f32::consts::FRAC_PI_2;
            let arc = Path::new(|b| {
                b.arc(Arc {
                    center,
                    radius,
                    start_angle: iced::Radians(start),
                    end_angle: iced::Radians(start + std::f32::consts::TAU * frac),
                })
            });
            frame.stroke(
                &arc,
                Stroke::default()
                    .with_width(MINI_RING_W)
                    .with_color(iced::Color::WHITE)
                    .with_line_cap(LineCap::Round),
            );
        }
        vec![frame.into_geometry()]
    }
}

/// White round play/pause button (dark glyph): the one solid control
/// on the glow, like the design.
fn mini_play_button(
    _: &iced::Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let bg = match status {
        iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed => {
            iced::Color::from_rgb8(0xE6, 0xE6, 0xE6)
        }
        _ => iced::Color::WHITE,
    };
    iced::widget::button::Style {
        background: Some(iced::Background::Color(bg)),
        text_color: iced::Color::BLACK,
        border: iced::Border::default().rounded(999),
        ..Default::default()
    }
}

/// Floating Open/Close chip above the capsule.
fn mini_chip(_: &iced::Theme, status: iced::widget::button::Status) -> iced::widget::button::Style {
    let hovered = matches!(
        status,
        iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed
    );
    iced::widget::button::Style {
        background: Some(iced::Background::Color(if hovered {
            iced::Color::from_rgb8(0x2E, 0x2E, 0x2E)
        } else {
            iced::Color::from_rgba8(0x1E, 0x1E, 0x1E, 0.94)
        })),
        text_color: if hovered { iced::Color::WHITE } else { MUTED },
        border: iced::Border {
            color: iced::Color::from_rgba(1.0, 1.0, 1.0, 0.1),
            width: 1.0,
            radius: (MINI_CHIPS_H / 2.0).into(),
        },
        ..Default::default()
    }
}

/// Icon-only control on the glow: light glyph, round soft hover.
fn mini_icon_button(name: &'static str, msg: Message) -> iced::widget::Button<'static, Message> {
    iced::widget::button(ctrl_box(btn_icon(
        name,
        14.0,
        iced::Color::from_rgb8(0xEE, 0xE4, 0xE8),
    )))
    .style(mini_icon_hover)
    .on_press(msg)
    .padding(7)
}

fn mini_icon_hover(
    _: &iced::Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let hovered = matches!(
        status,
        iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed
    );
    iced::widget::button::Style {
        background: hovered.then_some(iced::Background::Color(iced::Color::from_rgba(
            1.0, 1.0, 1.0, 0.12,
        ))),
        text_color: iced::Color::WHITE,
        border: iced::Border::default().rounded(999),
        ..Default::default()
    }
}

impl AppState {
    /// Logical scale the mini window uses (same as the main window).
    pub(crate) fn mini_scale(&self) -> f32 {
        clamp_scale(self.ui_scale)
    }

    /// Player bar button that opens the mini player (accent while it is
    /// open, like the Now Playing toggle beside it).
    pub(crate) fn mini_player_button(&self) -> iced::Element<'_, Message> {
        use iced::widget::{button, text, tooltip};
        let open = self.mini.is_some();
        let btn = if open {
            button(ctrl_box(btn_icon(
                "picture-in-picture-2",
                14.0,
                iced::Color::WHITE,
            )))
            .style(accent_button)
        } else {
            button(ctrl_box(btn_icon("picture-in-picture-2", 14.0, MUTED))).style(icon_hover)
        };
        tooltip(
            btn.on_press(Message::ToggleMiniPlayer).padding(7),
            text(self.language.mini_player()).size(12),
            tooltip::Position::Top,
        )
        .gap(8)
        .into()
    }

    /// Open the mini player (focus it when it is already open).
    pub(crate) fn open_mini_player(&mut self) -> iced::Task<Message> {
        if let Some(mini) = self.mini {
            return iced::window::gain_focus(mini.id);
        }
        let lyrics = self.mini_lyrics_pref;
        let platform = MiniPlatform::detect();
        // Only client positions go to the window settings (Hyprland's
        // are layout coordinates, and Wayland ignores them anyway).
        let saved = self.mini_pos.filter(|_| platform.client_positions());
        let (id, open) = iced::window::open(mini_window_settings(
            lyrics,
            self.mini_scale(),
            saved,
            platform,
        ));
        self.mini = Some(MiniPlayer {
            id,
            lyrics,
            hovered: false,
            drag_hold: None,
            platform,
        });
        #[cfg_attr(
            not(any(target_os = "linux", target_os = "macos", target_os = "windows")),
            allow(unused_mut)
        )]
        let mut open = open.discard();
        #[cfg(target_os = "windows")]
        if platform.opaque_window() {
            open = open.chain(windows_round_corners(id));
        }
        #[cfg(target_os = "macos")]
        if platform.pins_layer_gravity() {
            open = open.chain(macos_pin_layers(id));
        }
        // Hyprland: move it back where it was once it is mapped.
        #[cfg(target_os = "linux")]
        if let (true, Some(at)) = (platform.hyprland(), self.mini_pos) {
            use yutu_desktop::hyprland as hl;
            open = open.chain(iced::Task::perform(
                blocking(move || {
                    let w = hl::wait_class_window(
                        MINI_APP_ID,
                        None,
                        std::time::Duration::from_secs(2),
                    )?;
                    hl::move_window(&w.address, at.x as i64, at.y as i64)
                }),
                |_| Message::NoOp,
            ));
        }
        let mut tasks = vec![open];
        // The ring shows the hero cover (sharp at any scale): warm it
        // when a restored session never started the track.
        if let Some(t) = self.queue.current().cloned() {
            tasks.extend(self.large_art_task(&t));
        }
        if lyrics {
            tasks.extend(self.mini_lyrics_fetch());
        }
        iced::Task::batch(tasks)
    }

    /// Close the mini player window (playback keeps going), keeping
    /// its position for the next open.
    pub(crate) fn close_mini_player(&mut self) -> iced::Task<Message> {
        let Some(mini) = self.mini.take() else {
            return iced::Task::none();
        };
        self.save_mini_pos();
        let close = iced::window::close::<Message>(mini.id);
        // Hyprland: read where the user left it before it goes away
        // (the answer lands after close and is saved then).
        #[cfg(target_os = "linux")]
        if mini.platform.hyprland() {
            let id = mini.id;
            return hypr_mini_window().then(move |w| {
                let close = iced::window::close(id);
                match w {
                    Some(w) => iced::Task::done(Message::MiniMoved(iced::Point::new(
                        w.at.0 as f32,
                        w.at.1 as f32,
                    )))
                    .chain(close),
                    None => close,
                }
            });
        }
        close
    }

    /// The mini window moved (or Hyprland reported where it is).
    pub(crate) fn mini_moved(&mut self, at: iced::Point) {
        self.mini_pos = Some(at);
        // Moves stream in while dragging: persist only once closed.
        if self.mini.is_none() {
            self.save_mini_pos();
        }
    }

    fn save_mini_pos(&self) {
        let Some(at) = self.mini_pos else { return };
        if let Ok(store) = yutu_core::LibraryStore::open(&self.library_path) {
            let _ = store.set_pref(MINI_POS_PREF, &format_mini_pos(at));
        }
    }

    /// Pointer over / off the mini window. Wayland only: a leave right
    /// after a drag starts is the compositor's move grab, not the
    /// pointer leaving, so the chips stay (the pointer is still on the
    /// window once the drag ends). Every other leave hides them.
    pub(crate) fn mini_hover(&mut self, over: bool) {
        let Some(mini) = self.mini.as_mut() else {
            return;
        };
        let grab_leave = mini
            .drag_hold
            .is_some_and(|at| at.elapsed().as_millis() < MINI_GRAB_LEAVE_MS);
        mini.drag_hold = None;
        if over {
            mini.hovered = true;
        } else if !grab_leave {
            mini.hovered = false;
        }
    }

    /// Press on the drag handle: hand the move to the compositor.
    pub(crate) fn mini_drag(&mut self) -> iced::Task<Message> {
        let Some(mini) = self.mini.as_mut() else {
            return iced::Task::none();
        };
        mini.hovered = true;
        if mini.platform.grab_leave() {
            mini.drag_hold = Some(std::time::Instant::now());
        }
        iced::window::drag(mini.id)
    }

    /// Follow the current synced line after every update, for every
    /// lyrics view on screen (mini player ticker, Now Playing, Lyrics
    /// page): a change of up to three lines on the same song starts a
    /// glide; a new song, a far seek, or disabled animations jump.
    pub(crate) fn verse_sync(&mut self) {
        let shown = self.mini.is_some_and(|m| m.lyrics)
            || self.drawer_open
            || matches!(self.route, Route::Lyrics(_));
        let live = match self.queue.current() {
            Some(t) if shown => Some((t.id.clone(), self.lyric_cursor_at(&t.id))),
            _ => None,
        };
        if self.verse_glide.is_some_and(|g| !g.running()) {
            self.verse_glide = None;
        }
        if live == self.verse_at {
            return;
        }
        let glide = match (&self.verse_at, &live) {
            (Some((was, from)), Some((now, to))) if was == now && self.animations_enabled => {
                let near = match (from, to) {
                    (Some(f), Some(t)) => f.abs_diff(*t) <= 3,
                    (None, Some(t)) => *t <= 2,
                    (Some(f), None) => *f <= 2,
                    (None, None) => false,
                };
                near.then(|| TickerGlide {
                    from: *from,
                    to: *to,
                    started: std::time::Instant::now(),
                })
            }
            _ => None,
        };
        self.verse_glide = glide;
        self.verse_at = live;
    }

    /// A verse glide still running (drives the animation tick).
    pub(crate) fn verse_gliding(&self) -> bool {
        self.verse_glide.is_some_and(|g| g.running())
    }

    /// Continuous position of the current line of track `id` for its
    /// synced lyrics: mid-glide it moves from the old line to the new
    /// (before the first verse counts as line -1, so the opening lines
    /// read as upcoming). Lines are styled by their distance from it.
    pub(crate) fn verse_focus(&self, id: &str, at: Option<usize>) -> f32 {
        let pos = |at: Option<usize>| at.map_or(-1.0, |i| i as f32);
        let ours = self.verse_at.as_ref().is_some_and(|(t, _)| t == id);
        match self
            .verse_glide
            .filter(|g| ours && g.running() && g.to == at)
        {
            Some(g) => {
                let (from, to) = (pos(g.from), pos(g.to));
                from + (to - from) * g.progress()
            }
            None => pos(at),
        }
    }

    /// The synced ticker as a wheel: every line pinned at its slot,
    /// mid-glide when the current line just changed.
    fn mini_wheel(&self, visible: &[&str], at: Option<usize>) -> iced::Element<'_, Message> {
        use iced::widget::{container, pin, text, text::Wrapping, Stack};
        use iced::Fill;
        let target = wheel_layout(visible, at);
        let lines = match self.verse_glide.filter(|g| g.running() && g.to == at) {
            Some(g) => wheel_frame(&wheel_layout(visible, g.from), &target, g.progress()),
            None => target,
        };
        let width = ticker_text_width();
        let mut wheel = Stack::new()
            .width(Fill)
            .height(iced::Length::Fixed(MINI_LYRICS_H));
        for l in lines {
            let mut line = text(l.text)
                .size(l.size)
                .color(iced::Color::from_rgba(
                    1.0,
                    1.0,
                    1.0,
                    l.alpha.clamp(0.0, 1.0),
                ))
                .align_x(iced::alignment::Horizontal::Center)
                .width(Fill);
            // The current verse wraps whole; neighbors are cut to one
            // line already, so a measuring hair never wraps them.
            line = if l.bold {
                line.font(bold_font()).wrapping(Wrapping::WordOrGlyph)
            } else {
                line.wrapping(Wrapping::None)
            };
            wheel = wheel.push(
                pin(container(line).width(iced::Length::Fixed(width)))
                    .x(MINI_TICKER_PAD.1)
                    .y(l.top),
            );
        }
        container(wheel)
            .width(Fill)
            .height(iced::Length::Fixed(MINI_LYRICS_H))
            .clip(true)
            .into()
    }

    /// Player bar button: open the mini player, or close it when it is
    /// already open.
    pub(crate) fn toggle_mini_player(&mut self) -> iced::Task<Message> {
        if self.mini.is_some() {
            self.close_mini_player()
        } else {
            self.open_mini_player()
        }
    }

    /// Close the mini player and bring the main window forward. Most
    /// platforms honor the focus request; Hyprland ignores it unless
    /// `misc:focus_on_activate` is set, so there the app asks the
    /// compositor to focus its main window (switching workspace too).
    pub(crate) fn restore_from_mini(&mut self) -> iced::Task<Message> {
        #[cfg_attr(not(target_os = "linux"), allow(unused_variables))]
        let platform = self.mini.map(|m| m.platform);
        let close = self.close_mini_player();
        let Some(main) = self.main_window else {
            return close;
        };
        #[cfg_attr(not(target_os = "linux"), allow(unused_mut))]
        let mut tasks = vec![
            close,
            iced::window::minimize(main, false),
            iced::window::gain_focus(main),
        ];
        #[cfg(target_os = "linux")]
        if platform.is_some_and(MiniPlatform::hyprland) {
            tasks.push(iced::Task::perform(
                blocking(yutu_desktop::hyprland::focus_main),
                |_| Message::NoOp,
            ));
        }
        iced::Task::batch(tasks)
    }

    /// Lyrics ticker on/off: grows or shrinks the window downward and
    /// remembers the choice for the next time the mini player opens.
    pub(crate) fn toggle_mini_lyrics(&mut self) -> iced::Task<Message> {
        let Some(mini) = self.mini.as_mut() else {
            return iced::Task::none();
        };
        mini.lyrics = !mini.lyrics;
        let (id, lyrics, platform) = (mini.id, mini.lyrics, mini.platform);
        self.mini_lyrics_pref = lyrics;
        if let Ok(store) = yutu_core::LibraryStore::open(&self.library_path) {
            let _ = store.set_pref(MINI_LYRICS_PREF, if lyrics { "1" } else { "0" });
        }
        let size = mini_window_size(lyrics, self.mini_scale(), platform);
        #[cfg_attr(not(any(target_os = "linux", target_os = "macos")), allow(unused_mut))]
        let mut resize = mini_resize(platform, id, size);
        // macOS: pin again right before resizing (the Metal layer may
        // have been created after the open-time pin).
        #[cfg(target_os = "macos")]
        if platform.pins_layer_gravity() {
            resize = macos_pin_layers(id).chain(resize);
        }
        // Hyprland keeps a resized floating window's center, so the
        // ticker would grow up and down: read the top-left first, then
        // put it back once the new size is applied.
        #[cfg(target_os = "linux")]
        if platform.hyprland() {
            use yutu_desktop::hyprland as hl;
            let want = (size.width.round() as i64, size.height.round() as i64);
            resize = hypr_mini_window().then(move |before| {
                let resize = mini_resize(platform, id, size);
                let Some(w) = before else { return resize };
                resize.chain(iced::Task::perform(
                    blocking(move || {
                        let _ = hl::wait_class_window(
                            MINI_APP_ID,
                            Some(want),
                            std::time::Duration::from_secs(1),
                        );
                        hl::move_window(&w.address, w.at.0, w.at.1)
                    }),
                    |_| Message::NoOp,
                ))
            });
        }
        let mut tasks = vec![resize];
        if lyrics {
            tasks.extend(self.mini_lyrics_fetch());
        }
        iced::Task::batch(tasks)
    }

    /// Lyrics for the live song when nothing fetched them yet (a
    /// restored session never started the track).
    fn mini_lyrics_fetch(&mut self) -> Vec<iced::Task<Message>> {
        #[cfg(any(feature = "lyrics", feature = "innertube"))]
        if let Some(t) = self.queue.current().cloned() {
            return self.start_lyrics_fetch(t);
        }
        Vec::new()
    }

    /// The mini window was closed from outside (compositor shortcut):
    /// forget it so the bar button opens a fresh one.
    pub(crate) fn mini_window_closed(&mut self, id: iced::window::Id) {
        if self.mini.is_some_and(|m| m.id == id) {
            self.mini = None;
            self.save_mini_pos();
        }
    }

    /// Window title per window (the mini player names itself so task
    /// switchers can tell the two apart).
    pub fn title_for(&self, window: iced::window::Id) -> String {
        if self.mini.is_some_and(|m| m.id == window) {
            format!("{} · {}", self.language.mini_player(), self.window_title())
        } else {
            self.window_title()
        }
    }

    /// Theme per window: the mini window paints only its capsule.
    pub fn theme_for(&self, window: iced::window::Id) -> iced::Theme {
        if let Some(mini) = self.mini.filter(|m| m.id == window) {
            mini_theme(mini.platform)
        } else {
            self.theme()
        }
    }

    /// View per window: the mini player for its window, the app for
    /// every other.
    pub fn view_for(&self, window: iced::window::Id) -> iced::Element<'_, Message> {
        if self.mini.is_some_and(|m| m.id == window) {
            self.mini_view()
        } else {
            self.view()
        }
    }

    /// The mini player content (capsule, chips, ticker).
    pub(crate) fn mini_view(&self) -> iced::Element<'_, Message> {
        use iced::widget::{column, container, mouse_area, row, Space};
        use iced::Fill;
        let Some(mini) = self.mini else {
            return Space::new().into();
        };
        let opaque = mini.platform.opaque_window();
        let chips: iced::Element<'_, Message> = if mini.hovered {
            row![
                Space::new().width(Fill),
                self.mini_chip_button(
                    "maximize-2",
                    self.language.mini_open(),
                    Message::MiniRestore
                ),
                self.mini_chip_button("x", self.language.mini_close(), Message::MiniClose),
            ]
            .spacing(6)
            .height(iced::Length::Fixed(MINI_CHIPS_H))
            .into()
        } else {
            Space::new()
                .width(Fill)
                .height(iced::Length::Fixed(MINI_CHIPS_H))
                .into()
        };
        let mut body = column![self.mini_header_row(mini.lyrics, opaque && mini.hovered)];
        if mini.lyrics {
            body = body.push(self.mini_ticker());
        }
        let radius = if opaque {
            MINI_WIN_RADIUS
        } else if mini.lyrics {
            26.0
        } else {
            MINI_PILL_H / 2.0
        };
        let body_h = if mini.lyrics {
            MINI_PILL_H + MINI_LYRICS_H
        } else {
            MINI_PILL_H
        };
        let capsule = self.mini_capsule(body.into(), radius, body_h, !opaque);
        // Opaque window (Windows): the window is the capsule itself, no
        // chip row or shadow margin (they would paint black).
        let content: iced::Element<'_, Message> = if opaque {
            capsule
        } else {
            container(
                column![chips, capsule]
                    .spacing(MINI_CHIPS_GAP)
                    .width(iced::Length::Fixed(MINI_W)),
            )
            .padding(MINI_MARGIN)
            .width(Fill)
            .height(Fill)
            .into()
        };
        mouse_area(content)
            .on_enter(Message::MiniHover(true))
            .on_exit(Message::MiniHover(false))
            .into()
    }

    /// Opaque window (Windows): Open/Close as small icon buttons in the
    /// header row (in place of the title while hovered), labelled by a
    /// tooltip.
    fn mini_inline_chip(
        &self,
        icon_name: &'static str,
        label: String,
        msg: Message,
    ) -> iced::Element<'static, Message> {
        use iced::widget::{button, text, tooltip};
        tooltip(
            button(ctrl_box(btn_icon(
                icon_name,
                12.0,
                iced::Color::from_rgb8(0xE0, 0xE0, 0xE0),
            )))
            .style(mini_icon_hover)
            .on_press(msg)
            .padding(5),
            text(label).size(11),
            tooltip::Position::Bottom,
        )
        .gap(6)
        .into()
    }

    fn mini_chip_button(
        &self,
        icon_name: &'static str,
        label: String,
        msg: Message,
    ) -> iced::Element<'static, Message> {
        use iced::widget::{button, row, text};
        button(
            row![
                btn_icon(icon_name, 12.0, iced::Color::from_rgb8(0xE0, 0xE0, 0xE0)),
                text(label).size(11),
            ]
            .spacing(6)
            .align_y(iced::alignment::Vertical::Center),
        )
        .style(mini_chip)
        .on_press(msg)
        .padding([5, 10])
        .height(iced::Length::Fixed(MINI_CHIPS_H))
        .into()
    }

    /// Capsule surface: base color, the cover glow, a readability veil,
    /// then the content, all clipped to the same rounded shape.
    fn mini_capsule<'a>(
        &'a self,
        content: iced::Element<'a, Message>,
        radius: f32,
        height: f32,
        shadow: bool,
    ) -> iced::Element<'a, Message> {
        use iced::widget::{container, image, Space, Stack};
        use iced::Fill;
        let blur = self.queue.current().and_then(|t| self.track_blur(t));
        let rounded = move |bg: iced::Color| {
            move |_: &iced::Theme| container::Style {
                background: Some(iced::Background::Color(bg)),
                border: iced::Border::default().rounded(radius),
                ..Default::default()
            }
        };
        // Fixed four slots (surface, glow, veil, content): a changing
        // layer count would rebuild the content on every song change.
        let (glow, veil): (iced::Element<'a, Message>, iced::Element<'a, Message>) = match blur {
            Some(handle) => (
                image(handle)
                    .content_fit(iced::ContentFit::Cover)
                    .filter_method(image::FilterMethod::Linear)
                    .opacity(MINI_GLOW_OPACITY)
                    .border_radius(radius)
                    .width(Fill)
                    .height(Fill)
                    .into(),
                container(Space::new())
                    .style(rounded(MINI_VEIL))
                    .width(Fill)
                    .height(Fill)
                    .into(),
            ),
            None => (Space::new().into(), Space::new().into()),
        };
        let shell = container(Space::new())
            .style(move |_: &iced::Theme| container::Style {
                background: Some(iced::Background::Color(MINI_SURFACE)),
                border: iced::Border {
                    color: iced::Color::from_rgba(1.0, 1.0, 1.0, 0.08),
                    width: 1.0,
                    radius: radius.into(),
                },
                shadow: if shadow {
                    iced::Shadow {
                        color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.5),
                        offset: iced::Vector::new(0.0, 3.0),
                        blur_radius: 8.0,
                    }
                } else {
                    iced::Shadow::default()
                },
                ..Default::default()
            })
            .width(Fill)
            .height(Fill);
        Stack::with_children(vec![shell.into(), glow, veil, content])
            .width(Fill)
            .height(iced::Length::Fixed(height))
            .into()
    }

    /// Header row: ring art and title (the drag handle), transport,
    /// lyrics toggle; with `inline_chips` (opaque window, hovered) the
    /// Open/Close actions as icon buttons in place of the title.
    fn mini_header_row(&self, lyrics_open: bool, inline_chips: bool) -> iced::Element<'_, Message> {
        use iced::widget::{button, column, container, mouse_area, row, text, text::Wrapping};
        use iced::Fill;
        let current = self.queue.current();
        let loading = self.loading_track.is_some();
        let title = if loading {
            self.language.loading_ellipsis()
        } else {
            current.map(|t| t.title.clone()).unwrap_or_default()
        };
        let artist = current.map(|t| t.artist.clone()).unwrap_or_default();
        let duration = self.audio_duration();
        let frac = if duration > 0.0 {
            (self.position_secs / duration) as f32
        } else {
            0.0
        };
        // Inline chips (opaque window, hovered) take the title's place,
        // so neither is squeezed; the title is back once the pointer
        // leaves. The space around them still drags the window.
        let info: iced::Element<'_, Message> = if inline_chips {
            row![
                self.mini_inline_chip(
                    "maximize-2",
                    self.language.mini_open(),
                    Message::MiniRestore
                ),
                self.mini_inline_chip("x", self.language.mini_close(), Message::MiniClose),
            ]
            .spacing(4)
            .into()
        } else {
            column![
                text(truncate_title(&title, MINI_TITLE_CHARS))
                    .size(13)
                    .font(bold_font())
                    .wrapping(Wrapping::None),
                text(truncate_title(&artist, MINI_TITLE_CHARS))
                    .size(11)
                    .color(iced::Color::from_rgb8(0xE6, 0xD3, 0xDA))
                    .wrapping(Wrapping::None),
            ]
            .spacing(2)
            .into()
        };
        let handle = row![
            self.mini_ring_art(frac),
            container(info).width(Fill).clip(true),
        ]
        .spacing(10)
        .align_y(iced::alignment::Vertical::Center);
        let play_icon = if self.paused { "play" } else { "pause" };
        let lyrics_btn: iced::Element<'_, Message> = if lyrics_open {
            button(ctrl_box(btn_icon("mic-vocal", 14.0, iced::Color::WHITE)))
                .style(accent_button)
                .on_press(Message::MiniToggleLyrics)
                .padding(7)
                .into()
        } else {
            mini_icon_button("mic-vocal", Message::MiniToggleLyrics).into()
        };
        let controls = row![
            mouse_area(handle)
                .on_press(Message::MiniDrag)
                .interaction(iced::mouse::Interaction::Grab),
            mini_icon_button("skip-back", Message::Prev),
            button(ctrl_box(btn_icon(play_icon, 16.0, iced::Color::BLACK)))
                .style(mini_play_button)
                .on_press(Message::TogglePause)
                .padding(9),
            mini_icon_button("skip-forward", Message::Next),
            lyrics_btn,
        ]
        .spacing(6)
        .align_y(iced::alignment::Vertical::Center);
        container(controls)
            .padding(iced::Padding {
                top: 0.0,
                right: 10.0,
                bottom: 0.0,
                left: 4.0,
            })
            .height(iced::Length::Fixed(MINI_PILL_H))
            .align_y(iced::alignment::Vertical::Center)
            .into()
    }

    /// Ring cover downscale for the current song, once its hero art is
    /// on disk (the hero is fetched when a song starts; run on Tick).
    pub(crate) fn mini_cover_task(&mut self) -> iced::Task<Message> {
        if self.mini.is_none() {
            return iced::Task::none();
        }
        let Some(url) = self.queue.current().and_then(|t| self.hero_art_for(t)) else {
            return iced::Task::none();
        };
        if self.mini_cover.as_ref().is_some_and(|(u, _)| *u == url)
            || self.mini_cover_loading.as_ref() == Some(&url)
        {
            return iced::Task::none();
        }
        let Some(path) = artwork_path_for(&url).filter(|p| std::path::Path::new(p).exists()) else {
            return iced::Task::none();
        };
        self.mini_cover_loading = Some(url.clone());
        iced::Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    let img = image::open(&path).ok()?;
                    Some(
                        img.resize_to_fill(
                            MINI_COVER_PX,
                            MINI_COVER_PX,
                            image::imageops::FilterType::Triangle,
                        )
                        .to_rgba8()
                        .into_raw(),
                    )
                })
                .await
                .ok()
                .flatten()
            },
            move |rgba| Message::MiniCoverReady {
                url: url.clone(),
                rgba,
            },
        )
    }

    pub(crate) fn mini_cover_ready(&mut self, url: String, rgba: Option<Vec<u8>>) {
        if self.mini_cover_loading.as_ref() == Some(&url) {
            self.mini_cover_loading = None;
        }
        if let Some(rgba) = rgba {
            let handle = iced::widget::image::Handle::from_rgba(MINI_COVER_PX, MINI_COVER_PX, rgba);
            self.mini_cover = Some((url, handle));
        }
    }

    /// Round cover inside the progress ring (fixed size placeholder
    /// until the art lands, so the row never jumps). The filtered hero
    /// downscale (`mini_cover`) keeps it sharp on hidpi screens; the
    /// 120px row raster is only the fallback until it lands.
    fn mini_ring_art(&self, frac: f32) -> iced::Element<'_, Message> {
        use iced::widget::{canvas, container, image, Stack};
        let handle = self.queue.current().and_then(|t| {
            let hero = self.hero_art_for(t);
            self.mini_cover
                .as_ref()
                .filter(|(u, _)| hero.as_ref() == Some(u))
                .map(|(_, h)| h.clone())
                .or_else(|| {
                    self.art_url_for(t, MINI_ART)
                        .and_then(|u| self.art_handle(&u))
                })
        });
        let art: iced::Element<'_, Message> = match handle {
            Some(h) => image(h)
                .content_fit(iced::ContentFit::Cover)
                .border_radius(MINI_ART / 2.0)
                .width(iced::Length::Fixed(MINI_ART))
                .height(iced::Length::Fixed(MINI_ART))
                .into(),
            None => container(btn_icon("music", 16.0, MUTED))
                .style(|_: &iced::Theme| container::Style {
                    background: Some(iced::Background::Color(iced::Color::from_rgb8(
                        0x2A, 0x2A, 0x2A,
                    ))),
                    border: iced::Border::default().rounded(MINI_ART / 2.0),
                    ..Default::default()
                })
                .width(iced::Length::Fixed(MINI_ART))
                .height(iced::Length::Fixed(MINI_ART))
                .center_x(iced::Length::Fixed(MINI_ART))
                .center_y(iced::Length::Fixed(MINI_ART))
                .into(),
        };
        Stack::with_children(vec![
            canvas(ProgressRing { frac })
                .width(iced::Length::Fixed(MINI_RING))
                .height(iced::Length::Fixed(MINI_RING))
                .into(),
            container(art)
                .center_x(iced::Length::Fixed(MINI_RING))
                .center_y(iced::Length::Fixed(MINI_RING))
                .into(),
        ])
        .width(iced::Length::Fixed(MINI_RING))
        .height(iced::Length::Fixed(MINI_RING))
        .into()
    }

    /// Unsynced lyrics in the ticker: every line wrapped (never cut),
    /// blank lines kept as stanza gaps, scrolled by hand with the slim
    /// bar that shows on hover.
    fn mini_plain_lyrics(&self, plain: &str) -> iced::Element<'_, Message> {
        use iced::widget::{column, scrollable, text, text::Wrapping, Space};
        use iced::Fill;
        let lines: Vec<iced::Element<'_, Message>> = plain
            .trim()
            .lines()
            .map(str::trim)
            .map(|l| -> iced::Element<'_, Message> {
                if l.is_empty() {
                    Space::new().height(iced::Length::Fixed(6.0)).into()
                } else {
                    text(l.to_string())
                        .size(13)
                        .color(iced::Color::from_rgba(1.0, 1.0, 1.0, 0.72))
                        .align_x(iced::alignment::Horizontal::Center)
                        .wrapping(Wrapping::WordOrGlyph)
                        .width(Fill)
                        .into()
                }
            })
            .collect();
        scrollable(column(lines).spacing(6).padding(iced::Padding {
            right: 10.0,
            ..iced::Padding::ZERO
        }))
        .direction(scrollable::Direction::Vertical(slim_bar()))
        .style(|t, s| slim_scroll_alpha(t, s, 0.0))
        .width(Fill)
        .height(Fill)
        .into()
    }

    /// Five-line synced ticker: the current verse large and white, its
    /// neighbors smaller and fading with distance. Unsynced lyrics,
    /// instrumentals, loading and empty states show one quiet line.
    fn mini_ticker(&self) -> iced::Element<'_, Message> {
        use iced::widget::{container, text};
        use iced::Fill;
        let id = self.queue.current().map(|t| t.id.clone());
        let notice = |s: String| -> iced::Element<'_, Message> {
            text(s)
                .size(13)
                .color(MUTED)
                .align_x(iced::alignment::Horizontal::Center)
                .into()
        };
        let body: iced::Element<'_, Message> = match id.as_deref() {
            None => notice(self.language.nothing_playing()),
            Some(id) => match self.lyrics_display_entry(id) {
                Some((_, cached)) if cached.instrumental => notice(self.language.instrumental()),
                Some((_, cached)) if !cached.synced.is_empty() => {
                    let visible: Vec<&str> = cached
                        .synced
                        .iter()
                        .map(|l| l.text.trim())
                        .filter(|t| !t.is_empty())
                        .collect();
                    let at = self.lyric_cursor_at(id);
                    return self.mini_wheel(&visible, at);
                }
                // Plain lyrics carry no timing: the whole text, wrapped,
                // static and scrollable by hand, so the ticker never
                // pretends to follow. Keyed by song, so a new song
                // starts at the top.
                Some((_, cached)) => iced::widget::keyed_column([(
                    {
                        use std::hash::{Hash, Hasher};
                        let mut h = std::collections::hash_map::DefaultHasher::new();
                        id.hash(&mut h);
                        h.finish()
                    },
                    self.mini_plain_lyrics(&cached.plain),
                )])
                .height(Fill)
                .into(),
                None if self.lyrics_fetch_pending(id) => {
                    notice(self.language.loading_lyrics(self.lyrics_provider.label()))
                }
                None => notice(self.language.no_lyrics()),
            },
        };
        container(body)
            .padding([MINI_TICKER_PAD.0, MINI_TICKER_PAD.1])
            .width(Fill)
            .height(iced::Length::Fixed(MINI_LYRICS_H))
            .center_x(Fill)
            .center_y(iced::Length::Fixed(MINI_LYRICS_H))
            .clip(true)
            .into()
    }
}
