//! App-specific: a cover. Until the engine provides art, a colour from the track and a note.

use iced::widget::container;
use iced::{Background, Border, Color, Length};
use iced_adaptive_design::Element;

use crate::icons::Glyph;

const COLORS: [(u8, u8, u8); 8] = [
    (0x62, 0xa0, 0xea),
    (0x57, 0xe3, 0x89),
    (0xf6, 0xd3, 0x2d),
    (0xff, 0xa3, 0x48),
    (0xed, 0x33, 0x3b),
    (0xc0, 0x61, 0xcb),
    (0x8f, 0xf0, 0xa4),
    (0x99, 0xc1, 0xf1),
];

pub fn color(id: usize) -> Color {
    let (r, g, b) = COLORS[id % COLORS.len()];
    Color::from_rgb8(r, g, b)
}

pub fn cover<'a, M: 'a>(id: usize, size: f32) -> Element<'a, M> {
    let c = color(id);
    container(Glyph::Note.colored(size * 0.45, Color::from_rgba8(0, 0, 0, 0.55)))
        .center(Length::Fixed(size))
        .style(move |_: &iced_adaptive_design::Theme| container::Style {
            background: Some(Background::Color(c)),
            border: Border::default().rounded((size * 0.14).max(4.0)),
            ..Default::default()
        })
        .into()
}
