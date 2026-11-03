//! The views and the two layouts. A view only renders state and fires messages; a layout only
//! arranges views and triggers. Navigation state lives in `App`, so switching layout loses
//! nothing (docs/ui/layout.md).

use iad::adaptive::adaptive;
use iad::widget::{
    self as w, Column, Menu, MenuItem, Popover, ReorderableList, ResponsiveTable, SidebarItem,
    SidebarList, SidebarPadding, SidebarStyle, ToggleItem, typography as ty,
};
use iad::window::{Chrome, State as WindowState};
use iad::{Icon, style};
use iced::widget::{
    column, container, mouse_area, row, scrollable, space, stack, text, text_input,
};
use iced::{Alignment, Background, Color, Length};
use iced_adaptive_design as iad;

use crate::components::cover::cover;
use crate::components::detail_header::detail_header;
use crate::components::editable_text::{Messages, editable_text};
use crate::components::histogram_range::histogram_range;
use crate::components::play_button::play_button;
use crate::components::playlist_icon::playlist_icon;
use crate::components::synced_text::synced_text;
use crate::components::tag_badge::{tag_chip, tag_pill};
use crate::components::transport::glyph_button;
use crate::data::{self, Filter, Track, clock};
use crate::icons::Glyph;
use crate::{App, Col, Dest, Element, LYRICS, Layout, Message, Mode3, PRESETS};

const SIDEBAR: f32 = 240.0;

fn card<'a>(content: impl Into<Element<'a>>) -> Element<'a> {
    container(content)
        .padding(12)
        .style(style::container::card)
        .into()
}

fn section<'a>(title: &'a str, body: impl Into<Element<'a>>) -> Element<'a> {
    column![ty::heading(title), body.into()].spacing(8).into()
}

fn wrap<'a>(items: Vec<Element<'a>>) -> Element<'a> {
    row(items).spacing(12).wrap().into()
}

/// The menu of a track: queue it, edit it, go to its album or artist.
fn track_menu<'a>(id: usize) -> Element<'a> {
    w::menu_button(
        Icon::ViewMore,
        Menu::new("Track")
            .section([
                MenuItem::action("Play next", Message::PlayNext(id)),
                MenuItem::action("Add to queue", Message::AddToQueue(id)),
            ])
            .section([
                MenuItem::action("Edit tags and rating…", Message::EditTrack(id)).icon(Icon::Edit),
                MenuItem::action("View album", Message::ViewAlbum(id)),
                MenuItem::action("View artist", Message::ViewArtist(id)),
            ]),
    )
    .tooltip("Track options")
    .into()
}

