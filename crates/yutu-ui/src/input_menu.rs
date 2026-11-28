//! Right-click edit menu for text inputs (Iced 0.14 has none): Cut, Copy,
//! Paste and Select all, like any desktop text field.
//!
//! Wraps one `text_input`. A right press over it focuses the input
//! (keeping any selection) and opens a small card at the cursor, drawn in
//! an overlay so it floats above everything. Actions run against the
//! wrapped input's own state and the system clipboard:
//! - Copy writes the selection; Cut also removes it.
//! - Paste inserts at the cursor (replacing a selection).
//! - Edits go out through `on_edit` with the whole new value (the same
//!   message the input's `on_input` sends), then the cursor is placed
//!   after the edit.
//!
//! Disabled rows (no selection, read-only, secure fields never copy) are
//! drawn muted and ignore clicks. Esc, a click elsewhere or the wheel
//! closes the card.

use crate::tk;
use iced::advanced::layout::{self, Layout, Node};
use iced::advanced::mouse;
use iced::advanced::overlay;
use iced::advanced::renderer::{self, Quad};
use iced::advanced::text::{self, Renderer as _};
use iced::advanced::widget::{self, tree, Tree};
use iced::advanced::{clipboard, Clipboard, Renderer as _, Shell};
use iced::widget::text_input::{cursor, State as InputState, Value};
use iced::{keyboard, Element, Event, Length, Point, Rectangle, Size, Vector};

/// Paragraph type behind the app renderer's text inputs.
type Paragraph = <iced::Renderer as text::Renderer>::Paragraph;

const MENU_W: f32 = 168.0;
const ROW_H: f32 = 30.0;
const PAD: f32 = 4.0;
const TEXT_SIZE: f32 = 13.0;

/// The four menu actions, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EditAction {
    Cut,
    Copy,
    Paste,
    SelectAll,
}

impl EditAction {
    const ALL: [EditAction; 4] = [
        EditAction::Cut,
        EditAction::Copy,
        EditAction::Paste,
        EditAction::SelectAll,
    ];
}

/// Text input with a right-click edit menu (see the module docs).
pub(crate) struct InputMenu<'a, Message> {
    content: Element<'a, Message>,
    value: String,
    on_edit: Option<Box<dyn Fn(String) -> Message + 'a>>,
    secure: bool,
    labels: [String; 4],
}

/// Wrap `input` (showing `value`) with the edit menu. `on_edit` is the
/// input's change message (`None` = read-only: no Cut / Paste);
/// `labels` are Cut, Copy, Paste, Select all in the UI language.
pub(crate) fn input_menu<'a, Message: Clone + 'a>(
    input: impl Into<Element<'a, Message>>,
    value: &str,
    on_edit: Option<impl Fn(String) -> Message + 'a>,
    labels: [String; 4],
) -> InputMenu<'a, Message> {
    InputMenu {
        content: input.into(),
        value: value.to_string(),
        on_edit: on_edit.map(|f| Box::new(f) as Box<dyn Fn(String) -> Message + 'a>),
        secure: false,
        labels,
    }
}

impl<Message> InputMenu<'_, Message> {
    /// Password-style field: Copy and Cut stay disabled.
    #[must_use]
    pub(crate) fn secure(mut self, secure: bool) -> Self {
        self.secure = secure;
        self
    }
}

/// Wrapper state: where the open card sits (`None` = closed) and which
/// row the pointer is over.
#[derive(Debug, Default)]
struct MenuState {
    open: Option<Point>,
    hovered: Option<usize>,
}

/// The wrapped input's state, when the child really is a text input.
fn input_state(tree: &mut Tree) -> Option<&mut InputState<Paragraph>> {
    match &mut tree.state {
        tree::State::Some(any) => any.downcast_mut::<InputState<Paragraph>>(),
        tree::State::None => None,
    }
}

/// Current selection as grapheme indices, if any.
fn selection(state: &InputState<Paragraph>, value: &Value) -> Option<(usize, usize)> {
    state.cursor().selection(value)
}

/// Insert point / replaced range: the selection, or the caret.
fn edit_range(state: &InputState<Paragraph>, value: &Value) -> (usize, usize) {
    match state.cursor().state(value) {
        cursor::State::Index(i) => (i, i),
        cursor::State::Selection { start, end } => (start.min(end), start.max(end)),
    }
}

/// `value` with graphemes `start..end` replaced by `insert`.
fn splice(value: &Value, start: usize, end: usize, insert: &str) -> String {
    format!(
        "{}{}{}",
        value.select(0, start),
        insert,
        value.select(end, value.len())
    )
}

/// Which actions apply right now.
fn enabled(
    action: EditAction,
    has_selection: bool,
    editable: bool,
    secure: bool,
    empty: bool,
) -> bool {
    match action {
        EditAction::Cut => has_selection && editable && !secure,
        EditAction::Copy => has_selection && !secure,
        EditAction::Paste => editable,
        EditAction::SelectAll => !empty,
    }
}

