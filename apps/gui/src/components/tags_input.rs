//! Inline pill flow: the last child is a text field that fills the remaining line.
use iced::advanced::{
    Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer,
    widget::{Operation, Tree},
};
use iced::{Element, Event, Length, Point, Rectangle, Size, Vector};
use iced_adaptive_design::Theme;

pub fn flow<'a, M: 'a>(items: Vec<Element<'a, M, Theme>>) -> Element<'a, M, Theme> {
    Element::new(TagsInputFlow {
        items,
        caret: None,
        navigation: None,
    })
}

/// Places the last (raw text input) child at a visual insertion point without
/// reordering its tree or layout index. Out-of-range positions are clamped.
pub fn flow_with_caret<'a, M: Clone + 'a>(
    items: Vec<Element<'a, M, Theme>>,
    caret_index: usize,
    query: &'a str,
    previous: Option<M>,
    next: Option<M>,
) -> Element<'a, M, Theme> {
    Element::new(TagsInputFlow {
        items,
        caret: Some(caret_index),
        navigation: Some(Box::new(move |tree, event| {
            let direction = boundary_arrow(tree, event, query)?;
            if direction {
                next.clone()
            } else {
                previous.clone()
            }
        })),
    })
}

type InputState =
    iced::widget::text_input::State<<iced::Renderer as iced::advanced::text::Renderer>::Paragraph>;
type Navigation<'a, M> = Box<dyn Fn(&Tree, &Event) -> Option<M> + 'a>;

fn boundary_arrow(tree: &Tree, event: &Event, query: &str) -> Option<bool> {
    use iced::keyboard::{Event as KeyEvent, Key, key::Named};
    use iced::widget::text_input::{Value, cursor};
    let Event::Keyboard(KeyEvent::KeyPressed { key, modifiers, .. }) = event else {
        return None;
    };
    if !modifiers.is_empty() {
        return None;
    }
    let state = tree.state.downcast_ref::<InputState>();
    if !state.is_focused() {
        return None;
    }
    let value = Value::new(query);
    let cursor::State::Index(index) = state.cursor().state(&value) else {
        return None;
    };
    match key {
        Key::Named(Named::ArrowLeft) if index == 0 => Some(false),
        Key::Named(Named::ArrowRight) if index == value.len() => Some(true),
        _ => None,
    }
}

struct TagsInputFlow<'a, M> {
    items: Vec<Element<'a, M, Theme>>,
    caret: Option<usize>,
    navigation: Option<Navigation<'a, M>>,
}

