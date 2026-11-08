//! One modal flow: bounded desktop dialog, full-screen narrow sheet.
use iced::widget::{Stack, container, opaque};
use iced::{Length, Size};
use iced_adaptive_design::{self as iad, Element, prelude as w};

fn sheet_style(theme: &iad::Theme, window_radius: f32) -> container::Style {
    let mut style = iad::styles::container::window(theme);
    // This surface overlays the frame, so it must preserve the window's transparent corners.
    style.border.radius = window_radius.into();
    style
}

pub fn panel<'a, M: 'a>(
    content: Element<'a, M>,
    narrow: bool,
    viewport: Size,
    window_radius: f32,
) -> Element<'a, M> {
    let panel = container(content).padding(20);
    if narrow {
        panel
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |theme| sheet_style(theme, window_radius))
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
    window_radius: f32,
) -> Element<'a, M> {
    let (content, close) = match editor {
        Some((content, close)) => (
            Some(panel(content, narrow, viewport, window_radius)),
            Some(close),
        ),
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

#[cfg(test)]
mod tests {
    use super::*;
    use iad::windows::{Chrome, State};

    #[test]
    fn fullscreen_sheet_matches_all_window_corners() {
        let theme = iad::Theme::light();
        for (fullscreen, csd) in [(false, true), (true, true), (false, false)] {
            let chrome = Chrome::new(State {
                maximized: false,
                fullscreen,
            })
            .csd_override(csd);
            let radius = chrome.radius();
            let style = sheet_style(&theme, radius);
            assert_eq!(style.border.radius, iced::border::Radius::from(radius));
            assert_eq!(
                style.background,
                iad::styles::container::window(&theme).background
            );
        }
    }
}
