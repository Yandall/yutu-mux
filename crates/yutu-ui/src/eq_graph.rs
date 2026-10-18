//! Graphic-EQ canvas widget: ten vertical drag sliders under one smooth
//! curve, dB values on top, frequencies below. Theme-colored (red accent
//! on dark cards, like every other control).
//!
//! Layout and curve math are pure and unit-tested here; the [`Program`]
//! only wires pointer events to `Message::SetEqBand` (press jumps,
//! drag slides, release drops). Slider rows elsewhere are gone: presets
//! are predefined curves for these same gains.

use crate::{Message, MUTED, SPOT_GREEN};
use iced::advanced::text::Alignment;
use iced::widget::canvas::{
    self, Action, Event, Frame, Geometry, LineCap, Path, Program, Stroke, Text,
};
use iced::{Color, Length, Point, Rectangle, Size};
use yutu_player::{BAND_COUNT, EQ_FREQUENCIES, MAX_GAIN_DB, MIN_GAIN_DB};

/// Canvas height: bars + top values + bottom labels.
pub const GRAPH_HEIGHT: f32 = 232.0;
const PAD_LEFT: f32 = 30.0;
const PAD_RIGHT: f32 = 10.0;
const PAD_TOP: f32 = 24.0;
const PAD_BOTTOM: f32 = 22.0;
const HANDLE_R: f32 = 8.0;
const BAR_W: f32 = 5.0;
const TRACK_W: f32 = 3.0;

/// Track gray matches the themed horizontal sliders (same rail color).
const TRACK_GRAY: Color = Color::from_rgb8(0x3A, 0x3A, 0x3A);

/// Plot geometry for one canvas size: plot rect + band column centers.
pub struct GraphLayout {
    pub left: f32,
    pub top: f32,
    pub plot_w: f32,
    pub plot_h: f32,
    pub centers: [f32; BAND_COUNT],
    pub spacing: f32,
}

/// Plot geometry: axis gutter left, labels top/bottom, columns spread
/// across the rest. Degenerate sizes collapse to a zero plot (draw
/// guards skip, hit tests miss).
#[must_use]
pub fn layout(size: Size) -> GraphLayout {
    let left = PAD_LEFT;
    let top = PAD_TOP;
    let plot_w = (size.width - PAD_LEFT - PAD_RIGHT).max(0.0);
    let plot_h = (size.height - PAD_TOP - PAD_BOTTOM).max(0.0);
    let spacing = if BAND_COUNT > 1 {
        plot_w / BAND_COUNT as f32
    } else {
        0.0
    };
    let mut centers = [0.0f32; BAND_COUNT];
    for (i, c) in centers.iter_mut().enumerate() {
        *c = left + spacing * (i as f32 + 0.5);
    }
    GraphLayout {
        left,
        top,
        plot_w,
        plot_h,
        centers,
        spacing,
    }
}

/// Gain to canvas y (top = max boost, bottom = max cut).
#[must_use]
pub fn gain_to_y(gain_db: f32, layout: &GraphLayout) -> f32 {
    layout.top + (MAX_GAIN_DB - gain_db) / (MAX_GAIN_DB - MIN_GAIN_DB) * layout.plot_h
}

/// Canvas y to gain: clamped to +/-12 dB, quantized to 0.5 dB slider
/// steps (the same steps presets use, so exact-match relabeling holds).
#[must_use]
pub fn y_to_gain(y: f32, layout: &GraphLayout) -> f32 {
    if layout.plot_h <= 0.0 {
        return 0.0;
    }
    let raw = MAX_GAIN_DB - (y - layout.top) / layout.plot_h * (MAX_GAIN_DB - MIN_GAIN_DB);
    ((raw * 2.0).round() / 2.0).clamp(MIN_GAIN_DB, MAX_GAIN_DB)
}

