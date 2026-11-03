//! Generic: a scrolling list of **fixed-height rows** that builds only the rows in view. The
//! caller keeps a [`Viewport`] fed by `on_scroll`, asks it for the window with
//! `viewport.window(total, row_height, overscan)`, builds those rows, and passes them with the
//! index of the first. Spacers above and below keep the scroll bar true to the whole list.
//! Rows have no separators and no surface of their own: the list is flat.
//! Candidate for upstreaming next to `ResponsiveTable::virtualized`.

use iced::Length;
use iced::widget::{column, container, scrollable, space};
use iced_adaptive_design::Element;
use iced_adaptive_design::primitives::virtualize::Viewport;

pub fn virtual_list<'a, M: 'a>(
    rows: Vec<Element<'a, M>>,
    total: usize,
    first: usize,
    row_height: f32,
    on_scroll: impl Fn(Viewport) -> M + 'a,
) -> Element<'a, M> {
    let below = total.saturating_sub(first + rows.len());
    let rows: Vec<Element<'a, M>> = rows
        .into_iter()
        .map(|r| container(r).height(Length::Fixed(row_height)).into())
        .collect();
    scrollable(column![
        space::vertical().height(Length::Fixed(first as f32 * row_height)),
        column(rows),
        space::vertical().height(Length::Fixed(below as f32 * row_height)),
    ])
    .on_scroll(move |v| {
        on_scroll(Viewport {
            offset: v.absolute_offset().y,
            height: v.bounds().height,
        })
    })
    .height(Length::Fill)
    .into()
}
