//! Input-anchored dropdown: the closed face renders in-flow (a plain
//! button), while the open menu floats in an overlay positioned from the
//! face's own bounds (below it, viewport-clamped), never from the cursor.
//! Open state lives in the app (like the menus); this only owns placement.
//! Dismissal stays top-level (fullscreen scrim + Esc), exactly like the
//! context menus, so outside clicks land underneath as before.

use iced::advanced::layout::{self, Layout, Node};
use iced::advanced::mouse;
use iced::advanced::overlay;
use iced::advanced::renderer;
use iced::advanced::widget::{self, Tree};
use iced::advanced::{Clipboard, Shell};
use iced::{Element, Event, Length, Rectangle, Size, Vector};

/// Dropdown face plus optional open menu card (fixed width, built by the
/// caller). Closed it behaves exactly like the face button.
pub struct Dropdown<'a, Message> {
    content: Element<'a, Message>,
    menu: Option<Element<'a, Message>>,
    gap: f32,
}

/// Dropdown from a face element and the open menu card (`None` = closed).
pub fn dropdown<'a, Message>(
    content: impl Into<Element<'a, Message>>,
    menu: Option<Element<'a, Message>>,
) -> Dropdown<'a, Message>
where
    Message: 'a + Clone,
{
    Dropdown {
        content: content.into(),
        menu,
        gap: 4.0,
    }
}

impl<Message> Dropdown<'_, Message>
where
    Message: Clone,
{
    /// Gap between the face and the open menu, in px.
    #[must_use]
    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = gap;
        self
    }
}

impl<'a, Message> widget::Widget<Message, iced::Theme, iced::Renderer> for Dropdown<'a, Message>
where
    Message: 'a + Clone,
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
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
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
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn children(&self) -> Vec<Tree> {
        let mut children = vec![Tree::new(self.content.as_widget())];
        if let Some(menu) = &self.menu {
            children.push(Tree::new(menu.as_widget()));
        }
        children
    }

    fn diff(&self, tree: &mut Tree) {
        let mut children: Vec<&dyn widget::Widget<Message, iced::Theme, iced::Renderer>> =
            vec![self.content.as_widget()];
        if let Some(menu) = &self.menu {
            children.push(menu.as_widget());
        }
        tree.diff_children(&children);
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
            .operate(&mut tree.children[0], layout, renderer, operation);
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
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
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
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        let mut children = tree.children.iter_mut();
        let content = self.content.as_widget_mut().overlay(
            children.next().unwrap(),
            layout,
            renderer,
            viewport,
            translation,
        );
        let position = layout.position() + translation;
        let menu = self.menu.as_mut().map(|menu| {
            overlay::Element::new(Box::new(MenuOverlay {
                menu,
                tree: children.next().unwrap(),
                face: Rectangle {
                    x: position.x,
                    y: position.y,
                    width: layout.bounds().width,
                    height: layout.bounds().height,
                },
                viewport: *viewport,
                gap: self.gap,
            }))
        });
        if content.is_some() || menu.is_some() {
            Some(overlay::Group::with_children(content.into_iter().chain(menu).collect()).overlay())
        } else {
            None
        }
    }
}

/// Open menu card floating under the face: measured, then placed below
/// the face and clamped into the viewport (flips above when there is no
/// room below). Never follows the cursor.
struct MenuOverlay<'a, 'b, Message, Theme, Renderer> {
    menu: &'b mut Element<'a, Message, Theme, Renderer>,
    tree: &'b mut widget::Tree,
    face: Rectangle,
    viewport: Rectangle,
    gap: f32,
}

impl<Message, Theme, Renderer> overlay::Overlay<Message, Theme, Renderer>
    for MenuOverlay<'_, '_, Message, Theme, Renderer>
where
    Renderer: iced::advanced::renderer::Renderer,
{
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        let limits = layout::Limits::new(Size::ZERO, bounds);
        let node = self
            .menu
            .as_widget_mut()
            .layout(self.tree, renderer, &limits);
        let size = node.size();
        let mut x = self.face.x;
        if x + size.width > bounds.width {
            x = (bounds.width - size.width).max(0.0);
        }
        let mut y = self.face.y + self.face.height + self.gap;
        if y + size.height > bounds.height {
            y = self.face.y - size.height - self.gap;
        }
        if y < 0.0 {
            y = 0.0;
        }
        layout::Node::with_children(size, vec![node]).translate(Vector::new(x, y))
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        let child = layout.children().next().unwrap();
        self.menu.as_widget().draw(
            self.tree,
            renderer,
            theme,
            style,
            child,
            cursor,
            &self.viewport,
        );
    }

    fn operate(
        &mut self,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        let child = layout.children().next().unwrap();
        self.menu
            .as_widget_mut()
            .operate(self.tree, child, renderer, operation);
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        let child = layout.children().next().unwrap();
        self.menu.as_widget_mut().update(
            self.tree,
            event,
            child,
            cursor,
            renderer,
            clipboard,
            shell,
            &self.viewport,
        );
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let child = layout.children().next().unwrap();
        self.menu
            .as_widget()
            .mouse_interaction(self.tree, child, cursor, &self.viewport, renderer)
    }
}

impl<'a, Message> From<Dropdown<'a, Message>> for Element<'a, Message>
where
    Message: 'a + Clone,
{
    fn from(dropdown: Dropdown<'a, Message>) -> Self {
        Element::new(dropdown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::advanced::widget::Widget;

    #[test]
    fn closed_dropdown_has_content_tree_only() {
        let plain: Dropdown<()> = dropdown("x", None);
        assert_eq!(plain.children().len(), 1);
    }

    #[test]
    fn open_dropdown_adds_menu_tree() {
        let open: Dropdown<()> = dropdown("x", Some(iced::widget::text("pick").into()));
        assert_eq!(open.children().len(), 2);
    }

    #[test]
    fn gap_defaults_and_sets() {
        let plain: Dropdown<()> = dropdown("x", None);
        assert_eq!(plain.gap, 4.0);
        let spaced: Dropdown<()> = dropdown("x", None).gap(8.0);
        assert_eq!(spaced.gap, 8.0);
    }
}