impl<'a, Message: Clone + 'a> widget::Widget<Message, iced::Theme, iced::Renderer>
    for InputMenu<'a, Message>
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<MenuState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(MenuState::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(self.content.as_widget())]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

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
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) = event {
            if let Some(at) = cursor.position_over(layout.bounds()) {
                // Focus without moving the caret, so a selection made
                // with the left button survives into Copy / Cut.
                if let Some(input) = input_state(&mut tree.children[0]) {
                    if !input.is_focused() {
                        input.focus();
                        input.move_cursor_to_end();
                    }
                    let menu = tree.state.downcast_mut::<MenuState>();
                    menu.open = Some(at);
                    menu.hovered = None;
                    shell.capture_event();
                    shell.request_redraw();
                    return;
                }
            }
        }
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
        let Tree {
            state, children, ..
        } = tree;
        let menu = state.downcast_mut::<MenuState>();
        let Some(anchor) = menu.open else {
            return self.content.as_widget_mut().overlay(
                &mut children[0],
                layout,
                renderer,
                viewport,
                translation,
            );
        };
        Some(overlay::Element::new(Box::new(MenuOverlay {
            menu,
            input: &mut children[0],
            anchor: anchor + translation,
            value: &self.value,
            on_edit: self.on_edit.as_deref(),
            secure: self.secure,
            labels: &self.labels,
        })))
    }
}

/// The open card: self-drawn rows (no child widgets), so clicks act on
/// the wrapped input directly.
struct MenuOverlay<'a, 'b, Message> {
    menu: &'b mut MenuState,
    input: &'b mut Tree,
    anchor: Point,
    value: &'b str,
    on_edit: Option<&'b (dyn Fn(String) -> Message + 'a)>,
    secure: bool,
    labels: &'b [String; 4],
}

impl<Message> MenuOverlay<'_, '_, Message> {
    fn row_enabled(&mut self, action: EditAction) -> bool {
        let value = Value::new(self.value);
        let has_selection = input_state(self.input)
            .map(|s| selection(s, &value).is_some())
            .unwrap_or(false);
        enabled(
            action,
            has_selection,
            self.on_edit.is_some(),
            self.secure,
            self.value.is_empty(),
        )
    }

    /// Row index under `p` inside the card at `bounds`.
    fn row_at(bounds: Rectangle, p: Point) -> Option<usize> {
        if !bounds.contains(p) {
            return None;
        }
        let i = ((p.y - bounds.y - PAD) / ROW_H).floor();
        (i >= 0.0 && (i as usize) < EditAction::ALL.len()).then_some(i as usize)
    }

    fn run(
        &mut self,
        action: EditAction,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        let value = Value::new(self.value);
        let Some(state) = input_state(self.input) else {
            return;
        };
        match action {
            EditAction::Copy | EditAction::Cut => {
                let Some((start, end)) = selection(state, &value) else {
                    return;
                };
                clipboard.write(
                    clipboard::Kind::Standard,
                    value.select(start, end).to_string(),
                );
                if action == EditAction::Cut {
                    if let Some(on_edit) = self.on_edit {
                        shell.publish(on_edit(splice(&value, start, end, "")));
                        state.move_cursor_to(start);
                    }
                }
            }
            EditAction::Paste => {
                let Some(on_edit) = self.on_edit else {
                    return;
                };
                let Some(pasted) = clipboard.read(clipboard::Kind::Standard) else {
                    return;
                };
                // Single-line field: line breaks become spaces.
                let pasted = pasted
                    .trim_end_matches(['\r', '\n'])
                    .replace("\r\n", " ")
                    .replace(['\n', '\r'], " ");
                let (start, end) = edit_range(state, &value);
                shell.publish(on_edit(splice(&value, start, end, &pasted)));
                state.move_cursor_to(start + Value::new(&pasted).len());
            }
            EditAction::SelectAll => state.select_all(),
        }
    }
}

