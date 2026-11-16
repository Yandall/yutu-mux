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
//!     the pointer is over the mini window.
//!   - The art and title are the drag handle (no title bar).
//!   - Sizes are logical units times the UI scale, so the window always
//!     fits its content at any scale.

use super::*;

/// Pref remembering whether the ticker was open ("1") last time.
pub(crate) const MINI_LYRICS_PREF: &str = "mini_lyrics";
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
/// Char budget for the title/artist beside the controls.
const MINI_TITLE_CHARS: usize = 18;
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
}

/// Content size in UI units: capsule plus chip row plus shadow margin.
pub(crate) fn mini_content_size(lyrics: bool) -> iced::Size {
    let body = if lyrics {
        MINI_PILL_H + MINI_LYRICS_H
    } else {
        MINI_PILL_H
    };
    iced::Size::new(
        MINI_W + 2.0 * MINI_MARGIN,
        MINI_CHIPS_H + MINI_CHIPS_GAP + body + 2.0 * MINI_MARGIN,
    )
}

/// Logical window size: the content scaled by the UI scale, so the
/// layout never clips when the user enlarges the interface.
pub(crate) fn mini_window_size(lyrics: bool, scale: f32) -> iced::Size {
    let c = mini_content_size(lyrics);
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
/// windows where the platform allows it.
pub(crate) fn mini_window_settings(lyrics: bool, scale: f32) -> iced::window::Settings {
    let base = window_settings();
    iced::window::Settings {
        size: mini_window_size(lyrics, scale),
        min_size: None,
        position: iced::window::Position::SpecificWith(mini_start_position),
        resizable: false,
        decorations: false,
        transparent: true,
        minimizable: false,
        level: iced::window::Level::AlwaysOnTop,
        ..base
    }
}

/// Mini window theme: the dark theme with a transparent window
/// background, so only the capsule paints.
pub(crate) fn mini_theme() -> iced::Theme {
    let palette = iced::theme::Palette {
        background: iced::Color::TRANSPARENT,
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
            btn.on_press(Message::OpenMiniPlayer).padding(7),
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
        let (id, open) = iced::window::open(mini_window_settings(lyrics, self.mini_scale()));
        self.mini = Some(MiniPlayer {
            id,
            lyrics,
            hovered: false,
        });
        let mut tasks = vec![open.discard()];
        if lyrics {
            tasks.extend(self.mini_lyrics_fetch());
        }
        iced::Task::batch(tasks)
    }

    /// Close the mini player window (playback keeps going).
    pub(crate) fn close_mini_player(&mut self) -> iced::Task<Message> {
        match self.mini.take() {
            Some(mini) => iced::window::close(mini.id),
            None => iced::Task::none(),
        }
    }

    /// Close the mini player and bring the main window forward
    /// (best effort: some compositors refuse focus requests).
    pub(crate) fn restore_from_mini(&mut self) -> iced::Task<Message> {
        let close = self.close_mini_player();
        let Some(main) = self.main_window else {
            return close;
        };
        iced::Task::batch([
            close,
            iced::window::minimize(main, false),
            iced::window::gain_focus(main),
        ])
    }

    /// Lyrics ticker on/off: grows or shrinks the window downward and
    /// remembers the choice for the next time the mini player opens.
    pub(crate) fn toggle_mini_lyrics(&mut self) -> iced::Task<Message> {
        let Some(mini) = self.mini.as_mut() else {
            return iced::Task::none();
        };
        mini.lyrics = !mini.lyrics;
        let (id, lyrics) = (mini.id, mini.lyrics);
        self.mini_lyrics_pref = lyrics;
        if let Ok(store) = yutu_core::LibraryStore::open(&self.library_path) {
            let _ = store.set_pref(MINI_LYRICS_PREF, if lyrics { "1" } else { "0" });
        }
        let mut tasks = vec![iced::window::resize(
            id,
            mini_window_size(lyrics, self.mini_scale()),
        )];
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
        if self.mini.is_some_and(|m| m.id == window) {
            mini_theme()
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
        let mut body = column![self.mini_header_row(mini.lyrics)];
        if mini.lyrics {
            body = body.push(self.mini_ticker());
        }
        let radius = if mini.lyrics { 26.0 } else { MINI_PILL_H / 2.0 };
        let body_h = if mini.lyrics {
            MINI_PILL_H + MINI_LYRICS_H
        } else {
            MINI_PILL_H
        };
        let capsule = self.mini_capsule(body.into(), radius, body_h);
        let content = column![chips, capsule]
            .spacing(MINI_CHIPS_GAP)
            .width(iced::Length::Fixed(MINI_W));
        mouse_area(
            container(content)
                .padding(MINI_MARGIN)
                .width(Fill)
                .height(Fill),
        )
        .on_enter(Message::MiniHover(true))
        .on_exit(Message::MiniHover(false))
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
                shadow: iced::Shadow {
                    color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.5),
                    offset: iced::Vector::new(0.0, 3.0),
                    blur_radius: 8.0,
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
    /// lyrics toggle.
    fn mini_header_row(&self, lyrics_open: bool) -> iced::Element<'_, Message> {
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
        let handle = row![
            self.mini_ring_art(frac),
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
            .width(Fill),
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
        container(
            row![
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
            .align_y(iced::alignment::Vertical::Center),
        )
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

    /// Round cover inside the progress ring (fixed size placeholder
    /// until the art lands, so the row never jumps).
    fn mini_ring_art(&self, frac: f32) -> iced::Element<'_, Message> {
        use iced::widget::{canvas, container, image, Stack};
        let handle = self
            .queue
            .current()
            .and_then(|t| self.art_url_for(t, MINI_ART))
            .and_then(|u| self.art_handle(&u));
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

    /// Five-line synced ticker: the current verse large and white, its
    /// neighbors smaller and fading with distance. Unsynced lyrics,
    /// instrumentals, loading and empty states show one quiet line.
    fn mini_ticker(&self) -> iced::Element<'_, Message> {
        use iced::widget::{column, container, text, text::Wrapping};
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
                    let lines: Vec<iced::Element<'_, Message>> = ticker_window(visible.len(), at)
                        .into_iter()
                        .map(|(i, d)| {
                            let (size, color) = match d.abs() {
                                0 => (21.0, iced::Color::WHITE),
                                1 => (14.0, iced::Color::from_rgba(1.0, 1.0, 1.0, 0.72)),
                                _ => (12.0, iced::Color::from_rgba(1.0, 1.0, 1.0, 0.5)),
                            };
                            let mut line = text(visible[i].to_string())
                                .size(size)
                                .color(color)
                                .align_x(iced::alignment::Horizontal::Center)
                                .wrapping(Wrapping::WordOrGlyph)
                                .width(Fill);
                            if d == 0 {
                                line = line.font(bold_font());
                            }
                            line.into()
                        })
                        .collect();
                    column(lines)
                        .spacing(10)
                        .align_x(iced::alignment::Horizontal::Center)
                        .into()
                }
                // Plain lyrics carry no timing: the opening lines,
                // static, so the ticker never pretends to follow.
                Some((_, cached)) => column(
                    cached
                        .plain
                        .lines()
                        .map(str::trim)
                        .filter(|l| !l.is_empty())
                        .take(5)
                        .map(|l| {
                            text(l.to_string())
                                .size(13)
                                .color(iced::Color::from_rgba(1.0, 1.0, 1.0, 0.72))
                                .align_x(iced::alignment::Horizontal::Center)
                                .width(Fill)
                                .into()
                        })
                        .collect::<Vec<_>>(),
                )
                .spacing(8)
                .into(),
                None if self.lyrics_fetch_pending(id) => {
                    notice(self.language.loading_lyrics(self.lyrics_provider.label()))
                }
                None => notice(self.language.no_lyrics()),
            },
        };
        container(body)
            .padding([16, 24])
            .width(Fill)
            .height(iced::Length::Fixed(MINI_LYRICS_H))
            .center_x(Fill)
            .center_y(iced::Length::Fixed(MINI_LYRICS_H))
            .clip(true)
            .into()
    }
}
