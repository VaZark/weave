//! Track lists (flat rows, one setting for compact), the library pages, the detail pages
//! (playlist, artist, album) and Home.

use iad::Icon;
use iad::prelude::{self as w, Menu, MenuItem, VirtualList, typography as ty};
use iced::alignment::Horizontal;
use iced::widget::{button, column, container, row, scrollable, space};
use iced::{Alignment, Length};
use iced_adaptive_design as iad;

use crate::components::media_list::MediaListLayout;
use crate::data::{Filter, RecentOrder, Track};
use crate::icons::{cover, playlist_avatar};
use crate::{App, Col, Dest, Element, Message, Sheet};
use iad::prelude::surface;

/// The album column of a wide track list; the title takes what the others leave.
const ALBUM_WIDTH: f32 = 240.0;
const TIME_WIDTH: f32 = 56.0;

/// What a track row shows besides the title and its artist subtitle and the duration.
#[derive(Clone, Copy)]
pub struct RowOpts {
    /// The album column and the "View album" entry: not on an album's own page.
    pub album: bool,
    pub bpm: bool,
    pub menu: bool,
}

fn stars(n: f32) -> String {
    let full = n.round().clamp(0.0, 5.0) as usize;
    "★".repeat(full) + &"☆".repeat(5 - full)
}

/// A section with a "View all" link to the full list.
fn section_all<'a>(title: &'a str, all: Dest, body: impl Into<Element<'a>>) -> Element<'a> {
    column![
        w::SectionHeading::new(title).link("View all", Message::Open(all)),
        body.into()
    ]
    .spacing(8)
    .width(Length::Fill)
    .into()
}

/// `body`, or when there is nothing to show a dimmed line saying what will appear here and how.
fn or_note<'a>(empty: bool, note: &'static str, body: impl Into<Element<'a>>) -> Element<'a> {
    if empty {
        container(ty::caption(note).style(iad::styles::text::dim))
            .padding([8, 4])
            .into()
    } else {
        body.into()
    }
}

/// The most characters of a playlist's name its tile shows: about two lines.
const PLAYLIST_NAME_MAX: usize = 25;

/// `text` cut to `max` characters, with `…` where it was cut.
fn limit_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let kept: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}