impl App {
    pub fn view(&self) -> Element<'_> {
        let body = adaptive(&self.adaptive)
            .layout(Layout::Narrow, || self.narrow())
            .layout(Layout::Wide, || self.wide());
        let framed = self.chrome(true, true).frame(body, Message::Window);
        // The queue is a full-height sheet that slides up over the whole window.
        let framed: Element<'_> = if self.queue_open {
            w::Overlay::new(framed, self.queue_sheet())
                .docked(w::Side::Bottom, w::Dim::Fill)
                .scrim(true)
                .on_outside(Message::ToggleQueue)
                .into()
        } else {
            framed
        };
        // The playlist filter and track edit dialogs are modal and cannot be dismissed from outside: it is
        // closed by its Cancel or Save.
        w::present_dialog(
            framed,
            self.draft
                .as_ref()
                .map(|f| self.filter_dialog(f))
                .or_else(|| self.edit_track.as_ref().map(|e| self.track_dialog(e)))
                .or_else(|| self.bulk.as_ref().map(|b| self.bulk_dialog(b))),
            None,
            0.0,
        )
    }

    fn chrome(&self, left: bool, right: bool) -> Chrome {
        Chrome::new(WindowState {
            maximized: false,
            fullscreen: false,
        })
        .edges(left, right)
    }

    fn bar(&self, left: bool, right: bool) -> w::HeaderBar<'_, Message> {
        w::header_bar().chrome(&self.chrome(left, right), Message::Window)
    }

    // ---- triggers ---------------------------------------------------------------------

    pub fn page_title(&self) -> String {
        if self.searching {
            return "Search".into();
        }
        match self.dest {
            Dest::Home => "Home".into(),
            Dest::Recent => "Recently played".into(),
            Dest::Tracks => "Tracks".into(),
            Dest::Artists => "Artists".into(),
            Dest::Albums => "Albums".into(),
            Dest::Playlists => "Playlists".into(),
            Dest::Playlist(i) => self.lib.playlists[i].name.into(),
            Dest::Artist(i) => self.lib.artists()[i].0.clone(),
            Dest::Album(i) => self.lib.albums()[i].0.clone(),
        }
    }

    fn filter_badge(&self) -> Element<'_> {
        let n = self.filter.count();
        let label = if n == 0 {
            "Filters".to_string()
        } else {
            format!("Filters · {n}")
        };
        row![Icon::Filter.view(16.0), text(label)]
            .spacing(6)
            .align_y(Alignment::Center)
            .into()
    }

    /// The filter button: the filter page in a popover, applying live as it is edited.
    fn filter_button(&self) -> Element<'_> {
        // The popover supplies the placement; the surface (background, border, padding) is ours.
        let surface = container(self.filter_page(false))
            .padding(12)
            .style(style::container::popover);
        Popover::new(self.filter_badge(), surface)
            .popover_width(400.0)
            .open_initially(std::env::var("WEAVE_GUI_START").as_deref() == Ok("popover"))
            .into()
    }

    fn search_entry(&self) -> Element<'_> {
        row![
            text_input("Search tracks, artists, albums", &self.query)
                .on_input(Message::SearchText)
                .width(Length::Fill),
            self.filter_button(),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
    }

    // ---- views ------------------------------------------------------------------------

    /// The tempo: the distribution of the tracks under the handles (press a bar to take its
    /// range), the exact handles, and the usual bands as presets.
    fn bpm_section<'a>(
        &'a self,
        filter: &'a Filter,
        on_bpm: fn((f32, f32)) -> Message,
    ) -> Element<'a> {
        const BINS: usize = 12;
        let (lo, hi) = filter.bpm;
        let bins = self.lib.bpm_histogram(filter, BINS);
        let band = |label: &'static str, r: (f32, f32)| -> Element<'a> {
            let b = w::flat_button(ty::label(label)).on_press(on_bpm(r));
            if filter.bpm == r {
                w::suggested_button(ty::label(label))
                    .on_press(on_bpm(r))
                    .into()
            } else {
                b.into()
            }
        };
        column![
            row![
                ty::heading("Tempo"),
                space::horizontal(),
                ty::body(if filter.bpm == data::BPM {
                    "any".to_string()
                } else {
                    format!("{lo:.0}–{hi:.0} BPM")
                }),
            ]
            .align_y(Alignment::Center),
            histogram_range(&bins, data::BPM.0, data::BPM.1, (lo, hi), 44.0, on_bpm),
            w::range_slider(data::BPM.0..=data::BPM.1, (lo, hi), on_bpm)
                .step(5.0)
                .min_gap(5.0),
            row![
                band("Any", data::BPM),
                band("Slow · under 90", (60.0, 90.0)),
                band("Mid · 90–120", (90.0, 120.0)),
                band("Fast · 120+", (120.0, 180.0)),
            ]
            .spacing(6)
            .wrap(),
        ]
        .spacing(6)
        .into()
    }

    /// The fields of a filter: the tags as toggles you can narrow by typing, a BPM range and the
    /// match count. Shared by the search popover, the jam sheet and the playlist dialog, each
    /// with its own filter. A tag also matches the tags under it.
    fn filter_fields<'a>(
        &'a self,
        filter: &'a Filter,
        height: f32,
        on_toggle: fn(String) -> Message,
        on_bpm: fn((f32, f32)) -> Message,
    ) -> iced::widget::Column<'a, Message, iad::Theme> {
        let n = self.lib.filtered(filter).len();
        let q = self.tag_query.trim().to_lowercase();
        let chips = self
            .lib
            .tag_names()
            .into_iter()
            .filter(|t| q.is_empty() || t.to_lowercase().contains(&q))
            .map(|t| {
                let on = filter.tags.contains(&t);
                tag_chip(&t, &self.lib.style_of(&t), on, on_toggle(t.clone()))
            });
        column![
            text_input("Find a tag", &self.tag_query).on_input(Message::TagQuery),
            scrollable(row(chips).spacing(6).wrap()).height(Length::Fixed(height)),
            self.bpm_section(filter, on_bpm),
            ty::caption(format!("{n} matching tracks")),
        ]
        .spacing(10)
    }

    /// The filter page of search and jam: one state, in a popover or a sheet.
    fn filter_page(&self, jam: bool) -> Element<'_> {
        let n = self.lib.filtered(&self.filter).len();
        let clear = w::flat_button(ty::label("Clear"))
            .on_press_maybe((!self.filter.is_empty()).then_some(Message::ClearFilter));
        if !jam {
            return self
                .filter_fields(&self.filter, 120.0, Message::ToggleTag, Message::Bpm)
                .push(row![space::horizontal(), clear])
                .into();
        }
        let wide = matches!(self.adaptive.shown().last(), Some(Layout::Wide));
        let top = row![
            w::flat_button(ty::label("🎲 Surprise me")).on_press(Message::Surprise),
            space::horizontal(),
            clear,
        ]
        .align_y(Alignment::Center);
        // Filters are open by default; the header folds them away on request.
        let open = self.jam_filters_open;
        let active = self.filter.count();
        let header = w::flat_button(
            row![
                ty::heading(if active == 0 {
                    "Filters".to_string()
                } else {
                    format!("Filters · {active}")
                }),
                space::horizontal(),
                ty::caption(format!("{n} matches")),
                if open { Icon::GoUp } else { Icon::GoDown }.view(16.0),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .on_press(Message::JamFilters(!open));
        let filters = w::Collapsible::new(
            header,
            self.filter_fields(
                &self.filter,
                if wide { 110.0 } else { 64.0 },
                Message::ToggleTag,
                Message::Bpm,
            ),
            open,
        );
        // The same round play button as a track list, right after the filters.
        let play = play_button((n > 0).then_some(Message::StartJam));
        if wide {
            // Filters and play on the left, what they match on the right.
            row![
                column![top, filters, play]
                    .spacing(10)
                    .width(Length::FillPortion(1)),
                column![ty::heading("Matches"), self.jam_list()]
                    .spacing(8)
                    .width(Length::FillPortion(1)),
            ]
            .spacing(24)
            .height(Length::Fill)
            .into()
        } else {
            column![top, filters, play, self.jam_list()]
                .spacing(8)
                .height(Length::Fill)
                .into()
        }
    }

    /// The tracks a jam would play, as a virtualised list: only the rows in view are built.
    fn jam_list(&self) -> Element<'_> {
        ResponsiveTable::new(&self.jam_window)
            .column(
                Column::new("Title", |t: &Track| ty::body(t.title.clone()).into()).min_width(120.0),
            )
            .column(
                Column::new("Artist", |t: &Track| ty::caption(t.artist.clone()).into())
                    .min_width(100.0),
            )
            .column(
                Column::new("BPM", |t: &Track| {
                    ty::caption(format!("{:.0}", t.bpm)).into()
                })
                .width(Length::Fixed(48.0))
                .min_width(48.0),
            )
            .mode(w::TableMode::Table)
            .virtualized(self.jam_total, self.jam_first, crate::JAM_ROW)
            .on_scroll(Message::JamScroll)
            .into()
    }

    /// Edit the tags of every track on the page: delete or replace a tag where it matches, and
    /// add tags to all of them. Modal: closed by Cancel or Save.
    fn bulk_dialog<'a>(&'a self, b: &'a crate::BulkEdit) -> Element<'a> {
        let q = self.tag_query.trim().to_lowercase();
        let names = || {
            self.lib
                .tag_names()
                .into_iter()
                .filter(|n| q.is_empty() || n.to_lowercase().contains(&q))
        };
        let existing: Vec<Element<'a>> = b
            .rows
            .iter()
            .map(|r| {
                let tag = r.tag.clone();
                let modes = w::toggle_group(
                    vec![
                        ToggleItem::new(Mode3::Keep, "Keep"),
                        ToggleItem::new(Mode3::Delete, "Delete"),
                        ToggleItem::new(Mode3::Replace, "Replace with"),
                    ],
                    Some(r.mode),
                    {
                        let tag = tag.clone();
                        move |m| Message::BulkMode(tag.clone(), m)
                    },
                );
                let mut line = column![
                    row![
                        tag_pill(&r.tag, &self.lib.style_of(&r.tag), 18.0),
                        ty::caption(format!("on {} of {}", r.count, b.ids.len())),
                        space::horizontal(),
                        modes,
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center)
                ]
                .spacing(6);
                if r.mode == Mode3::Replace {
                    let chips = names().map(|n| {
                        let on = r.target.as_deref() == Some(n.as_str());
                        tag_chip(
                            &n,
                            &self.lib.style_of(&n),
                            on,
                            Message::BulkTarget(r.tag.clone(), n.clone()),
                        )
                    });
                    line = line.push(row(chips).spacing(6).wrap());
                }
                line.into()
            })
            .collect();
        let add = names().map(|n| {
            let on = b.add.contains(&n);
            tag_chip(
                &n,
                &self.lib.style_of(&n),
                on,
                Message::BulkAddToggle(n.clone()),
            )
        });
        container(
            column![
                ty::title_3(format!("Tags · {}", b.title)),
                ty::caption(format!("Applies to all {} tracks.", b.ids.len())),
                text_input("Find a tag", &self.tag_query).on_input(Message::TagQuery),
                scrollable(
                    column![
                        ty::heading("Already on these tracks"),
                        column(existing).spacing(10),
                        ty::heading("Add to every track"),
                        row(add).spacing(6).wrap(),
                    ]
                    .spacing(10)
                    .padding(iced::Padding {
                        right: 12.0,
                        ..iced::Padding::ZERO
                    }),
                )
                .height(Length::Fixed(340.0)),
                row![
                    space::horizontal(),
                    w::flat_button(ty::label("Cancel")).on_press(Message::BulkCancel),
                    w::suggested_button(ty::label("Save")).on_press(Message::BulkSave),
                ]
                .spacing(8),
            ]
            .spacing(10),
        )
        .padding(20)
        .width(Length::Fixed(620.0))
        .style(iad::style::container::popover)
        .into()
    }

    /// Edit a track's rating and tags. Modal: closed by Cancel or Save.
    fn track_dialog<'a>(&'a self, e: &'a crate::TrackEdit) -> Element<'a> {
        let t = self.track(e.id);
        let q = self.tag_query.trim().to_lowercase();
        let chips = self
            .lib
            .tag_names()
            .into_iter()
            .filter(|n| q.is_empty() || n.to_lowercase().contains(&q))
            .map(|n| {
                let on = e.tags.contains(&n);
                tag_chip(
                    &n,
                    &self.lib.style_of(&n),
                    on,
                    Message::EditToggleTag(n.clone()),
                )
            });
        let stars = row((1..=5u8).map(|i| {
            w::flat_button(ty::heading(if i <= e.rating { "★" } else { "☆" }))
                .on_press(Message::EditRating(i))
                .into()
        }))
        .spacing(2);
        container(
            column![
                ty::title_3(t.title.clone()),
                ty::caption(format!("{} · {}", t.artist, t.album)),
                ty::heading("Rating"),
                stars,
                ty::heading("Tags"),
                text_input("Find a tag", &self.tag_query).on_input(Message::TagQuery),
                scrollable(row(chips).spacing(6).wrap()).height(Length::Fixed(150.0)),
                row![
                    space::horizontal(),
                    w::flat_button(ty::label("Cancel")).on_press(Message::CancelTrack),
                    w::suggested_button(ty::label("Save")).on_press(Message::SaveTrack),
                ]
                .spacing(8),
            ]
            .spacing(10),
        )
        .padding(20)
        .width(Length::Fixed(480.0))
        .style(iad::style::container::popover)
        .into()
    }

    /// The rules of a dynamic playlist. Modal and not dismissable: Cancel or Save.
    fn filter_dialog<'a>(&'a self, draft: &'a Filter) -> Element<'a> {
        container(
            column![
                ty::title_3("Playlist filter"),
                ty::caption("The playlist holds every track that matches these rules, now and as the library changes."),
                self.filter_fields(draft, 180.0, Message::DraftToggle, Message::DraftBpm),
                row![
                    space::horizontal(),
                    w::flat_button(ty::label("Cancel")).on_press(Message::CancelFilter),
                    w::suggested_button(ty::label("Save")).on_press(Message::SaveFilter),
                ]
                .spacing(8),
            ]
            .spacing(12),
        )
        .padding(20)
        .width(Length::Fixed(480.0))
        .style(iad::style::container::popover)
        .into()
    }

    /// The jam sheet: over the content area only, so the player bar stays visible.
    fn with_jam<'a>(&'a self, content: Element<'a>) -> Element<'a> {
        if !self.jam_open {
            return content;
        }
        let sheet = container(
            column![
                row![
                    ty::title_3("Jam"),
                    ty::caption("matching tracks play shuffled and it keeps going"),
                    space::horizontal(),
                    w::flat_button(Icon::WindowClose.view(16.0)).on_press(Message::CloseJam),
                ]
                .spacing(12)
                .align_y(Alignment::Center),
                self.filter_page(true),
            ]
            .spacing(8)
            .max_width(980)
            .height(Length::Fill),
        )
        .padding(20)
        .center_x(Length::Fill)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(iad::style::container::window);
        // Swallow clicks on the sheet so they do not reach the page below.
        stack![content, mouse_area(sheet)].into()
    }

    fn home(&self) -> Element<'_> {
        let playlists = wrap(
            self.lib
                .playlists
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let n = self.lib.playlist_tracks(p).len();
                    w::card_button(
                        row![
                            playlist_icon(p.name, p.icon, 40.0),
                            column![
                                ty::heading(p.name),
                                ty::caption(format!(
                                    "{} · {n} tracks",
                                    if p.filter.is_some() {
                                        "dynamic"
                                    } else {
                                        "manual"
                                    }
                                )),
                            ]
                            .spacing(2)
                        ]
                        .spacing(10)
                        .align_y(Alignment::Center)
                        .width(Length::Fixed(210.0)),
                    )
                    .on_press(Message::Open(Dest::Playlist(i)))
                    .into()
                })
                .collect(),
        );
        let jams = wrap(
            PRESETS
                .iter()
                .enumerate()
                .map(|(i, (name, _, _))| {
                    w::flat_button(
                        row![Glyph::Jam.view(16.0), ty::label(*name)]
                            .spacing(6)
                            .align_y(Alignment::Center),
                    )
                    .on_press(Message::Preset(i))
                    .into()
                })
                .collect(),
        );
        let artists = wrap(
            self.lib
                .artists()
                .into_iter()
                .enumerate()
                .take(8)
                .map(|(i, (a, _))| {
                    w::flat_button(
                        column![w::avatar(48.0, a.clone()), ty::caption(a.clone())]
                            .spacing(4)
                            .align_x(Alignment::Center)
                            .width(Length::Fixed(88.0)),
                    )
                    .on_press(Message::Open(Dest::Artist(i)))
                    .into()
                })
                .collect(),
        );
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
                ToggleItem::new(7u8, "7 days"),
                ToggleItem::new(30u8, "30 days"),
            ],
            Some(self.days),
            Message::Days,
        );
        scrollable(
            column![
                section("Recent jams", jams),
                section("Playlists", playlists),
                section("Artists", artists),
                row![ty::heading("Usage"), space::horizontal(), days].align_y(Alignment::Center),
                usage,
                ty::caption("Sample data: Home needs play and jam history from the engine."),
            ]
            .spacing(20)
            .padding(16),
        )
        .height(Length::Fill)
        .into()
    }

    fn tracks_table(&self) -> Element<'_> {
        let stars = |n: u8| "★".repeat(n as usize) + &"☆".repeat(5 - n as usize);
        // Every cell is as tall as the row and centred in it, so text lines up across columns.
        let cell = |e: Element<'static>| -> Element<'static> {
            container(e)
                .height(Length::Fixed(40.0))
                .align_y(Alignment::Center)
                .into()
        };
        ResponsiveTable::new(&self.rows)
            .column(
                Column::new("Title", move |t: &Track| {
                    cell(
                        w::flat_button(ty::body(t.title.clone()))
                            .padding(0)
                            .on_press(Message::PlayTrack(t.id))
                            .into(),
                    )
                })
                .sortable(Col::Title)
                .min_width(120.0),
            )
            .column(
                Column::new("Artist", move |t: &Track| {
                    cell(ty::body(t.artist.clone()).into())
                })
                .sortable(Col::Artist)
                .min_width(100.0),
            )
            .column(
                Column::new("Album", move |t: &Track| {
                    cell(ty::body(t.album.clone()).into())
                })
                .sortable(Col::Album)
                .min_width(100.0),
            )
            .column(
                Column::new("BPM", move |t: &Track| {
                    cell(ty::body(format!("{:.0}", t.bpm)).into())
                })
                .sortable(Col::Bpm)
                .width(Length::Fixed(60.0))
                .min_width(60.0),
            )
            .column(
                Column::new("Rating", move |t: &Track| {
                    cell(ty::body(stars(t.rating)).into())
                })
                .sortable(Col::Rating)
                .width(Length::Fixed(92.0))
                .min_width(92.0),
            )
            .column(
                Column::new("", move |t: &Track| cell(track_menu(t.id)))
                    .width(Length::Fixed(44.0))
                    .min_width(44.0),
            )
            .card(|t: &Track| {
                row![
                    w::flat_button(
                        row![
                            cover(t.id, 44.0),
                            column![
                                ty::body(t.title.clone()),
                                ty::caption(format!("{} · {}", t.artist, t.album)),
                            ]
                            .width(Length::Fill),
                            ty::caption(format!("{:.0} BPM", t.bpm)),
                        ]
                        .spacing(12)
                        .align_y(Alignment::Center),
                    )
                    .on_press(Message::PlayTrack(t.id))
                    .width(Length::Fill),
                    track_menu(t.id),
                ]
                .align_y(Alignment::Center)
                .into()
            })
            .mode(
                if matches!(self.adaptive.shown().last(), Some(Layout::Narrow)) {
                    w::TableMode::Cards
                } else {
                    w::TableMode::Table
                },
            )
            .sort(&self.sort)
            .on_sort(Message::Sort)
            .into()
    }

    /// The play circle, just above the tracks it plays.
    fn play_row(&self) -> Element<'_> {
        let ids: Vec<usize> = self.rows.iter().map(|t| t.id).collect();
        container(play_button(
            (!ids.is_empty()).then_some(Message::PlayAll(ids)),
        ))
        .padding([8, 16])
        .into()
    }

    /// A page of tracks: what is listed is what the play circle plays.
    fn track_page(&self) -> Element<'_> {
        column![
            container(ty::caption(format!("{} tracks", self.rows.len()))).padding([8, 16]),
            self.play_row(),
            self.tracks_table(),
        ]
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

    /// A detail page (playlist, artist, album): title, a space for a description, the facts,
    /// then the play circle over the tracks.
    fn detail_page(&self) -> Element<'_> {
        // Clamped to two lines; expanding pushes the rest of the page down.
        let description: Element<'_> = editable_text(
            self.desc_key()
                .and_then(|k| self.descriptions.get(&k))
                .map_or("", String::as_str),
            "Add a description",
            self.editing_desc,
            self.desc_expanded,
            44.0,
            Messages {
                on_edit: Message::EditDescription(true),
                on_input: Box::new(Message::Describe),
                on_done: Message::EditDescription(false),
                on_expand: Box::new(Message::DescExpanded),
            },
        );
        let (leading, title, facts, trailing): (
            Option<Element<'_>>,
            String,
            Element<'_>,
            Option<Element<'_>>,
        ) = match self.dest {
            Dest::Playlist(i) => {
                let p = &self.lib.playlists[i];
                let (extra, menu) = match &p.filter {
                    Some(f) => (
                        Some(self.filter_sentence(f)),
                        Some(
                            w::menu_button(
                                Icon::Settings,
                                Menu::new("Playlist").section([MenuItem::action(
                                    "Edit filter…",
                                    Message::EditFilter,
                                )]),
                            )
                            .tooltip("Playlist settings")
                            .into(),
                        ),
                    ),
                    None => (None, None),
                };
                (
                    Some(playlist_icon(p.name, p.icon, 88.0)),
                    p.name.to_string(),
                    self.facts(extra),
                    menu,
                )
            }
            Dest::Artist(i) => {
                let name = self.lib.artists()[i].0.clone();
                (
                    Some(w::avatar(88.0, name.clone()).into()),
                    name,
                    self.facts(None),
                    Some(self.bulk_menu()),
                )
            }
            Dest::Album(i) => {
                let (album, artist, _) = self.lib.albums()[i].clone();
                (
                    Some(cover(i, 96.0)),
                    album,
                    self.facts(Some(artist)),
                    Some(self.bulk_menu()),
                )
            }
            _ => unreachable!("detail_page is for playlists, artists and albums"),
        };
        column![
            container(detail_header(leading, title, description, facts, trailing)).padding(16),
            self.play_row(),
            self.tracks_table(),
        ]
        .into()
    }

    /// The settings menu of an album or artist page.
    fn bulk_menu(&self) -> Element<'_> {
        w::menu_button(
            Icon::Settings,
            Menu::new("Options").section([MenuItem::action(
                "Edit tags of all tracks…",
                Message::BulkOpen,
            )
            .icon(Icon::Edit)]),
        )
        .tooltip("Options")
        .into()
    }

    /// A dynamic playlist's filter as a sentence: "bachata · 60–120 BPM".
    fn filter_sentence(&self, f: &Filter) -> String {
        let mut parts: Vec<String> = f
            .tags
            .iter()
            .map(|t| t.split_once(':').map_or(t.as_str(), |(_, v)| v).to_string())
            .collect();
        if f.bpm != data::BPM {
            parts.push(format!("{:.0}–{:.0} BPM", f.bpm.0, f.bpm.1));
        }
        if parts.is_empty() {
            "everything".into()
        } else {
            parts.join(" · ")
        }
    }

    fn artists(&self) -> Element<'_> {
        scrollable(
            container(wrap(
                self.lib
                    .artists()
                    .into_iter()
                    .enumerate()
                    .map(|(i, (a, n))| {
                        w::card_button(
                            column![
                                w::avatar(56.0, a.clone()),
                                ty::heading(a.clone()),
                                ty::caption(format!("{n} tracks")),
                            ]
                            .spacing(4)
                            .align_x(Alignment::Center)
                            .width(Length::Fixed(128.0)),
                        )
                        .on_press(Message::Open(Dest::Artist(i)))
                        .into()
                    })
                    .collect(),
            ))
            .padding(16),
        )
        .height(Length::Fill)
        .into()
    }

    fn albums(&self) -> Element<'_> {
        scrollable(
            container(wrap(
                self.lib
                    .albums()
                    .into_iter()
                    .enumerate()
                    .map(|(i, (a, artist, n))| {
                        w::card_button(
                            column![
                                cover(i, 128.0),
                                ty::heading(a),
                                ty::caption(format!("{artist} · {n} tracks")),
                            ]
                            .spacing(4)
                            .width(Length::Fixed(144.0)),
                        )
                        .on_press(Message::Open(Dest::Album(i)))
                        .into()
                    })
                    .collect(),
            ))
            .padding(16),
        )
        .height(Length::Fill)
        .into()
    }

    fn playlists(&self) -> Element<'_> {
        scrollable(
            container(wrap(
                self.lib
                    .playlists
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        w::card_button(
                            row![
                                playlist_icon(p.name, p.icon, 40.0),
                                column![
                                    ty::heading(p.name),
                                    ty::caption(if p.filter.is_some() {
                                        "dynamic"
                                    } else {
                                        "manual"
                                    }),
                                ]
                                .spacing(2)
                            ]
                            .spacing(10)
                            .align_y(Alignment::Center)
                            .width(Length::Fixed(210.0)),
                        )
                        .on_press(Message::Open(Dest::Playlist(i)))
                        .into()
                    })
                    .collect(),
            ))
            .padding(16),
        )
        .height(Length::Fill)
        .into()
    }

    /// The current page, without the layout around it.
    fn page(&self, search_entry: bool) -> Element<'_> {
        if self.now_playing {
            return self.now_playing_view();
        }
        if self.searching {
            let results = self.track_page();
            return if search_entry {
                column![container(self.search_entry()).padding([8, 16]), results].into()
            } else {
                results
            };
        }
        match self.dest {
            Dest::Home => self.home(),
            Dest::Recent | Dest::Tracks => self.track_page(),
            Dest::Playlist(_) | Dest::Artist(_) | Dest::Album(_) => self.detail_page(),
            Dest::Artists => self.artists(),
            Dest::Albums => self.albums(),
            Dest::Playlists => self.playlists(),
        }
    }

    // ---- player -----------------------------------------------------------------------

    fn play_pause(&self, size: f32) -> Element<'_> {
        let g = if self.player.playing {
            Glyph::Pause
        } else {
            Glyph::Play
        };
        w::suggested_button(g.view(size))
            .on_press(Message::TogglePlay)
            .into()
    }

    fn controls(&self, size: f32) -> Element<'_> {
        row![
            glyph_button(Glyph::Previous, size, false, Message::Previous),
            self.play_pause(size),
            glyph_button(Glyph::Next, size, false, Message::Next),
        ]
        .spacing(4)
        .align_y(Alignment::Center)
        .into()
    }

    fn seek(&self, t: &Track) -> Element<'_> {
        row![
            ty::caption(clock(self.player.pos)),
            w::scale(
                0.0..=t.secs as f32,
                self.player.pos.min(t.secs as f32),
                Message::Seek
            )
            .step(0.25f32),
            ty::caption(t.duration()),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
    }

    /// Wide layout: the player bar.
    fn player_bar(&self) -> Element<'_> {
        let Some(t) = self.now() else {
            return container(ty::caption("Nothing playing")).padding(12).into();
        };
        container(
            row![
                mouse_area(
                    row![
                        cover(t.id, 48.0),
                        column![ty::body(t.title.clone()), ty::caption(t.artist.clone())]
                            .width(Length::Fixed(200.0)),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center)
                )
                .on_press(Message::NowPlaying(!self.now_playing)),
                self.controls(16.0),
                container(self.seek(t)).width(Length::Fill),
                glyph_button(Glyph::Shuffle, 16.0, self.player.shuffle, Message::Shuffle),
                glyph_button(Glyph::Repeat, 16.0, self.player.repeat, Message::Repeat),
                glyph_button(Glyph::Queue, 16.0, self.queue_open, Message::ToggleQueue),
                glyph_button(
                    if self.now_playing {
                        Glyph::Collapse
                    } else {
                        Glyph::Expand
                    },
                    16.0,
                    false,
                    Message::NowPlaying(!self.now_playing),
                ),
            ]
            .spacing(14)
            .align_y(Alignment::Center),
        )
        .padding([8, 16])
        .style(style::container::headerbar)
        .into()
    }

    /// Narrow layout: the mini player. Tapping it expands Now Playing.
    fn mini_player(&self) -> Element<'_> {
        let Some(t) = self.now() else {
            return space::vertical().height(0).into();
        };
        let frac = self.player.pos / t.secs as f32;
        container(column![
            w::progress_bar(0.0..=1.0, frac),
            row![
                mouse_area(
                    row![
                        cover(t.id, 40.0),
                        column![ty::body(t.title.clone()), ty::caption(t.artist.clone())]
                            .width(Length::Fill),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center)
                )
                .on_press(Message::NowPlaying(true)),
                self.play_pause(16.0),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .padding([6, 12]),
        ])
        .style(style::container::headerbar)
        .into()
    }

    /// The queue as a full-height sheet: scroll it, drag rows to reorder, close with the chevron.
    fn queue_sheet(&self) -> Element<'_> {
        let list = ReorderableList::of(&self.player.queue, |id: &usize| {
            let t = &self.lib.tracks[*id];
            w::action_row(t.title.clone())
                .subtitle(t.artist.clone())
                .into()
        })
        .title("Up next")
        .on_event(Message::Queue);
        container(column![
            Element::from(
                self.bar(true, true)
                    .title("Queue")
                    .pack_start(w::icon_button(Icon::GoDown).on_press(Message::ToggleQueue))
            ),
            scrollable(container(list).padding(12)).height(Length::Fill),
        ])
        .width(Length::Fill)
        .height(Length::Fill)
        .style(iad::style::container::window)
        .into()
    }

    /// Now Playing: the cover, or the lyrics in its place; the queue is its own sheet.
    fn now_playing_view(&self) -> Element<'_> {
        let Some(t) = self.now() else {
            return container(ty::body("Nothing playing"))
                .center(Length::Fill)
                .into();
        };
        let art: Element<'_> = if self.show_lyrics {
            synced_text(
                iced::widget::Id::new(LYRICS),
                &self.lyrics,
                self.lyric_line,
                Message::Seek,
            )
        } else {
            container(cover(t.id, 260.0)).center(Length::Fill).into()
        };
        let tags = row(t
            .tags
            .iter()
            .map(|g| tag_pill(g, &self.lib.style_of(g), 22.0)))
        .spacing(6)
        .wrap();
        let body = column![
            container(art).height(Length::Fill).width(Length::Fill),
            column![
                ty::title_2(t.title.clone()),
                ty::body(format!("{} · {}", t.artist, t.album)),
                tags,
            ]
            .spacing(6)
            .align_x(Alignment::Center),
            self.seek(t),
            self.controls(20.0),
            row![
                glyph_button(Glyph::Shuffle, 16.0, self.player.shuffle, Message::Shuffle),
                glyph_button(Glyph::Repeat, 16.0, self.player.repeat, Message::Repeat),
                glyph_button(Glyph::Lyrics, 16.0, self.show_lyrics, Message::ToggleLyrics),
                glyph_button(Glyph::Queue, 16.0, self.queue_open, Message::ToggleQueue),
            ]
            .spacing(8),
        ]
        .spacing(12)
        .align_x(Alignment::Center)
        .padding(16)
        .max_width(640)
        .height(Length::Fill);
        let close = row![
            w::flat_button(Glyph::Collapse.view(16.0)).on_press(Message::NowPlaying(false)),
            space::horizontal(),
        ];
        let backdrop = crate::components::cover::color(t.id);
        container(column![
            close,
            container(body).center_x(Length::Fill).height(Length::Fill)
        ])
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_: &iad::Theme| iced::widget::container::Style {
            background: Some(Background::Color(Color {
                a: 0.12,
                ..backdrop
            })),
            ..Default::default()
        })
        .into()
    }

    // ---- layouts ----------------------------------------------------------------------

    fn sidebar_items(&self) -> Vec<SidebarItem<Message>> {
        let here = |d: Dest| !self.searching && !self.now_playing && self.dest == d;
        let mut items = vec![
            SidebarItem::new("Home", Message::Go(Dest::Home))
                .icon(Icon::Home)
                .selected(here(Dest::Home)),
            SidebarItem::new("Recently played", Message::Go(Dest::Recent))
                .icon(Icon::Calendar)
                .selected(here(Dest::Recent)),
            SidebarItem::new("Tracks", Message::Go(Dest::Tracks))
                .icon(Icon::Document)
                .divider_above()
                .selected(here(Dest::Tracks)),
            SidebarItem::new("Artists", Message::Go(Dest::Artists))
                .icon(Icon::Folder)
                .selected(here(Dest::Artists)),
            SidebarItem::new("Albums", Message::Go(Dest::Albums))
                .icon(Icon::Folder)
                .selected(here(Dest::Albums)),
        ];
        for (i, p) in self.lib.playlists.iter().enumerate() {
            let mut item = SidebarItem::new(p.name, Message::Go(Dest::Playlist(i)))
                .icon(if p.filter.is_some() {
                    Icon::Filter
                } else {
                    Icon::Document
                })
                .selected(here(Dest::Playlist(i)));
            if i == 0 {
                item = item.divider_above();
            }
            items.push(item);
        }
        items
    }

    fn wide(&self) -> Element<'_> {
        let sidebar = container(column![
            Element::from(self.bar(true, false).title("Weave")),
            Element::from(
                SidebarList::new(self.sidebar_items())
                    .style(SidebarStyle::List)
                    .padding(SidebarPadding::Sm)
            ),
        ])
        .width(Length::Fixed(SIDEBAR))
        .height(Length::Fill)
        .style(style::container::sidebar);

        let mut header = self.bar(false, true);
        header = if self.searching {
            header
                .pack_start(w::icon_button(Icon::WindowClose).on_press(Message::SearchOpen(false)))
                .title_widget(container(self.search_entry()).max_width(560))
        } else {
            let mut h = header.title(self.page_title());
            if !self.history.is_empty() {
                h = h.pack_start(w::icon_button(Icon::GoPrevious).on_press(Message::Back));
            }
            h.pack_end(w::icon_button(Icon::Search).on_press(Message::SearchOpen(true)))
        };
        header = header.pack_end(glyph_button(
            Glyph::Jam,
            16.0,
            self.jam_open,
            Message::OpenJam,
        ));

        let content = column![Element::from(header), self.with_jam(self.page(false)),]
            .width(Length::Fill)
            .height(Length::Fill);

        let main = row![sidebar, content];
        column![main.height(Length::Fill), self.player_bar()].into()
    }

    fn pills(&self) -> Element<'_> {
        let active = match self.dest {
            Dest::Playlist(_) | Dest::Playlists => Dest::Playlists,
            Dest::Artist(_) => Dest::Artists,
            Dest::Album(_) => Dest::Albums,
            d => d,
        };
        container(w::toggle_group(
            vec![
                ToggleItem::new(Dest::Tracks, "Tracks"),
                ToggleItem::new(Dest::Artists, "Artists"),
                ToggleItem::new(Dest::Albums, "Albums"),
                ToggleItem::new(Dest::Playlists, "Playlists"),
            ],
            Some(active),
            Message::Go,
        ))
        .padding([8, 16])
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
        let in_library = matches!(
            self.dest,
            Dest::Tracks
                | Dest::Artists
                | Dest::Albums
                | Dest::Playlists
                | Dest::Playlist(_)
                | Dest::Recent
        );
        let jam = self.jam_open;
        let search = self.searching && !jam;
        let library = in_library && !self.searching && !jam;
        let home = self.dest == Dest::Home && !self.searching && !jam;
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
                    Message::SearchOpen(true)
                ),
                self.tab(
                    Glyph::Library.view(16.0).into(),
                    "Library",
                    library,
                    Message::Go(if in_library { self.dest } else { Dest::Tracks }),
                ),
                self.tab(Glyph::Jam.view(16.0).into(), "Jam", jam, Message::OpenJam),
            ]
            .padding(4),
        )
        .style(style::container::headerbar)
        .into()
    }

    fn narrow(&self) -> Element<'_> {
        if self.now_playing {
            return column![
                Element::from(self.bar(true, true).title("Now playing")),
                self.now_playing_view(),
            ]
            .into();
        }
        let in_library = !self.searching
            && matches!(
                self.dest,
                Dest::Tracks | Dest::Artists | Dest::Albums | Dest::Playlists
            );
        let page: Element<'_> = if in_library {
            column![self.pills(), self.page(true)].into()
        } else {
            self.page(true)
        };
        column![
            Element::from({
                let mut h = self.bar(true, true).title(self.page_title());
                if !self.history.is_empty() {
                    h = h.pack_start(w::icon_button(Icon::GoPrevious).on_press(Message::Back));
                }
                h
            }),
            container(self.with_jam(page)).height(Length::Fill),
            self.mini_player(),
            self.tab_bar(),
        ]
        .into()
    }
}
