//! App-specific: a tag drawn as iad's `Chip`, with the tag's own icon and colour and its value as
//! the label. The full name (with the key) is on hover for the read-only one.

use iced_adaptive_design::Element;
use iced_adaptive_design::prelude::{Chip, ChipSize};

use crate::data::TagStyle;

/// The part after the key: `bachata` for `genre:bachata`.
pub fn value_of(name: &str) -> &str {
    name.split_once(':').map_or(name, |(_, v)| v)
}

fn chip<'a, M: Clone + 'a>(name: &str, style: &TagStyle) -> Chip<'a, M> {
    Chip::new(value_of(name))
        .icon(crate::icons::for_key(&style.icon))
        .color(style.color)
}

/// A read-only tag.
pub fn tag_pill<'a, M: Clone + 'a>(name: &str, style: &TagStyle, size: ChipSize) -> Element<'a, M> {
    iced_adaptive_design::prelude::tooltip(
        Element::from(chip::<M>(name, style).size(size)),
        name.to_string(),
        iced_adaptive_design::prelude::TooltipPosition::Bottom,
    )
}
