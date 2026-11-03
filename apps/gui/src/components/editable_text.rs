//! Generic: text that is read until you ask to change it, with a long text clamped to a few
//! lines that **pushes what is below it down** when expanded.
//!
//! One piece of content, two variants chosen by the application's `editing` flag with the same
//! `primitives::meta` rules a `MetaComponent` uses (a selected-key trigger, then a fallback). It
//! calls `meta::pick` directly instead of building a `MetaComponent`: that one is a `responsive`,
//! which cannot shrink to its content's height, and this has to flow with the page like
//! `ReorderableList` does.
//!
//! * **text**: a `Spoiler` (clipped to `max_height`, a fade over the cut, Show more / Hide that
//!   slides open in the layout flow) and an edit button; a dimmed placeholder while empty.
//! * **entry**: a text input; Enter or the check button finishes.
//!
//! Candidate for upstreaming, together with a shrinking mode for `MetaComponent`.

use iced::widget::{container, row, text_input};
use iced::{Alignment, Length};
use iced_adaptive_design::breakpoints::Breakpoints;
use iced_adaptive_design::primitives::When;
use iced_adaptive_design::primitives::meta::{self, Trigger};
use iced_adaptive_design::widget::{self as w, typography as ty};
use iced_adaptive_design::{Element, Icon, Theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Variant {
    Entry,
    Text,
}

/// What the application keeps and sends.
pub struct Messages<'a, M> {
    pub on_edit: M,
    pub on_input: Box<dyn Fn(String) -> M + 'a>,
    pub on_done: M,
    pub on_expand: Box<dyn Fn(bool) -> M + 'a>,
}

pub fn editable_text<'a, M: Clone + 'a>(
    value: &'a str,
    placeholder: &'a str,
    editing: bool,
    expanded: bool,
    max_height: f32,
    msg: Messages<'a, M>,
) -> Element<'a, M> {
    let variants = [
        (Variant::Entry, Trigger::selected(Variant::Entry)),
        (Variant::Text, Trigger::from(When::always())),
    ];
    let selected: Vec<Variant> = editing.then_some(Variant::Entry).into_iter().collect();
    let picked = meta::pick(
        variants.iter().map(|(_, t)| t),
        &selected,
        0.0,
        &Breakpoints::current(),
    )
    .map_or(Variant::Text, |i| variants[i].0);

    match picked {
        Variant::Entry => row![
            text_input(placeholder, value)
                .on_input(msg.on_input)
                .on_submit(msg.on_done.clone())
                .width(Length::Fill),
            w::icon_button(Icon::Check).on_press(msg.on_done),
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .into(),
        Variant::Text if value.is_empty() => w::flat_button(
            row![
                ty::body(placeholder).style(|t: &Theme| iced::widget::text::Style {
                    color: Some(iced::Color {
                        a: 0.55,
                        ..t.palette.window_fg_color
                    }),
                }),
                Icon::Edit.view(14.0),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .on_press(msg.on_edit)
        .into(),
        Variant::Text => {
            let on_expand = msg.on_expand;
            row![
                container(
                    w::Spoiler::new(ty::body(value.to_string()))
                        .max_height(max_height)
                        .expanded(expanded)
                        .on_toggle(on_expand)
                )
                .width(Length::Fill),
                w::icon_button(Icon::Edit).on_press(msg.on_edit),
            ]
            .spacing(6)
            .align_y(Alignment::Start)
            .into()
        }
    }
}
