//! Generic: the head of a detail page: an optional cover, the title, a place for a description,
//! a line of facts, and an optional control at the end (a menu).

use iced::widget::{column, row};
use iced::{Alignment, Length};
use iced_adaptive_design::Element;
use iced_adaptive_design::widget::typography as ty;

pub fn detail_header<'a, M: 'a>(
    leading: Option<Element<'a, M>>,
    title: impl Into<String>,
    description: impl Into<Element<'a, M>>,
    facts: impl Into<Element<'a, M>>,
    trailing: Option<Element<'a, M>>,
) -> Element<'a, M> {
    let mut r = row![].spacing(16).align_y(Alignment::Start);
    if let Some(l) = leading {
        r = r.push(l);
    }
    r = r.push(
        column![ty::title_1(title.into()), description.into(), facts.into()]
            .spacing(6)
            .width(Length::Fill),
    );
    if let Some(t) = trailing {
        r = r.push(t);
    }
    r.into()
}
