//! A histogram over a range slider that edit **one** range: pressing a bar picks that bin's range,
//! dragging a thumb moves the ends, and the bars inside the range are accented.
//!
//! This used to be `iced-adaptive-design`'s `HistogramRangeSlider` (with its charting library); the
//! library dropped charts, so the app owns it. It is still a `ComposedComponent` (two parts, one
//! shared range, one rule `lo <= hi`) over the library's `range_slider`; the bars are plain widgets.
//!
//! ```ignore
//! HistogramRangeSlider::new(histogram, (lo, hi), Message::Bpm).step(5.0).height(48.0)
//! ```

use iad::metas::ComposedComponent;
use iad::widgets::range_slider;
use iced::widget::{container, mouse_area, row, space};
use iced::Length;
use iced_adaptive_design as iad;
use std::borrow::Cow;
use std::rc::Rc;


/// Counts per bin. Bin `i` covers `[min + i·w, min + (i+1)·w)`, with `w = (max - min) / bins`.
#[derive(Debug, Clone, PartialEq)]
pub struct Histogram {
    min: f32,
    max: f32,
    counts: Vec<usize>,
}

impl Histogram {
    /// Already counted bins covering `min..max`.
    pub fn from_counts(min: f32, max: f32, counts: impl Into<Vec<usize>>) -> Self {
        Self {
            min,
            max,
            counts: counts.into(),
        }
    }

    /// Counts `values` into `bins` bins over `min..max`. Values outside are ignored; `max`
    /// itself falls in the last bin.
    pub fn from_values(
        values: impl IntoIterator<Item = f32>,
        min: f32,
        max: f32,
        bins: usize,
    ) -> Self {
        let mut h = Self::from_counts(min, max, vec![0; bins]);
        for v in values {
            if let Some(i) = h.bin_of(v) {
                h.counts[i] += 1;
            }
        }
        h
    }

    pub fn min(&self) -> f32 {
        self.min
    }
    pub fn max(&self) -> f32 {
        self.max
    }
    pub fn counts(&self) -> &[usize] {
        &self.counts
    }
    pub fn len(&self) -> usize {
        self.counts.len()
    }
    pub fn is_empty(&self) -> bool {
        self.counts.is_empty()
    }
    /// The highest count (at least 1, so it can divide).
    pub fn peak(&self) -> usize {
        self.counts.iter().copied().max().unwrap_or(0).max(1)
    }
    /// The width of one bin.
    pub fn bin_width(&self) -> f32 {
        (self.max - self.min) / self.counts.len().max(1) as f32
    }
    /// The `(lo, hi)` bounds of bin `i`.
    pub fn bin_range(&self, i: usize) -> (f32, f32) {
        let w = self.bin_width();
        (self.min + i as f32 * w, self.min + (i + 1) as f32 * w)
    }
    /// The bin holding `value`, if it is inside `min..=max`.
    pub fn bin_of(&self, value: f32) -> Option<usize> {
        if self.counts.is_empty() || !(self.min..=self.max).contains(&value) || self.max <= self.min
        {
            return None;
        }
        let i = ((value - self.min) / self.bin_width()) as usize;
        Some(i.min(self.counts.len() - 1))
    }
    /// How tall bin `i` is, as a fraction of the tallest.
    pub fn fraction(&self, i: usize) -> f32 {
        self.counts.get(i).copied().unwrap_or(0) as f32 / self.peak() as f32
    }
    /// Whether bin `i` overlaps `selection` (`lo <= hi`).
    pub fn selected(&self, i: usize, selection: (f32, f32)) -> bool {
        let (lo, hi) = self.bin_range(i);
        hi > selection.0 && lo < selection.1
    }
    /// How many values the `selection` holds, by bin.
    pub fn count_in(&self, selection: (f32, f32)) -> usize {
        (0..self.len())
            .filter(|i| self.selected(*i, selection))
            .map(|i| self.counts[i])
            .sum()
    }
}

/// A histogram, borrowed or owned, where either is asked for: `&hist` from the app's state, or one
/// built on the spot in a view when it depends on a filter
/// (`HistogramRangeSlider::new(Histogram::from_counts(..), ..)`).
impl<'a> From<&'a Histogram> for std::borrow::Cow<'a, Histogram> {
    fn from(h: &'a Histogram) -> Self {
        std::borrow::Cow::Borrowed(h)
    }
}

impl From<Histogram> for std::borrow::Cow<'_, Histogram> {
    fn from(h: Histogram) -> Self {
        std::borrow::Cow::Owned(h)
    }
}

#[cfg(test)]
mod histogram_tests {
    use super::*;

    #[test]
    fn values_are_counted_into_bins() {
        let h = Histogram::from_values([0.0, 1.0, 4.9, 5.0, 10.0, 11.0, -1.0], 0.0, 10.0, 2);
        assert_eq!(h.counts(), &[3, 2]);
        assert_eq!(h.bin_of(10.0), Some(1));
        assert_eq!(h.bin_of(11.0), None);
    }

    #[test]
    fn bins_cover_the_range() {
        let h = Histogram::from_counts(0.0, 100.0, [1, 2, 4, 2]);
        assert_eq!(h.bin_range(0), (0.0, 25.0));
        assert_eq!(h.bin_range(3), (75.0, 100.0));
        assert_eq!(h.fraction(2), 1.0);
        assert_eq!(h.fraction(0), 0.25);
    }

