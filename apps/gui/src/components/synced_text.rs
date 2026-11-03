//! Generic: a scrolling list of timed lines with the current one highlighted. Tapping a line
//! asks to seek to it. The caller keeps the scroll position with
//! [`primitives::synced::scroll_to`](crate::primitives::synced::scroll_to) and
//! `iced::widget::operation::snap_to` whenever the current line changes.
//!
//! ```ignore
//! synced_text(LYRICS_ID, &lines, synced::current(&lines, pos), Message::Seek)
//! ```

use iced::widget::{Id, column, container, scrollable};
use iced::{Alignment, Length};
use iced_adaptive_design::widget::{self as w, typography as ty};
use iced_adaptive_design::{Element, Theme};

use crate::primitives::synced::Line;

pub fn synced_text<'a, M: Clone + 'a>(
    id: Id,
    lines: &'a [Line],
    current: Option<usize>,
    on_seek: impl Fn(f32) -> M + 'a,
) -> Element<'a, M> {
    if lines.is_empty() {
        return container(ty::body("No lyrics for this track."))
            .center(Length::Fill)
            .into();
    }
    let rows = lines.iter().enumerate().map(|(i, l)| {
        let text = if Some(i) == current {
            ty::title_3(l.text.as_str())
        } else {
            ty::body(l.text.as_str()).size(18)
        };
        let text = if Some(i) == current {
            text
        } else {
            text.style(|t: &Theme| iced::widget::text::Style {
                color: Some(iced::Color {
                    a: 0.55,
                    ..t.palette.window_fg_color
                }),
            })
        };
        w::flat_button(container(text).padding([4, 8]))
            .on_press(on_seek(l.at))
            .into()
    });
    scrollable(
        column(rows)
            .spacing(2)
            .padding([60, 12])
            .align_x(Alignment::Start)
            .width(Length::Fill),
    )
    .id(id)
    .height(Length::Fill)
    .into()
}
