//! Dialogs (modal, closed by their buttons), the filter fields shared by search, jam and the
//! playlist dialog, and the jam page.

use iad::Icon;
use iad::widget::{self as w, ToggleItem, typography as ty};
use iced::widget::{column, container, row, scrollable, space, text_input};
use iced::{Alignment, Length};
use iced_adaptive_design as iad;

use crate::components::histogram_range::histogram_range_slider;
use crate::components::play_button::play_button;
use crate::components::tag_badge::{tag_chip, tag_pill};
use crate::data::{self, Filter};
use crate::{App, Element, Message, Mode3};
use iad::widget::Histogram;

impl App {
    /// The fields of a filter: a clear heading with the count as a badge and a clear icon, the tags
    /// as toggles across the full width (never clamped, they wrap), then the tempo. Shared by the
    /// search popover, the jam page and the playlist dialog, each with its own filter. A tag also
    /// matches the tags under it. `clear` is `None` where the caller draws its own header.
    pub fn filter_fields<'a>(
        &'a self,
        filter: &'a Filter,
        on_toggle: fn(String) -> Message,
        on_bpm: fn((f32, f32)) -> Message,
        clear: Option<Message>,
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
        let mut col = column![].spacing(10).width(Length::Fill);
        if let Some(clear) = clear {
            col = col.push(
                row![
                    ty::heading("Filters"),
                    ty::caption(format!("{n} matches")),
                    space::horizontal(),
                    w::icon_button(Icon::Delete)
                        .on_press_maybe((!filter.is_empty()).then_some(clear)),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            );
        }
        col.push(text_input("Find a tag", &self.tag_query).on_input(Message::TagQuery))
            .push(container(row(chips).spacing(6).wrap()).width(Length::Fill))
            // A little more air between the groups.
            .push(space::vertical().height(6))
            .push(self.bpm_section(filter, on_bpm))
    }

