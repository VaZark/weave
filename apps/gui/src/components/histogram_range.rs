//! Generic: a histogram that doubles as a range picker. Bars inside the selected range are
//! accented, the others dimmed, and pressing a bar selects that bin's range. Put a
//! `range_slider` under it for the exact handles. Candidate for upstreaming.
//!
//! After the numeric facets of Esri's `HistogramRangeSlider` and Hooked on Facets: the
//! distribution shows where values cluster, which two bare handles cannot.

use iced::widget::{column, container, mouse_area, row, space};
use iced::{Alignment, Background, Border, Color, Length};
use iced_adaptive_design::{Element, Theme};

/// `bins` evenly cover `min..max`; `selection` is the chosen `(lo, hi)`; `on_pick` gets a bin's
/// range when its bar is pressed.
pub fn histogram_range<'a, M: Clone + 'a>(
    bins: &[usize],
    min: f32,
    max: f32,
    selection: (f32, f32),
    height: f32,
    on_pick: impl Fn((f32, f32)) -> M + 'a,
) -> Element<'a, M> {
    let peak = bins.iter().copied().max().unwrap_or(0).max(1) as f32;
    let width = (max - min) / bins.len().max(1) as f32;
    let bars = bins.iter().enumerate().map(|(i, count)| {
        let (lo, hi) = (min + i as f32 * width, min + (i + 1) as f32 * width);
        let inside = hi > selection.0 && lo < selection.1;
        let h = if *count == 0 {
            2.0
        } else {
            (*count as f32 / peak * height).max(4.0)
        };
        let bar = container(space::horizontal())
            .width(Length::Fill)
            .height(Length::Fixed(h))
            .style(move |t: &Theme| container::Style {
                background: Some(Background::Color(if inside {
                    t.palette.accent_bg_color
                } else {
                    Color {
                        a: 0.22,
                        ..t.palette.window_fg_color
                    }
                })),
                border: Border::default().rounded(2),
                ..Default::default()
            });
        mouse_area(
            column![space::vertical().height(Length::Fill), bar]
                .height(Length::Fixed(height))
                .align_x(Alignment::Center),
        )
        .on_press(on_pick((lo, hi)))
        .into()
    });
    row(bars).spacing(2).width(Length::Fill).into()
}
