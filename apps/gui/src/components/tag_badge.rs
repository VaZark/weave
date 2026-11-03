//! App-specific: a tag drawn as iad's `Chip`, with the tag's own icon and colour and its value as
//! the label. The full name (with the key) is on hover for the read-only one.

use iced::widget::{text, tooltip};
use iced_adaptive_design::Element;
use iced_adaptive_design::widget::Chip;

use crate::data::TagStyle;

/// The part after the key: `bachata` for `genre:bachata`.
fn value_of(name: &str) -> &str {
    name.split_once(':').map_or(name, |(_, v)| v)
}

fn chip<'a, M: Clone + 'a>(name: &str, style: &TagStyle) -> Chip<'a, M> {
    Chip::new(value_of(name))
        .icon(crate::icons::for_key(&style.icon))
        .color(style.color)
}

/// A read-only tag.
pub fn tag_pill<'a, M: Clone + 'a>(name: &str, style: &TagStyle) -> Element<'a, M> {
    tooltip(
        Element::from(chip::<M>(name, style)),
        text(name.to_string()).size(12),
        tooltip::Position::Bottom,
    )
    .gap(4)
    .into()
}

/// A tag as a toggle: solid in its colour while selected.
pub fn tag_chip<'a, M: Clone + 'a>(
    name: &str,
    style: &TagStyle,
    selected: bool,
    on_press: M,
) -> Element<'a, M> {
    chip(name, style)
        .selected(selected)
        .on_press(on_press)
        .into()
}
