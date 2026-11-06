//! The view and the layouts. A view only renders state and fires messages; a layout only arranges
//! views and triggers. Navigation state lives in `App`, so switching layout loses nothing
//! (docs/ui/layout.md). There is no sidebar: Home reaches every page, and one header serves both
//! layouts; they differ in the player (a bar, or a mini player and tabs) and in a few details.

use iad::adaptive::adaptive;
use iad::prelude::{self as w, typography as ty};
use iad::windows::{Chrome, State as WindowState};
use iad::{Icon, styles as style};
use iced::widget::{column, container, row};
use iced::{Alignment, Length};
use iced_adaptive_design as iad;

use crate::{App, DensityPref, Dest, Element, Layout, Message, Sheet};

/// Rough heights of the fixed parts of a layout, to know how far a sliding layer travels.
pub const HEADER_H: f32 = 47.0;
pub const BAR_H: f32 = 65.0;
pub const TAB_H: f32 = 56.0;
pub const MINI_H: f32 = 60.0;

impl App {
    pub fn view(&self) -> Element<'_> {
        let body: Element<'_> = if self.minimal {
            self.minimal_view()
        } else {
            adaptive(&self.adaptive)
                .layout(Layout::Narrow, || self.narrow())
                .layout(Layout::Wide, || self.wide())
                .into()
        };
        let framed = self.chrome(true, true).frame(body, Message::Window);
        // Dialogs are modal and cannot be dismissed from outside: Cancel or Save closes them.
        let jam_filters = self.jam_filters_showing() && !self.is_narrow();
        let saving = self.save_name.is_some() && !self.save_manual && !self.is_narrow();
        let backdrop = if self.manual_filter_review.is_some() {
            Some(Message::CancelManualFilterReview)
        } else if self.import_report.is_some() && !self.is_narrow() {
            Some(Message::DismissImportReport)
        } else if jam_filters {
            Some(Message::JamFilters(false))
        } else if self.playlist_tracks_open && !self.is_narrow() {
            Some(Message::PlaylistTracksOpen(false))
        } else if saving {
            Some(Message::CancelSaveJam)
        } else {
            None
        };
        let dialog = w::present_dialog(framed, self.dialog(), backdrop, 0.0);
        // A line for a moment: what an import did.
        w::toast_stack(
            dialog,
            self.notice
                .iter()
                .map(|(text, _)| w::Toast::new(text.clone())),
        )
    }

    /// Whether a dialog (not a sheet) is showing: for the tests.
    #[cfg(test)]
    pub fn dialog_open_for_test(&self) -> bool {
        self.dialog().is_some()
    }

    /// Whether the jam's filters are open (a dialog wide, a sheet narrow).
    fn jam_filters_showing(&self) -> bool {
        self.jam_open && self.jam_filters_open
    }

    fn dialog(&self) -> Option<Element<'_>> {
        // Rule changes remove curated tracks only after a dialog, at every window width.
        if let Some(review) = &self.manual_filter_review {
            return Some(self.manual_filter_confirmation(review));
        }
        if self.playlist_tracks_open && !self.is_narrow() {
            return Some(
                container(self.playlist_tracks_form())
                    .padding(20)
                    .width(Length::Fixed(560.0))
                    .height(Length::Fixed((self.win_h - 80.0).clamp(200.0, 640.0)))
                    .style(style::container::popover)
                    .into(),
            );
        }
        if self.page_filters_open && !self.is_narrow() {
            return Some(
                container(self.page_filters_view())
                    .padding(20)
                    .width(Length::Fixed(560.0))
                    .height(Length::Fixed((self.win_h - 80.0).clamp(200.0, 640.0)))
                    .style(crate::ui_dialogs::filter_surface)
                    .into(),
            );
        }
        if let Some(report) = &self.import_report
            && !self.is_narrow()
        {
            return Some(
                container(crate::import_picker::report_view(report))
                    .padding(20)
                    .width(Length::Fixed(640.0))
                    .height(Length::Fixed((self.win_h - 80.0).clamp(200.0, 680.0)))
                    .style(style::container::popover)
                    .into(),
            );
        }
        if self.save_name.is_some() && !self.save_manual && !self.is_narrow() {
            return Some(self.save_dialog());
        }
        if self.jam_filters_showing() && !self.is_narrow() {
            return Some(self.jam_filters_dialog());
        }
        if !self.is_narrow()
            && let Some(name) = &self.delete_playlist
        {
            return Some(self.delete_playlist_dialog(name));
        }
        self.draft
            .as_ref()
            .map(|f| self.filter_dialog(f))
            .or_else(|| self.edit_track.as_ref().map(|e| self.track_dialog(e)))
            .or_else(|| self.rate_track.map(|id| self.rate_dialog(id)))
            .or_else(|| self.info_track.map(|id| self.info_dialog(id)))
            // On narrow, Edit details is a bottom sheet (see `narrow`), not a dialog.
            .or_else(|| {
                self.edit
                    .as_ref()
                    .filter(|_| !self.is_narrow())
                    .map(|d| self.details_dialog(d))
            })
    }

    fn chrome(&self, left: bool, right: bool) -> Chrome {
        Chrome::new(WindowState {
            maximized: false,
            fullscreen: self.fullscreen,
        })
        .edges(left, right)
    }

    /// The header line: flat, so the page under it (and its colour wash) runs to the top of the
    /// window. It is shared by every page rather than a bar of its own.
    pub fn bar(&self, left: bool, right: bool) -> w::HeaderBar<'_, Message> {
        w::header_bar()
            .chrome(&self.chrome(left, right), Message::Window)
            .flat(true)
    }

    /// The colour of the page's wash, when it has one: a detail page's (playlist, artist, album)
    /// or the jam's.
    pub fn page_tint(&self) -> Option<iced::Color> {
        use iad::color::{from_index, from_seed};
        if self.jam_open {
            return Some(from_index(7));
        }
        match self.dest {
            Dest::Playlist(i) => self.lib.playlists.get(i).map(|p| from_seed(&p.name)),
            Dest::Artist(i) | Dest::Album(i) => Some(from_index(i)),
            Dest::Tracks => Some(from_seed("All tracks")),
            Dest::Artists => Some(from_seed("All artists")),
            Dest::Albums => Some(from_seed("All albums")),
            Dest::Playlists => Some(from_seed("All playlists")),
            _ => None,
        }
    }

    /// `body` with the page's colour wash behind it, fading down from the top of the window
    /// through the header line and the top of the page.
    fn with_wash<'a>(&'a self, body: Element<'a>) -> Element<'a> {
        match self.page_tint() {
            // The bottom layer fills the window (a stack takes the size of its first layer), with
            // the wash at its top.
            Some(tint) => iced::widget::stack![
                column![
                    container(iced::widget::space())
                        .width(Length::Fill)
                        .height(Length::Fixed(HEADER_H + 300.0))
                        .style(style::container::wash(tint)),
                ]
                .width(Length::Fill)
                .height(Length::Fill),
                body,
            ]
            .into(),
            None => body,
        }
    }

    pub fn is_narrow(&self) -> bool {
        matches!(self.adaptive.shown().last(), Some(Layout::Narrow))
    }

    pub fn page_title(&self) -> String {
        if self.save_manual && self.save_name.is_some() {
            return "Create playlist".into();
        }
        if self.jam_open {
            return "Jam".into();
        }
        match self.dest {
            Dest::Home => "Home".into(),
            Dest::Explore => "Explore".into(),
            Dest::Rediscover => "Rediscover".into(),
            Dest::AboutMe => "About me".into(),
            Dest::Recent => "Recent tracks".into(),
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
        if self.save_manual && self.save_name.is_some() {
            h = h.pack_start(w::icon_button(Icon::GoPrevious).on_press(Message::CancelSaveJam));
        } else if self.history.can_back() {
            h = h.pack_start(w::icon_button(Icon::GoPrevious).on_press(Message::Back));
        }
        h = h.title(self.page_title());
        // The later a button is packed, the further from the window controls it sits.
        h = h.pack_end(self.main_menu());
        if !narrow {
            // Explore and Jam are tabs on narrow, so only desktop has them here.
            h = h
                .pack_end(
                    w::IconToggleButton::new(
                        Icon::Compass,
                        self.dest == Dest::Explore && !self.jam_open,
                    )
                    .size(16.0)
                    .on_toggle(move |_| Message::ToggleExplore),
                )
                .pack_end(
                    w::IconToggleButton::new(Icon::Sparkle, self.jam_open)
                        .size(16.0)
                        .on_toggle(move |_| Message::ToggleJam),
                );
        }
        if self.can_search_page() {
            h = h.pack_end(
                w::IconToggleButton::new(Icon::Search, self.search_open)
                    .tooltip("Search this page")
                    .on_toggle(Message::SearchOpen),
            );
        }
        h.into()
    }

    /// The main menu: the density switch and the unified file/folder import action.
    fn main_menu(&self) -> Element<'_> {
        if self.is_narrow() {
            return w::icon_button(Icon::OpenMenu)
                .on_press(Message::OpenSheet(Some(Sheet::Main)))
                .into();
        }
        self.menu_button(Icon::OpenMenu, "Main menu", self.main_menu_def(|m| m))
    }

    /// Share the contents and checks between the desktop popover and narrow bottom sheet.
    pub fn main_menu_def(&self, wrap: impl Fn(Message) -> Message) -> w::Menu<Message> {
        let compact = self.compact();
        w::Menu::new("Menu")
            .check_style(w::MenuCheckStyle::Mark)
            .section([w::MenuItem::check(
                "Compact",
                compact,
                wrap(Message::SetDensity(if compact {
                    DensityPref::Default
                } else {
                    DensityPref::Compact
                })),
            )])
            .section([
                w::MenuItem::action(
                    "Import files…",
                    wrap(Message::Import(crate::import_picker::Mode::Files)),
                )
                .icon(Icon::Document),
                w::MenuItem::action(
                    "Import folder…",
                    wrap(Message::Import(crate::import_picker::Mode::Folder)),
                )
                .icon(Icon::Folder),
            ])
    }

    // ---- sheets -----------------------------------------------------------------------

    /// Now Playing is a bottom sheet over `base`: iad's `Dock` (full size, overlay) shows and hides
    /// it with its slide, in both directions.
    fn np_dock<'a>(&'a self, base: Element<'a>) -> Element<'a> {
        w::Dock::new(base, self.now_playing_view())
            .open(self.now_playing)
            .size(w::DockSize::FullScreen)
            .mode(w::DockMode::Overlay)
            .scrim(false)
            .into()
    }

    // ---- layouts ----------------------------------------------------------------------

    /// The page area. (There are no library pills: Home and Explore link to every list.)
    fn content(&self) -> Element<'_> {
        if self.save_manual && self.save_name.is_some() {
            self.create_playlist_page()
        } else {
            self.page()
        }
    }

    /// The queue opening or closing: the same final layout, only animated. The now-playing bar
    /// rises from the bottom to the top with the page pushed up ahead of it, and the space it
    /// leaves below becomes the queue; closing runs it backwards. Nothing about the layouts at
    /// either end changes. Both halves are iad `ClipWindow`s: the content is translated, never
    /// squeezed.
    fn queue_stage(&self) -> Element<'_> {
        let narrow = self.is_narrow();
        let e = self.queue_slide.shown();
        let bar_h = if narrow { MINI_H } else { BAR_H };
        // The page area: what is left of the window under the header (and above the tabs) after
        // the bar.
        let p = (self.win_h - 2.0 - HEADER_H - if narrow { TAB_H } else { 0.0 } - bar_h).max(120.0);
        // Above the bar: the page, pinned to the bottom of a shrinking window, so the rising bar
        // pushes it up and out.
        let above = w::ClipWindow::new(container(self.content()).height(Length::Fixed(p)))
            .height((1.0 - e) * p)
            .anchor(w::ClipAnchor::Bottom);
        // Below the bar: the queue, revealed from the bar down.
        let below = w::ClipWindow::new(container(self.queue_body()).height(Length::Fixed(p)))
            .height(e * p)
            .anchor(w::ClipAnchor::Top);
        column![above, self.now_bar(), below].into()
    }

    /// The page area when the queue is (or is becoming) the page.
    fn queue_area(&self) -> Element<'_> {
        if self.queue_slide.shown() >= 1.0 {
            container(self.queue_page()).height(Length::Fill).into()
        } else {
            self.queue_stage()
        }
    }

    fn wide(&self) -> Element<'_> {
        if self.queue_slide.mounted() {
            return column![self.header(), self.queue_area()].into();
        }
        // Now Playing takes over the whole window: the header and the bar go under it.
        self.np_dock(
            self.with_wash(
                column![
                    self.header(),
                    container(self.content()).height(Length::Fill),
                    self.player_bar(),
                ]
                .into(),
            ),
        )
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
        let explore = !jam && self.dest == Dest::Explore;
        let home = !jam && !explore;
        container(
            row![
                self.tab(
                    Icon::Home.view(16.0).into(),
                    "Home",
                    home,
                    Message::Go(Dest::Home)
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
            .padding(4), // gutter-ok: a component's own padding, not a page gutter
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
        let base = self.np_dock(self.with_wash(body.into()));
        let base = match self.sheet {
            Some(sheet) => w::Dock::new(base, self.sheet_view(sheet))
                .scope(w::DockScope::Window)
                .mode(w::DockMode::Overlay)
                .on_outside(Message::OpenSheet(None))
                .into(),
            None => base,
        };
        // The jam filters use a bounded bottom sheet over the page.
        let base = if self.jam_filters_showing() {
            w::Dock::new(base, self.jam_filters_sheet())
                .scope(w::DockScope::Window)
                .size(w::DockSize::Fraction(0.8))
                .mode(w::DockMode::Overlay)
                .on_outside(Message::JamFilters(false))
                .into()
        } else {
            base
        };
        // Delete playlist: a bottom sheet over everything.
        let base = match &self.delete_playlist {
            Some(name) => w::Dock::new(base, self.delete_playlist_sheet(name))
                .scope(w::DockScope::Window)
                .mode(w::DockMode::Overlay)
                .on_outside(Message::CancelDeletePlaylist)
                .into(),
            None => base,
        };
        let base = if self.playlist_tracks_open {
            w::Dock::new(base, container(self.playlist_tracks_form()).padding(20))
                .scope(w::DockScope::Window)
                .size(w::DockSize::Fraction(0.8))
                .mode(w::DockMode::Overlay)
                .on_outside(Message::PlaylistTracksOpen(false))
                .into()
        } else {
            base
        };
        // Save as playlist: a bottom sheet over everything.
        let base = if self.save_name.is_some() && !self.save_manual {
            w::Dock::new(base, self.save_sheet())
                .scope(w::DockScope::Window)
                .mode(w::DockMode::Overlay)
                .on_outside(Message::CancelSaveJam)
                .into()
        } else {
            base
        };
        // Edit details: a bottom sheet over everything, with the page behind it.
        let base = match &self.edit {
            Some(draft) => w::Dock::new(base, self.details_sheet(draft))
                .scope(w::DockScope::Window)
                .mode(w::DockMode::Overlay)
                .on_outside(Message::CancelDetails)
                .into(),
            None => base,
        };
        let base = if self.page_filters_open {
            w::Dock::new(
                base,
                container(self.page_filters_view())
                    .padding(20)
                    .height(Length::Fill)
                    .style(style::container::window),
            )
            .scope(w::DockScope::Window)
            .size(w::DockSize::Fraction(0.8))
            .mode(w::DockMode::Overlay)
            .on_outside(Message::PageFilters(false))
            .into()
        } else {
            base
        };
        match &self.import_report {
            Some(report) => w::Dock::new(
                base,
                container(crate::import_picker::report_view(report))
                    .padding(20)
                    .height(Length::Fill)
                    .style(style::container::window),
            )
            .scope(w::DockScope::Window)
            .size(w::DockSize::Fraction(0.8))
            .mode(w::DockMode::Overlay)
            .on_outside(Message::DismissImportReport)
            .into(),
            None => base,
        }
    }
}