impl App {
    /// A desktop dropdown. Narrow callers open a shared bottom sheet instead.
    /// Icons and checks sit at the end of each item (iad's default).
    pub fn menu_button<'a>(
        &self,
        icon: Icon,
        tip: &'static str,
        menu: Menu<Message>,
    ) -> Element<'a> {
        w::menu_button(icon, menu).tooltip(tip).into()
    }

    /// What a track's menu offers, in sections: icon, label, message. One list for the dropdown
    /// (desktop) and the bottom sheet (narrow). `album` is false on an album's own page, where
    /// "View album" is moot.
    fn track_actions(
        id: usize,
        album: bool,
        favourite: bool,
    ) -> Vec<Vec<(iad::IconSource, &'static str, Message)>> {
        let mut views = vec![];
        if album {
            views.push((
                crate::icons::library(),
                "View album",
                Message::ViewAlbum(id),
            ));
        }
        views.push((
            Icon::MusicNote.into(),
            "View artist",
            Message::ViewArtist(id),
        ));
        vec![
            vec![
                (Icon::GoNext.into(), "Play next", Message::PlayNext(id)),
                (
                    Icon::ListAdd.into(),
                    "Add to queue",
                    Message::AddToQueue(id),
                ),
            ],
            vec![
                (
                    Icon::Heart.into(),
                    if favourite {
                        "Remove from favourites"
                    } else {
                        "Add to favourites"
                    },
                    Message::ToggleFavoriteTrack(id),
                ),
                (Icon::Star.into(), "Rate", Message::RateTrack(id)),
                (Icon::Edit.into(), "Edit tags", Message::EditTrack(id)),
                (
                    Icon::DialogInformation.into(),
                    "More info",
                    Message::InfoTrack(id),
                ),
            ],
            views,
        ]
    }

    /// The track's actions as an iad `Menu`, each message passed through `wrap` (the sheet closes
    /// on a choice, the dropdown does not need to).
    fn track_menu_def(
        &self,
        title: String,
        id: usize,
        album: bool,
        favourite: bool,
        wrap: impl Fn(Message) -> Message,
    ) -> Menu<Message> {
        let mut menu = Menu::new(title);
        for section in Self::track_actions(id, album, favourite) {
            menu = menu.section(
                section
                    .into_iter()
                    .map(|(icon, label, m)| MenuItem::action(label, wrap(m)).icon(icon)),
            );
        }
        if self.playlist_edit.is_none()
            && self
                .manual_playlist()
                .is_some_and(|p| p.tracks.contains(&id))
        {
            menu = menu.section([MenuItem::action(
                "Remove from playlist",
                wrap(Message::PlaylistTrackToggle(id)),
            )
            .icon(Icon::Delete)]);
        }
        menu
    }

    /// The menu of a track: a dropdown on desktop, a button that opens the bottom sheet on narrow.
    fn track_menu<'a>(&self, id: usize, album: bool) -> Element<'a> {
        if self.is_narrow() {
            return w::icon_button(Icon::ViewMore)
                .on_press(Message::OpenSheet(Some(Sheet::Track(id, album))))
                .into();
        }
        self.menu_button(
            Icon::ViewMore,
            "Track options",
            self.track_menu_def("Track".into(), id, album, self.is_favourite(id), |m| m),
        )
    }

    /// The track menu as a bottom sheet: iad's `Menu::sheet`, closing on a choice.
    pub fn track_sheet(&self, id: usize, album: bool) -> Element<'_> {
        let t = self.track(id);
        let menu = self.track_menu_def(t.title.clone(), id, album, self.is_favourite(id), |m| {
            Message::SheetAction(Box::new(m))
        });
        Menu::sheet(&menu, Some(t.artist.clone()))
    }

    // ---- rows and lists -----------------------------------------------------------------

    /// The columns of a track list, written once for the header and the rows: the picture (not
    /// in compact mode), the title with its artist, the album where there is room, the tempo
    /// where asked for, the time, and the menu.
    /// The album column: on desktop only; narrow and compact lists leave it out.
    fn show_album(&self) -> bool {
        !self.is_narrow() && !self.compact()
    }

    fn media_columns(&self, o: RowOpts) -> MediaListLayout<Col> {
        let mut c = MediaListLayout::new(self.compact()).title(Col::Title, "Title");
        if o.album && self.show_album() {
            c = c.column(
                Col::Album,
                "Album",
                Length::Fixed(ALBUM_WIDTH),
                Horizontal::Left,
            );
        }
        if o.bpm {
            c = c.plain_column("BPM", Length::Fixed(40.0), Horizontal::Right);
        }
        c.column(
            Col::Duration,
            "Time",
            Length::Fixed(TIME_WIDTH),
            Horizontal::Right,
        )
        .menu(o.menu)
    }

    pub fn track_row<'a>(&'a self, t: &Track, o: RowOpts, on_press: Message) -> Element<'a> {
        let mut r = self
            .media_columns(o)
            .row(t.title.clone())
            .subtitle(t.artist.clone())
            .height(Length::Fill)
            .on_press(on_press);
        if !self.compact() {
            r = r.picture(cover(t.id, 40.0));
        }
        if o.album && self.show_album() {
            r = r.cell(ty::caption(t.album.clone()));
        }
        if o.bpm {
            r = r.cell(ty::caption(format!("{:.0}", t.bpm)));
        }
        r = r.cell(ty::caption(t.duration()));
        if o.menu {
            // "View album" is for a track that has an album.
            r = r.menu(self.track_menu(t.id, o.album && !t.album.is_empty()));
        }
        r.into()
    }

    /// Every row, in the page's own scroll (for pages whose lists are short).
    pub fn plain_list<'a>(&'a self, tracks: &[Track], o: RowOpts) -> Element<'a> {
        let h = self.row_h();
        column(tracks.iter().map(|t| {
            container(self.track_row(t, o, Message::PlayTrack(t.id)))
                .height(Length::Fixed(h))
                .into()
        }))
        .into()
    }

    /// The header of a long list: sortable Title, Album and Time over the rows' columns.
    fn sort_header(&self, o: RowOpts) -> Element<'_> {
        self.media_columns(o).header(&self.sort, Message::Sort)
    }

    /// The page's tracks as a virtualised list: only the rows in view are built.
    pub fn virtual_tracks(&self, o: RowOpts) -> Element<'_> {
        let mut list = column![];
        if !self.jam_open {
            list = list.push(surface::band(self.sort_header(o)));
        }
        list.push(
            VirtualList::new(self.rows.len(), self.row_h(), &self.list_vp, move |i| {
                let t = &self.rows[i];
                surface::row(self.track_row(t, o, Message::PlayTrack(t.id)))
            })
            .on_scroll(Message::ListScroll)
            .overscan(4),
        )
        .height(Length::Fill)
        .into()
    }

    fn facts(&self, extra: Option<String>) -> Element<'_> {
        let mins = self.rows.iter().map(|t| t.secs).sum::<u32>() / 60;
        let mut line = format!("{} tracks · {mins} min", self.rows.len());
        if let Some(e) = extra {
            line.push_str(" · ");
            line.push_str(&e);
        }
        ty::caption(line).into()
    }

    pub(super) fn play_all(&self) -> Element<'_> {
        let ids: Vec<usize> = self.rows.iter().map(|t| t.id).collect();
        let available = !ids.is_empty() && self.playlist_edit.is_none();
        w::suggested_circular_button(self.action_icon(Icon::Play, 20.0, available))
            .size(44.0)
            .on_press_maybe(available.then_some(Message::PlayAll(ids)))
            .into()
    }

    /// Tracks and Recent tracks: the play button, then the long list.
    fn track_page(&self) -> Element<'_> {
        let recent = self.dest == Dest::Recent;
        if !recent {
            let body = if let Some(error) = &self.query_error {
                surface::band(ty::body(format!("Cannot search: {error}")))
            } else if self.rows.is_empty() {
                surface::band(ty::body(if self.collection_query.is_empty() {
                    "Nothing here yet"
                } else {
                    "No matching results"
                }))
            } else {
                self.virtual_tracks(RowOpts {
                    album: true,
                    bpm: false,
                    menu: true,
                })
            };
            return column![self.collection_header("All tracks", self.rows.len()), body]
                .height(Length::Fill)
                .into();
        }
        // Nothing played yet: "recently played" shows what was added last, and says so.
        let note = if recent
            && self.recent_order == RecentOrder::Played
            && self.lib.recently_played(1).is_empty()
        {
            format!(
                "{} tracks · nothing played yet, showing recently added",
                self.rows.len()
            )
        } else {
            format!("{} tracks", self.rows.len())
        };
        let mut bar = row![self.play_all(), ty::caption(note)]
            .spacing(12)
            .align_y(Alignment::Center);
        if recent {
            bar = bar.push(space::horizontal()).push(self.recent_menu());
        }
        column![
            self.collection_toolbar(
                if recent {
                    "Recent tracks"
                } else {
                    "All tracks"
                },
                self.rows.len()
            ),
            if self.search_open {
                space::vertical().height(0).into()
            } else {
                surface::toolbar(bar)
            },
            if self.rows.is_empty() {
                surface::band(ty::body(if self.collection_query.is_empty() {
                    "Nothing here yet"
                } else {
                    "No matching results"
                }))
            } else {
                self.virtual_tracks(RowOpts {
                    album: true,
                    bpm: false,
                    menu: true,
                })
            },
        ]
        .height(Length::Fill)
        .into()
    }

    // ---- grids ----------------------------------------------------------------------------

    /// Cards that fit the width, or (in compact mode) the same entries as a tight list: iad's
    /// `FlowGrid`, with the density decided once.
    pub(super) fn grid<'a>(&self, items: Vec<Element<'a>>, card_width: f32) -> Element<'a> {
        // Compact shrinks the cards into tight rows but they still flow as a grid, never as one
        // vertical column (`FlowGrid::cols(1)` would be that).
        w::FlowGrid::new(items)
            .max_width(if self.compact() { 220.0 } else { card_width })
            .spacing(if self.compact() { 8.0 } else { 12.0 })
            .into()
    }

    /// An album, an artist or a playlist: a card, or a compact row (the picture shrinks with it).
    fn media_card<'a>(
        &self,
        title: String,
        subtitle: String,
        picture: impl Fn(f32) -> Element<'a>,
        card_picture: f32,
        raised: bool,
        on_press: Message,
    ) -> Element<'a> {
        let compact = self.compact();
        if !compact {
            // The picture
            // at the left and the title beside it, centered on it, the name cut at
            // `PLAYLIST_NAME_MAX` characters (about two lines). (The kind and the
            // count are left out; compact rows still show them.) Text wraps at words
            // (`heading` alone never wraps).
            let wrap = iced::widget::text::Wrapping::WordOrGlyph;
            let body = row![
                picture(card_picture),
                ty::heading(limit_chars(&title, PLAYLIST_NAME_MAX))
                    .wrapping(wrap)
                    .width(Length::Fill)
            ]
            .spacing(12)
            .align_y(Alignment::Center);
            let surface = container(body)
                .padding(12) // gutter-ok: the entry's own padding
                .width(Length::Fill)
                .height(Length::Fill);
            let surface = if raised {
                surface.style(iad::styles::container::card)
            } else {
                surface
            };
            return button(surface)
                .padding(0)
                .style(move |theme, status| {
                    if raised {
                        iad::styles::button::events(theme, status)
                    } else {
                        iad::styles::button::flat(theme, status)
                    }
                })
                .width(Length::Fill)
                .height(Length::Fill)
                .on_press(on_press)
                .into();
        }
        w::ItemCard::new(title)
            .subtitle(subtitle)
            .picture(picture(if compact { 36.0 } else { card_picture }))
            .density(if compact {
                iad::config::Density::Compact
            } else {
                iad::config::Density::Comfort
            })
            .on_press(on_press)
            .into()
    }

    /// Artist and album collections use full-width rows narrow, flat horizontal entries wide.
    pub(super) fn media_grid<'a>(&self, items: Vec<Element<'a>>) -> Element<'a> {
        if self.is_narrow() {
            column(items).spacing(4).width(Length::Fill).into()
        } else {
            self.grid(items, 260.0)
        }
    }

    pub(super) fn album_card(&self, i: usize, show_artist: bool) -> Element<'_> {
        let (name, artist, _) = self.lib.albums()[i].clone();
        let subtitle = if show_artist {
            artist.clone()
        } else {
            self.lib.year_label(&name, &artist)
        };
        if self.is_narrow() {
            return w::action_row(name)
                .subtitle(subtitle)
                .prefix(cover(i, 40.0))
                .chevron(true)
                .on_activate(Message::Open(Dest::Album(i)))
                .into();
        }
        self.media_card(
            name,
            subtitle,
            |s| cover(i, s),
            40.0,
            false,
            Message::Open(Dest::Album(i)),
        )
    }

    fn playlist_card(&self, i: usize) -> Element<'_> {
        let p = &self.lib.playlists[i];
        let n = self.lib.playlist_tracks(p).len();
        let kind = if !p.manual { "dynamic" } else { "manual" };
        self.media_card(
            p.name.clone(),
            format!("{kind} · {n} tracks"),
            |s| playlist_avatar(&p.name, p.icon, s),
            40.0,
            true,
            Message::Open(Dest::Playlist(i)),
        )
    }

    pub(super) fn artist_card(&self, i: usize, with_plays: Option<u32>) -> Element<'_> {
        let name = self.lib.artists()[i].0.clone();
        let subtitle = with_plays.map(|plays| format!("{plays} plays"));
        if self.is_narrow() {
            let mut entry = w::action_row(name.clone())
                .prefix(w::avatar(40.0, name))
                .chevron(true)
                .on_activate(Message::Open(Dest::Artist(i)));
            if let Some(subtitle) = subtitle {
                entry = entry.subtitle(subtitle);
            }
            return entry.into();
        }
        self.media_card(
            name.clone(),
            subtitle.unwrap_or_default(),
            |s| w::avatar(s, name.clone()).into(),
            40.0,
            false,
            Message::Open(Dest::Artist(i)),
        )
    }

    fn artists(&self) -> Element<'_> {
        self.collection_list(Dest::Artists)
    }
    fn albums(&self) -> Element<'_> {
        self.collection_list(Dest::Albums)
    }
    fn playlists(&self) -> Element<'_> {
        self.collection_list(Dest::Playlists)
    }

    // ---- Home -----------------------------------------------------------------------------

    /// A short list of tracks in the page's own flat rows.
    fn short_list(&self, tracks: &[Track]) -> Element<'_> {
        let o = RowOpts {
            album: true,
            bpm: false,
            menu: true,
        };
        let h = self.row_h();
        column(tracks.iter().map(|t| {
            container(self.track_row(t, o, Message::PlayOne(t.id)))
                .height(Length::Fixed(h))
                .into()
        }))
        .into()
    }

    /// Home is only what is recent: jams, tracks, playlists, albums, and the usage summary.
    fn home(&self) -> Element<'_> {
        // Recent tracks: what was played, or until anything was, what was added.
        let recent: Vec<Track> = self
            .lib
            .recent_tracks(5, RecentOrder::Played)
            .into_iter()
            .cloned()
            .collect();
        let recent_order = weave_player_lib::core::CollectionOrder::Recent;
        let playlist_indices = self
            .collection_playlist_indices("", recent_order)
            .unwrap_or_default();
        let no_playlists = playlist_indices.is_empty();
        let playlists = self.grid(
            playlist_indices
                .into_iter()
                .take(6)
                .map(|i| self.playlist_card(i))
                .collect(),
            260.0,
        );
        let recent_artists = self
            .collection_artist_indices("", recent_order)
            .unwrap_or_default();
        let no_artists = recent_artists.is_empty();
        let artists = self.media_grid(
            recent_artists
                .into_iter()
                .take(8)
                .map(|i| self.artist_card(i, None))
                .collect(),
        );
        let recent_albums = self
            .collection_album_indices("", recent_order)
            .unwrap_or_default();
        let n_albums = recent_albums.len();
        let albums = self.media_grid(
            recent_albums
                .into_iter()
                .take(6)
                .map(|i| self.album_card(i, true))
                .collect(),
        );
        let page = column![
            section_all(
                "Recent playlists",
                Dest::Playlists,
                or_note(
                    no_playlists,
                    "No playlists yet. Save a jam to make one.",
                    playlists
                )
            ),
            section_all(
                "Recent tracks",
                Dest::Recent,
                or_note(
                    recent.is_empty(),
                    "Nothing here yet. Let's start listening: press play on any track.",
                    self.short_list(&recent)
                )
            ),
            section_all(
                "Recent albums",
                Dest::Albums,
                or_note(
                    n_albums == 0,
                    "Albums show up here when your tracks have an album tag.",
                    albums
                )
            ),
            section_all(
                "Recent artists",
                Dest::Artists,
                or_note(
                    no_artists,
                    "The artists you listen to will show up here.",
                    artists
                )
            ),
        ]
        .spacing(20);
        surface::scrolling(page)
    }

    /// Rediscover what you loved and explore what you might: old favourites, what you play most,
    /// what you might like, then a direct link to each full list.
    fn explore(&self) -> Element<'_> {
        self.explore_page()
    }

    // ---- detail pages -----------------------------------------------------------------------

    /// The options of a detail page as an iad `Menu`, each message passed through `wrap`: edit its
    /// details, and a dynamic playlist's filter.
    fn options_menu_def(
        title: String,
        dynamic: bool,
        deletable: bool,
        wrap: impl Fn(Message) -> Message,
    ) -> Menu<Message> {
        let mut items =
            vec![MenuItem::action("Edit details", wrap(Message::EditDetails)).icon(Icon::Edit)];
        if dynamic {
            items.push(
                MenuItem::action("Edit filter", wrap(Message::EditFilter)).icon(Icon::Filter),
            );
        }
        let mut menu = Menu::new(title).section(items);
        if deletable {
            menu = menu.section([MenuItem::action(
                "Delete playlist…",
                wrap(Message::AskDeletePlaylist),
            )
            .icon(Icon::Delete)]);
        }
        menu
    }

    /// The shuffle toggle of a playlist or an album page, beside its ⋮: on, Play shuffles it (from
    /// a random track); off, Play plays it in order. It is kept with the playlist or the album.
    ///
    /// A page that has no such setting (see `page_shuffle`) has no toggle.
    fn shuffle_toggle(&self) -> Option<Element<'_>> {
        let on = self.page_shuffle()? && !self.has_playback_sort();
        Some(
            w::IconToggleButton::new(Icon::Shuffle, on)
                .size(20.0)
                .tooltip("Shuffle")
                .on_toggle(|on| Message::SetShufflePref(on.then_some(true)))
                .into(),
        )
    }

    /// The options of a detail page: a dropdown on desktop, a button that opens the bottom sheet
    /// on narrow.
    fn options_menu(&self, dynamic: bool) -> Element<'_> {
        if self.is_narrow() {
            return w::icon_button(Icon::ViewMore)
                .on_press(Message::OpenSheet(Some(Sheet::Options(dynamic))))
                .into();
        }
        self.menu_button(
            Icon::ViewMore,
            "Options",
            Self::options_menu_def("Options".into(), dynamic, self.can_delete_playlist(), |m| m),
        )
    }

    /// The page options as a bottom sheet, closing on a choice.
    fn options_sheet(&self, dynamic: bool) -> Element<'_> {
        let menu = Self::options_menu_def(
            self.page_title(),
            dynamic,
            self.can_delete_playlist(),
            |m| Message::SheetAction(Box::new(m)),
        );
        Menu::sheet(&menu, None)
    }

    /// The order of Recent tracks as an iad `Menu` of check items, each message passed through
    /// `wrap`: by when it was played (until anything was, by when it was added) or by when it was
    /// added.
    fn recent_menu_def(&self, title: &str, wrap: impl Fn(Message) -> Message) -> Menu<Message> {
        let item = |label: &'static str, order: RecentOrder| {
            MenuItem::check(
                label,
                self.recent_order == order,
                wrap(Message::SetRecentOrder(order)),
            )
        };
        Menu::new(title)
            .check_style(w::MenuCheckStyle::Mark)
            .section([
                item("Recently played", RecentOrder::Played),
                item("Recently added", RecentOrder::Added),
            ])
    }

    /// The ⋮ of Recent tracks: a dropdown on desktop, a bottom sheet on narrow.
    fn recent_menu(&self) -> Element<'_> {
        if self.is_narrow() {
            return w::icon_button(Icon::ViewMore)
                .on_press(Message::OpenSheet(Some(Sheet::Recent)))
                .into();
        }
        self.menu_button(
            Icon::ViewMore,
            "Order",
            self.recent_menu_def("Order", |m| m),
        )
    }

    /// What the bottom sheet shows.
    pub fn sheet_view(&self, sheet: Sheet) -> Element<'_> {
        match sheet {
            Sheet::Main => {
                let menu = self.main_menu_def(|m| Message::SheetAction(Box::new(m)));
                Menu::sheet(&menu, None)
            }
            Sheet::Recent => {
                let menu = self.recent_menu_def("Order", |m| Message::SheetAction(Box::new(m)));
                Menu::sheet(&menu, None)
            }
            Sheet::Sort => self
                .collection_sort_menu(|m| Message::SheetAction(Box::new(m)))
                .sheet(None),
            Sheet::Sleep => self
                .sleep_menu(|m| Message::SheetAction(Box::new(m)))
                .sheet(None),
            Sheet::Track(id, album) => self.track_sheet(id, album),
            Sheet::Options(dynamic) => self.options_sheet(dynamic),
        }
    }

    fn playlist_order_actions(&self) -> Element<'_> {
        use iad::primitives::reorder::Event;
        match &self.playlist_edit {
            Some(edit) => row![
                w::tooltip(
                    self.icon_action(
                        Icon::Check,
                        edit.order
                            .is_dirty()
                            .then_some(Message::PlaylistOrder(Event::Submit))
                    ),
                    "Save order",
                    w::TooltipPosition::Bottom
                ),
                w::tooltip(
                    self.icon_action(
                        Icon::WindowClose,
                        Some(Message::PlaylistOrder(Event::Cancel))
                    ),
                    "Cancel reordering",
                    w::TooltipPosition::Bottom
                ),
            ]
            .spacing(8)
            .into(),
            None => {
                let available = matches!(self.dest, Dest::Playlist(i) if self.lib.playlists.get(i).is_some_and(|p| p.manual && p.tracks.len() > 1));
                w::tooltip(
                    self.icon_action(
                        Icon::Edit,
                        available.then_some(Message::PlaylistOrder(Event::Edit)),
                    ),
                    "Reorder tracks",
                    w::TooltipPosition::Bottom,
                )
            }
        }
    }
    fn playlist_order_list<'a>(&'a self, edit: &'a crate::PlaylistOrderEdit) -> Element<'a> {
        use iad::primitives::reorder::Event;
        w::ReorderableList::of(&edit.order, |slug| {
            match self.lib.tracks.iter().find(|t| &t.slug == slug) {
                Some(track) => row![
                    cover(track.id, 40.0),
                    column![
                        ty::body(track.title.clone()),
                        ty::caption(track.artist.clone())
                    ]
                    .width(Length::Fill),
                    ty::caption(track.duration()),
                ]
                .spacing(12)
                .align_y(Alignment::Center)
                .into(),
                None => ty::body(slug.clone()).into(),
            }
        })
        .controls(false)
        .row_action(|slug| {
            let index = edit
                .order
                .items()
                .iter()
                .position(|item| item == slug)
                .unwrap_or(0);
            row![
                w::tooltip(
                    self.icon_action(
                        Icon::GoUp,
                        index
                            .checked_sub(1)
                            .map(|to| Message::PlaylistOrder(Event::Reorder { from: index, to }))
                    ),
                    "Move up",
                    w::TooltipPosition::Bottom
                ),
                w::tooltip(
                    self.icon_action(
                        Icon::GoDown,
                        (index + 1 < edit.order.items().len()).then_some(Message::PlaylistOrder(
                            Event::Reorder {
                                from: index,
                                to: index + 1
                            }
                        ))
                    ),
                    "Move down",
                    w::TooltipPosition::Bottom
                ),
            ]
            .spacing(2)
            .into()
        })
        .on_event(Message::PlaylistOrder)
        .into()
    }

    fn detail_page(&self) -> Element<'_> {
        let narrow = self.is_narrow();
        let big = 96.0;
        // The description is only read here: no edit button. Edit details is in the page's menu
        // (a dropdown, a bottom sheet on narrow).
        let description: Element<'_> = match self
            .desc_key()
            .and_then(|k| self.descriptions.get(&k))
            .filter(|d| !d.is_empty())
        {
            Some(text) => w::Spoiler::new(ty::body(text.as_str()))
                .max_height(44.0)
                .expanded(self.desc_expanded)
                .on_toggle(Message::DescExpanded)
                .into(),
            None => space::vertical().height(0).into(),
        };
        let (cover_el, kind, title, facts, actions): (
            Element<'_>,
            String,
            String,
            Element<'_>,
            Vec<Element<'_>>,
        ) = match self.dest {
            Dest::Playlist(i) => {
                let p = &self.lib.playlists[i];
                let extra = p.filter.as_ref().and_then(|filter| {
                    let mut remaining = filter.clone();
                    remaining.tags.clear();
                    remaining.retained.tag_blocks.clear();
                    let summary = self.filter_sentence(&remaining);
                    (summary != "everything").then_some(summary)
                });
                let mut facts = column![self.facts(extra)].spacing(6);
                if let Some(filter) = &p.filter {
                    let chips = self.tag_selection_chips(
                        &filter.tags,
                        &filter.retained.tag_blocks,
                        None,
                        None,
                    );
                    if !chips.is_empty() {
                        facts = facts.push(row(chips).spacing(6).align_y(Alignment::Center).wrap());
                    }
                }
                (
                    playlist_avatar(&p.name, p.icon, big),
                    if !p.manual {
                        "Dynamic playlist"
                    } else {
                        "Playlist"
                    }
                    .into(),
                    p.name.to_string(),
                    facts.into(),
                    self.shuffle_toggle()
                        .into_iter()
                        .chain([self.options_menu(true)])
                        .collect(),
                )
            }
            Dest::Artist(i) => {
                let name = self.lib.artists()[i].0.clone();
                let avg = self.lib.artist_rating(&name);
                let fav = self.favorites.contains(&name);
                let facts = row![
                    ty::body(stars(avg)),
                    ty::caption(if avg > 0.0 {
                        format!("{avg:.1}")
                    } else {
                        "not rated".into()
                    }),
                    ty::caption(format!("· {} tracks", self.rows.len())),
                ]
                .spacing(8)
                .align_y(Alignment::Center);
                (
                    w::avatar(big, name.clone()).into(),
                    "Artist".into(),
                    name.clone(),
                    facts.into(),
                    vec![
                        w::IconToggleButton::new(Icon::Heart, fav)
                            .size(20.0)
                            .tooltip("Favorite")
                            .on_toggle({
                                let name = name.clone();
                                move |_| Message::ToggleFavorite(name.clone())
                            })
                            .into(),
                        self.options_menu(false),
                    ],
                )
            }
            Dest::Album(i) => {
                let (album, artist, _) = self.lib.albums()[i].clone();
                let year = self.lib.year_label(&album, &artist);
                (
                    cover(i, big),
                    "Album".into(),
                    album,
                    self.facts(Some(format!("{artist} · {year}"))),
                    self.shuffle_toggle()
                        .into_iter()
                        .chain([self.options_menu(false)])
                        .collect(),
                )
            }
            _ => unreachable!("detail_page is for playlists, artists and albums"),
        };
        // The kind ("Dynamic playlist", "Album") sits above the facts and tags line, not over the
        // name.
        let facts: Element<'_> = column![ty::caption(kind).style(iad::styles::text::dim), facts]
            .spacing(2)
            .into();
        let mut header = crate::components::MediaHeader::new(title)
            .cover(cover_el)
            .description(description)
            .facts(facts)
            .primary(self.play_all())
            .narrow(narrow);
        header = header.action(self.collection_sort_control());
        if matches!(self.dest, Dest::Playlist(i) if self.lib.playlists[i].manual) {
            if self.playlist_edit.is_none() {
                header = header.action(w::tooltip(
                    w::icon_button(Icon::ListAdd).on_press(Message::PlaylistTracksOpen(true)),
                    "Add or remove tracks",
                    w::TooltipPosition::Bottom,
                ));
            }
            header = header.action(self.playlist_order_actions());
        }
        if self.playlist_edit.is_none() {
            for action in actions {
                header = header.action(action);
            }
        }
        let header: Element<'_> = if self.search_open {
            self.page_search_bar()
        } else {
            header.into()
        };
        let o = RowOpts {
            album: !matches!(self.dest, Dest::Album(_)),
            bpm: false,
            menu: true,
        };
        let mut page = column![header].spacing(16);
        if let Some(edit) = &self.playlist_edit {
            return scrollable(page.push(surface::band(self.playlist_order_list(edit))))
                .height(Length::Fill)
                .into();
        }
        if let Dest::Artist(i) = self.dest {
            let name = self.lib.artists()[i].0.clone();
            let open = self.artist_open.get(&name).copied().unwrap_or([true; 3]);
            let popular = self.lib.popular_albums(&name, 4);
            let latest = self.lib.latest_albums(&name, 4);
            if !self.search_open && !popular.is_empty() {
                page = page.push(surface::band(
                    self.section_group(
                        "Popular albums",
                        open[0],
                        Message::ArtistSection(0),
                        self.media_grid(
                            popular
                                .into_iter()
                                .map(|a| self.album_card(a, false))
                                .collect(),
                        ),
                    ),
                ));
            }
            if !self.search_open && !latest.is_empty() {
                page = page.push(surface::band(
                    self.section_group(
                        "Latest albums",
                        open[1],
                        Message::ArtistSection(1),
                        self.media_grid(
                            latest
                                .into_iter()
                                .map(|a| self.album_card(a, false))
                                .collect(),
                        ),
                    ),
                ));
            }
            page = page.push(surface::band(self.section_group(
                "All tracks",
                open[2],
                Message::ArtistSection(2),
                self.plain_list(&self.rows, o),
            )));
        } else {
            page = page.push(surface::band(self.plain_list(&self.rows, o)));
        }
        scrollable(page).height(Length::Fill).into()
    }

    /// A dynamic playlist's filter as a sentence: "bachata · 60–120 BPM".
    pub fn filter_sentence(&self, f: &Filter) -> String {
        let mut parts: Vec<String> = f
            .tags
            .iter()
            .map(|t| t.split_once(':').map_or(t.as_str(), |(_, v)| v).to_string())
            .collect();
        for block in &f.retained.tag_blocks {
            let labels = block
                .iter()
                .map(crate::ui_dialogs::tag_group_label)
                .collect::<Vec<_>>();
            if !labels.is_empty() {
                parts.push(labels.join(", "));
            }
        }
        if let Some(days) = f.imported_within_days {
            parts.push(format!("imported in the last {days} days"));
        }
        if f.bpm != crate::data::BPM {
            parts.push(format!("{:.0}–{:.0} BPM", f.bpm.0, f.bpm.1));
        }
        for (names, none) in [(&f.artists, "Unknown artist"), (&f.albums, "No album")] {
            if !names.is_empty() {
                let shown: Vec<&str> = names
                    .iter()
                    .map(|n| {
                        if n == crate::data::UNKNOWN {
                            none
                        } else {
                            n.as_str()
                        }
                    })
                    .collect();
                parts.push(shown.join(" or "));
            }
        }
        let spec = f.to_spec();
        if !spec.tag_exclude.is_empty() {
            parts.push(format!("excluding {}", spec.tag_exclude.join(", ")));
        }
        if let Some(query) = spec.query {
            parts.push(format!("text: {query}"));
        }
        if !spec.sources.is_empty() {
            parts.push(format!("from {}", spec.sources.join(", ")));
        }
        if let Some(days) = spec.not_played_for_days {
            parts.push(format!("inactive {days} days"));
        }
        if let Some(played) = spec.has_played {
            parts.push(if played {
                "previously played".into()
            } else {
                "never played".into()
            });
        }
        if spec.rating_min.is_some() || spec.rating_max.is_some() {
            parts.push(format!(
                "rating {}–{}",
                spec.rating_min.unwrap_or(1),
                spec.rating_max.unwrap_or(5)
            ));
        }
        if spec.year_min.is_some() || spec.year_max.is_some() {
            parts.push(format!(
                "year {}–{}",
                spec.year_min
                    .map(|y| y.to_string())
                    .unwrap_or_else(|| "any".into()),
                spec.year_max
                    .map(|y| y.to_string())
                    .unwrap_or_else(|| "any".into())
            ));
        }
        if !spec.tag_any.is_empty() {
            parts.push(format!("any of {}", spec.tag_any.join(", ")));
        }
        if !spec.tag_clauses.is_empty() {
            parts.push(format!("{} additional tag rules", spec.tag_clauses.len()));
        }
        if let Some(limit) = spec.limit {
            parts.push(format!("first {limit}"));
        }
        if parts.is_empty() {
            "everything".into()
        } else {
            parts.join(" · ")
        }
    }

    // ---- dispatch ---------------------------------------------------------------------------

    /// A page with nothing to show: an icon, a title, a line saying what will appear and how, and
    /// an optional action.
    fn empty_page(
        &self,
        icon: Icon,
        title: &'static str,
        description: Option<&'static str>,
        action: Option<(&'static str, Message)>,
    ) -> Element<'_> {
        let dim = self.theme().semantic.dim_solid();
        let mut page = w::status_page::<Message>()
            .icon(icon.colored(64.0, dim))
            .title(title);
        if let Some(d) = description {
            page = page.description(d);
        }
        if let Some((label, m)) = action {
            page = page.child(w::suggested_pill_button(ty::body(label)).on_press(m));
        }
        container(page).center(Length::Fill).into()
    }

    /// An empty library offers a direct folder import; the menu also offers individual files.
    fn empty_library(&self, title: &'static str) -> Element<'_> {
        self.empty_page(
            Icon::MusicNote,
            title,
            None,
            Some((
                "Import folder…",
                Message::Import(crate::import_picker::Mode::Folder),
            )),
        )
    }

    /// The current page, without the layout around it.
    pub fn page(&self) -> Element<'_> {
        if self.jam_open {
            return self.jam_page();
        }
        let empty = self.lib.tracks.is_empty();
        match self.dest {
            Dest::Home if empty => self.empty_library("No tracks yet"),
            Dest::Home => self.home(),
            Dest::Explore => self.explore(),
            Dest::Rediscover => self.rediscover_page(),
            Dest::AboutMe => self.about_page(),
            Dest::Recent | Dest::Tracks => self.track_page(),
            Dest::Artists => self.artists(),
            Dest::Albums => self.albums(),
            Dest::Playlists => self.playlists(),
            Dest::Playlist(_) | Dest::Artist(_) | Dest::Album(_) => self.detail_page(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::limit_chars;

    #[test]
    fn a_long_name_is_cut_with_an_ellipsis_and_a_short_one_is_kept() {
        assert_eq!(limit_chars("Road trip", 50), "Road trip");
        let fifty = "x".repeat(50);
        assert_eq!(limit_chars(&fifty, 50), fifty, "exactly the limit is kept");
        let long = format!("{} tail", "word ".repeat(20));
        let cut = limit_chars(&long, 50);
        assert!(cut.chars().count() <= 50 && cut.ends_with('…'), "{cut}");
        // Characters, not bytes: accents do not shorten the allowance.
        assert_eq!(limit_chars(&"é".repeat(60), 50).chars().count(), 50);
    }
}
