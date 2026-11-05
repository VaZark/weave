//! Dialogs (modal, closed by their buttons), the filter fields shared by search, jam and the
//! playlist dialog, and the jam page.

use iad::Icon;
use iad::prelude::{self as w, typography as ty};
use iced::widget::{column, container, row, scrollable, space};
use iced::{Alignment, Color, Length};
use iced_adaptive_design as iad;

use crate::components::tag_badge::{tag_chip_removable, tag_pill, value_of};
use crate::data::{self, Filter};
use crate::icons::{cover, playlist_avatar};
use crate::ui_lists::RowOpts;
use crate::{App, Dest, Element, Message};
use iad::prelude::surface;
use iad::prelude::{ChipGroup, ChipOption, ChipSize};
use iad::primitives::histogram::Histogram;

/// The id of the Jam search row's first entry, which takes focus as the row opens.
const JAM_ARTIST_ID: &str = "jam-artist";

impl App {
    /// The vocabulary as chip options: each tag's value as the label, with its own icon and colour.
    /// `query` (when not empty) keeps only the tags whose value contains it.
    pub fn tag_options(&self, query: &str) -> Vec<ChipOption<String>> {
        let q = query.trim().to_lowercase();
        self.lib
            .tag_names()
            .into_iter()
            .filter(|t| q.is_empty() || value_of(t).to_lowercase().contains(&q))
            .map(|t| {
                let style = self.lib.style_of(&t);
                ChipOption::new(t.clone(), value_of(&t))
                    .icon(crate::icons::for_key(&style.icon))
                    .color(style.color)
            })
            .collect()
    }

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
        col.push(
            ChipGroup::new(self.tag_options(""), &filter.tags, on_toggle)
                .search(&self.tag_query, Message::TagQuery)
                .placeholder("Find a tag")
                .on_color(Color::WHITE),
        )
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
        let (lo, hi) = filter.bpm;
        // Built here: it depends on the filter's tags, so nothing is kept in the app.
        let histogram =
            Histogram::from_counts(data::BPM.0, data::BPM.1, self.lib.bpm_histogram(filter, 12));
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
            w::HistogramRangeSlider::new(histogram, (lo, hi), on_bpm)
                .step(5.0)
                .height(48.0),
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

