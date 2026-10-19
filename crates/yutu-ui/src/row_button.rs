//! Row-sized click target: a real `iced` button (same press/release,
//! keyboard, and style semantics) that reports the arrow cursor instead
//! of the pointing hand.
//!
//! `iced` buttons always report `Pointer` on hover, which is right for
//! small controls (hearts, pills, nav icons) but wrong for full-width
//! content surfaces (track rows, cards, menus), where every pixel
//! would show a hand. Use [`row_button`] there and plain `button` for
//! controls. Only the cursor differs: events, styles, and messages are
//! the wrapped button's, so conversions are behavior-preserving.

use iced::advanced::layout::{self, Layout, Node};
use iced::advanced::mouse;
use iced::advanced::overlay;
use iced::advanced::renderer;
use iced::advanced::widget::{self, Tree};
use iced::advanced::{Clipboard, Shell};
use iced::widget::Button;
use iced::{Element, Event, Length, Rectangle, Size, Vector};

/// A button that shows the arrow cursor on hover. Build it exactly
/// like [`Button`] (`row_button(content).style(...).on_press(...)`).
pub struct RowButton<'a, Message> {
    inner: Button<'a, Message>,
}

/// Row click target from content (mirrors [`Button::new`]).
pub fn row_button<'a, Message>(content: impl Into<Element<'a, Message>>) -> RowButton<'a, Message>
where
    Message: 'a + Clone,
{
    RowButton {
        inner: Button::new(content),
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
        self
    }

    /// Optional message on click (mirrors [`Button::on_press_maybe`]:
    /// `None` renders the button disabled, same as [`Button`]).
    #[must_use]
    pub fn on_press_maybe(mut self, message: Option<Message>) -> Self {
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

    /// The one difference from [`Button`]: arrow on hover, never the
    /// hand. Unhovered it votes nothing (like the default), so it can
    /// never shadow another widget's cursor.
    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Idle
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
