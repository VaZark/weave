//! Inline pill flow: the last child is a text field that fills the remaining line.
use iced::advanced::{
    Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer,
    widget::{Operation, Tree},
};
use iced::{Element, Event, Length, Point, Rectangle, Size, Vector};
use iced_adaptive_design::Theme;

pub fn flow<'a, M: 'a>(items: Vec<Element<'a, M, Theme>>) -> Element<'a, M, Theme> {
    Element::new(TagsInputFlow { items })
}

struct TagsInputFlow<'a, M> {
    items: Vec<Element<'a, M, Theme>>,
}

impl<M> Widget<M, Theme, iced::Renderer> for TagsInputFlow<'_, M> {
    fn children(&self) -> Vec<Tree> {
        self.items.iter().map(Tree::new).collect()
    }
    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&self.items);
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
        let mut nodes = Vec::new();
        let (mut x, mut y, mut line_height) = (0.0_f32, 0.0_f32, 28.0_f32);
        let last = self.items.len().saturating_sub(1);
        for (index, (item, state)) in self.items.iter_mut().zip(&mut tree.children).enumerate() {
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
            nodes.push(node);
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
