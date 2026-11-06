//! One modal flow: bounded desktop dialog, full-screen narrow sheet.
use iced::widget::{Stack, container, opaque};
use iced::{Length, Size};
use iced_adaptive_design::{self as iad, Element, prelude as w};

pub fn panel<'a, M: 'a>(content: Element<'a, M>, narrow: bool, viewport: Size) -> Element<'a, M> {
    let panel = container(content).padding(20);
    if narrow {
        panel
            .width(Length::Fill)
            .height(Length::Fill)
            .style(iad::styles::container::window)
            .into()
    } else {
        panel
            .width(Length::Fixed(560.0_f32.min(viewport.width)))
            .height(Length::Fixed(
                640.0_f32.min((viewport.height - 80.0).max(160.0)),
            ))
            .style(iad::styles::container::dialog)
            .into()
    }
}

pub fn present<'a, M: Clone + 'a>(
    base: Element<'a, M>,
    editor: Option<(Element<'a, M>, M)>,
    narrow: bool,
    viewport: Size,
) -> Element<'a, M> {
    let (content, close) = match editor {
        Some((content, close)) => (Some(panel(content, narrow, viewport)), Some(close)),
        None => (None, None),
    };
    if narrow {
        let mut layers = Stack::with_children(vec![base])
            .width(Length::Fill)
            .height(Length::Fill);
        if let Some(content) = content {
            layers = layers.push(opaque(w::FocusScope::new(content).trap(true)));
        }
        layers.into()
    } else {
        w::present_dialog(base, content, close, 0.0)
    }
}
