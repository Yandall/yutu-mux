//! Cursor-hand hunt: logs what the tree actually votes, without changing
//! any vote.
//!
//! Wrap the view root and every cursor-vote transition is logged as
//! `[cursor-trace]` with the vote name, position, and route, e.g.
//! `Idle vote at (512, 300) on Settings`. Compare with what the screen
//! shows: log says `Idle` but screen shows hand means the platform
//! renders the default cursor wrong (outside our tree); log says
//! `Pointer` where nothing clickable lives means a real leaking vote
//! (the position names the widget area to investigate).
//!
//! Delete this module and its single use in `view()` once the hunt
//! ends: it exists only for the empty-space-hand investigation.

use iced::advanced::layout::{self, Layout, Node};
use iced::advanced::mouse;
use iced::advanced::overlay;
use iced::advanced::renderer;
use iced::advanced::widget::{self, Tree};
use iced::advanced::{Clipboard, Shell};
use iced::{Element, Event, Length, Rectangle, Size, Vector};

/// Pass-through wrapper that traces hand votes to the log.
pub struct CursorTrace<'a, Message> {
    content: Element<'a, Message>,
    where_built: String,
    last: std::cell::Cell<mouse::Interaction>,
}

/// Wrap `content`: hand votes inside are logged with position, votes
/// themselves unchanged.
pub fn trace_cursor<'a, Message>(
    content: Element<'a, Message>,
    where_built: String,
) -> CursorTrace<'a, Message> {
    CursorTrace {
        content,
        where_built,
        last: std::cell::Cell::new(mouse::Interaction::None),
    }
}

impl<'a, Message> widget::Widget<Message, iced::Theme, iced::Renderer>
    for CursorTrace<'a, Message>
{
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
        // Transitions only: holding still over one control must not
        // spam the log at event rate.
        if vote != self.last.get() {
            self.last.set(vote);
            let at = match cursor.position() {
                Some(pos) => format!("({:.0}, {:.0})", pos.x, pos.y),
                None => "(off-window)".to_string(),
            };
            yutu_core::app_log::info(
                "cursor-trace",
                format_args!("{:?} vote at {at} on {}", vote, self.where_built),
            );
        }
        vote
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

impl<'a, Message> From<CursorTrace<'a, Message>> for Element<'a, Message>
where
    Message: 'a,
{
    fn from(trace: CursorTrace<'a, Message>) -> Self {
        Element::new(trace)
    }
}
