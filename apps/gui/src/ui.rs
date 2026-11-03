//! The view and the layouts. A view only renders state and fires messages; a layout only arranges
//! views and triggers. Navigation state lives in `App`, so switching layout loses nothing
//! (docs/ui/layout.md). There is no sidebar: Home reaches every page, and one header serves both
//! layouts; they differ in the player (a bar, or a mini player and tabs) and in a few details.

use iad::adaptive::adaptive;
use iad::widget::{self as w, typography as ty};
use iad::window::{Chrome, State as WindowState};
use iad::{Icon, style};
use iced::widget::{column, container, mouse_area, row, space, stack, text, text_input};
use iced::{Alignment, Length};
use iced_adaptive_design as iad;

use crate::{App, Dest, Element, Layout, Message};

const SEARCH_ID: &str = "search";

/// Rough heights of the fixed parts of a layout, to know how far a sliding layer travels.
pub const HEADER_H: f32 = 47.0;
pub const BAR_H: f32 = 65.0;
pub const TAB_H: f32 = 56.0;
pub const MINI_H: f32 = 60.0;

/// The one place the page gutter exists. A page is built only from these: nothing else in the app
/// may hard-code a gutter (a test fails if a page file does), so every page, in either density,
/// keeps the same distance from the window edge. The base is iad's `Container`.
pub(crate) mod surface {
    use super::*;

    /// The gutter: headings, cards and the edges of lists keep this far from the window edge.
    const GUTTER: f32 = 16.0;
    /// Breathing room above and below a toolbar band.
    const BAND: f32 = 8.0;

    fn sides() -> iced::Padding {
        iced::Padding {
            top: 0.0,
            bottom: 0.0,
            left: GUTTER,
            right: GUTTER,
        }
    }

    fn base<'a>(content: impl Into<Element<'a>>, padding: iced::Padding) -> Element<'a> {
        w::Container::new(content)
            .fluid(true)
            .padding(padding)
            .into()
    }

    /// A page that scrolls. The surface is inside the scrollable, so the scroll bar stays at the
    /// window edge while the content keeps the gutter on every side.
    pub fn scrolling<'a>(content: impl Into<Element<'a>>) -> Element<'a> {
        iced::widget::scrollable(base(content, iced::Padding::from(GUTTER)))
            .height(Length::Fill)
            .into()
    }

    /// A page that does not scroll as a whole (a jam, a sheet's body): the gutter on every side.
    pub fn full<'a>(content: impl Into<Element<'a>>) -> Element<'a> {
        container(base(content, iced::Padding::from(GUTTER)))
            .height(Length::Fill)
            .into()
    }

    /// A band inside a page (a section, a heading, a list header): the side gutter only.
    pub fn band<'a>(content: impl Into<Element<'a>>) -> Element<'a> {
        base(content, sides())
    }

    /// A toolbar band (the play row, a search entry): the side gutter and a little air.
    pub fn toolbar<'a>(content: impl Into<Element<'a>>) -> Element<'a> {
        base(
            content,
            iced::Padding {
                top: BAND,
                bottom: BAND,
                ..sides()
            },
        )
    }

    /// A row of a flat list: inset so its hover surface starts on the gutter.
    pub fn row<'a>(content: impl Into<Element<'a>>) -> Element<'a> {
        container(base(content, sides()))
            .height(Length::Fill)
            .into()
    }

    /// The padding of a bar (the player bar): the gutter at the sides.
    pub fn bar_padding() -> iced::Padding {
        iced::Padding {
            top: BAND,
            bottom: BAND,
            ..sides()
        }
    }
}

/// Shows `content` through a window of `height`, translating it instead of squeezing it: a
/// scrollable that does not scroll gives its content unbounded room. `from_bottom` pins the
/// content's bottom to the window's bottom (it is pushed up as the window shrinks).
fn clip<'a>(content: Element<'a>, height: f32, from_bottom: bool) -> Element<'a> {
    let s = iced::widget::scrollable(content)
        .direction(iced::widget::scrollable::Direction::Vertical(
            iced::widget::scrollable::Scrollbar::hidden(),
        ))
        .width(Length::Fill)
        .height(Length::Fixed(height));
    if from_bottom {
        s.anchor_bottom().into()
    } else {
        s.into()
    }
}

pub fn card<'a>(content: impl Into<Element<'a>>) -> Element<'a> {
    container(content)
        .padding(12)
        .style(style::container::card)
        .into()
}