    /// An icon button with a tooltip, and a count badge when `count` is more than none.
    fn badged_icon(
        &self,
        icon: Icon,
        tip: &'static str,
        count: usize,
        on_press: Message,
    ) -> Element<'_> {
        let mut r = row![w::icon_button(icon).on_press(on_press)]
            .spacing(2)
            .align_y(Alignment::Center);
        if count > 0 {
            r = r.push(
                w::Badge::count(count as u32, 99)
                    .role(iad::Role::Accent)
                    .solid(true),
            );
        }
        w::tooltip(r, tip, w::TooltipPosition::Bottom)
    }

    /// The active filters as chips, each clearing itself; a generic search is marked as not
    /// saved. `None` when nothing is active.
    fn active_chips(&self) -> Option<Element<'_>> {
        let f = &self.filter;
        let mut chips: Vec<Element<'_>> = f
            .tags
            .iter()
            .map(|t| {
                tag_chip_removable(
                    t,
                    &self.lib.style_of(t),
                    ChipSize::Medium,
                    Message::ToggleTag(t.clone()),
                )
            })
            .collect();
        if f.bpm != data::BPM {
            let (lo, hi) = f.bpm;
            chips.push(
                w::Chip::new(format!("{lo:.0}–{hi:.0} BPM"))
                    .icon(Icon::Clock)
                    .size(ChipSize::Medium)
                    .on_remove(Message::Bpm(data::BPM))
                    .into(),
            );
        }
        let text_chip = |label: String, icon: Icon, clear: Message| -> Element<'_> {
            w::Chip::new(label)
                .icon(icon)
                .size(ChipSize::Medium)
                .on_remove(clear)
                .into()
        };
        if !f.artist.trim().is_empty() {
            chips.push(text_chip(
                format!("artist: {}", f.artist.trim()),
                Icon::MusicNote,
                Message::JamArtist(String::new()),
            ));
        }
        if !f.album.trim().is_empty() {
            chips.push(text_chip(
                format!("album: {}", f.album.trim()),
                Icon::MusicNote,
                Message::JamAlbum(String::new()),
            ));
        }
        if !f.q.trim().is_empty() {
            chips.push(w::tooltip(
                text_chip(
                    format!("“{}”", f.q.trim()),
                    Icon::Search,
                    Message::JamQuery(String::new()),
                ),
                "Not saved with a playlist",
                w::TooltipPosition::Bottom,
            ));
        }
        (!chips.is_empty()).then(|| row(chips).spacing(6).wrap().into())
    }

    /// The search row of the Jam page: artist and album (kept in a saved playlist) and anything
    /// (title, artist or album; not kept).
    fn jam_search_row(&self) -> Element<'_> {
        let entry = |placeholder: &'static str,
                     value: &str,
                     on_input: fn(String) -> Message,
                     id: Option<&'static str>| {
            let mut input = iced::widget::text_input(placeholder, value)
                .on_input(on_input)
                .width(Length::Fill);
            if let Some(id) = id {
                input = input.id(id);
            }
            let clear = w::icon_button(Icon::WindowClose)
                .on_press_maybe((!value.is_empty()).then(|| on_input(String::new())));
            row![input, clear]
                .spacing(2)
                .align_y(Alignment::Center)
                .width(Length::FillPortion(1))
        };
        let f = &self.filter;
        let entries = [
            entry("Artist", &f.artist, Message::JamArtist, Some(JAM_ARTIST_ID)),
            entry("Album", &f.album, Message::JamAlbum, None),
            entry("Anything", &f.q, Message::JamQuery, None),
        ];
        let body: Element<'_> = if self.is_narrow() {
            column(entries.into_iter().map(Element::from))
                .spacing(6)
                .into()
        } else {
            row(entries.into_iter().map(Element::from))
                .spacing(12)
                .into()
        };
        w::FocusScope::new(body)
            .trap(false)
            .initial_focus(JAM_ARTIST_ID)
            .into()
    }

    /// The Jam page, with the look of an Album or Playlist page: a header with the active filters,
    /// icon buttons (Play, Tag filters, Search) and at the far corner the match count and Save as
    /// playlist; then the matching tracks in a sortable table, as Search showed them.
    pub fn jam_page(&self) -> Element<'_> {
        let narrow = self.is_narrow();
        let n = self.rows.len();
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
        let matches = ty::caption(format!("{n} matches")).style(iad::styles::text::dim);
        let chips = self.active_chips().unwrap_or_else(|| {
            ty::caption("No filters: every track matches.")
                .style(iad::styles::text::dim)
                .into()
        });
        let play = w::suggested_circular_button(Icon::Play.view(20.0))
            .size(44.0)
            .on_press_maybe((n > 0).then_some(Message::StartJam));
        let tag_filters = self.badged_icon(
            Icon::Filter,
            "Tag filters",
            self.filter.tag_count(),
            Message::JamFilters(true),
        );
        let search = self.badged_icon(
            Icon::Search,
            "Search",
            self.filter.search_count(),
            Message::JamSearchOpen(!self.jam_search_open),
        );
        let cover: Element<'_> = w::avatar(96.0, "Jam")
            .icon(Icon::Sparkle)
            .shape(w::AvatarShape::Rounded)
            .into();
        let mut header = w::DetailHeader::new("Jam")
            .kind("Tracks by filters")
            .cover(cover)
            .primary(play)
            .action(tag_filters)
            .action(search)
            .tint(iad::color::from_index(7))
            .narrow(narrow);
        if narrow {
            header = header.facts(
                column![
                    chips,
                    row![matches, space::horizontal(), save].align_y(Alignment::Center),
                ]
                .spacing(6),
            );
        } else {
            header = header
                .facts(chips)
                .end(row![matches, save].spacing(12).align_y(Alignment::Center));
        }
        let header: Element<'_> = header.into();
        let mut page = column![header].spacing(8).height(Length::Fill);
        if self.jam_search_open {
            page = page.push(surface::band(self.jam_search_row()));
        }
        page = page.push(self.virtual_tracks(RowOpts {
            album: true,
            bpm: true,
            menu: true,
        }));
        page.into()
    }

    // ---- jam filters --------------------------------------------------------------------

    /// The jam's filters, edited live: a header with the matches, Clear filters and Done, then the
    /// shared filter fields. The same content in the dialog (wide) and the full-screen sheet
    /// (narrow).
    fn jam_filters_form(&self) -> Element<'_> {
        let n = self.lib.filtered(&self.filter).len();
        column![
            row![
                ty::title_3("Tag filters"),
                ty::caption(format!("{n} matches")),
                space::horizontal(),
                w::tooltip(
                    w::flat_button(ty::label("🎲")).on_press(Message::Surprise),
                    "Surprise me",
                    w::TooltipPosition::Bottom,
                ),
                w::tooltip(
                    w::icon_button(Icon::Delete).on_press_maybe(
                        (self.filter.tag_count() > 0).then_some(Message::ClearTagFilters)
                    ),
                    "Clear tag filters",
                    w::TooltipPosition::Bottom,
                ),
                w::suggested_button(ty::label("Done")).on_press(Message::JamFilters(false)),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            scrollable(self.filter_fields(&self.filter, Message::ToggleTag, Message::Bpm, None))
                .height(Length::Fill),
        ]
        .spacing(12)
        .into()
    }

    /// The jam's filters as a dialog (wide).
    pub fn jam_filters_dialog(&self) -> Element<'_> {
        container(self.jam_filters_form())
            .padding(20) // gutter-ok: a component's own padding, not a page gutter
            .width(Length::Fixed(560.0))
            .height(Length::Fixed(520.0))
            .style(iad::styles::container::popover)
            .into()
    }

    /// The jam's filters as a full-screen bottom sheet (narrow).
    pub fn jam_filters_sheet(&self) -> Element<'_> {
        container(self.jam_filters_form())
            .padding(20) // gutter-ok: a component's own padding, not a page gutter
            .width(Length::Fill)
            .height(Length::Fill)
            .style(iad::styles::container::window)
            .into()
    }

    // ---- delete playlist ----------------------------------------------------------------

    /// The question before a playlist is deleted: what goes and what stays.
    fn delete_playlist_form(&self, name: &str) -> Element<'_> {
        let dependents = self.backend.playlist_dependents(name);
        let mut col = column![
            ty::title_3(format!("Delete “{name}”?")),
            ty::body("The playlist is removed. Its tracks stay in your library."),
        ]
        .spacing(10);
        if dependents > 0 {
            col = col.push(
                ty::caption(format!(
                    "{dependents} other playlist{} draw{} on it and will stop doing so.",
                    if dependents == 1 { "" } else { "s" },
                    if dependents == 1 { "s" } else { "" },
                ))
                .style(iad::styles::text::dim),
            );
        }
        col.push(
            row![
                space::horizontal(),
                w::flat_button(ty::label("Cancel")).on_press(Message::CancelDeletePlaylist),
                w::destructive_button(ty::label("Delete")).on_press(Message::ConfirmDeletePlaylist),
            ]
            .spacing(8),
        )
        .into()
    }

    /// Delete playlist as a dialog (wide).
    pub fn delete_playlist_dialog<'a>(&'a self, name: &'a str) -> Element<'a> {
        container(self.delete_playlist_form(name))
            .padding(20) // gutter-ok: a component's own padding, not a page gutter
            .width(Length::Fixed(440.0))
            .style(iad::styles::container::popover)
            .into()
    }

    /// Delete playlist as a bottom sheet (narrow).
    pub fn delete_playlist_sheet<'a>(&'a self, name: &'a str) -> Element<'a> {
        container(self.delete_playlist_form(name))
            .padding(20) // gutter-ok: a component's own padding, not a page gutter
            .width(Length::Fill)
            .into()
    }

    // ---- dialogs ------------------------------------------------------------------------

    /// The content of Edit details, as Spotify's has it: a close cross at the top, the cover at
    /// the left with the name and the description beside it, and Save. A playlist has a name to
    /// edit; an artist and an album only a description. `cover_size` is smaller in the narrow
    /// sheet.
    fn details_form<'a>(&'a self, draft: &'a crate::DetailsDraft, cover_size: f32) -> Element<'a> {
        let cover: Element<'_> = match self.dest {
            Dest::Playlist(i) => self.lib.playlists.get(i).map_or_else(
                || space::horizontal().into(),
                |p| playlist_avatar(&p.name, p.icon, cover_size),
            ),
            Dest::Album(i) => cover(i, cover_size),
            Dest::Artist(i) => {
                let name = self
                    .lib
                    .artists()
                    .get(i)
                    .map(|a| a.0.clone())
                    .unwrap_or_default();
                w::avatar(cover_size, name).into()
            }
            _ => space::horizontal().into(),
        };
        // Entry rows in a boxed list, as in the library's own forms: each title floats above its
        // entry.
        let mut rows: Vec<Element<'_>> = Vec::new();
        if let Some(name) = &draft.name {
            rows.push(
                w::EntryRow::new("Name", name.as_str())
                    .on_input(Message::NameInput)
                    .on_submit(Message::SaveDetails)
                    .into(),
            );
        }
        rows.push(
            w::EntryRow::new("Description", draft.description.as_str())
                .on_input(Message::DescInput)
                .on_submit(Message::SaveDetails)
                .into(),
        );
        let fields = container(w::boxed_list(rows)).width(Length::Fill);
        column![
            row![
                ty::title_3("Edit details"),
                space::horizontal(),
                w::icon_button(Icon::WindowClose).on_press(Message::CancelDetails),
            ]
            .align_y(Alignment::Center),
            row![cover, fields].spacing(16).align_y(Alignment::Center),
            row![
                space::horizontal(),
                w::suggested_pill_button(ty::label("Save")).on_press(Message::SaveDetails),
            ],
        ]
        .spacing(16)
        .into()
    }

    /// Edit details as a dialog (wide).
    pub fn details_dialog<'a>(&'a self, draft: &'a crate::DetailsDraft) -> Element<'a> {
        container(self.details_form(draft, 112.0))
            .padding(20) // gutter-ok: a component's own padding, not a page gutter
            .width(Length::Fixed(520.0))
            .style(iad::styles::container::popover)
            .into()
    }

    /// Edit details as a bottom sheet (narrow): the same form over the page, closed by its cross,
    /// by Save, or by a tap outside it.
    pub fn details_sheet<'a>(&'a self, draft: &'a crate::DetailsDraft) -> Element<'a> {
        container(self.details_form(draft, 72.0))
            .padding(20) // gutter-ok: a component's own padding, not a page gutter
            .width(Length::Fill)
            .into()
    }

    /// The rules of a dynamic playlist. Modal and not dismissable: Cancel or Save.
    pub fn filter_dialog<'a>(&'a self, draft: &'a Filter) -> Element<'a> {
        container(
            column![
                ty::title_3("Playlist filter"),
                ty::caption(
                    "The playlist holds every track that matches these rules, now and as the library changes."
                ),
                row![
                    iced::widget::text_input("Artist", &draft.artist)
                        .on_input(Message::DraftArtist)
                        .width(Length::Fill),
                    iced::widget::text_input("Album", &draft.album)
                        .on_input(Message::DraftAlbum)
                        .width(Length::Fill),
                ]
                .spacing(8),
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
        .padding(20) // gutter-ok: a component's own padding, not a page gutter
        .width(Length::Fixed(520.0))
        .style(iad::styles::container::popover)
        .into()
    }

    /// Edit a track's tags. Modal: closed by Cancel or Save.
    pub fn track_dialog<'a>(&'a self, e: &'a crate::TrackEdit) -> Element<'a> {
        let t = self.track(e.id);
        container(
            column![
                ty::title_3(format!("Tags · {}", t.title)),
                ty::caption(t.by_line()),
                ChipGroup::new(self.tag_options(""), &e.tags, Message::EditToggleTag)
                    .search(&self.tag_query, Message::TagQuery)
                    .placeholder("Find a tag")
                    .on_color(Color::WHITE),
                row![
                    space::horizontal(),
                    w::flat_button(ty::label("Cancel")).on_press(Message::CancelTrack),
                    w::suggested_button(ty::label("Save")).on_press(Message::SaveTrack),
                ]
                .spacing(8),
            ]
            .spacing(10),
        )
        .padding(20) // gutter-ok: a component's own padding, not a page gutter
        .width(Length::Fixed(520.0))
        .style(iad::styles::container::popover)
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
                ty::caption(t.by_line()),
                stars,
                row![
                    space::horizontal(),
                    w::flat_button(ty::label("Cancel")).on_press(Message::CloseRate),
                ],
            ]
            .spacing(10)
            .align_x(Alignment::Center),
        )
        .padding(20) // gutter-ok: a component's own padding, not a page gutter
        .width(Length::Fixed(360.0))
        .style(iad::styles::container::popover)
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
            .map(|g| tag_pill(g, &self.lib.style_of(g), ChipSize::Small)))
        .spacing(6)
        .wrap();
        container(
            column![
                ty::title_3(t.title.clone()),
                line("Artist", t.artist.clone()),
                // A track with no album tag has no album line.
                if t.album.is_empty() {
                    Element::from(space::vertical().height(0))
                } else {
                    line("Album", t.album.clone())
                },
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
        .padding(20) // gutter-ok: a component's own padding, not a page gutter
        .width(Length::Fixed(460.0))
        .style(iad::styles::container::popover)
        .into()
    }
}
