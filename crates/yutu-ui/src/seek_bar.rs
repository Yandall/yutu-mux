//! Progress seek bar canvas: red fill + white knob, Pointer cursor,
//! press/drag previews without committing, release commits the seek.
//!
//! The stock slider cannot do this: its Grab/Grabbing cursor is fixed
//! and every tick seeks live (dragging to the end would advance
//! mid-drag). Here press jumps the preview, drags slide it, and only
//! release pushes `SeekRelease`; the app pauses while held and resumes
//! on release when it was playing. Pure math is unit-tested; the
//! [`Program`] only wires pointer events to preview/commit messages.

use crate::{Message, ACCENT};
use iced::widget::canvas::{self, Action, Event, Frame, Geometry, LineCap, Path, Program, Stroke};
use iced::{Color, Length, Point, Rectangle};

/// Bar height: knob + touch padding.
pub const SEEK_HEIGHT: f32 = 26.0;
const KNOB_R: f32 = 8.0;
const TRACK_W: f32 = 4.0;
const TRACK_GRAY: Color = Color::from_rgb8(0x3A, 0x3A, 0x3A);

/// Canvas x for a value (knob-radius inset so the knob never clips).
#[must_use]
pub fn value_to_x(value: f32, max: f32, width: f32) -> f32 {
    if max <= 0.0 || width <= 2.0 * KNOB_R {
        return KNOB_R;
    }
    KNOB_R + (value.clamp(0.0, max) / max) * (width - 2.0 * KNOB_R)
}

/// Canvas x to a whole-second value (matches the old 1s slider steps).
#[must_use]
pub fn x_to_value(x: f32, max: f32, width: f32) -> f32 {
    if max <= 0.0 || width <= 2.0 * KNOB_R {
        return 0.0;
    }
    (((x - KNOB_R) / (width - 2.0 * KNOB_R) * max).round()).clamp(0.0, max)
}

/// The canvas program: current value + max (fresh props per view),
/// dragging flag in state. Press/move publish previews only on real
/// changes; release commits. Cursor is Pointer on hover (and while
/// dragging, even off-bounds); it abstains otherwise so ancestor votes
/// win elsewhere.
pub struct SeekBar {
    pub value: f32,
    pub max: f32,
}

impl Program<Message> for SeekBar {
    type State = bool;

    fn update(
        &self,
        state: &mut Self::State,
        event: &Event,
        bounds: Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> Option<Action<Message>> {
        match event {
            Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)) => {
                let p = cursor.position_in(bounds)?;
                *state = true;
                let value = x_to_value(p.x, self.max, bounds.width);
                if value != self.value {
                    Some(Action::publish(Message::SeekPreview(value)).and_capture())
                } else {
                    Some(Action::capture())
                }
            }
            Event::Mouse(iced::mouse::Event::CursorMoved { .. }) => {
                if !*state {
                    return None;
                }
                let p = match cursor.position_in(bounds) {
                    Some(p) => p,
                    // Overshot mid-drag: pin to the bar edge instead of
                    // freezing (x_to_value clamps).
                    None => {
                        let abs = cursor.position()?;
                        Point::new(abs.x - bounds.x, abs.y - bounds.y)
                    }
                };
                let value = x_to_value(p.x, self.max, bounds.width);
                if value != self.value {
                    Some(Action::publish(Message::SeekPreview(value)).and_capture())
                } else {
                    Some(Action::capture())
                }
            }
            Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                if *state {
                    *state = false;
                    Some(Action::publish(Message::SeekRelease).and_capture())
                } else {
                    None
                }
            }
            Event::Keyboard(iced::keyboard::Event::KeyPressed { key, .. }) => {
                // Arrow steps commit at once (the stock slider's keys).
                let delta = match key {
                    iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowUp)
                    | iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowRight) => 5.0,
                    iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowDown)
                    | iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowLeft) => -5.0,
                    _ => return None,
                };
                cursor.position_in(bounds)?;
                let value = (self.value + delta).clamp(0.0, self.max.max(0.0));
                Some(Action::publish(Message::SeekTo(value)).and_capture())
            }
            _ => None,
        }
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        bounds: Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> iced::mouse::Interaction {
        // Gated on hover: the canvas widget forwards our vote without
        // checking bounds, so an unconditional Pointer wins the
        // ancestor max-merge everywhere (hand over rows and empty
        // space, outranking Idle but losing to Text/Grab). Dragging
        // keeps the hand even off-bounds (update pins overshoots to
        // the bar edge, so leaving mid-drag is routine).
        if *state || cursor.is_over(bounds) {
            iced::mouse::Interaction::Pointer
        } else {
            iced::mouse::Interaction::None
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
        if bounds.width <= 0.0 || bounds.height <= 0.0 {
            return vec![];
        }
        let mut frame = Frame::new(renderer, bounds.size());
        let mid = bounds.height / 2.0;
        let from = Point::new(KNOB_R, mid);
        let to = Point::new((bounds.width - KNOB_R).max(KNOB_R), mid);
        // Track, then red fill up to the knob, then the white knob.
        frame.stroke(
            &Path::line(from, to),
            Stroke::default()
                .with_color(TRACK_GRAY)
                .with_width(TRACK_W)
                .with_line_cap(LineCap::Round),
        );
        let knob = Point::new(value_to_x(self.value, self.max, bounds.width), mid);
        if self.max > 0.0 {
            frame.stroke(
                &Path::line(from, knob),
                Stroke::default()
                    .with_color(ACCENT)
                    .with_width(TRACK_W)
                    .with_line_cap(LineCap::Round),
            );
            frame.fill(&Path::circle(knob, KNOB_R), Color::WHITE);
        }
        vec![frame.into_geometry()]
    }
}

