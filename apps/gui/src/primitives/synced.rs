//! Generic: lines that carry a time, a position that moves through them, and where to scroll.
//! Lyrics are the first use; subtitles, a transcript or a changelog timeline are the same thing.

use iced::widget::scrollable::RelativeOffset;

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// Seconds from the start.
    pub at: f32,
    pub text: String,
}

/// The line being sung at `pos`: the last one that started. `None` before the first.
/// `lines` must be ordered by time.
pub fn current(lines: &[Line], pos: f32) -> Option<usize> {
    lines.partition_point(|l| l.at <= pos).checked_sub(1)
}

/// The scroll offset that keeps line `current` a third of the way down a list of `total` lines
/// (so what follows is visible), as a fraction of the scrollable range.
pub fn scroll_to(current: Option<usize>, total: usize) -> RelativeOffset {
    let y = match current {
        Some(i) if total > 1 => (i as f32 - total as f32 * 0.0).max(0.0) / (total - 1) as f32,
        _ => 0.0,
    };
    RelativeOffset {
        x: 0.0,
        y: y.clamp(0.0, 1.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines() -> Vec<Line> {
        [0.0, 5.0, 9.5]
            .iter()
            .map(|at| Line {
                at: *at,
                text: String::new(),
            })
            .collect()
    }

    #[test]
    fn the_current_line_is_the_last_one_started() {
        let l = lines();
        assert_eq!(current(&l, -1.0), None);
        assert_eq!(current(&l, 0.0), Some(0));
        assert_eq!(current(&l, 4.99), Some(0));
        assert_eq!(current(&l, 5.0), Some(1));
        assert_eq!(current(&l, 100.0), Some(2));
        assert_eq!(current(&[], 3.0), None);
    }

    #[test]
    fn scrolling_follows_the_line() {
        assert_eq!(scroll_to(None, 10).y, 0.0);
        assert_eq!(scroll_to(Some(0), 10).y, 0.0);
        assert_eq!(scroll_to(Some(9), 10).y, 1.0);
        assert_eq!(scroll_to(Some(3), 1).y, 0.0);
    }
}
