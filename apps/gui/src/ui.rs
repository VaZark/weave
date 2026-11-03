//! The view and the layouts. A view only renders state and fires messages; a layout only arranges
//! views and triggers. Navigation state lives in `App`, so switching layout loses nothing
//! (docs/ui/layout.md). There is no sidebar: Home reaches every page, and one header serves both
//! layouts; they differ in the player (a bar, or a mini player and tabs) and in a few details.

use iad::adaptive::adaptive;
use iad::widget::{self as w, typography as ty};
use iad::window::{Chrome, State as WindowState};
use iad::{Icon, style};
use iced::widget::{button, column, container, row, text, text_input};
use iced::{Alignment, Background, Border, Color, Length};
use iced_adaptive_design as iad;

use crate::components::transport::glyph_button;
use crate::{App, Dest, Element, Layout, Message};

const SEARCH_ID: &str = "search";

/// Cards that fit the width: as many columns as fit, each at most `max_width` wide, the cards
/// filling their cells. (iced has this natively: `Grid::fluid`.)
pub fn autofit<'a>(items: Vec<Element<'a>>, max_width: f32) -> Element<'a> {
    iced::widget::grid(items)
        .fluid(max_width)
        .spacing(12)
        .height(iced::widget::grid::Sizing::EvenlyDistribute(Length::Shrink))
        .into()
}

pub fn card<'a>(content: impl Into<Element<'a>>) -> Element<'a> {
    container(content)
        .padding(12)
        .style(style::container::card)
        .into()
}

/// A list row: no surface, no separator; the whole row lights up under the pointer.
pub fn row_style(t: &iad::Theme, s: button::Status) -> button::Style {
    let fg = t.palette.window_fg_color;
    let a = match s {
        button::Status::Hovered => 0.08,
        button::Status::Pressed => 0.14,
        _ => 0.0,
    };
    button::Style {
        background: (a > 0.0).then_some(Background::Color(Color { a, ..fg })),
        text_color: fg,
        border: Border::default().rounded(8),
        shadow: Default::default(),
        snap: false,
    }
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
        if !self.history.is_empty() {
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
                .pack_end(glyph_button(
                    Icon::Compass,
                    16.0,
                    self.dest == Dest::Explore && !self.jam_open && !self.searching,
                    Message::ToggleExplore,
                ))
                .pack_end(
                    w::icon_button(Icon::Search).on_press(Message::SearchOpen(!self.searching)),
                )
                .pack_end(glyph_button(
                    Icon::Sparkle,
                    16.0,
                    self.jam_open,
                    Message::ToggleJam,
                ));
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

    /// Now Playing and the queue slide up over `base`: Now Playing first, the queue over it.
    fn sheets<'a>(&'a self, base: Element<'a>) -> Element<'a> {
        let mut e = base;
        if self.now_playing {
            e = w::Overlay::new(e, self.now_playing_view())
                .docked(w::Side::Bottom, w::Dim::Fill)
                .scrim(false)
                .into();
        }
        if self.queue_open {
            e = w::Overlay::new(e, self.queue_view())
                .docked(w::Side::Bottom, w::Dim::Fill)
                .scrim(false)
                .into();
        }
        e
    }

    // ---- layouts ----------------------------------------------------------------------

    /// The page area. (There are no library pills: Home and Explore link to every list.)
    fn content(&self) -> Element<'_> {
        self.page()
    }

    fn wide(&self) -> Element<'_> {
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
        let body = column![
            self.header(),
            container(self.content()).height(Length::Fill),
            self.mini_player(),
            self.tab_bar(),
        ];
        self.sheets(body.into())
    }
}