impl<M> Widget<M, Theme, iced::Renderer> for TagsInputFlow<'_, M> {
    fn children(&self) -> Vec<Tree> {
        self.items.iter().map(Tree::new).collect()
    }
    fn diff(&self, tree: &mut Tree) {
        if self.caret.is_some() && !self.items.is_empty() {
            let input = tree.children.pop();
            tree.diff_children(&self.items[..self.items.len() - 1]);
            let item = self.items.last().unwrap();
            let mut input = input.unwrap_or_else(|| Tree::new(item));
            input.diff(item.as_widget());
            tree.children.push(input);
        } else {
            tree.diff_children(&self.items);
        }
    }
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Shrink)
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let width = limits.max().width;
        let mut nodes = vec![layout::Node::new(Size::ZERO); self.items.len()];
        let (mut x, mut y, mut line_height) = (0.0_f32, 0.0_f32, 28.0_f32);
        let last = self.items.len().saturating_sub(1);
        let caret = self.caret.unwrap_or(last).min(last);
        let has_items = !self.items.is_empty();
        let order = (0..caret)
            .chain(std::iter::once(last).filter(|_| has_items))
            .chain(caret..last);
        for index in order {
            let item = &mut self.items[index];
            let state = &mut tree.children[index];
            if index == last && x > 0.0 && width - x < 120.0_f32.min(width) {
                x = 0.0;
                y += line_height + 4.0;
                line_height = 28.0;
            }
            let available = if index == last {
                (width - x).max(0.0)
            } else {
                width
            };
            let child_limits = layout::Limits::new(Size::ZERO, Size::new(available, f32::INFINITY));
            let mut node = item.as_widget_mut().layout(state, renderer, &child_limits);
            if index != last && x > 0.0 && x + node.size().width > width {
                x = 0.0;
                y += line_height + 4.0;
                line_height = 28.0;
            }
            let height = node.size().height;
            node.move_to_mut(Point::new(x, y + (28.0 - height).max(0.0) / 2.0));
            x += node.size().width + 4.0;
            line_height = line_height.max(height);
            nodes[index] = node;
        }
        layout::Node::with_children(Size::new(width, y + line_height), nodes)
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        operation.container(None, layout.bounds());
        operation.traverse(&mut |op| {
            for ((item, state), layout) in self
                .items
                .iter_mut()
                .zip(&mut tree.children)
                .zip(layout.children())
            {
                item.as_widget_mut().operate(state, layout, renderer, op);
            }
        });
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        if let Some(message) = self.navigation.as_ref().and_then(|navigate| {
            tree.children
                .last()
                .and_then(|input| navigate(input, event))
        }) {
            shell.publish(message);
            shell.capture_event();
            return;
        }
        for ((item, state), layout) in self
            .items
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
        {
            item.as_widget_mut().update(
                state, event, layout, cursor, renderer, clipboard, shell, viewport,
            );
        }
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.items
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
            .map(|((item, state), layout)| {
                item.as_widget()
                    .mouse_interaction(state, layout, cursor, viewport, renderer)
            })
            .max()
            .unwrap_or_default()
    }
    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let Some(viewport) = layout.bounds().intersection(viewport) else {
            return;
        };
        for ((item, state), layout) in self.items.iter().zip(&tree.children).zip(layout.children())
        {
            item.as_widget()
                .draw(state, renderer, theme, style, layout, cursor, &viewport);
        }
    }
    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, M, Theme, iced::Renderer>> {
        iced::advanced::overlay::from_children(
            &mut self.items,
            tree,
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::advanced::renderer::Headless;

    fn key_event(key: iced::keyboard::key::Named, modifiers: iced::keyboard::Modifiers) -> Event {
        use iced::keyboard::{Key, Location, key::Physical};
        Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key: Key::Named(key),
            modified_key: Key::Named(key),
            physical_key: Physical::Unidentified(iced::keyboard::key::NativeCode::Unidentified),
            location: Location::Standard,
            modifiers,
            text: None,
            repeat: false,
        })
    }

    fn caret_flow(count: usize, caret: usize) -> Element<'static, (), Theme> {
        let mut items: Vec<Element<'static, (), Theme>> = (0..count)
            .map(|_| iced::widget::space().width(80).height(22).into())
            .collect();
        items.push(
            iced::widget::text_input("", "a\u{301}b")
                .on_input(|_| ())
                .into(),
        );
        flow_with_caret(items, caret, "a\u{301}b", Some(()), Some(()))
    }

    #[test]
    fn caret_layout_keeps_input_last_but_places_it_between_groups() {
        let renderer = iced::futures::executor::block_on(iced::Renderer::new(
            iced::Font::DEFAULT,
            iced::Pixels(14.0),
            Some("tiny-skia"),
        ))
        .unwrap();
        for (caret, x, y) in [
            (0, 0.0, 0.0),
            (1, 84.0, 0.0),
            (2, 168.0, 0.0),
            (99, 168.0, 0.0),
        ] {
            let mut element = caret_flow(2, caret);
            let mut tree = Tree::new(&element);
            let node = element.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, Size::new(400.0, 200.0)),
            );
            let input = node.children()[2].bounds();
            assert_eq!((input.x, input.y, input.width), (x, y, 400.0 - x));
            if caret < 2 {
                assert!(node.children()[caret].bounds().y >= input.y + input.height);
            }
        }
    }

    #[test]
    fn input_focus_and_cursor_survive_group_count_and_caret_changes() {
        let element = caret_flow(2, 1);
        let mut tree = Tree::new(&element);
        let state = tree
            .children
            .last_mut()
            .unwrap()
            .state
            .downcast_mut::<InputState>();
        state.focus();
        state.move_cursor_to(1);
        for count in [4, 0, 1, 3] {
            let element = caret_flow(count, count / 2);
            tree.diff(element.as_widget());
            assert_eq!(tree.children.len(), count + 1);
            let state = tree
                .children
                .last()
                .unwrap()
                .state
                .downcast_ref::<InputState>();
            assert!(state.is_focused());
            assert_eq!(
                state
                    .cursor()
                    .state(&iced::widget::text_input::Value::new("ab")),
                iced::widget::text_input::cursor::State::Index(1)
            );
        }
    }

    #[test]
    fn arrows_only_navigate_focused_unmodified_unselected_boundaries() {
        use iced::keyboard::{Modifiers, key::Named};
        let element = caret_flow(0, 0);
        let mut tree = Tree::new(&element);
        let input = tree.children.last_mut().unwrap();
        let left = key_event(Named::ArrowLeft, Modifiers::empty());
        let right = key_event(Named::ArrowRight, Modifiers::empty());
        let query = "a\u{301}b";
        assert_eq!(boundary_arrow(input, &left, query), None);
        input.state.downcast_mut::<InputState>().focus();
        assert_eq!(boundary_arrow(input, &right, query), Some(true));
        input
            .state
            .downcast_mut::<InputState>()
            .move_cursor_to_front();
        assert_eq!(boundary_arrow(input, &left, query), Some(false));
        assert_eq!(boundary_arrow(input, &right, query), None);
        for modifiers in [
            Modifiers::SHIFT,
            Modifiers::CTRL,
            Modifiers::ALT,
            Modifiers::LOGO,
        ] {
            assert_eq!(
                boundary_arrow(input, &key_event(Named::ArrowLeft, modifiers), query),
                None
            );
        }
        input.state.downcast_mut::<InputState>().move_cursor_to(1);
        assert_eq!(boundary_arrow(input, &left, query), None);
        assert_eq!(boundary_arrow(input, &right, query), None);
        input.state.downcast_mut::<InputState>().select_all();
        assert_eq!(boundary_arrow(input, &left, query), None);
        assert_eq!(boundary_arrow(input, &right, query), None);
        input
            .state
            .downcast_mut::<InputState>()
            .move_cursor_to_front();
        assert_eq!(boundary_arrow(input, &left, ""), Some(false));
        assert_eq!(boundary_arrow(input, &right, ""), Some(true));
    }

    #[test]
    fn navigation_publishes_and_captures_without_losing_focus() {
        use iced::keyboard::{Modifiers, key::Named};
        let renderer = iced::futures::executor::block_on(iced::Renderer::new(
            iced::Font::DEFAULT,
            iced::Pixels(14.0),
            Some("tiny-skia"),
        ))
        .unwrap();
        let mut element = flow_with_caret(
            vec![iced::widget::text_input("", "ab").on_input(|_| 0).into()],
            0,
            "ab",
            Some(1),
            None,
        );
        let mut tree = Tree::new(&element);
        let node = element.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(400.0, 200.0)),
        );
        tree.children[0].state.downcast_mut::<InputState>().focus();
        tree.children[0]
            .state
            .downcast_mut::<InputState>()
            .move_cursor_to_front();
        for (key, expected, captured) in [
            (Named::ArrowLeft, vec![1], true),
            (Named::ArrowRight, vec![], true),
        ] {
            let mut messages = Vec::new();
            let mut shell = Shell::new(&mut messages);
            element.as_widget_mut().update(
                &mut tree,
                &key_event(key, Modifiers::empty()),
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut shell,
                &Rectangle::with_size(Size::new(400.0, 200.0)),
            );
            assert_eq!(shell.is_event_captured(), captured);
            assert_eq!(messages, expected);
            assert!(
                tree.children[0]
                    .state
                    .downcast_ref::<InputState>()
                    .is_focused()
            );
        }
        // The right arrow was handled by the input itself, not by navigation.
        assert_eq!(
            tree.children[0]
                .state
                .downcast_ref::<InputState>()
                .cursor()
                .state(&iced::widget::text_input::Value::new("ab")),
            iced::widget::text_input::cursor::State::Index(1)
        );
    }

    #[test]
    fn typing_field_fills_the_last_pill_line_and_wraps_when_needed() {
        let renderer = iced::futures::executor::block_on(iced::Renderer::new(
            iced::Font::DEFAULT,
            iced::Pixels(14.0),
            Some("tiny-skia"),
        ))
        .unwrap();
        for (width, expected_x, expected_y) in [(400.0, 168.0, 0.0), (250.0, 0.0, 32.0)] {
            let mut element = flow::<()>(vec![
                iced::widget::space().width(80).height(22).into(),
                iced::widget::space().width(80).height(22).into(),
                iced::widget::space().width(Length::Fill).height(28).into(),
            ]);
            let mut tree = Tree::new(&element);
            let node = element.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, Size::new(width, 200.0)),
            );
            let field = node.children().last().unwrap().bounds();
            assert_eq!(field.x, expected_x);
            assert_eq!(field.y, expected_y);
            assert_eq!(field.width, width - expected_x);
            assert_eq!(node.size().height, expected_y + 28.0);
        }
    }

    #[test]
    fn narrow_fields_wrap_pills_without_overlapping_the_typing_field() {
        let renderer = iced::futures::executor::block_on(iced::Renderer::new(
            iced::Font::DEFAULT,
            iced::Pixels(14.0),
            Some("tiny-skia"),
        ))
        .unwrap();
        let mut element = flow::<()>(vec![
            iced::widget::space().width(100).height(22).into(),
            iced::widget::space().width(100).height(22).into(),
            iced::widget::space().width(Length::Fill).height(28).into(),
        ]);
        let mut tree = Tree::new(&element);
        let node = element.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(140.0, 200.0)),
        );
        let children = node.children();
        assert!(children[1].bounds().y >= children[0].bounds().y + children[0].size().height);
        assert!(children[2].bounds().y >= children[1].bounds().y + children[1].size().height);
        assert_eq!(children[2].size().width, 140.0);
    }
}
