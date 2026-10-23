//! Slider that reports the pointing hand on hover instead of the grab
//! hand. `iced`'s [`Slider`](iced::widget::Slider) hardcodes
//! `Grab`/`Grabbing` in `mouse_interaction` (no style knob changes
//! it), so this wraps it exactly like [`crate::row_button`] wraps
//! [`Button`](iced::widget::Button): same construction, same style
//! plumbing, only the grab votes map to `Pointer`.

use iced::advanced::layout::{self, Layout, Node};
use iced::advanced::mouse;
use iced::advanced::overlay;
use iced::advanced::renderer;
use iced::advanced::widget::{self, Tree};
use iced::advanced::{Clipboard, Shell};
use iced::widget::Slider;
use iced::{Element, Event, Length, Rectangle, Size, Vector};

/// Map grab votes to the pointing hand; every other vote (off-slider
/// default, Windows fallbacks the inner widget already handles)
/// passes through untouched.
fn pointing_interaction(inner: mouse::Interaction) -> mouse::Interaction {
    match inner {
        mouse::Interaction::Grab | mouse::Interaction::Grabbing => mouse::Interaction::Pointer,
        other => other,
    }
}

/// A slider that shows the pointing hand while hovering or dragging.
/// Build it exactly like [`Slider`] (`pointing_slider(range, value,
/// on_change).step(...).style(...)`).
pub struct PointingSlider<'a, T, Message>
where
    Message: Clone,
{
    inner: Slider<'a, T, Message>,
}

/// Pointing slider from range, value, and change message (mirrors
/// [`Slider::new`]).
pub fn pointing_slider<'a, T, Message>(
    range: std::ops::RangeInclusive<T>,
    value: T,
    on_change: impl Fn(T) -> Message + 'a,
) -> PointingSlider<'a, T, Message>
where
    T: Copy + From<u8> + PartialOrd + Into<f64> + num_traits::FromPrimitive,
    Message: Clone + 'a,
{
    PointingSlider {
        inner: Slider::new(range, value, on_change),
    }
}

impl<'a, T, Message> PointingSlider<'a, T, Message>
where
    T: Copy + From<u8> + PartialOrd + Into<f64> + num_traits::FromPrimitive,
    Message: Clone,
{
    /// Step size (mirrors [`Slider::step`]).
    #[must_use]
    pub fn step(mut self, step: T) -> Self {
        self.inner = self.inner.step(step);
        self
    }

    /// Style (mirrors [`Slider::style`]).
    #[must_use]
    pub fn style(
        mut self,
        style: impl Fn(&iced::Theme, iced::widget::slider::Status) -> iced::widget::slider::Style + 'a,
    ) -> Self {
        self.inner = self.inner.style(style);
        self
    }

    /// Width (mirrors [`Slider::width`]).
    #[must_use]
    pub fn width(mut self, width: impl Into<iced::Length>) -> Self {
        self.inner = self.inner.width(width);
        self
    }
}

impl<'a, T, Message> widget::Widget<Message, iced::Theme, iced::Renderer>
    for PointingSlider<'a, T, Message>
where
    T: Copy + From<u8> + PartialOrd + Into<f64> + num_traits::FromPrimitive + 'a,
    Message: Clone + 'a,
{
    fn size(&self) -> Size<Length> {
        <Slider<'a, T, Message> as widget::Widget<Message, iced::Theme, iced::Renderer>>::size(
            &self.inner,
        )
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> Node {
        self.inner.layout(tree, renderer, limits)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.inner
            .draw(tree, renderer, theme, style, layout, cursor, viewport);
    }

    fn tag(&self) -> widget::tree::Tag {
        <Slider<'a, T, Message> as widget::Widget<Message, iced::Theme, iced::Renderer>>::tag(
            &self.inner,
        )
    }

    fn state(&self) -> widget::tree::State {
        <Slider<'a, T, Message> as widget::Widget<Message, iced::Theme, iced::Renderer>>::state(
            &self.inner,
        )
    }

    fn children(&self) -> Vec<Tree> {
        <Slider<'a, T, Message> as widget::Widget<Message, iced::Theme, iced::Renderer>>::children(
            &self.inner,
        )
    }

    fn diff(&self, tree: &mut Tree) {
        <Slider<'a, T, Message> as widget::Widget<Message, iced::Theme, iced::Renderer>>::diff(
            &self.inner,
            tree,
        )
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.inner.operate(tree, layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.inner.update(
            tree, event, layout, cursor, renderer, clipboard, shell, viewport,
        );
    }

    /// The one difference from [`Slider`]: grab votes become the
    /// pointing hand, so sliders match every other clickable control.
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        pointing_interaction(<Slider<'a, T, Message> as widget::Widget<
            Message,
            iced::Theme,
            iced::Renderer,
        >>::mouse_interaction(
            &self.inner, tree, layout, cursor, viewport, renderer
        ))
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        self.inner
            .overlay(tree, layout, renderer, viewport, translation)
    }
}

impl<'a, T, Message> From<PointingSlider<'a, T, Message>>
    for Element<'a, Message, iced::Theme, iced::Renderer>
where
    T: Copy + From<u8> + PartialOrd + Into<f64> + num_traits::FromPrimitive + 'a,
    Message: Clone + 'a,
{
    fn from(slider: PointingSlider<'a, T, Message>) -> Self {
        Element::new(slider)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grab_votes_become_pointing_and_rest_passes_through() {
        assert_eq!(
            pointing_interaction(mouse::Interaction::Grab),
            mouse::Interaction::Pointer
        );
        assert_eq!(
            pointing_interaction(mouse::Interaction::Grabbing),
            mouse::Interaction::Pointer
        );
        assert_eq!(
            pointing_interaction(mouse::Interaction::Pointer),
            mouse::Interaction::Pointer
        );
        assert_eq!(
            pointing_interaction(mouse::Interaction::Idle),
            mouse::Interaction::Idle
        );
        assert_eq!(
            pointing_interaction(mouse::Interaction::default()),
            mouse::Interaction::default()
        );
    }
}