/// Column under x (press must land near a slider, not in the gutter).
/// Within-plot x always maps to the nearest column (edges clamp).
#[must_use]
pub fn column_at(x: f32, layout: &GraphLayout) -> Option<usize> {
    if layout.spacing <= 0.0 || x < layout.left || x > layout.left + layout.plot_w {
        return None;
    }
    let i = ((x - layout.left) / layout.spacing) as usize;
    Some(i.min(BAND_COUNT - 1))
}

/// Frequency label under each column ("31" … "16K", like hardware).
#[must_use]
pub fn freq_label(freq_hz: f32) -> String {
    if freq_hz >= 1000.0 {
        format!("{}K", freq_hz / 1000.0)
    } else {
        format!("{freq_hz}")
    }
}

/// dB value above each column (one decimal, bare zero like hardware).
#[must_use]
pub fn gain_label(gain_db: f32) -> String {
    if gain_db == 0.0 {
        "0.0".to_string()
    } else {
        format!("{gain_db:.1}")
    }
}

/// Catmull-Rom control points for one segment (p1 -> p2 with neighbors
/// p0, p3): the smooth curve through the handles.
fn segment_controls(p0: Point, p1: Point, p2: Point, p3: Point) -> (Point, Point) {
    let c1 = Point::new(p1.x + (p2.x - p0.x) / 6.0, p1.y + (p2.y - p0.y) / 6.0);
    let c2 = Point::new(p2.x - (p3.x - p1.x) / 6.0, p2.y - (p3.y - p1.y) / 6.0);
    (c1, c2)
}

/// Smooth path through the handle points (Catmull-Rom as beziers,
/// endpoints duplicated). Empty input builds an empty path.
fn curve_path(points: &[Point]) -> Path {
    Path::new(|b| {
        let Some(first) = points.first() else { return };
        b.move_to(*first);
        if points.len() < 2 {
            return;
        }
        for i in 0..points.len() - 1 {
            let p0 = if i == 0 { points[0] } else { points[i - 1] };
            let p3 = if i + 2 < points.len() {
                points[i + 2]
            } else {
                points[points.len() - 1]
            };
            let (c1, c2) = segment_controls(p0, points[i], points[i + 1], p3);
            b.bezier_curve_to(c1, c2, points[i + 1]);
        }
    })
}

/// Area path under the curve (curve, then down to the plot bottom and
/// closed) for the translucent wash.
fn area_path(points: &[Point], bottom: f32) -> Path {
    Path::new(|b| {
        let Some(first) = points.first() else { return };
        let Some(last) = points.last() else { return };
        b.move_to(*first);
        if points.len() >= 2 {
            for i in 0..points.len() - 1 {
                let p0 = if i == 0 { points[0] } else { points[i - 1] };
                let p3 = if i + 2 < points.len() {
                    points[i + 2]
                } else {
                    points[points.len() - 1]
                };
                let (c1, c2) = segment_controls(p0, points[i], points[i + 1], p3);
                b.bezier_curve_to(c1, c2, points[i + 1]);
            }
        }
        b.line_to(Point::new(last.x, bottom));
        b.line_to(Point::new(first.x, bottom));
        b.close();
    })
}

/// The canvas program: owns a gains snapshot (fresh props per view),
/// drag column in state. Press jumps, drag slides, release drops;
/// messages fire only on real value changes (plus the app throttle).
pub struct EqGraph {
    pub gains: [f32; BAND_COUNT],
}

impl Program<Message> for EqGraph {
    type State = Option<usize>;

