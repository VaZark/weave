//! App-specific: the round play button of a list (no label).

use iced::widget::{button, container};
use iced::{Length, Padding};
use iced_adaptive_design::{Element, style};

use crate::icons::Glyph;

pub fn play_button<'a, M: Clone + 'a>(on_press: Option<M>) -> Element<'a, M> {
    let size = 44.0;
    button(container(Glyph::Play.view(20.0)).center(Length::Fixed(size)))
        .padding(Padding::from(0.0))
        .style(move |t: &iced_adaptive_design::Theme, st| {
            let base = style::button::suggested(t, st);
            button::Style {
                border: iced::Border {
                    radius: (size / 2.0).into(),
                    ..base.border
                },
                ..base
            }
        })
        .on_press_maybe(on_press)
        .into()
}