impl<Message> overlay::Overlay<Message, iced::Theme, iced::Renderer>
    for MenuOverlay<'_, '_, Message>
{
    fn layout(&mut self, _renderer: &iced::Renderer, bounds: Size) -> Node {
        let size = Size::new(MENU_W, 2.0 * PAD + ROW_H * EditAction::ALL.len() as f32);
        // Opens down-right of the cursor, flipped/clamped on screen.
        let mut x = self.anchor.x + 2.0;
        if x + size.width > bounds.width {
            x = (self.anchor.x - size.width).max(0.0);
        }
        let mut y = self.anchor.y + 2.0;
        if y + size.height > bounds.height {
            y = (self.anchor.y - size.height).max(0.0);
        }
        Node::new(size).translate(Vector::new(x, y))
    }

    fn draw(
        &self,
        renderer: &mut iced::Renderer,
        _theme: &iced::Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
    ) {
        let bounds = layout.bounds();
        renderer.fill_quad(
            Quad {
                bounds,
                border: iced::Border {
                    color: tk().outline,
                    width: 1.0,
                    radius: 8.0.into(),
                },
                shadow: iced::Shadow {
                    color: tk().shadow.scale_alpha(0.45),
                    offset: Vector::new(0.0, 4.0),
                    blur_radius: 14.0,
                },
                snap: true,
            },
            tk().menu,
        );
        // Enabled flags need the input state, which `draw` (&self) can
        // only read: recompute from an immutable view.
        let value = Value::new(self.value);
        let has_selection = match &self.input.state {
            tree::State::Some(any) => any
                .downcast_ref::<InputState<Paragraph>>()
                .and_then(|s| selection(s, &value))
                .is_some(),
            tree::State::None => false,
        };
        for (i, action) in EditAction::ALL.iter().enumerate() {
            let row = Rectangle {
                x: bounds.x + PAD,
                y: bounds.y + PAD + i as f32 * ROW_H,
                width: bounds.width - 2.0 * PAD,
                height: ROW_H,
            };
            let on = enabled(
                *action,
                has_selection,
                self.on_edit.is_some(),
                self.secure,
                self.value.is_empty(),
            );
            if on && self.menu.hovered == Some(i) {
                renderer.fill_quad(
                    Quad {
                        bounds: row,
                        border: iced::Border::default().rounded(6),
                        ..Quad::default()
                    },
                    tk().outline,
                );
            }
            renderer.fill_text(
                iced::advanced::Text {
                    content: self.labels[i].clone(),
                    bounds: Size::new(row.width - 20.0, row.height),
                    size: iced::Pixels(TEXT_SIZE),
                    line_height: text::LineHeight::default(),
                    font: renderer.default_font(),
                    align_x: text::Alignment::Left,
                    align_y: iced::alignment::Vertical::Center,
                    shaping: text::Shaping::Advanced,
                    wrapping: text::Wrapping::None,
                },
                Point::new(row.x + 10.0, row.center_y()),
                if on { tk().text } else { tk().text_disabled },
                row,
            );
        }
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        let bounds = layout.bounds();
        match event {
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                let hovered = Self::row_at(bounds, *position);
                if hovered != self.menu.hovered {
                    self.menu.hovered = hovered;
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(button)) => {
                let row = cursor.position().and_then(|p| Self::row_at(bounds, p));
                match (button, row) {
                    (mouse::Button::Left, Some(i)) => {
                        let action = EditAction::ALL[i];
                        if self.row_enabled(action) {
                            self.run(action, clipboard, shell);
                            self.menu.open = None;
                        }
                        shell.capture_event();
                    }
                    // Any press on the card's padding is swallowed;
                    // presses elsewhere close it and pass through.
                    (_, None) if cursor.is_over(bounds) => shell.capture_event(),
                    _ => self.menu.open = None,
                }
                shell.request_redraw();
            }
            Event::Mouse(mouse::Event::WheelScrolled { .. }) => {
                self.menu.open = None;
                shell.request_redraw();
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Escape),
                ..
            }) => {
                self.menu.open = None;
                shell.capture_event();
                shell.request_redraw();
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let bounds = layout.bounds();
        match cursor.position().and_then(|p| Self::row_at(bounds, p)) {
            Some(_) => mouse::Interaction::Pointer,
            None if cursor.is_over(bounds) => mouse::Interaction::Idle,
            None => mouse::Interaction::None,
        }
    }
}

impl<'a, Message: Clone + 'a> From<InputMenu<'a, Message>> for Element<'a, Message> {
    fn from(menu: InputMenu<'a, Message>) -> Self {
        Element::new(menu)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splice_replaces_by_grapheme() {
        let v = Value::new("Sedúceme ñu");
        assert_eq!(splice(&v, 0, 0, "x"), "xSedúceme ñu");
        assert_eq!(splice(&v, 3, 5, ""), "Sedeme ñu");
        assert_eq!(splice(&v, 9, 11, "ok"), "Sedúceme ok");
    }

    #[test]
    fn rows_enable_only_when_they_apply() {
        use EditAction::*;
        // No selection: nothing to cut or copy, paste still works.
        assert!(!enabled(Cut, false, true, false, false));
        assert!(!enabled(Copy, false, true, false, false));
        assert!(enabled(Paste, false, true, false, false));
        assert!(enabled(SelectAll, false, true, false, false));
        // Selection on a normal field: everything.
        assert!(enabled(Cut, true, true, false, false));
        assert!(enabled(Copy, true, true, false, false));
        // Secure fields never copy out.
        assert!(!enabled(Copy, true, true, true, false));
        assert!(!enabled(Cut, true, true, true, false));
        // Read-only: no cut or paste; empty: nothing to select.
        assert!(!enabled(Paste, false, false, false, false));
        assert!(!enabled(SelectAll, false, true, false, true));
    }
}
