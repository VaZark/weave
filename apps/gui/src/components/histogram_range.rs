//! A histogram over a range slider that edit **one** range: pressing a bar picks that bin's range,
//! dragging a thumb moves the ends, and the bars inside the range are accented.
//!
//! This is iced-adaptive-design's `ComposedComponent` (several parts, one shared state, one
//! `on_change`, one rule) filled with its `BarChart::histogram` and `range_slider`; it replaces
//! the hand-drawn bars this file used to hold. The same component is defined in iad's gallery
//! (`composed_demo.rs`); this one takes the bounds from the histogram instead of 0..=100.

use iced_adaptive_design::Element;
use iced_adaptive_design::widget::{BarChart, ComposedComponent, Histogram, range_slider};

/// `histogram` covers `min..max`; `range` is the chosen `(lo, hi)`; `step` is how far a thumb
/// moves at a time. `on_change` gets the next range from whichever part the user touched.
pub fn histogram_range_slider<'a, M: 'a>(
    histogram: &Histogram,
    range: (f32, f32),
    step: f32,
    on_change: impl Fn((f32, f32)) -> M + 'a,
) -> Element<'a, M> {
    let (min, max) = (histogram.min(), histogram.max());
    let (for_selected, for_pick) = (histogram.clone(), histogram.clone());
    ComposedComponent::new(range, on_change)
        // Part 1: the bars. Those inside the range are accented; pressing one picks its range.
        .part(move |r| {
            BarChart::histogram(&for_selected)
                .height(48.0)
                .selected({
                    let h = for_selected.clone();
                    let r = *r;
                    move |i| h.selected(i, r)
                })
                .on_pick(move |i| for_pick.bin_range(i))
                .minimal()
                .gap(0.07)
                .into()
        })
        // Part 2: the thumbs, for the exact ends.
        .part(move |r| {
            range_slider(min..=max, *r, |moved| moved)
                .step(step)
                .min_gap(step)
                .into()
        })
        // The rule, once, whichever part sent the value: lo <= hi.
        .normalize(|(lo, hi)| (lo.min(hi), lo.max(hi)))
        .spacing(6.0)
        .into()
}