    fn update(
        &self,
        state: &mut Self::State,
        event: &Event,
        bounds: Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> Option<Action<Message>> {
        let layout = layout(bounds.size());
        match event {
            Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)) => {
                if layout.plot_h <= 0.0 {
                    return None;
                }
                let p = cursor.position_in(bounds)?;
                // Labels are not sliders: value/dB rows must not slam
                // bands to full boost/cut.
                if p.y < layout.top || p.y > layout.top + layout.plot_h {
                    return None;
                }
                let i = column_at(p.x, &layout)?;
                *state = Some(i);
                let gain = y_to_gain(p.y, &layout);
                if gain != self.gains[i] {
                    Some(Action::publish(Message::SetEqBand(i, gain)).and_capture())
                } else {
                    Some(Action::capture())
                }
            }
            Event::Mouse(iced::mouse::Event::CursorMoved { .. }) => {
                let i = (*state)?;
                let p = match cursor.position_in(bounds) {
                    Some(p) => p,
                    // Overshot the widget mid-drag: pin to the plot edge
                    // (`y_to_gain` clamps) instead of freezing short.
                    None => {
                        let abs = cursor.position()?;
                        Point::new(abs.x - bounds.x, abs.y - bounds.y)
                    }
                };
                let gain = y_to_gain(p.y, &layout);
                if gain != self.gains[i] {
                    Some(Action::publish(Message::SetEqBand(i, gain)).and_capture())
                } else {
                    Some(Action::capture())
                }
            }
            Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                state.take().map(|_| Action::capture())
            }
            Event::Keyboard(iced::keyboard::Event::KeyPressed { key, .. }) => {
                // Arrow keys nudge the hovered column (the horizontal
                // sliders this replaced were keyboard-adjustable).
                let delta = match key {
                    iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowUp)
                    | iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowRight) => 0.5,
                    iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowDown)
                    | iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowLeft) => -0.5,
                    _ => return None,
                };
                let p = cursor.position_in(bounds)?;
                let i = column_at(p.x, &layout)?;
                let gain = (self.gains[i] + delta).clamp(MIN_GAIN_DB, MAX_GAIN_DB);
                if gain != self.gains[i] {
                    *state = Some(i);
                    Some(Action::publish(Message::SetEqBand(i, gain)).and_capture())
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        _bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> iced::mouse::Interaction {
        if state.is_some() {
            iced::mouse::Interaction::Grabbing
        } else {
            iced::mouse::Interaction::default()
        }
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let layout = layout(bounds.size());
        if layout.plot_w <= 0.0 || layout.plot_h <= 0.0 {
            return vec![];
        }
        let mut frame = Frame::new(renderer, bounds.size());
        let bottom = layout.top + layout.plot_h;
        // Narrow windows: shrink handles/bars/type with the column
        // pitch so labels never soup (no panic either way).
        let scale = (layout.spacing / 32.0).clamp(0.55, 1.0);
        let handle_r = HANDLE_R * scale;
        let bar_w = (BAR_W * scale).max(2.0);
        let track_w = (TRACK_W * scale).max(1.0);
        let label_size = 11.0 * scale;
        let axis_size = 10.0 * scale;

        // Axis hairlines + labels (+12 / 0 / -12).
        for gain in [MAX_GAIN_DB, 0.0, MIN_GAIN_DB] {
            let y = gain_to_y(gain, &layout);
            frame.stroke(
                &Path::line(
                    Point::new(layout.left, y),
                    Point::new(layout.left + layout.plot_w, y),
                ),
                Stroke::default()
                    .with_color(Color {
                        a: 0.10,
                        ..Color::WHITE
                    })
                    .with_width(1.0),
            );
            frame.fill_text(Text {
                content: gain_label(gain),
                position: Point::new(layout.left - 5.0, y),
                color: MUTED,
                size: axis_size.into(),
                align_x: Alignment::Right,
                align_y: iced::alignment::Vertical::Center,
                ..Text::default()
            });
        }

        let handles: Vec<Point> = layout
            .centers
            .iter()
            .zip(self.gains.iter())
            .map(|(x, gain)| Point::new(*x, gain_to_y(*gain, &layout)))
            .collect();

        // Column hairlines (faint hardware grid).
        for x in &layout.centers {
            frame.stroke(
                &Path::line(Point::new(*x, layout.top), Point::new(*x, bottom)),
                Stroke::default()
                    .with_color(Color {
                        a: 0.08,
                        ..Color::WHITE
                    })
                    .with_width(1.0),
            );
        }

        // Wash under the curve.
        frame.fill(
            &area_path(&handles, bottom),
            Color {
                a: 0.15,
                ..SPOT_GREEN
            },
        );

        // Track (full) + fill (handle to bottom) per column.
        for handle in &handles {
            frame.stroke(
                &Path::line(
                    Point::new(handle.x, layout.top),
                    Point::new(handle.x, bottom),
                ),
                Stroke::default()
                    .with_color(TRACK_GRAY)
                    .with_width(track_w)
                    .with_line_cap(LineCap::Round),
            );
            frame.stroke(
                &Path::line(*handle, Point::new(handle.x, bottom)),
                Stroke::default()
                    .with_color(SPOT_GREEN)
                    .with_width(bar_w)
                    .with_line_cap(LineCap::Round),
            );
        }

        // Curve through the handles.
        frame.stroke(
            &curve_path(&handles),
            Stroke::default()
                .with_color(SPOT_GREEN)
                .with_width(2.0)
                .with_line_cap(LineCap::Round),
        );

        // Handles on top of the curve.
        for handle in &handles {
            frame.fill(&Path::circle(*handle, handle_r), SPOT_GREEN);
            frame.stroke(
                &Path::circle(*handle, handle_r),
                Stroke::default().with_color(Color::WHITE).with_width(1.5),
            );
        }

        // dB values on top, frequencies below.
        for (i, handle) in handles.iter().enumerate() {
            frame.fill_text(Text {
                content: gain_label(self.gains[i]),
                position: Point::new(handle.x, 8.0),
                color: Color::WHITE,
                size: label_size.into(),
                align_x: Alignment::Center,
                align_y: iced::alignment::Vertical::Top,
                ..Text::default()
            });
            frame.fill_text(Text {
                content: freq_label(EQ_FREQUENCIES[i]),
                position: Point::new(handle.x, bottom + 5.0),
                color: MUTED,
                size: label_size.into(),
                align_x: Alignment::Center,
                align_y: iced::alignment::Vertical::Top,
                ..Text::default()
            });
        }

        vec![frame.into_geometry()]
    }
}

