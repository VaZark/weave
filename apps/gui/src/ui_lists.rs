//! Track lists (flat rows, one setting for compact), the library pages, the detail pages
//! (playlist, artist, album) and Home.

use iad::Icon;
use iad::widget::{self as w, Menu, MenuItem, MenuPresentation, VirtualList, typography as ty};
use iced::alignment::Horizontal;
use iced::widget::{column, container, row, scrollable, space};
use iced::{Alignment, Length};
use iced_adaptive_design as iad;

use crate::data::{Filter, Track};
use crate::icons::{cover, playlist_avatar};
use crate::ui::card;
use crate::{App, Col, Dest, Element, Message};
use iad::color::{from_index, from_seed};

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

fn right<'a>(s: String) -> Element<'a> {
    ty::caption(s)
        .width(Length::Fill)
        .align_x(Horizontal::Right)
        .into()
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

    /// The menu of a track. `album` is false on an album's own page, where "View album" is moot.
    fn track_menu<'a>(&self, id: usize, album: bool) -> Element<'a> {
        let mut views = vec![];
        if album {
            views.push(
                MenuItem::action("View album", Message::ViewAlbum(id))
                    .icon(crate::icons::library()),
            );
        }
        views.push(MenuItem::action("View artist", Message::ViewArtist(id)).icon(Icon::MusicNote));
        self.menu_button(
            Icon::ViewMore,
            "Track options",
            Menu::new("Track")
                .section([
                    MenuItem::action("Play next", Message::PlayNext(id)).icon(Icon::GoNext),
                    MenuItem::action("Add to queue", Message::AddToQueue(id)).icon(Icon::ListAdd),
                ])
                .section([
                    MenuItem::action("Rate", Message::RateTrack(id)).icon(Icon::Star),
                    MenuItem::action("Edit tags", Message::EditTrack(id)).icon(Icon::Edit),
                    MenuItem::action("More info", Message::InfoTrack(id))
                        .icon(Icon::DialogInformation),
                ])
                .section(views),
        )
    }

    // ---- rows and lists -----------------------------------------------------------------

    /// A flat row: the picture (not when compact), the title with its artist beneath it, the
    /// album, the tempo where asked for, the duration and the menu. The whole row is the
    /// button, so it lights up as one.
    pub fn track_row<'a>(&'a self, t: &Track, o: RowOpts, on_press: Message) -> Element<'a> {
        let wide = !self.is_narrow();
        let mut r = w::MediaRow::new(t.title.clone())
            .subtitle(t.artist.clone())
            .compact(self.compact)
            .height(Length::Fill)
            .on_press(on_press);
        if !self.compact {
            r = r.picture(cover(t.id, 40.0));
        }
        if o.album && wide {
            r = r.column(Length::FillPortion(2), ty::caption(t.album.clone()));
        }
        if o.bpm {
            r = r.column(Length::Fixed(40.0), right(format!("{:.0}", t.bpm)));
        }
        r = r.column(Length::Fixed(44.0), right(t.duration()));
        if o.menu {
            r = r.suffix(self.track_menu(t.id, o.album));
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

    /// The header of a long list: sortable Title, Album and Time over the row's columns.
    fn sort_header(&self) -> Element<'_> {
        let mut h = w::SortHeader::new(&self.sort, Message::Sort);
        if !self.compact {
            h = h.space(Length::Fixed(40.0));
        }
        h = h.column(Col::Title, "Title", Length::FillPortion(3));
        if self.compact {
            h = h.column(Col::Artist, "Artist", Length::FillPortion(2));
        }
        if !self.is_narrow() {
            h = h.column(Col::Album, "Album", Length::FillPortion(2));
        }
        h.column(Col::Duration, "Time", Length::Fixed(44.0))
            .space(Length::Fixed(36.0))
            .into()
    }

    /// The page's tracks as a virtualised list: only the rows in view are built.
    pub fn virtual_tracks(&self, o: RowOpts) -> Element<'_> {
        column![
            self.sort_header(),
            VirtualList::new(self.rows.len(), self.row_h(), &self.list_vp, move |i| {
                let t = &self.rows[i];
                self.track_row(t, o, Message::PlayTrack(t.id))
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
        w::suggested_circular_button(Icon::Play.view(16.0))
            .on_press_maybe((!ids.is_empty()).then_some(Message::PlayAll(ids)))
            .into()
    }

    /// Tracks and Recently played: the play button, then the long list.
    fn track_page(&self) -> Element<'_> {
        column![
            container(
                row![
                    self.play_all(),
                    ty::caption(format!("{} tracks", self.rows.len()))
                ]
                .spacing(12)
                .align_y(Alignment::Center)
            )
            .padding([8, 16]),
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
            container(ty::caption(format!("{} tracks", self.rows.len()))).padding([8, 16]),
            self.virtual_tracks(RowOpts {
                album: true,
                bpm: false,
                menu: true
            }),
        ]
        .height(Length::Fill);
        if self.is_narrow() {
            column![container(self.search_entry()).padding([8, 16]), list].into()
        } else {
            list.into()
        }
    }

    // ---- grids ----------------------------------------------------------------------------

    /// Cards that fit the width; in compact mode (as in a music app's compact library) the same
    /// entries are tight rows, several to a line, with a small picture and the name beside it.
    fn grid<'a>(&self, items: Vec<Element<'a>>, card_width: f32) -> Element<'a> {
        w::CardGrid::new(items)
            .max_width(if self.compact { 280.0 } else { card_width })
            .spacing(12.0)
            .into()
    }

    /// One compact entry: a small picture, the name, and a line of detail.
    fn compact_entry<'a>(
        &self,
        picture: Element<'a>,
        name: String,
        sub: String,
        on_press: Message,
    ) -> Element<'a> {
        w::MediaRow::new(name)
            .subtitle(sub)
            .picture(picture)
            .on_press(on_press)
            .into()
    }

    fn album_card(&self, i: usize, show_artist: bool) -> Element<'_> {
        let (name, artist, n) = self.lib.albums()[i].clone();
        let year = crate::data::Library::album_year(&name, &artist);
        let sub = if show_artist {
            format!("{artist} · {year}")
        } else {
            format!("{year} · {n} tracks")
        };
        if self.compact {
            return self.compact_entry(cover(i, 36.0), name, sub, Message::Open(Dest::Album(i)));
        }
        w::card_button(
            column![
                container(cover(i, 88.0)).center_x(Length::Fill),
                ty::heading(name),
                ty::caption(sub)
            ]
            .spacing(4)
            .width(Length::Fill),
        )
        .on_press(Message::Open(Dest::Album(i)))
        .width(Length::Fill)
        .into()
    }

    fn playlist_card(&self, i: usize) -> Element<'_> {
        let p = &self.lib.playlists[i];
        let n = self.lib.playlist_tracks(p).len();
        let kind = if p.filter.is_some() {
            "dynamic"
        } else {
            "manual"
        };
        if self.compact {
            return self.compact_entry(
                playlist_avatar(&p.name, p.icon, 36.0),
                p.name.clone(),
                format!("{kind} · {n} tracks"),
                Message::Open(Dest::Playlist(i)),
            );
        }
        w::card_button(
            row![
                playlist_avatar(&p.name, p.icon, 40.0),
                column![
                    ty::heading(p.name.clone()),
                    ty::caption(format!(
                        "{} · {n} tracks",
                        if p.filter.is_some() {
                            "dynamic"
                        } else {
                            "manual"
                        }
                    )),
                ]
                .spacing(2),
            ]
            .spacing(10)
            .align_y(Alignment::Center)
            .width(Length::Fill),
        )
        .on_press(Message::Open(Dest::Playlist(i)))
        .width(Length::Fill)
        .into()
    }

    fn artist_card(&self, i: usize, with_plays: Option<u32>) -> Element<'_> {
        let a = self.lib.artists()[i].0.clone();
        let sub = match with_plays {
            Some(p) => format!("{p} plays"),
            None => format!("{} tracks", self.lib.artists()[i].1),
        };
        if self.compact {
            return self.compact_entry(
                w::avatar(36.0, a.clone()).into(),
                a,
                sub,
                Message::Open(Dest::Artist(i)),
            );
        }
        w::flat_button(
            column![w::avatar(56.0, a.clone()), ty::heading(a), ty::caption(sub)]
                .spacing(4)
                .align_x(Alignment::Center)
                .width(Length::Fill),
        )
        .on_press(Message::Open(Dest::Artist(i)))
        .width(Length::Fill)
        .into()
    }

    fn artists(&self) -> Element<'_> {
        scrollable(
            container(
                self.grid(
                    (0..self.lib.artists().len())
                        .map(|i| self.artist_card(i, None))
                        .collect(),
                    130.0,
                ),
            )
            .padding(16),
        )
        .height(Length::Fill)
        .into()
    }

    fn albums(&self) -> Element<'_> {
        scrollable(
            container(
                self.grid(
                    (0..self.lib.albums().len())
                        .map(|i| self.album_card(i, true))
                        .collect(),
                    130.0,
                ),
            )
            .padding(16),
        )
        .height(Length::Fill)
        .into()
    }

    fn playlists(&self) -> Element<'_> {
        scrollable(
            container(
                self.grid(
                    (0..self.lib.playlists.len())
                        .map(|i| self.playlist_card(i))
                        .collect(),
                    260.0,
                ),
            )
            .padding(16),
        )
        .height(Length::Fill)
        .into()
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
        let artists = self.grid(
            self.lib
                .recent_artists(8)
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
            section_all("Recent playlists", Dest::Playlists, playlists),
            section_all("Recently played", Dest::Recent, self.short_list(&recent)),
            section_all("Recent albums", Dest::Albums, albums),
            section_all("Recent artists", Dest::Artists, artists),
        ]
        .spacing(20)
        .padding(16);
        scrollable(page).height(Length::Fill).into()
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
        let top_artists = self.grid(
            self.lib
                .most_listened_artists(6)
                .into_iter()
                .map(|(i, plays)| self.artist_card(i, Some(plays)))
                .collect(),
            130.0,
        );
        let might_like = self.grid(
            (0..self.lib.artists().len())
                .skip(2)
                .take(6)
                .map(|i| self.artist_card(i, None))
                .collect(),
            130.0,
        );
        let link = |label: &'static str, icon: Element<'static>, to: Dest| -> Element<'_> {
            w::card_button(
                row![
                    icon,
                    ty::heading(label),
                    space::horizontal(),
                    Icon::GoNext.view(14.0)
                ]
                .spacing(10)
                .align_y(Alignment::Center)
                .width(Length::Fill),
            )
            .on_press(Message::Open(to))
            .width(Length::Fill)
            .into()
        };
        let links = w::CardGrid::new(vec![
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
        let mut rediscover = column![].spacing(12);
        if !while_tracks.is_empty() {
            rediscover = rediscover.push(
                column![
                    ty::heading("It's been a while"),
                    ty::caption(format!(
                        "Rated highly, not played for more than {} days (change it in Settings)",
                        self.while_days
                    )),
                    self.short_list(&while_tracks),
                ]
                .spacing(4),
            );
        }
        rediscover = rediscover
            .push(section("Your most listened artists", top_artists))
            .push(section(
                "Your most listened tracks",
                self.short_list(&top_tracks),
            ));
        let page = column![
            links,
            row![ty::heading("Usage"), space::horizontal(), days].align_y(Alignment::Center),
            usage,
            ty::caption("Sample data: Explore needs play and jam history from the engine."),
            ty::title_3("Rediscover"),
            rediscover,
            ty::title_3("Explore"),
            section("You might like", might_like),
        ]
        .spacing(20)
        .padding(16);
        scrollable(page).height(Length::Fill).into()
    }

    // ---- detail pages -----------------------------------------------------------------------

    /// Options of a detail page: the playlist's filter (if dynamic) and the tags of all its tracks.
    fn options_menu(&self, dynamic: bool) -> Element<'_> {
        let mut items = Vec::new();
        if dynamic {
            items.push(MenuItem::action("Edit filter", Message::EditFilter).icon(Icon::Filter));
        }
        items.push(MenuItem::action("Edit tags of all tracks", Message::BulkOpen).icon(Icon::Edit));
        self.menu_button(
            Icon::ViewMore,
            "Options",
            Menu::new("Options").section(items),
        )
    }

    /// "List" at the far end: the same compact setting as the global one.
    fn view_menu(&self) -> Element<'_> {
        self.menu_button(
            Icon::OpenMenu,
            "View",
            Menu::new("View")
                .check_style(w::MenuCheckStyle::Mark)
                .section([
                    MenuItem::check("List", !self.compact, Message::ToggleCompact),
                    MenuItem::check("Compact", self.compact, Message::ToggleCompact),
                ]),
        )
    }

    fn detail_page(&self) -> Element<'_> {
        let narrow = self.is_narrow();
        let big = 200.0;
        let description: Element<'_> = w::EditableText::new(
            self.desc_key()
                .and_then(|k| self.descriptions.get(&k))
                .map_or("", String::as_str),
            Message::Describe,
        )
        .placeholder("Add a description")
        .editing(self.editing_desc)
        .expanded(self.desc_expanded)
        .max_height(44.0)
        .on_edit(Message::EditDescription(true))
        .on_done(Message::EditDescription(false))
        .on_expand(Message::DescExpanded)
        .into();
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
                let year = crate::data::Library::album_year(&album, &artist);
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
        let mut header = w::DetailHeader::new(title)
            .kind(kind)
            .cover(cover_el)
            .description(description)
            .facts(facts)
            .primary(self.play_all())
            .end(self.view_menu())
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
                page = page.push(
                    container(section(
                        "Popular albums",
                        self.grid(
                            popular
                                .into_iter()
                                .map(|a| self.album_card(a, false))
                                .collect(),
                            130.0,
                        ),
                    ))
                    .padding([0, 16])
                    .width(Length::Fill),
                );
            }
            if !latest.is_empty() {
                page = page.push(
                    container(section(
                        "Latest albums",
                        self.grid(
                            latest
                                .into_iter()
                                .map(|a| self.album_card(a, false))
                                .collect(),
                            130.0,
                        ),
                    ))
                    .padding([0, 16])
                    .width(Length::Fill),
                );
            }
            page = page.push(container(ty::heading("All tracks")).padding([0, 16]));
        }
        page = page.push(container(self.plain_list(&self.rows, o)).padding([0, 8]));
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

    /// The current page, without the layout around it.
    pub fn page(&self) -> Element<'_> {
        if self.jam_open {
            return self.jam_page();
        }
        if self.searching {
            return self.results_page();
        }
        match self.dest {
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