impl App {
    pub fn view(&self) -> Element<'_> {
        let body = adaptive(&self.adaptive)
            .layout(Layout::Narrow, || self.narrow())
            .layout(Layout::Wide, || self.wide());
        let framed = self.chrome(true, true).frame(body, Message::Window);
        // Dialogs are modal and cannot be dismissed from outside: Cancel or Save closes them.
        // The settings dialog closes with its button or a click outside; the editing dialogs only
        // with their own buttons.
        let dismiss = self.settings_open.then_some(Message::CloseSettings);
        w::present_dialog(framed, self.dialog(), dismiss, 0.0)
    }

    fn dialog(&self) -> Option<Element<'_>> {
        self.draft
            .as_ref()
            .map(|f| self.filter_dialog(f))
            .or_else(|| self.edit_track.as_ref().map(|e| self.track_dialog(e)))
            .or_else(|| self.bulk.as_ref().map(|b| self.bulk_dialog(b)))
            .or_else(|| self.rate_track.map(|id| self.rate_dialog(id)))
            .or_else(|| self.info_track.map(|id| self.info_dialog(id)))
            .or_else(|| self.settings_open.then(|| self.settings_dialog()))
    }

    fn chrome(&self, left: bool, right: bool) -> Chrome {
        Chrome::new(WindowState {
            maximized: false,
            fullscreen: false,
        })
        .edges(left, right)
    }

    pub fn bar(&self, left: bool, right: bool) -> w::HeaderBar<'_, Message> {
        w::header_bar().chrome(&self.chrome(left, right), Message::Window)
    }

    pub fn is_narrow(&self) -> bool {
        matches!(self.adaptive.shown().last(), Some(Layout::Narrow))
    }

    pub fn page_title(&self) -> String {
        if self.jam_open {
            return "Jam".into();
        }
        if self.searching {
            return "Search".into();
        }
        match self.dest {
            Dest::Home => "Home".into(),
            Dest::Explore => "Explore".into(),
            Dest::Recent => "Recently played".into(),
            Dest::Tracks => "Tracks".into(),
            Dest::Artists => "Artists".into(),
            Dest::Albums => "Albums".into(),
            Dest::Playlists => "Playlists".into(),
            Dest::Playlist(i) => self.lib.playlists[i].name.clone(),
            Dest::Artist(i) => self.lib.artists()[i].0.clone(),
            Dest::Album(i) => self.lib.albums()[i].0.clone(),
        }
    }

    // ---- header -----------------------------------------------------------------------

    /// One header for both layouts: Back, the title (or the search entry), and the global actions.
    fn header(&self) -> Element<'_> {
        let narrow = self.is_narrow();
        let mut h = self.bar(true, true);
        if self.history.can_back() {
            h = h.pack_start(w::icon_button(Icon::GoPrevious).on_press(Message::Back));
        }
        h = if self.searching && !narrow {
            h.pack_start(w::icon_button(Icon::WindowClose).on_press(Message::SearchOpen(false)))
                .title_widget(container(self.search_entry()).max_width(560))
        } else {
            h.title(self.page_title())
        };
        // The later a button is packed, the further from the window controls it sits.
        h = h.pack_end(w::icon_button(Icon::Settings).on_press(Message::OpenSettings));
        if !narrow {
            // Explore and Jam are tabs on narrow, so only desktop has them here.
            h = h
                .pack_end(
                    w::IconToggleButton::new(
                        Icon::Compass,
                        self.dest == Dest::Explore && !self.jam_open && !self.searching,
                    )
                    .size(16.0)
                    .on_toggle(move |_| Message::ToggleExplore),
                )
                .pack_end(
                    w::icon_button(Icon::Search).on_press(Message::SearchOpen(!self.searching)),
                )
                .pack_end(
                    w::IconToggleButton::new(Icon::Sparkle, self.jam_open)
                        .size(16.0)
                        .on_toggle(move |_| Message::ToggleJam),
                );
        }
        h.into()
    }

    fn filter_badge(&self) -> Element<'_> {
        let n = self.filter.count();
        let mut r = row![Icon::Filter.view(16.0), text("Filters")]
            .spacing(6)
            .align_y(Alignment::Center);
        if n > 0 {
            r = r.push(
                w::Badge::count(n as u32, 99)
                    .role(iad::Role::Accent)
                    .solid(true),
            );
        }
        r.into()
    }

    /// The filter button: the filter page in a popover, applying live as it is edited.
    fn filter_button(&self) -> Element<'_> {
        // The popover supplies the placement; the surface (background, border, padding) is ours.
        let surface = container(self.filter_fields(
            &self.filter,
            Message::ToggleTag,
            Message::Bpm,
            Some(Message::ClearFilter),
        ))
        .padding(12)
        .style(style::container::popover);
        w::Popover::new(self.filter_badge(), surface)
            .popover_width(420.0)
            .open_initially(std::env::var("WEAVE_GUI_START").as_deref() == Ok("popover"))
            .into()
    }

    /// The search field takes focus as it appears: a focus scope that does not trap Tab, so the
    /// header's other controls stay reachable.
    pub fn search_entry(&self) -> Element<'_> {
        w::FocusScope::new(
            row![
                text_input("Search tracks, artists, albums", &self.query)
                    .id(SEARCH_ID)
                    .on_input(Message::SearchText)
                    .width(Length::Fill),
                self.filter_button(),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .trap(false)
        .initial_focus(SEARCH_ID)
        .into()
    }

    // ---- sheets -----------------------------------------------------------------------

    /// The height of the page area (or, on narrow, the whole body): how far Now Playing travels.
    pub fn region_h(&self) -> f32 {
        let h = (self.win_h - 2.0).max(200.0);
        if self.is_narrow() {
            h
        } else {
            (h - HEADER_H - BAR_H).max(200.0)
        }
    }

    /// Now Playing slides up over `base` and slides back down when it closes: it stays mounted
    /// while it moves. (The queue is not a sheet: it is page content that slides in.)
    fn sheets<'a>(&'a self, base: Element<'a>) -> Element<'a> {
        if !self.np_slide.mounted() {
            return base;
        }
        let e = self.np_slide.eased();
        let layer: Element<'a> = if e >= 0.999 {
            self.now_playing_view()
        } else {
            let h = self.region_h();
            container(column![
                space::vertical().height(Length::Fixed((1.0 - e) * h)),
                container(self.now_playing_view()).height(Length::Fixed(h)),
            ])
            .width(Length::Fill)
            .height(Length::Fill)
            .clip(true)
            .into()
        };
        // The layer sits over the page: it swallows presses so they do not reach what is below.
        stack![base, mouse_area(layer).on_press(Message::Noop)].into()
    }

    // ---- layouts ----------------------------------------------------------------------

    /// The page area. (There are no library pills: Home and Explore link to every list.)
    fn content(&self) -> Element<'_> {
        self.page()
    }

    /// The queue opening or closing: the same final layout, only animated. The now-playing bar
    /// rises from the bottom to the top with the page (or Now Playing, if it is open) pushed up
    /// ahead of it, and the space it leaves below becomes the queue; closing runs it backwards.
    /// Nothing about the layouts at either end changes.
    fn queue_stage(&self) -> Element<'_> {
        let narrow = self.is_narrow();
        let e = self.queue_slide.eased();
        let bar_h = if narrow { MINI_H } else { BAR_H };
        // The page area: what is left of the window under the header (and above the tabs) after
        // the bar.
        let p = (self.win_h - 2.0 - HEADER_H - if narrow { TAB_H } else { 0.0 } - bar_h).max(120.0);
        // Above the bar: Now Playing if it is open, else the page, pinned to the bottom of a
        // shrinking clip, so the rising bar pushes it up and out.
        let top: Element<'_> = if self.np_slide.mounted() {
            container(self.now_playing_view())
                .height(Length::Fixed(p))
                .into()
        } else {
            container(self.content()).height(Length::Fixed(p)).into()
        };
        let above = clip(top, (1.0 - e) * p, true);
        // Below the bar: the queue, revealed from the bar down.
        let below = clip(
            container(self.queue_body()).height(Length::Fixed(p)).into(),
            e * p,
            false,
        );
        column![above, self.now_bar(), below].into()
    }

    /// The page area when the queue is (or is becoming) the page.
    fn queue_area(&self) -> Element<'_> {
        if self.queue_slide.shown >= 1.0 {
            container(self.queue_page()).height(Length::Fill).into()
        } else {
            self.queue_stage()
        }
    }

    fn wide(&self) -> Element<'_> {
        if self.queue_slide.mounted() {
            return column![self.header(), self.queue_area()].into();
        }
        column![
            self.header(),
            container(self.sheets(self.content())).height(Length::Fill),
            self.player_bar(),
        ]
        .into()
    }

    fn tab(
        &self,
        glyph: Element<'static>,
        label: &'static str,
        active: bool,
        m: Message,
    ) -> Element<'_> {
        let label = if active {
            ty::heading(label)
        } else {
            ty::caption(label)
        };
        w::flat_button(
            column![glyph, label]
                .spacing(2)
                .align_x(Alignment::Center)
                .width(Length::Fill),
        )
        .on_press(m)
        .width(Length::Fill)
        .into()
    }

    fn tab_bar(&self) -> Element<'_> {
        let jam = self.jam_open;
        let search = self.searching && !jam;
        let explore = !jam && !self.searching && self.dest == Dest::Explore;
        let home = !jam && !self.searching && !explore;
        container(
            row![
                self.tab(
                    Icon::Home.view(16.0).into(),
                    "Home",
                    home,
                    Message::Go(Dest::Home)
                ),
                self.tab(
                    Icon::Search.view(16.0).into(),
                    "Search",
                    search,
                    Message::SearchOpen(!self.searching),
                ),
                self.tab(
                    Icon::Compass.view(16.0).into(),
                    "Explore",
                    explore,
                    Message::Go(Dest::Explore),
                ),
                self.tab(
                    Icon::Sparkle.view(16.0).into(),
                    "Jam",
                    jam,
                    Message::ToggleJam
                ),
            ]
            .padding(4),
        )
        .style(style::container::headerbar)
        .into()
    }

    fn narrow(&self) -> Element<'_> {
        // With the queue open it takes the page, and the mini player moves to the top of it.
        let body = if self.queue_slide.mounted() {
            column![self.header(), self.queue_area(), self.tab_bar()]
        } else {
            column![
                self.header(),
                container(self.content()).height(Length::Fill),
                self.mini_player(),
                self.tab_bar(),
            ]
        };
        if self.queue_slide.mounted() {
            // The queue's own animation carries Now Playing with it.
            return body.into();
        }
        self.sheets(body.into())
    }
}