/// Widget constructor: fixed height, full width, fresh value per view.
#[must_use]
pub fn seek_bar(value: f32, max: f32) -> iced::widget::canvas::Canvas<SeekBar, Message> {
    canvas::Canvas::new(SeekBar { value, max })
        .width(Length::Fill)
        .height(Length::Fixed(SEEK_HEIGHT))
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::mouse::{self, Button, Cursor};
    use iced::widget::canvas::Event as CanvasEvent;

    fn bounds() -> Rectangle {
        Rectangle {
            x: 0.0,
            y: 0.0,
            width: 400.0,
            height: SEEK_HEIGHT,
        }
    }

    #[test]
    fn values_round_trip_and_clamp() {
        // Whole seconds survive; edges clamp (knob never clips).
        for v in [0.0, 1.0, 30.0, 199.0, 200.0] {
            assert_eq!(x_to_value(value_to_x(v, 200.0, 400.0), 200.0, 400.0), v);
        }
        assert_eq!(x_to_value(-50.0, 200.0, 400.0), 0.0);
        assert_eq!(x_to_value(500.0, 200.0, 400.0), 200.0);
        assert_eq!(x_to_value(200.0, 0.0, 400.0), 0.0);
        assert_eq!(value_to_x(50.0, 200.0, 0.0), KNOB_R);
    }

    #[test]
    fn cursor_is_pointer_only_over_the_bar() {
        // Regression: the canvas widget forwards our vote without
        // checking bounds, so an unconditional Pointer won the ancestor
        // max-merge everywhere (hand over rows and empty space).
        let bar = SeekBar {
            value: 10.0,
            max: 200.0,
        };
        assert_eq!(
            bar.mouse_interaction(&false, bounds(), Cursor::Available(Point::new(300.0, 13.0))),
            mouse::Interaction::Pointer
        );
        assert_eq!(
            bar.mouse_interaction(
                &false,
                bounds(),
                Cursor::Available(Point::new(300.0, 300.0))
            ),
            mouse::Interaction::None
        );
        assert_eq!(
            bar.mouse_interaction(&false, bounds(), Cursor::Unavailable),
            mouse::Interaction::None
        );
        // Dragging keeps the hand even off-bounds (overshoot pins to
        // the bar edge instead of dropping the drag).
        assert_eq!(
            bar.mouse_interaction(&true, bounds(), Cursor::Available(Point::new(300.0, 300.0))),
            mouse::Interaction::Pointer
        );
    }

    #[test]
    fn press_drags_and_release_commits() {
        let bar = SeekBar {
            value: 10.0,
            max: 200.0,
        };
        let mut dragging = false;
        let press = CanvasEvent::Mouse(mouse::Event::ButtonPressed(Button::Left));
        // Press elsewhere jumps the preview and captures the drag.
        let action = bar.update(
            &mut dragging,
            &press,
            bounds(),
            Cursor::Available(Point::new(300.0, 13.0)),
        );
        assert!(action.is_some());
        assert!(dragging);
        // Press on the live value captures without a message.
        let same = SeekBar {
            value: x_to_value(300.0, 200.0, 400.0),
            max: 200.0,
        };
        let mut dragging2 = false;
        let action = same.update(
            &mut dragging2,
            &press,
            bounds(),
            Cursor::Available(Point::new(300.0, 13.0)),
        );
        assert!(action.is_some());
        assert!(dragging2);
        // Move without a drag is ignored.
        let moved = CanvasEvent::Mouse(mouse::Event::CursorMoved {
            position: Point::new(100.0, 13.0),
        });
        let mut idle = false;
        assert!(bar
            .update(
                &mut idle,
                &moved,
                bounds(),
                Cursor::Available(Point::new(100.0, 13.0))
            )
            .is_none());
        // Release without a drag is ignored; with one it commits.
        let release = CanvasEvent::Mouse(mouse::Event::ButtonReleased(Button::Left));
        assert!(bar
            .update(
                &mut idle,
                &release,
                bounds(),
                Cursor::Available(Point::new(100.0, 13.0))
            )
            .is_none());
        let action = bar.update(
            &mut dragging,
            &release,
            bounds(),
            Cursor::Available(Point::new(300.0, 13.0)),
        );
        assert!(action.is_some());
        assert!(!dragging);
    }

    #[test]
    fn arrows_step_five_seconds() {
        use iced::keyboard::{self, key::Named, Key};
        let bar = SeekBar {
            value: 10.0,
            max: 200.0,
        };
        let mut dragging = false;
        let at = Cursor::Available(Point::new(100.0, 13.0));
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
        assert!(bar
            .update(&mut dragging, &key(Named::ArrowRight), bounds(), at)
            .is_some());
        assert!(!dragging, "keyboard never captures the drag");
        assert!(bar
            .update(&mut dragging, &key(Named::Enter), bounds(), at)
            .is_none());
    }
}
