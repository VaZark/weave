//! Generic: a round badge, a coloured disc around any content (an icon, a number, an initial).
//! Candidate for upstreaming next to `avatar`.

use iced::widget::container;
use iced::{Background, Border, Color, Length};
use iced_adaptive_design::{Element, Theme};

/// Black or white, whichever reads better on `bg`.
pub fn contrast(bg: Color) -> Color {
    let l = 0.2126 * bg.r + 0.7152 * bg.g + 0.0722 * bg.b;
    if l > 0.6 { Color::BLACK } else { Color::WHITE }
}

pub fn badge<'a, M: 'a>(
    content: impl Into<Element<'a, M>>,
    color: Color,
    size: f32,
) -> Element<'a, M> {
    container(content)
        .center(Length::Fixed(size))
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(color)),
            border: Border::default().rounded(size / 2.0),
            ..Default::default()
        })
        .into()
}