/// Widget constructor: fixed height, full width, fresh gains per view.
#[must_use]
pub fn eq_graph(gains: [f32; BAND_COUNT]) -> iced::widget::canvas::Canvas<EqGraph, Message> {
    canvas::Canvas::new(EqGraph { gains })
        .width(Length::Fill)
        .height(Length::Fixed(GRAPH_HEIGHT))
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::mouse::{self, Button, Cursor};
    use iced::widget::canvas::Event as CanvasEvent;

    fn sized(w: f32, h: f32) -> Size {
        Size::new(w, h)
    }

    #[test]
    fn gains_round_trip_through_y() {
        let l = layout(sized(400.0, GRAPH_HEIGHT));
        assert!(l.plot_h > 0.0);
        assert_eq!(gain_to_y(MAX_GAIN_DB, &l), l.top);
        assert_eq!(gain_to_y(MIN_GAIN_DB, &l), l.top + l.plot_h);
        // Quantized steps survive the round trip.
        let mut g = MIN_GAIN_DB;
        while g <= MAX_GAIN_DB {
            assert_eq!(y_to_gain(gain_to_y(g, &l), &l), g, "gain {g}");
            g += 0.5;
        }
    }

    #[test]
    fn y_clamps_and_quantizes() {
        let l = layout(sized(400.0, GRAPH_HEIGHT));
        assert_eq!(y_to_gain(l.top - 50.0, &l), MAX_GAIN_DB);
        assert_eq!(y_to_gain(l.top + l.plot_h + 50.0, &l), MIN_GAIN_DB);
        // A quarter-step above unity quantizes up (half steps, half
        // away from zero).
        let y = gain_to_y(0.0, &l) - l.plot_h / (MAX_GAIN_DB - MIN_GAIN_DB) * 0.25;
        assert_eq!(y_to_gain(y, &l), 0.5);
        // Degenerate height never divides by zero.
        let flat = layout(sized(400.0, PAD_TOP + PAD_BOTTOM));
        assert_eq!(y_to_gain(10.0, &flat), 0.0);
    }

    #[test]
    fn columns_hit_test() {
        let l = layout(sized(400.0, GRAPH_HEIGHT));
        assert_eq!(l.centers.len(), BAND_COUNT);
        assert_eq!(column_at(l.centers[0], &l), Some(0));
        assert_eq!(column_at(l.centers[9], &l), Some(9));
        // Plot edges clamp to the nearest column; the gutter misses.
        assert_eq!(column_at(l.left, &l), Some(0));
        assert_eq!(column_at(l.left + l.plot_w, &l), Some(9));
        assert_eq!(column_at(0.0, &l), None);
        assert_eq!(column_at(399.0, &l), None);
    }

    #[test]
    fn labels_fit_the_canvas() {
        let l = layout(sized(400.0, GRAPH_HEIGHT));
        // dB row (y 8, size 11) sits above the plot; freq row below it.
        assert!(8.0 + 11.0 <= l.top);
        assert!(l.top + l.plot_h + 5.0 + 11.0 <= GRAPH_HEIGHT);
        // Extreme handles sit exactly on the plot edges.
        assert_eq!(gain_to_y(MAX_GAIN_DB, &l), l.top);
        assert_eq!(gain_to_y(MIN_GAIN_DB, &l), l.top + l.plot_h);
        // First column clears the axis gutter.
        assert!(l.centers[0] - l.left > 10.0);
    }

    #[test]
    fn labels_match_hardware() {
        assert_eq!(freq_label(31.0), "31");
        assert_eq!(freq_label(1000.0), "1K");
        assert_eq!(freq_label(16000.0), "16K");
        assert_eq!(gain_label(0.0), "0.0");
        assert_eq!(gain_label(2.5), "2.5");
        assert_eq!(gain_label(-1.3), "-1.3");
    }

    #[test]
    fn curve_through_collinear_stays_straight() {
        let pts: Vec<Point> = (0..4).map(|i| Point::new(i as f32 * 10.0, 50.0)).collect();
        let (c1, c2) = segment_controls(pts[0], pts[1], pts[2], pts[3]);
        assert_eq!(c1.y, 50.0);
        assert_eq!(c2.y, 50.0);
        // Curve + area builders never panic (smoke: empty, one, many).
        let _ = curve_path(&[]);
        let _ = curve_path(&pts[..1]);
        let _ = curve_path(&pts);
        let _ = area_path(&[], 100.0);
        let _ = area_path(&pts, 100.0);
    }

    fn bounds() -> Rectangle {
        Rectangle {
            x: 0.0,
            y: 0.0,
            width: 400.0,
            height: GRAPH_HEIGHT,
        }
    }

    #[test]
    fn press_jumps_and_drags_slide() {
        let g = EqGraph {
            gains: [0.0; BAND_COUNT],
        };
        let mut drag: Option<usize> = None;
        let l = layout(bounds().size());
        // Press on column 3 near the top: jumps toward max boost.
        let at = Point::new(l.centers[3], l.top + 2.0);
        let press = CanvasEvent::Mouse(mouse::Event::ButtonPressed(Button::Left));
        let action = g.update(&mut drag, &press, bounds(), Cursor::Available(at));
        assert!(action.is_some());
        assert_eq!(drag, Some(3));
        // Press again at the same spot with matching gains: captured,
        // no message (nothing changed).
        let same = EqGraph {
            gains: {
                let mut arr = [0.0; BAND_COUNT];
                arr[3] = y_to_gain(at.y, &l);
                arr
            },
        };
        let mut drag2: Option<usize> = None;
        let action = same.update(&mut drag2, &press, bounds(), Cursor::Available(at));
        assert!(action.is_some());
        assert_eq!(drag2, Some(3));
        // Release drops the drag.
        let release = CanvasEvent::Mouse(mouse::Event::ButtonReleased(Button::Left));
        let _ = g.update(&mut drag, &release, bounds(), Cursor::Available(at));
        assert_eq!(drag, None);
    }

    #[test]
    fn label_rows_are_not_sliders() {
        let g = EqGraph {
            gains: [0.0; BAND_COUNT],
        };
        let mut drag: Option<usize> = None;
        let press = CanvasEvent::Mouse(mouse::Event::ButtonPressed(Button::Left));
        let l = layout(bounds().size());
        // dB value row: no band slam, no drag capture.
        let action = g.update(
            &mut drag,
            &press,
            bounds(),
            Cursor::Available(Point::new(l.centers[2], 8.0)),
        );
        assert!(action.is_none());
        assert_eq!(drag, None);
        // Frequency label row: same.
        let action = g.update(
            &mut drag,
            &press,
            bounds(),
            Cursor::Available(Point::new(l.centers[2], l.top + l.plot_h + 10.0)),
        );
        assert!(action.is_none());
        assert_eq!(drag, None);
    }

    #[test]
    fn overshoot_pins_instead_of_freezing() {
        let g = EqGraph {
            gains: [0.0; BAND_COUNT],
        };
        let mut drag: Option<usize> = Some(4);
        // Cursor flung above the widget: position_in misses, but the
        // absolute position still pins the band (a publish, not a
        // freeze). Value correctness is `y_to_gain`'s clamp (tested).
        let moved = CanvasEvent::Mouse(mouse::Event::CursorMoved {
            position: Point::new(200.0, -50.0),
        });
        let action = g.update(
            &mut drag,
            &moved,
            bounds(),
            Cursor::Available(Point::new(200.0, -50.0)),
        );
        assert!(action.is_some());
        assert_eq!(drag, Some(4));
        // Cursor gone entirely (unavailable): nothing to pin, drag held.
        let action = g.update(&mut drag, &moved, bounds(), Cursor::Unavailable);
        assert!(action.is_none());
        assert_eq!(drag, Some(4));
    }

    #[test]
    fn arrows_nudge_the_hovered_column() {
        use iced::keyboard::{self, key::Named, Key};
        let g = EqGraph {
            gains: [0.0; BAND_COUNT],
        };
        let mut drag: Option<usize> = None;
        let l = layout(bounds().size());
        let at = Cursor::Available(Point::new(l.centers[5], l.top + l.plot_h / 2.0));
        let key = |named| {
            CanvasEvent::Keyboard(keyboard::Event::KeyPressed {
                key: Key::Named(named),
                modified_key: Key::Named(named),
                physical_key: keyboard::key::Physical::Unidentified(
                    keyboard::key::NativeCode::Unidentified,
                ),
                location: keyboard::Location::Standard,
                modifiers: keyboard::Modifiers::empty(),
                text: None,
                repeat: false,
            })
        };
        // Up nudges +0.5 and captures the column for the next key.
        let action = g.update(&mut drag, &key(Named::ArrowUp), bounds(), at);
        assert!(action.is_some());
        assert_eq!(drag, Some(5));
        // Other keys are ignored.
        let action = g.update(&mut drag, &key(Named::Enter), bounds(), at);
        assert!(action.is_none());
    }

    #[test]
    fn press_in_gutter_is_ignored() {
        let g = EqGraph {
            gains: [0.0; BAND_COUNT],
        };
        let mut drag: Option<usize> = None;
        let press = CanvasEvent::Mouse(mouse::Event::ButtonPressed(Button::Left));
        let action = g.update(
            &mut drag,
            &press,
            bounds(),
            Cursor::Available(Point::new(2.0, 100.0)),
        );
        assert!(action.is_none());
        assert_eq!(drag, None);
    }
}
