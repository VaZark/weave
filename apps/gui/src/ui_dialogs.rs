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
use crate::{App, ChipPicker, Dest, Element, Message};
use iad::prelude::surface;
use iad::prelude::{ChipGroup, ChipOption, ChipSize};
use iad::primitives::histogram::Histogram;

/// The id of the Jam search bar, which takes focus as it appears.
const JAM_SEARCH_ID: &str = "jam-search";
/// The id of the Save as playlist name field.
const SAVE_NAME_ID: &str = "save-name";

/// What the controls of a filter send: the jam's Tag filters and the playlist-filter editor show
/// the same fields and differ only in this.
#[derive(Clone, Copy)]
pub struct FilterMsgs {
    pub tag: fn(String) -> Message,
    pub artist: fn(String) -> Message,
    pub album: fn(String) -> Message,
    pub bpm: fn((f32, f32)) -> Message,
    pub recent: fn(bool) -> Message,
}

const JAM_FILTER: FilterMsgs = FilterMsgs {
    tag: Message::ToggleTag,
    artist: Message::JamArtistToggle,
    album: Message::JamAlbumToggle,
    bpm: Message::Bpm,
    recent: Message::JamRecent,
};

const DRAFT_FILTER: FilterMsgs = FilterMsgs {
    tag: Message::DraftToggle,
    artist: Message::DraftArtistToggle,
    album: Message::DraftAlbumToggle,
    bpm: Message::DraftBpm,
    recent: Message::DraftRecent,
};

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

    /// The fields of a filter, as chips: the tags (all of them are listed, with a search), the
    /// artists and the albums (any of the chosen ones; initially three lines of chips, found by
    /// typing), then the tempo. Shared by the jam's Tag filters and the playlist-filter editor,
    /// each with its own messages.
    pub fn filter_fields<'a>(
        &'a self,
        filter: &'a Filter,
        msgs: FilterMsgs,
    ) -> iced::widget::Column<'a, Message, iad::Theme> {
        let (artists, no_artist) = self.lib.artist_choices();
        let (albums, no_album) = self.lib.album_choices();
        column![
            ChipGroup::new(self.tag_options(""), &filter.tags, msgs.tag)
                .search(&self.tag_query, Message::TagQuery)
                .placeholder("Find a tag")
                .max_lines(self.chip_line_limit)
                .expandable(self.chip_expanded[ChipPicker::Tags as usize], |on| {
                    Message::ChipExpand(ChipPicker::Tags, on)
                })
                .on_color(Color::WHITE),
            self.names_picker(
                ChipPicker::Artists,
                "Artists",
                artists,
                no_artist,
                "Unknown artist",
                &filter.artists,
                &self.artist_query,
                msgs.artist,
                Message::ArtistQuery,
                "Find an artist",
            ),
            self.names_picker(
                ChipPicker::Albums,
                "Albums",
                albums,
                no_album,
                "No album",
                &filter.albums,
                &self.album_query,
                msgs.album,
                Message::AlbumQuery,
                "Find an album",
            ),
            w::flat_button(ty::label(match filter.imported_within_days {
                Some(days) => format!("✓ Recently imported (last {days} days)"),
                None => "Recently imported (last 7 days)".to_string(),
            }))
            .on_press((msgs.recent)(filter.imported_within_days.is_none())),
            self.bpm_section(filter, msgs.bpm),
        ]
        .spacing(16)
        .width(Length::Fill)
    }

    /// Artists or albums as chips with a search entry: the chosen ones first, then those with the
    /// most tracks, then the "unknown" one; clamped with Show all, and typing is how the rest is
    /// found. Several chosen are "any of".
    #[allow(clippy::too_many_arguments)]
    fn names_picker<'a>(
        &'a self,
        picker: ChipPicker,
        title: &'static str,
        choices: Vec<(String, usize)>,
        unknown_count: usize,
        unknown_label: &'static str,
        selected: &'a [String],
        query: &'a str,
        on_toggle: fn(String) -> Message,
        on_query: fn(String) -> Message,
        placeholder: &'static str,
    ) -> Element<'a> {
        let label_of = |key: &str| -> String {
            if key == data::UNKNOWN {
                unknown_label.to_string()
            } else {
                key.to_string()
            }
        };
        let mut options: Vec<ChipOption<String>> = selected
            .iter()
            .map(|k| ChipOption::new(k.clone(), label_of(k)))
            .collect();
        for (name, _) in &choices {
            if !selected.contains(name) {
                options.push(ChipOption::new(name.clone(), name.clone()));
            }
        }
        if unknown_count > 0 && !selected.iter().any(|k| k == data::UNKNOWN) {
            options.push(ChipOption::new(data::UNKNOWN.to_string(), unknown_label));
        }
        column![
            row![
                ty::heading(title),
                ty::caption("any of").style(iad::styles::text::dim)
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            ChipGroup::new(options, selected, on_toggle)
                .search(query, on_query)
                .placeholder(placeholder)
                .max_lines(self.chip_line_limit)
                .expandable(self.chip_expanded[picker as usize], move |on| {
                    Message::ChipExpand(picker, on)
                })
                .empty_text("No match")
                .on_color(Color::WHITE),
        ]
        .spacing(8)
        .into()
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

    /// The active filters as chips, each clearing itself. `None` when nothing is active. The search
    /// text is not among them: it lives in the search bar.
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
        if let Some(days) = f.imported_within_days {
            chips.push(
                w::Chip::new(format!("Recently imported · {days} days"))
                    .size(ChipSize::Medium)
                    .on_remove(Message::JamRecent(false))
                    .into(),
            );
        }
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
        for (names, none, toggle) in [
            (
                &f.artists,
                "Unknown artist",
                Message::JamArtistToggle as fn(String) -> Message,
            ),
            (&f.albums, "No album", Message::JamAlbumToggle),
        ] {
            for name in names {
                let label = if name == data::UNKNOWN {
                    none.to_string()
                } else {
                    name.clone()
                };
                chips.push(
                    w::Chip::new(label)
                        .icon(Icon::MusicNote)
                        .size(ChipSize::Medium)
                        .on_remove(toggle(name.clone()))
                        .into(),
                );
            }
        }
        (!chips.is_empty()).then(|| row(chips).spacing(6).wrap().into())
    }

    /// The search at the top right of the Jam page: an icon that opens a pill-shaped search bar.
    /// What is typed only narrows the table; it is not kept, nor saved with a playlist.
    fn jam_search(&self) -> Element<'_> {
        let narrow = self.is_narrow();
        let control: Element<'_> = if self.jam_search_open {
            w::FocusScope::new(
                w::search_bar(&self.filter.q, Message::JamQuery)
                    .placeholder("Search tracks, artists, albums")
                    .on_close(Message::JamSearchOpen(false))
                    .id(JAM_SEARCH_ID)
                    .width(if narrow {
                        Length::Fill
                    } else {
                        Length::Fixed(360.0)
                    }),
            )
            .trap(false)
            .initial_focus(JAM_SEARCH_ID)
            .into()
        } else {
            w::tooltip(
                w::icon_button(Icon::Search).on_press(Message::JamSearchOpen(true)),
                "Search",
                w::TooltipPosition::Left,
            )
        };
        container(control)
            .width(Length::Fill)
            .align_x(iced::alignment::Horizontal::Right)
            .padding(12) // gutter-ok: a control laid over the header
            .into()
    }

    /// The Jam page, with the look of an Album or Playlist page: a header with the active filters,
    /// icon buttons (Play, Tag filters) and, at the far corner, the match count and Save as
    /// playlist (below the buttons on narrow); the search at the top right; then the matching
    /// tracks in a sortable table.
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
            None => {
                let can = self.filter.is_saveable() && n > 0;
                let button = w::flat_button(
                    row![Icon::ListAdd.view(16.0), ty::label("Save as playlist")]
                        .spacing(6)
                        .align_y(Alignment::Center),
                )
                .on_press_maybe(can.then_some(Message::SaveJamAsPlaylist));
                if self.filter.is_saveable() {
                    button.into()
                } else {
                    // The search is not saved, so a playlist needs a tag, tempo, artist or album.
                    w::tooltip(
                        button,
                        "Add a tag, tempo, artist or album to save",
                        w::TooltipPosition::Bottom,
                    )
                }
            }
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
        let cover: Element<'_> = w::avatar(96.0, "Jam")
            .icon(Icon::Sparkle)
            .shape(w::AvatarShape::Rounded)
            .into();
        let mut header = w::DetailHeader::new("Jam")
            .kind("Tracks by filters")
            .cover(cover)
            .primary(play)
            .action(tag_filters)
            .facts(chips)
            .narrow(narrow);
        // Wide: at the far corner of the header. Narrow: below the buttons, as its own row.
        let mut below = None;
        if narrow {
            below = Some(row![matches, space::horizontal(), save].align_y(Alignment::Center));
        } else {
            header = header.end(row![matches, save].spacing(12).align_y(Alignment::Center));
        }
        let header: Element<'_> = header.into();
        let top: Element<'_> = iced::widget::stack![header, self.jam_search()]
            .width(Length::Fill)
            .into();
        let mut page = column![top].spacing(8).height(Length::Fill);
        if let Some(row) = below {
            page = page.push(surface::band(row));
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
            scrollable(self.filter_fields(&self.filter, JAM_FILTER)).height(Length::Fill),
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

    // ---- save as playlist --------------------------------------------------------------

    /// What Save as playlist asks: a name, and what will be kept (and what will not).
    fn save_playlist_form(&self) -> Element<'_> {
        let name = self.save_name.clone().unwrap_or_default();
        let taken = self.name_is_taken(&name);
        let can = !name.trim().is_empty() && !taken;
        let mut col = column![
            ty::title_3("Save as playlist"),
            iced::widget::text_input("Playlist name", &name)
                .id(SAVE_NAME_ID)
                .on_input(Message::SaveNameInput)
                .on_submit_maybe(can.then_some(Message::ConfirmSaveJam)),
        ]
        .spacing(10);
        if taken {
            col = col.push(
                ty::caption("There is already a playlist with this name.")
                    .style(iad::styles::text::error),
            );
        }
        col = col.push(
            ty::caption(format!("Saves: {}.", self.filter_sentence(&self.filter)))
                .style(iad::styles::text::dim),
        );
        if !self.filter.q.trim().is_empty() {
            col = col.push(
                ty::caption(format!(
                    "The search text “{}” is not saved.",
                    self.filter.q.trim()
                ))
                .style(iad::styles::text::dim),
            );
        }
        col = col.push(
            row![
                space::horizontal(),
                w::flat_button(ty::label("Cancel")).on_press(Message::CancelSaveJam),
                w::suggested_button(ty::label("Save"))
                    .on_press_maybe(can.then_some(Message::ConfirmSaveJam)),
            ]
            .spacing(8),
        );
        w::FocusScope::new(col)
            .trap(false)
            .initial_focus(SAVE_NAME_ID)
            .into()
    }

    /// Save as playlist as a dialog (wide).
    pub fn save_dialog(&self) -> Element<'_> {
        container(self.save_playlist_form())
            .padding(20) // gutter-ok: a component's own padding, not a page gutter
            .width(Length::Fixed(440.0))
            .style(iad::styles::container::popover)
            .into()
    }

    /// Save as playlist as a bottom sheet (narrow).
    pub fn save_sheet(&self) -> Element<'_> {
        container(self.save_playlist_form())
            .padding(20) // gutter-ok: a component's own padding, not a page gutter
            .width(Length::Fill)
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
                scrollable(self.filter_fields(draft, DRAFT_FILTER)).height(Length::Fixed(380.0)),
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
                container(
                    scrollable(
                        ChipGroup::new(self.tag_options(""), &e.tags, Message::EditToggleTag)
                            .search(&self.tag_query, Message::TagQuery)
                            .placeholder("Find a tag")
                            .max_lines(self.chip_line_limit)
                            .expandable(self.chip_expanded[ChipPicker::TrackTags as usize], |on| {
                                Message::ChipExpand(ChipPicker::TrackTags, on)
                            })
                            .on_color(Color::WHITE)
                    )
                    .height(Length::Shrink)
                )
                .max_height((self.win_h - 170.0).clamp(40.0, 360.0)),
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
