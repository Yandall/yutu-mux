//! TEMPORARY diagnostic (cursor-hand hunt): caps the cursor vote of a
//! subtree at arrow and logs where hand votes originate.
//!
//! Wrap the view root and every `Pointer` vote becomes `Idle`, so if the
//! hand in empty space disappears the leak was a real vote from our
//! tree; if it persists with zero `cursor-trace` lines in the log, the
//! votes are innocent and the cause is outside the widget tree. Every
//! transition into `Pointer` is logged with position and route, so the
//! surviving log names the widget area when the leak is real.
//!
//! Revert by deleting this module and its single use in `view()`.

use iced::advanced::layout::{self, Layout, Node};
use iced::advanced::mouse;
use iced::advanced::overlay;
use iced::advanced::renderer;
use iced::advanced::widget::{self, Tree};
use iced::advanced::{Clipboard, Shell};
use iced::{Element, Event, Length, Rectangle, Size, Vector};

/// Cap a subtree's cursor at the arrow; trace hand votes to the log.
pub struct CursorCap<'a, Message> {
    content: Element<'a, Message>,
    where_built: String,
    last: std::cell::Cell<mouse::Interaction>,
}

/// Wrap `content`: hand votes inside become arrow votes, and each new
/// hand vote is logged as `[cursor-trace]` with position and context.
pub fn cap_cursor<'a, Message>(
    content: Element<'a, Message>,
    where_built: String,
) -> CursorCap<'a, Message> {
    CursorCap {
        content,
        where_built,
        last: std::cell::Cell::new(mouse::Interaction::None),
    }
}

impl<'a, Message> widget::Widget<Message, iced::Theme, iced::Renderer> for CursorCap<'a, Message> {
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> Node {
        self.content.as_widget_mut().layout(tree, renderer, limits)
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
        self.content
            .as_widget()
            .draw(tree, renderer, theme, style, layout, cursor, viewport);
    }

    fn tag(&self) -> widget::tree::Tag {
        self.content.as_widget().tag()
    }

    fn state(&self) -> widget::tree::State {
        self.content.as_widget().state()
    }

    fn children(&self) -> Vec<Tree> {
        self.content.as_widget().children()
    }

    fn diff(&self, tree: &mut Tree) {
        self.content.as_widget().diff(tree);
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(tree, layout, renderer, operation);
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
        self.content.as_widget_mut().update(
            tree, event, layout, cursor, renderer, clipboard, shell, viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let vote = self
            .content
            .as_widget()
            .mouse_interaction(tree, layout, cursor, viewport, renderer);
        // Transitions only: hovering one control must not spam the log
        // at event rate.
        if vote != self.last.get() {
            self.last.set(vote);
            if vote == mouse::Interaction::Pointer {
                match cursor.position() {
                    Some(pos) => yutu_core::app_log::info(
                        "cursor-trace",
                        format_args!(
                            "Pointer vote at ({:.0}, {:.0}) on {}",
                            pos.x, pos.y, self.where_built
                        ),
                    ),
                    None => yutu_core::app_log::info(
                        "cursor-trace",
                        format_args!("Pointer vote (off-window) on {}", self.where_built),
                    ),
                }
            }
        }
        // The experiment: no hand anywhere, whatever the tree votes.
        match vote {
            mouse::Interaction::Pointer => mouse::Interaction::Idle,
            other => other,
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
        self.content
            .as_widget_mut()
            .overlay(tree, layout, renderer, viewport, translation)
    }
}

impl<'a, Message> From<CursorCap<'a, Message>> for Element<'a, Message>
where
    Message: 'a,
{
    fn from(cap: CursorCap<'a, Message>) -> Self {
        Element::new(cap)
    }
}
