//! `MediaHeader`: the head of a detail page (a playlist, an album, an artist), after the usual
//! music-app pattern.
//!
//! * **Wide**: the cover on the left, a small kind label, the large title, the description and
//!   facts beside it, centered against the cover, and an action row under both: the main action first, then the options aligned to the right.
//! * **Narrow** ([`narrow`](MediaHeader::narrow)): the cover and the title centered, then the
//!   description and facts, and one action row with Play on the left and options on the right.
//! * A [wash](crate::styles::container::wash) of [`tint`](MediaHeader::tint) fades down from the
//!   top.
//!
//! The application decides which arrangement from its width (for example
//! `WidthClass::of(width) < WidthClass::Md`).
//!
//! ```ignore
//! MediaHeader::new("Evening drive")
//!     .kind("Playlist")
//!     .cover(cover)
//!     .description(ty::body("Slow songs for a long road."))
//!     .facts(ty::caption("24 songs, 1 hr 32 min"))
//!     .primary(suggested_circular_button(Icon::Play.view(16.0)).on_press(Msg::Play))
//!     .action(menu_button(Icon::ViewMore, menu))
//!     .tint(cover_color)
//!     .narrow(width < 576.0)
//! ```

use iced_adaptive_design::Element;
use iced_adaptive_design::components::typography as ty;

use iced::widget::{column, row, space};
use iced::{Alignment, Color, Length};
use iced_adaptive_design::Metrics;

/// See the [module](self).
pub struct MediaHeader<'a, M> {
    title: String,
    kind: Option<String>,
    cover: Option<Element<'a, M>>,
    description: Option<Element<'a, M>>,
    facts: Option<Element<'a, M>>,
    primary: Option<Element<'a, M>>,
    actions: Vec<Element<'a, M>>,
    tint: Option<Color>,
    narrow: bool,
    metrics: Metrics,
    top_right: Option<Element<'a, M>>,
}

impl<'a, M: 'a> MediaHeader<'a, M> {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            kind: None,
            cover: None,
            description: None,
            facts: None,
            primary: None,
            actions: Vec::new(),
            tint: None,
            narrow: false,
            metrics: Metrics::desktop(),
            top_right: None,
        }
    }
    /// The small label over the title: "Playlist", "Album", "Artist".
    pub fn kind(mut self, kind: impl Into<String>) -> Self {
        self.kind = Some(kind.into());
        self
    }
    /// The artwork.
    pub fn cover(mut self, cover: impl Into<Element<'a, M>>) -> Self {
        self.cover = Some(cover.into());
        self
    }
    pub fn description(mut self, e: impl Into<Element<'a, M>>) -> Self {
        self.description = Some(e.into());
        self
    }
    /// A line of facts under the description: counts, durations, dates.
    pub fn facts(mut self, e: impl Into<Element<'a, M>>) -> Self {
        self.facts = Some(e.into());
        self
    }
    /// The main action (play), always at the far left in both layouts.
    pub fn primary(mut self, e: impl Into<Element<'a, M>>) -> Self {
        self.primary = Some(e.into());
        self
    }
    /// A secondary action, grouped on the right of the control row.
    pub fn action(mut self, e: impl Into<Element<'a, M>>) -> Self {
        self.actions.push(e.into());
        self
    }
    /// The color of the wash behind the header (usually the cover's dominant color). Without
    /// one there is no wash.
    pub fn tint(mut self, color: Color) -> Self {
        self.tint = Some(color);
        self
    }
    /// The narrow arrangement (default wide).
    pub fn narrow(mut self, narrow: bool) -> Self {
        self.narrow = narrow;
        self
    }
    pub fn metrics(mut self, m: Metrics) -> Self {
        self.metrics = m;
        self
    }
    pub fn top_right(mut self, content: impl Into<Element<'a, M>>) -> Self {
        self.top_right = Some(content.into());
        self
    }
}

impl<'a, M: 'a> From<MediaHeader<'a, M>> for Element<'a, M> {
    fn from(h: MediaHeader<'a, M>) -> Self {
        let alignment = if h.narrow {
            Alignment::Center
        } else {
            Alignment::Start
        };
        let mut after = column![]
            .spacing(h.metrics.space_s)
            .width(Length::Fill)
            .align_x(alignment);
        if let Some(kind) = h.kind {
            after = after.push(
                ty::caption(kind)
                    .width(Length::Fill)
                    .align_x(alignment)
                    .style(iced_adaptive_design::styles::text::dim),
            );
        }
        if let Some(facts) = h.facts {
            after = after.push(facts);
        }
        let has_controls = h.primary.is_some() || !h.actions.is_empty();
        let mut controls = row![]
            .spacing(h.metrics.space_m)
            .align_y(Alignment::Center)
            .width(Length::Fill);
        if let Some(primary) = h.primary {
            controls = controls.push(primary);
        }
        controls = controls.push(space::horizontal());
        for action in h.actions {
            controls = controls.push(action);
        }
        let mut header = iced_adaptive_design::widgets::ProfileHeader::new(
            h.title,
            h.cover.unwrap_or_else(|| space().into()),
            h.description.unwrap_or_else(|| space().into()),
        )
        .after_name(after)
        .narrow(h.narrow)
        .metrics(h.metrics);
        if has_controls {
            header = header.controls(controls);
        }
        if let Some(end) = h.top_right {
            header = header.top_right(end);
        }
        if let Some(tint) = h.tint {
            header = header.tint(tint);
        }
        header.into()
    }
}