    #[test]
    fn a_bin_is_selected_when_it_overlaps() {
        let h = Histogram::from_counts(0.0, 100.0, [1, 2, 4, 2]);
        assert!(!h.selected(0, (25.0, 60.0)));
        assert!(h.selected(1, (25.0, 60.0)));
        assert!(h.selected(2, (25.0, 60.0)));
        assert!(!h.selected(3, (25.0, 60.0)));
        assert_eq!(h.count_in((25.0, 60.0)), 6);
    }

    #[test]
    fn empty_is_safe() {
        let h = Histogram::from_counts(0.0, 1.0, []);
        assert_eq!(h.bin_of(0.5), None);
        assert_eq!(h.peak(), 1);
        assert!(h.is_empty());
    }
}

/// See the [module](self).
pub struct HistogramRangeSlider<'a, M> {
    histogram: Cow<'a, Histogram>,
    range: (f32, f32),
    on_change: Box<dyn Fn((f32, f32)) -> M + 'a>,
    step: Option<f32>,
    height: f32,
}

impl<'a, M: 'a> HistogramRangeSlider<'a, M> {
    /// `histogram` is borrowed (`&hist`) or owned (built in the view, for one that depends on a
    /// filter). `range` is the chosen `(lo, hi)` within its `min..max`; `on_change` gets the next
    /// range from whichever part the user touched.
    pub fn new(
        histogram: impl Into<Cow<'a, Histogram>>,
        range: (f32, f32),
        on_change: impl Fn((f32, f32)) -> M + 'a,
    ) -> Self {
        Self {
            histogram: histogram.into(),
            range,
            on_change: Box::new(on_change),
            step: None,
            height: 48.0,
        }
    }
    /// How far a thumb moves at a time, and the least the thumbs may be apart.
    pub fn step(mut self, step: f32) -> Self {
        self.step = Some(step.max(f32::EPSILON));
        self
    }
    /// The height of the bars (default 48).
    pub fn height(mut self, px: f32) -> Self {
        self.height = px;
        self
    }
}

/// The bars: one column per bin, accented inside `range`; pressing one sends its range.
fn bars<'a>(h: &Histogram, range: (f32, f32), height: f32) -> iad::Element<'a, (f32, f32)> {
    let gap = 2.0;
    let cells = (0..h.len()).map(|i| {
        // A stub for an empty bin keeps the row of bars readable.
        let bar = (h.fraction(i) * height).max(2.0);
        let inside = h.selected(i, range);
        let fill = container(space().width(Length::Fill).height(Length::Fixed(bar)))
            .width(Length::Fill)
            .style(move |t: &iad::Theme| iced::widget::container::Style {
                background: Some(
                    if inside {
                        t.semantic.accent.bg
                    } else {
                        iad::color::alpha(t.semantic.window.fg, 0.2)
                    }
                    .into(),
                ),
                border: iced::Border::default().rounded(2.0),
                ..Default::default()
            });
        mouse_area(
            container(fill)
                .width(Length::Fill)
                .height(Length::Fixed(height))
                .align_bottom(Length::Fixed(height)),
        )
        .on_press(h.bin_range(i))
        .interaction(iced::mouse::Interaction::Pointer)
        .into()
    });
    row(cells).spacing(gap).width(Length::Fill).into()
}

impl<'a, M: 'a> From<HistogramRangeSlider<'a, M>> for iad::Element<'a, M> {
    fn from(c: HistogramRangeSlider<'a, M>) -> Self {
        let (step, height) = (c.step, c.height);
        let h: Rc<Histogram> = Rc::new(c.histogram.into_owned());
        let (min, max) = (h.min(), h.max());
        ComposedComponent::new(c.range, c.on_change)
            // Part 1: the bars. Those inside the range are accented; pressing one picks its range.
            .part({
                let h = h.clone();
                move |r| bars(&h, *r, height)
            })
            // Part 2: the thumbs, for the exact ends.
            .part(move |r| {
                let mut s = range_slider(min..=max, *r, |moved| moved);
                if let Some(step) = step {
                    s = s.step(step).min_gap(step);
                }
                s.into()
            })
            // The rule, once, whichever part sent the value: lo <= hi.
            .normalize(|(lo, hi)| (lo.min(hi), lo.max(hi)))
            .spacing(iad::Metrics::desktop().space_s)
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_builder_keeps_what_it_is_given() {
        let h = Histogram::from_counts(60.0, 180.0, [1, 3, 2]);
        let s: HistogramRangeSlider<'_, ()> = HistogramRangeSlider::new(&h, (80.0, 120.0), |_| ())
            .step(5.0)
            .height(32.0);
        assert_eq!((s.range, s.step, s.height), ((80.0, 120.0), Some(5.0), 32.0));
    }

    #[test]
    fn it_builds_as_an_element() {
        let h = Histogram::from_counts(0.0, 10.0, [1, 2, 3]);
        let _: iad::Element<'_, ()> = HistogramRangeSlider::new(h, (2.0, 6.0), |_| ()).into();
    }
}
