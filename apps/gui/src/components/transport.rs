//! App-specific: the transport buttons and the time readout.

use iced_adaptive_design::Element;
use iced_adaptive_design::widget as w;

use crate::icons::Glyph;

/// A flat glyph button; `active` makes it the accent one (shuffle on, queue open).
pub fn glyph_button<'a, M: Clone + 'a>(
    glyph: Glyph,
    size: f32,
    active: bool,
    on_press: M,
) -> Element<'a, M> {
    let icon = glyph.view(size);
    if active {
        w::suggested_button(icon).on_press(on_press).into()
    } else {
        w::flat_button(icon).on_press(on_press).into()
    }
}
