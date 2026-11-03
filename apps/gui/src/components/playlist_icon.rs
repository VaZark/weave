//! App-specific: a playlist's picture. Its icon on a colour taken from its name, or an avatar
//! (the initials) when it has no icon.

use iced_adaptive_design::Element;
use iced_adaptive_design::widget as w;

use super::badge::{badge, contrast};
use super::cover::color;

pub fn playlist_icon<'a, M: 'a>(name: &str, icon: Option<&str>, size: f32) -> Element<'a, M> {
    match icon {
        Some(key) => {
            // A stable colour per name, so a playlist keeps its look.
            let c = color(
                name.bytes()
                    .fold(0usize, |h, b| h.wrapping_mul(31).wrapping_add(b as usize)),
            );
            badge(
                crate::icons::for_key(key).colored(size * 0.5, contrast(c)),
                c,
                size,
            )
        }
        None => w::avatar(size, name.to_string()).into(),
    }
}
