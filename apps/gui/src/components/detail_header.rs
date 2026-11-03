//! Generic: the head of a detail page (playlist, artist, album), after the usual music-app
//! pattern. Wide: the cover on the left, a small kind label, the large title, the description and
//! facts beside it, and an action row under both (play, options, then something at the far end).
//! Narrow: the cover centred, then the title, description and facts, and one action row with the
//! options at the start and the round play button at the end. A wash of `tint` fades down from the
//! top. Candidate for upstreaming.

use iced::gradient::Linear;
use iced::widget::{column, container, row, space, text};
use iced::{Alignment, Background, Color, Length, Radians};
use iced_adaptive_design::widget::typography as ty;
use iced_adaptive_design::{Element, Theme};

pub struct Header<'a, M> {
    pub cover: Element<'a, M>,
    /// "Playlist", "Album", "Artist"...
    pub kind: String,
    pub title: String,
    pub description: Element<'a, M>,
    pub facts: Element<'a, M>,
    pub play: Element<'a, M>,
    /// Options next to the play button (a menu, a favourite toggle).
    pub actions: Vec<Element<'a, M>>,
    /// Pushed to the far end of the action row in the wide layout.
    pub end: Option<Element<'a, M>>,
    pub tint: Color,
}

pub fn detail_header<'a, M: 'a>(h: Header<'a, M>, narrow: bool) -> Element<'a, M> {
    let tint = h.tint;
    let wash = move |_: &Theme| container::Style {
        background: Some(Background::Gradient(
            Linear::new(Radians(std::f32::consts::FRAC_PI_2))
                .add_stop(0.0, Color { a: 0.42, ..tint })
                .add_stop(1.0, Color { a: 0.0, ..tint })
                .into(),
        )),
        ..Default::default()
    };
    if narrow {
        let mut actions = row![].spacing(8).align_y(Alignment::Center);
        for a in h.actions {
            actions = actions.push(a);
        }
        let actions = actions.push(space::horizontal()).push(h.play);
        return container(
            column![
                container(h.cover).center_x(Length::Fill),
                ty::title_2(h.title),
                h.description,
                h.facts,
                actions,
            ]
            .spacing(10),
        )
        .padding(16)
        .width(Length::Fill)
        .style(wash)
        .into();
    }
    let mut actions = row![h.play].spacing(12).align_y(Alignment::Center);
    for a in h.actions {
        actions = actions.push(a);
    }
    actions = actions.push(space::horizontal());
    if let Some(e) = h.end {
        actions = actions.push(e);
    }
    container(
        column![
            row![
                h.cover,
                column![
                    space::vertical().height(Length::Fill),
                    ty::caption(h.kind),
                    text(h.title).size(48).font(iced::Font {
                        weight: iced::font::Weight::Bold,
                        ..iced::Font::DEFAULT
                    }),
                    h.description,
                    h.facts,
                ]
                .spacing(6)
                .width(Length::Fill),
            ]
            .spacing(24)
            .height(Length::Shrink),
            actions,
        ]
        .spacing(16),
    )
    .padding(24)
    .width(Length::Fill)
    .style(wash)
    .into()
}
