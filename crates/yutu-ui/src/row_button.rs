//! Row-sized click target: a real `iced` button (same press/release,
//! keyboard, and style semantics) that reports the pointing hand on
//! hover when it actually clicks something (unhovered it votes nothing,
//! like the default, so it can never shadow another widget's cursor;
//! press-less rows keep the arrow, like static content).

use iced::advanced::layout::{self, Layout, Node};
use iced::advanced::mouse;
use iced::advanced::overlay;
use iced::advanced::renderer;
use iced::advanced::widget::{self, Tree};
use iced::advanced::{Clipboard, Shell};
use iced::widget::Button;
use iced::{Element, Event, Length, Rectangle, Size, Vector};

/// A button that shows the pointing hand on hover when pressable.
/// Build it exactly like [`Button`] (`row_button(content).style(...)...`);
/// every clickable row, card, and menu item goes through here so the
/// cursor contract holds in one place.
pub struct RowButton<'a, Message> {
    inner: Button<'a, Message>,
    press: bool,
}

/// Row click target from content (mirrors [`Button::new`]).
pub fn row_button<'a, Message>(content: impl Into<Element<'a, Message>>) -> RowButton<'a, Message>
where
    Message: 'a + Clone,
{
    RowButton {
        inner: Button::new(content),
        press: false,
    }
}

/// Cursor for a hovered row: the pointing hand when the row clicks,
/// the arrow otherwise (mirrors [`Button`], which only points when
/// `on_press` is set).
fn hover_interaction(press: bool) -> mouse::Interaction {
    if press {
        mouse::Interaction::Pointer
    } else {
        mouse::Interaction::Idle
    }
}

impl<'a, Message> RowButton<'a, Message>
where
    Message: 'a + Clone,
{
    /// Style the wrapped button (same [`Button::style`] function type,
    /// so `row_click`/`card_button`/`hover_row` all keep working).
    #[must_use]
    pub fn style(
        mut self,
        style: impl Fn(&iced::Theme, iced::widget::button::Status) -> iced::widget::button::Style + 'a,
    ) -> Self {
        self.inner = self.inner.style(style);
        self
    }

    /// Message on click (same release-inside semantics as [`Button`]).
    #[must_use]
    pub fn on_press(mut self, message: Message) -> Self {
        self.inner = self.inner.on_press(message);
        self.press = true;
        self
    }

    /// Optional message on click (mirrors [`Button::on_press_maybe`]:
    /// `None` renders the button disabled, same as [`Button`]).
    #[must_use]
    pub fn on_press_maybe(mut self, message: Option<Message>) -> Self {
        self.press = message.is_some();
        self.inner = self.inner.on_press_maybe(message);
        self
    }

    /// Inner padding (mirrors [`Button::padding`]).
    #[must_use]
    pub fn padding(mut self, padding: impl Into<iced::Padding>) -> Self {
        self.inner = self.inner.padding(padding);
        self
    }

    /// Width (mirrors [`Button::width`]).
    #[must_use]
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.inner = self.inner.width(width);
        self
    }

    /// Height (mirrors [`Button::height`]).
    #[must_use]
    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.inner = self.inner.height(height);
        self
    }

    /// Clip contents to the button bounds (mirrors [`Button::clip`]).
    #[must_use]
    pub fn clip(mut self, clip: bool) -> Self {
        self.inner = self.inner.clip(clip);
        self
    }
}

impl<'a, Message> widget::Widget<Message, iced::Theme, iced::Renderer> for RowButton<'a, Message>
where
    Message: 'a + Clone,
{
    fn size(&self) -> Size<Length> {
        self.inner.size()
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
        self.inner.tag()
    }

    fn state(&self) -> widget::tree::State {
        self.inner.state()
    }

    fn children(&self) -> Vec<Tree> {
        self.inner.children()
    }

    fn diff(&self, tree: &mut Tree) {
        self.inner.diff(tree);
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

    /// The one difference from [`Button`]: pointing hand on hover when
    /// pressable, arrow otherwise. Unhovered it votes nothing (like the
    /// default), so it can never shadow another widget's cursor.
    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        if cursor.is_over(layout.bounds()) {
            hover_interaction(self.press)
        } else {
            mouse::Interaction::None
        }
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

impl<'a, Message> From<RowButton<'a, Message>> for Element<'a, Message>
where
    Message: 'a + Clone,
{
    fn from(button: RowButton<'a, Message>) -> Self {
        Element::new(button)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hover_points_when_pressable_and_arrows_otherwise() {
        assert_eq!(hover_interaction(true), mouse::Interaction::Pointer);
        assert_eq!(hover_interaction(false), mouse::Interaction::Idle);
    }

    #[test]
    fn press_tracking_follows_on_press() {
        let plain: RowButton<()> = row_button("x");
        assert!(!plain.press);
        let clicked: RowButton<()> = row_button("x").on_press(());
        assert!(clicked.press);
        let maybe: RowButton<()> = row_button("x").on_press_maybe(Some(()));
        assert!(maybe.press);
        let disabled: RowButton<()> = row_button("x").on_press_maybe(None);
        assert!(!disabled.press);
    }
}