    /// The tempo: the distribution of the tracks under the handles (press a bar to take its
    /// range), the exact handles, and the usual bands as presets.
    pub fn bpm_section<'a>(
        &'a self,
        filter: &'a Filter,
        on_bpm: fn((f32, f32)) -> Message,
    ) -> Element<'a> {
        const BINS: usize = 12;
        let (lo, hi) = filter.bpm;
        let histogram = Histogram::from_counts(
            data::BPM.0,
            data::BPM.1,
            self.lib.bpm_histogram(filter, BINS),
        );
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
            // One component: the bars and the thumbs edit the same range.
            histogram_range_slider(&histogram, (lo, hi), 5.0, on_bpm),
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

    // ---- jam ----------------------------------------------------------------------------

    /// The jam is a page: Surprise me, the filters (open by default, folded on request), the round
    /// play button, and the virtualised list of what the filters match.
    pub fn jam_page(&self) -> Element<'_> {
        let n = self.lib.filtered(&self.filter).len();
        let wide = !self.is_narrow();
        let open = self.jam_filters_open;
        let header = row![
            w::flat_button(
                row![
                    ty::heading("Filters"),
                    if open { Icon::GoUp } else { Icon::GoDown }.view(16.0),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )
            .on_press(Message::JamFilters(!open)),
            space::horizontal(),
            w::icon_button(Icon::Delete)
                .on_press_maybe((!self.filter.is_empty()).then_some(Message::ClearFilter)),
        ]
        .align_y(Alignment::Center);
        let filters = w::Collapsible::new(
            header,
            self.filter_fields(&self.filter, Message::ToggleTag, Message::Bpm, None),
            open,
        );
        let top = row![w::flat_button(ty::label("🎲 Surprise me")).on_press(Message::Surprise),];
        // The same round play button as every track list, right after the filters, with the number
        // of matches beside it and some air above.
        // Saving stores the filters as a dynamic playlist (a jam is a query, not a list of tracks).
        let saved = self
            .jam_saved
            .as_ref()
            .filter(|(_, f)| *f == self.filter)
            .map(|(i, _)| *i);
        let save: Element<'_> = match saved {
            Some(i) => w::flat_button(
                row![
                    Icon::Check.view(16.0),
                    ty::label(format!("Saved as {}", self.lib.playlists[i].name)),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .on_press(Message::Open(crate::Dest::Playlist(i)))
            .into(),
            None => w::flat_button(
                row![Icon::ListAdd.view(16.0), ty::label("Save as playlist")]
                    .spacing(6)
                    .align_y(Alignment::Center),
            )
            .on_press_maybe((n > 0).then_some(Message::SaveJamAsPlaylist))
            .into(),
        };
        let play = column![
            space::vertical().height(Length::Fixed(14.0)),
            row![
                play_button((n > 0).then_some(Message::StartJam)),
                ty::caption(format!("{n} matches")),
                space::horizontal(),
                save,
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        ];
        let page: Element<'_> = if wide {
            row![
                scrollable(column![top, filters].spacing(10))
                    .width(Length::FillPortion(1))
                    .height(Length::Fill),
                // The play row sits on top of the list it plays.
                column![play, self.jam_list()]
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
        };
        container(page).padding(16).height(Length::Fill).into()
    }

    // ---- dialogs ------------------------------------------------------------------------

    /// Settings: a preferences dialog (iad's `preferences_dialog`) of groups of rows.
    pub fn settings_dialog(&self) -> Element<'_> {
        let page = w::PreferencesPage::<Message>::new()
            .push(
                w::PreferencesGroup::new()
                    .title("Density")
                    .description("How every list looks: tracks (including the jam's), albums, artists and playlists.")
                    .push(
                        w::SwitchRow::new("Compact mode", self.compact, Message::SetCompact)
                            .subtitle("Tracks on one line without pictures; albums, artists and playlists as tight rows with a small picture"),
                    ),
            )
            .push(
                w::PreferencesGroup::new()
                    .title("Explore")
                    .description("What comes back to you.")
                    .push(
                        w::SpinRow::new(
                            "Resurface after (days)",
                            f64::from(self.while_days),
                            30.0..=730.0,
                            30.0,
                            |d| Message::WhileDays(d as u32),
                        )
                        .subtitle("Tracks you rated highly and have not played for this long"),
                    ),
            );
        w::preferences_dialog("Settings", page, Message::CloseSettings)
    }

    /// The rules of a dynamic playlist. Modal and not dismissable: Cancel or Save.
    pub fn filter_dialog<'a>(&'a self, draft: &'a Filter) -> Element<'a> {
        container(
            column![
                ty::title_3("Playlist filter"),
                ty::caption(
                    "The playlist holds every track that matches these rules, now and as the library changes."
                ),
                self.filter_fields(draft, Message::DraftToggle, Message::DraftBpm, None),
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
        .width(Length::Fixed(520.0))
        .style(iad::style::container::popover)
        .into()
    }

    /// Edit a track's tags. Modal: closed by Cancel or Save.
    pub fn track_dialog<'a>(&'a self, e: &'a crate::TrackEdit) -> Element<'a> {
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
        container(
            column![
                ty::title_3(format!("Tags · {}", t.title)),
                ty::caption(format!("{} · {}", t.artist, t.album)),
                text_input("Find a tag", &self.tag_query).on_input(Message::TagQuery),
                container(row(chips).spacing(6).wrap()).width(Length::Fill),
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
        .width(Length::Fixed(520.0))
        .style(iad::style::container::popover)
        .into()
    }

    /// Rate a track: five stars; pressing the current rating clears it.
    pub fn rate_dialog(&self, id: usize) -> Element<'_> {
        let t = self.track(id);
        let stars = row((1..=5u8).map(|i| {
            w::flat_button(ty::title_2(if i <= t.rating { "★" } else { "☆" }))
                .on_press(Message::SetRating(id, i))
                .into()
        }))
        .spacing(4);
        container(
            column![
                ty::title_3(t.title.clone()),
                ty::caption(format!("{} · {}", t.artist, t.album)),
                stars,
                row![
                    space::horizontal(),
                    w::flat_button(ty::label("Cancel")).on_press(Message::CloseRate),
                ],
            ]
            .spacing(10)
            .align_x(Alignment::Center),
        )
        .padding(20)
        .width(Length::Fixed(360.0))
        .style(iad::style::container::popover)
        .into()
    }

    /// More about a track: what the lists leave out (tempo, play history, tags).
    pub fn info_dialog(&self, id: usize) -> Element<'_> {
        let t = self.track(id);
        let line = |k: &'static str, v: String| -> Element<'_> {
            row![
                ty::body(k).width(Length::Fixed(120.0)),
                ty::body(v).width(Length::Fill)
            ]
            .into()
        };
        let tags = row(t
            .tags
            .iter()
            .map(|g| tag_pill(g, &self.lib.style_of(g), 18.0)))
        .spacing(6)
        .wrap();
        container(
            column![
                ty::title_3(t.title.clone()),
                line("Artist", t.artist.clone()),
                line("Album", t.album.clone()),
                line("Duration", t.duration()),
                line("Tempo", format!("{:.0} BPM", t.bpm)),
                line(
                    "Rating",
                    if t.rating == 0 {
                        "not rated".into()
                    } else {
                        "★".repeat(t.rating as usize)
                    }
                ),
                line(
                    "Played",
                    format!("{} times, last {} days ago", t.plays, t.last_played_days)
                ),
                container(tags).width(Length::Fill),
                row![
                    space::horizontal(),
                    w::suggested_button(ty::label("Close")).on_press(Message::CloseInfo)
                ],
            ]
            .spacing(8),
        )
        .padding(20)
        .width(Length::Fixed(460.0))
        .style(iad::style::container::popover)
        .into()
    }

    /// Edit the tags of every track on the page: delete or replace a tag where it matches, and
    /// add tags to all of them. Modal: closed by Cancel or Save.
    pub fn bulk_dialog<'a>(&'a self, b: &'a crate::BulkEdit) -> Element<'a> {
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
}
