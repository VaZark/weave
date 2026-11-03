//! App-specific: a tag as its icon on its colour and its value; the full name (with the key) on
//! hover.

use iced::widget::{container, row, text, tooltip};
use iced::{Alignment, Background, Border, Color, Padding};
use iced_adaptive_design::{Element, Theme};

use super::badge::{badge, contrast};
use crate::data::TagStyle;
use crate::icons::Glyph;

/// The part after the key: `bachata` for `genre:bachata`.
fn value_of(name: &str) -> &str {
    name.split_once(':').map_or(name, |(_, v)| v)
}

/// A read-only tag: icon badge and value on a faint tint of the tag's colour.
pub fn tag_pill<'a, M: 'a>(name: &str, style: &TagStyle, size: f32) -> Element<'a, M> {
    let color = style.color;
    let glyph = Glyph::for_tag_icon(&style.icon).colored(size * 0.58, contrast(color));
    let pill = container(
        row![
            badge(glyph, color, size),
            text(value_of(name).to_string()).size(12)
        ]
        .spacing(5)
        .align_y(Alignment::Center),
    )
    .padding(Padding {
        top: 2.0,
        right: 8.0,
        bottom: 2.0,
        left: 2.0,
    })
    .style(move |_: &Theme| container::Style {
        background: Some(Background::Color(Color { a: 0.16, ..color })),
        border: Border::default().rounded(size),
        ..Default::default()
    });
    tooltip(
        pill,
        text(name.to_string()).size(12),
        tooltip::Position::Bottom,
    )
    .gap(4)
    .into()
}

/// A tag as a toggle: its badge and its value; the accent button while it is on.
pub fn tag_chip<'a, M: Clone + 'a>(
    name: &str,
    style: &TagStyle,
    selected: bool,
    on_press: M,
) -> Element<'a, M> {
    use iced::widget::button;
    use iced_adaptive_design::style as st;

    let glyph = Glyph::for_tag_icon(&style.icon).colored(11.0, contrast(style.color));
    let content = row![
        badge(glyph, style.color, 20.0),
        text(value_of(name).to_string())
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    let b = button(content)
        .padding(Padding {
            top: 3.0,
            right: 10.0,
            bottom: 3.0,
            left: 4.0,
        })
        .on_press(on_press);
    if selected {
        b.style(st::button::suggested).into()
    } else {
        b.style(st::button::default).into()
    }
}
