//! Track lists (flat rows, one setting for compact), the library pages, the detail pages
//! (playlist, artist, album) and Home.

use iad::Icon;
use iad::prelude::{self as w, Menu, MenuItem, MenuPresentation, VirtualList, typography as ty};
use iced::alignment::Horizontal;
use iced::widget::{button, column, container, row, scrollable, space};
use iced::{Alignment, Length};
use iced_adaptive_design as iad;

use crate::components::media_list::MediaListLayout;
use crate::data::{Filter, Track};
use crate::icons::{cover, playlist_avatar};
use crate::ui::card;
use crate::{App, Col, Dest, Element, Message, Sheet};
use iad::color::{from_index, from_seed};
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

fn section<'a>(title: &'a str, body: impl Into<Element<'a>>) -> Element<'a> {
    column![w::SectionHeading::new(title), body.into()]
        .spacing(8)
        .width(Length::Fill)
        .into()
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
    /// A menu button: iad's `Menu`, a dropdown on desktop and a full page on narrow. Its icons and
    /// checks sit at the end of each item (iad's default).
    pub fn menu_button<'a>(
        &self,
        icon: Icon,
        tip: &'static str,
        menu: Menu<Message>,
    ) -> Element<'a> {
        let presentation = if self.is_narrow() {
            MenuPresentation::FullPage
        } else {
            MenuPresentation::Dropdown
        };
        w::menu_button(icon, menu)
            .presentation(presentation)
            .tooltip(tip)
            .into()
    }

    /// What a track's menu offers, in sections: icon, label, message. One list for the dropdown
    /// (desktop) and the bottom sheet (narrow). `album` is false on an album's own page, where
    /// "View album" is moot.
    fn track_actions(id: usize, album: bool) -> Vec<Vec<(iad::IconSource, &'static str, Message)>> {
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
        title: String,
        id: usize,
        album: bool,
        wrap: impl Fn(Message) -> Message,
    ) -> Menu<Message> {
        let mut menu = Menu::new(title);
        for section in Self::track_actions(id, album) {
            menu = menu.section(
                section
                    .into_iter()
                    .map(|(icon, label, m)| MenuItem::action(label, wrap(m)).icon(icon)),
            );
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
            Self::track_menu_def("Track".into(), id, album, |m| m),
        )
    }

    /// The track menu as a bottom sheet: iad's `Menu::sheet`, closing on a choice.
    pub fn track_sheet(&self, id: usize, album: bool) -> Element<'_> {
        let t = self.track(id);
        let menu = Self::track_menu_def(t.title.clone(), id, album, |m| {
            Message::SheetAction(Box::new(m))
        });
        Menu::sheet(&menu, Some(t.artist.clone())).into()
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
            r = r.menu(self.track_menu(t.id, o.album));
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
    fn sort_header(&self) -> Element<'_> {
        self.media_columns(RowOpts {
            album: true,
            bpm: false,
            menu: true,
        })
        .header(&self.sort, Message::Sort)
    }

    /// The page's tracks as a virtualised list: only the rows in view are built.
    pub fn virtual_tracks(&self, o: RowOpts) -> Element<'_> {
        column![
            surface::band(self.sort_header()),
            VirtualList::new(self.rows.len(), self.row_h(), &self.list_vp, move |i| {
                let t = &self.rows[i];
                surface::row(self.track_row(t, o, Message::PlayTrack(t.id)))
            })
            .on_scroll(Message::ListScroll)
            .overscan(4),
        ]
        .height(Length::Fill)
        .into()
    }

    /// The tracks a jam would play: tempo and time, in the same flat rows.
    pub fn jam_list(&self) -> Element<'_> {
        let o = RowOpts {
            album: false,
            bpm: true,
            menu: false,
        };
        let rows: Vec<&Track> = self.lib.filtered(&self.filter);
        VirtualList::new(rows.len(), self.row_h(), &self.jam_vp, move |i| {
            let t = rows[i];
            self.track_row(t, o, Message::PlayAll(vec![t.id]))
        })
        .on_scroll(Message::JamScroll)
        .overscan(4)
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

    fn play_all(&self) -> Element<'_> {
        let ids: Vec<usize> = self.rows.iter().map(|t| t.id).collect();
        w::suggested_circular_button(Icon::Play.view(20.0))
            .size(44.0)
            .on_press_maybe((!ids.is_empty()).then_some(Message::PlayAll(ids)))
            .into()
    }

    /// Tracks and Recently played: the play button, then the long list.
    fn track_page(&self) -> Element<'_> {
        column![
            surface::toolbar(
                row![
                    self.play_all(),
                    ty::caption(format!("{} tracks", self.rows.len()))
                ]
                .spacing(12)
                .align_y(Alignment::Center)
            ),
            self.virtual_tracks(RowOpts {
                album: true,
                bpm: false,
                menu: true
            }),
        ]
        .height(Length::Fill)
        .into()
    }

    /// Search results: the same list, narrowed.
    fn results_page(&self) -> Element<'_> {
        let list = column![
            surface::toolbar(ty::caption(format!("{} tracks", self.rows.len()))),
            self.virtual_tracks(RowOpts {
                album: true,
                bpm: false,
                menu: true
            }),
        ]
        .height(Length::Fill);
        if self.is_narrow() {
            column![surface::toolbar(self.search_entry()), list].into()
        } else {
            list.into()
        }
    }

    // ---- grids ----------------------------------------------------------------------------

    /// Cards that fit the width, or (in compact mode) the same entries as a tight list: iad's
    /// `FlowGrid`, with the density decided once.
    fn grid<'a>(&self, items: Vec<Element<'a>>, card_width: f32) -> Element<'a> {
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
        row_layout: bool,
        on_press: Message,
    ) -> Element<'a> {
        // `row_layout`: the picture beside the text, always (playlists), not only when compact.
        let compact = self.compact();
        if row_layout && !compact {
            // Playlists: a card surface of its own, with `button` adding the events: the picture
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
                .padding(12) // gutter-ok: the card's own padding
                .width(Length::Fill)
                .height(Length::Fill)
                .style(iad::styles::container::card);
            return button(surface)
                .padding(0)
                .style(iad::styles::button::events)
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

    fn album_card(&self, i: usize, show_artist: bool) -> Element<'_> {
        let (name, artist, n) = self.lib.albums()[i].clone();
        let year = self.lib.year_label(&name, &artist);
        let sub = if show_artist {
            format!("{artist} · {year}")
        } else {
            format!("{year} · {n} tracks")
        };
        self.media_card(
            name,
            sub,
            |s| cover(i, s),
            88.0,
            false,
            Message::Open(Dest::Album(i)),
        )
    }

    fn playlist_card(&self, i: usize) -> Element<'_> {
        let p = &self.lib.playlists[i];
        let n = self.lib.playlist_tracks(p).len();
        let kind = if p.filter.is_some() {
            "dynamic"
        } else {
            "manual"
        };
        self.media_card(
            p.name.clone(),
            format!("{kind} · {n} tracks"),
            |s| playlist_avatar(&p.name, p.icon, s),
            40.0,
            true,
            Message::Open(Dest::Playlist(i)),
        )
    }

    fn artist_card(&self, i: usize, with_plays: Option<u32>) -> Element<'_> {
        let a = self.lib.artists()[i].0.clone();
        let sub = match with_plays {
            Some(p) => format!("{p} plays"),
            None => format!("{} tracks", self.lib.artists()[i].1),
        };
        self.media_card(
            a.clone(),
            sub,
            |s| w::avatar(s, a.clone()).into(),
            56.0,
            false,
            Message::Open(Dest::Artist(i)),
        )
    }

    fn artists(&self) -> Element<'_> {
        surface::scrolling(
            self.grid(
                (0..self.lib.artists().len())
                    .map(|i| self.artist_card(i, None))
                    .collect(),
                130.0,
            ),
        )
    }

    fn albums(&self) -> Element<'_> {
        surface::scrolling(
            self.grid(
                (0..self.lib.albums().len())
                    .map(|i| self.album_card(i, true))
                    .collect(),
                130.0,
            ),
        )
    }

    fn playlists(&self) -> Element<'_> {
        surface::scrolling(
            self.grid(
                (0..self.lib.playlists.len())
                    .map(|i| self.playlist_card(i))
                    .collect(),
                260.0,
            ),
        )
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
            container(self.track_row(t, o, Message::PlayAll(vec![t.id])))
                .height(Length::Fixed(h))
                .into()
        }))
        .into()
    }

    /// Home is only what is recent: jams, tracks, playlists, albums, and the usage summary.
    fn home(&self) -> Element<'_> {
        let recent: Vec<Track> = self.lib.recently_played(5).into_iter().cloned().collect();
        // The newest first: a jam saved as a playlist shows up here.
        let playlists = self.grid(
            (0..self.lib.playlists.len())
                .rev()
                .take(6)
                .map(|i| self.playlist_card(i))
                .collect(),
            260.0,
        );
        let recent_artists = self.lib.recent_artists(8);
        let no_artists = recent_artists.is_empty();
        let no_playlists = self.lib.playlists.is_empty();
        let artists = self.grid(
            recent_artists
                .into_iter()
                .map(|i| self.artist_card(i, None))
                .collect(),
            130.0,
        );
        let n_albums = self.lib.albums().len();
        let albums = self.grid(
            (0..n_albums)
                .rev()
                .take(6)
                .map(|i| self.album_card(i, true))
                .collect(),
            130.0,
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
                "Recently played",
                Dest::Recent,
                or_note(
                    recent.is_empty(),
                    "Nothing played yet. Let's start listening: press play on any track.",
                    self.short_list(&recent)
                )
            ),
            section_all("Recent albums", Dest::Albums, albums),
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
        // TODO(karaoke): a karaoke mode (large synced lyrics, vocal level, key and tempo shift)
        // belongs here and on Now Playing once the engine can separate or duck vocals.
        let while_tracks: Vec<Track> = self
            .lib
            .been_a_while(self.while_days, 5)
            .into_iter()
            .cloned()
            .collect();
        let top_tracks: Vec<Track> = self
            .lib
            .most_listened_tracks(5)
            .into_iter()
            .cloned()
            .collect();
        let most_listened = self.lib.most_listened_artists(6);
        let no_top_artists = most_listened.is_empty();
        let top_artists = self.grid(
            most_listened
                .into_iter()
                .map(|(i, plays)| self.artist_card(i, Some(plays)))
                .collect(),
            130.0,
        );
        let suggested: Vec<usize> = (0..self.lib.artists().len()).skip(2).take(6).collect();
        let no_suggestions = suggested.is_empty();
        let might_like = self.grid(
            suggested
                .into_iter()
                .map(|i| self.artist_card(i, None))
                .collect(),
            130.0,
        );
        // The engine records no listening history yet: until it does, usage has nothing to show.
        let has_history = self.lib.tracks.iter().any(|t| t.plays > 0);
        let link = |label: &'static str, icon: Element<'static>, to: Dest| -> Element<'_> {
            button(card(
                row![
                    icon,
                    ty::heading(label),
                    space::horizontal(),
                    Icon::GoNext.view(14.0)
                ]
                .spacing(10)
                .align_y(Alignment::Center)
                .width(Length::Fill),
            ))
            .on_press(Message::Open(to))
            .padding(0)
            .style(iad::styles::button::events)
            .width(Length::Fill)
            .into()
        };
        let links = w::FlowGrid::new(vec![
            link(
                "All playlists",
                Icon::Document.view(18.0).into(),
                Dest::Playlists,
            ),
            link(
                "All artists",
                Icon::MusicNote.view(18.0).into(),
                Dest::Artists,
            ),
            link(
                "All albums",
                crate::icons::library().view(18.0).into(),
                Dest::Albums,
            ),
            link("All tracks", Icon::Queue.view(18.0).into(), Dest::Tracks),
        ])
        .max_width(260.0)
        .spacing(12.0);
        let k = if self.days == 7 { 1 } else { 4 };
        let usage = row![
            card(column![ty::title_2(format!("{}h", 6 * k)), ty::caption("listened")].spacing(2)),
            card(
                column![
                    ty::title_2(format!("{}", 84 * k)),
                    ty::caption("tracks played")
                ]
                .spacing(2)
            ),
            card(column![ty::title_2("mood:romantic"), ty::caption("top tag")].spacing(2)),
        ]
        .spacing(12);
        let days = w::toggle_group(
            vec![
                w::ToggleItem::new(7u8, "7 days"),
                w::ToggleItem::new(30u8, "30 days"),
            ],
            Some(self.days),
            Message::Days,
        );
        let rediscover_head = column![
            ty::heading("It's been a while"),
            ty::caption(format!(
                "Rated highly, not played for more than {} days",
                self.while_days
            )),
        ]
        .spacing(4);
        let mut rediscover = column![
            column![
                rediscover_head,
                or_note(
                    while_tracks.is_empty(),
                    "Tracks you rate highly and have not played in a while will show up here.",
                    self.short_list(&while_tracks),
                ),
            ]
            .spacing(4)
        ]
        .spacing(12);
        rediscover = rediscover
            .push(section(
                "Your most listened artists",
                or_note(
                    no_top_artists,
                    "Play something and your most listened artists will show up here.",
                    top_artists,
                ),
            ))
            .push(section(
                "Your most listened tracks",
                or_note(
                    top_tracks.is_empty(),
                    "Play something and your most listened tracks will show up here.",
                    self.short_list(&top_tracks),
                ),
            ));
        let usage_block: Element<'_> = if has_history {
            column![
                row![ty::heading("Usage"), space::horizontal(), days].align_y(Alignment::Center),
                usage,
                ty::caption("Sample data: Explore needs play and jam history from the engine."),
            ]
            .spacing(20)
            .into()
        } else {
            column![
                ty::heading("Usage"),
                or_note(
                    true,
                    "Play some music and your listening time, tracks played and top tags will show up here.",
                    space::vertical().height(0),
                ),
            ]
            .spacing(8)
            .into()
        };
        let page = column![
            links,
            usage_block,
            ty::title_3("Rediscover"),
            rediscover,
            ty::title_3("Explore"),
            section(
                "You might like",
                or_note(
                    no_suggestions,
                    "Suggestions will show up here as your library grows.",
                    might_like,
                ),
            ),
        ]
        .spacing(20);
        surface::scrolling(page)
    }

    // ---- detail pages -----------------------------------------------------------------------

    /// The options of a detail page as an iad `Menu`, each message passed through `wrap`: edit its
    /// details, and a dynamic playlist's filter.
    fn options_menu_def(
        title: String,
        dynamic: bool,
        wrap: impl Fn(Message) -> Message,
    ) -> Menu<Message> {
        let mut items =
            vec![MenuItem::action("Edit details", wrap(Message::EditDetails)).icon(Icon::Edit)];
        if dynamic {
            items.push(
                MenuItem::action("Edit filter", wrap(Message::EditFilter)).icon(Icon::Filter),
            );
        }
        Menu::new(title).section(items)
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
            Self::options_menu_def("Options".into(), dynamic, |m| m),
        )
    }

    /// The page options as a bottom sheet, closing on a choice.
    fn options_sheet(&self, dynamic: bool) -> Element<'_> {
        let menu = Self::options_menu_def(self.page_title(), dynamic, |m| {
            Message::SheetAction(Box::new(m))
        });
        Menu::sheet(&menu, None).into()
    }

    /// What the bottom sheet shows.
    pub fn sheet_view(&self, sheet: Sheet) -> Element<'_> {
        match sheet {
            Sheet::Track(id, album) => self.track_sheet(id, album),
            Sheet::Options(dynamic) => self.options_sheet(dynamic),
        }
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
        let (cover_el, kind, title, facts, actions, tint): (
            Element<'_>,
            String,
            String,
            Element<'_>,
            Vec<Element<'_>>,
            iced::Color,
        ) = match self.dest {
            Dest::Playlist(i) => {
                let p = &self.lib.playlists[i];
                let extra = p.filter.as_ref().map(|f| self.filter_sentence(f));
                (
                    playlist_avatar(&p.name, p.icon, big),
                    if p.filter.is_some() {
                        "Dynamic playlist"
                    } else {
                        "Playlist"
                    }
                    .into(),
                    p.name.to_string(),
                    self.facts(extra),
                    vec![self.options_menu(p.filter.is_some())],
                    from_seed(&p.name),
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
                    from_index(i),
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
                    vec![self.options_menu(false)],
                    from_index(i),
                )
            }
            _ => unreachable!("detail_page is for playlists, artists and albums"),
        };
        // The kind ("Dynamic playlist", "Album") sits above the facts and tags line, not over the
        // name.
        let facts: Element<'_> = column![ty::caption(kind).style(iad::styles::text::dim), facts]
            .spacing(2)
            .into();
        let mut header = w::DetailHeader::new(title)
            .cover(cover_el)
            .description(description)
            .facts(facts)
            .primary(self.play_all())
            .tint(tint)
            .narrow(narrow);
        for a in actions {
            header = header.action(a);
        }
        let header: Element<'_> = header.into();
        let o = RowOpts {
            album: !matches!(self.dest, Dest::Album(_)),
            bpm: false,
            menu: true,
        };
        let mut page = column![header].spacing(16);
        if let Dest::Artist(i) = self.dest {
            let name = self.lib.artists()[i].0.clone();
            let popular = self.lib.popular_albums(&name, 4);
            let latest = self.lib.latest_albums(&name, 4);
            if !popular.is_empty() {
                page = page.push(surface::band(section(
                    "Popular albums",
                    self.grid(
                        popular
                            .into_iter()
                            .map(|a| self.album_card(a, false))
                            .collect(),
                        130.0,
                    ),
                )));
            }
            if !latest.is_empty() {
                page = page.push(surface::band(section(
                    "Latest albums",
                    self.grid(
                        latest
                            .into_iter()
                            .map(|a| self.album_card(a, false))
                            .collect(),
                        130.0,
                    ),
                )));
            }
            page = page.push(surface::band(ty::heading("All tracks")));
        }
        page = page.push(surface::band(self.plain_list(&self.rows, o)));
        scrollable(page).height(Length::Fill).into()
    }

    /// A dynamic playlist's filter as a sentence: "bachata · 60–120 BPM".
    pub fn filter_sentence(&self, f: &Filter) -> String {
        let mut parts: Vec<String> = f
            .tags
            .iter()
            .map(|t| t.split_once(':').map_or(t.as_str(), |(_, v)| v).to_string())
            .collect();
        if f.bpm != crate::data::BPM {
            parts.push(format!("{:.0}–{:.0} BPM", f.bpm.0, f.bpm.1));
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

    /// A library with nothing in it: a title and an Import button (it asks for a folder; the menu
    /// also imports single files). Every page that lists the library shows this.
    fn empty_library(&self, title: &'static str) -> Element<'_> {
        self.empty_page(
            Icon::MusicNote,
            title,
            None,
            Some(("Import", Message::Import(false))),
        )
    }

    /// The current page, without the layout around it.
    pub fn page(&self) -> Element<'_> {
        if self.jam_open {
            return self.jam_page();
        }
        if self.searching {
            return self.results_page();
        }
        let empty = self.lib.tracks.is_empty();
        match self.dest {
            Dest::Home | Dest::Explore | Dest::Tracks if empty => {
                self.empty_library("No tracks yet")
            }
            Dest::Artists if empty => self.empty_library("No artists yet"),
            Dest::Albums if empty => self.empty_library("No albums yet"),
            Dest::Recent if empty => self.empty_library("No tracks yet"),
            Dest::Playlists if self.lib.playlists.is_empty() => self.empty_page(
                Icon::MusicNote,
                "No playlists yet",
                Some("Save a jam as a playlist and it will show up here."),
                Some(("Open Jam", Message::ToggleJam)),
            ),
            Dest::Home => self.home(),
            Dest::Explore => self.explore(),
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
