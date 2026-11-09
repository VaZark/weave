//! Dialogs (modal, closed by their buttons), the filter fields shared by search, jam and the
//! playlist dialog, and the jam page.

use iad::prelude::{self as w, typography as ty};
use iced::widget::{column, container, row, scrollable, space};
use iced::{Alignment, Length};
use iced_adaptive_design as iad;
use weave_icons::Icon;

use crate::components::tag_badge::tag_pill;
use crate::data::{self, Filter};
use crate::icons::{cover, playlist_avatar};
use crate::ui_lists::RowOpts;
use crate::{App, ChipPicker, Dest, Element, FilterTarget, Message, TagBrowse};
use iad::prelude::{ChipGroup, ChipOption, ChipSize};
use iad::primitives::histogram::Histogram;

/// The id of the Save as playlist name field.
const SAVE_NAME_ID: &str = "save-name";
pub(super) const TAG_QUERY_ID: &str = "tag-filter-query";

fn tag_group_separator(
    blocks: &[Vec<weave_player_lib::core::TagFilter>],
    block: usize,
    index: usize,
) -> Option<&'static str> {
    if index + 1 < blocks[block].len() {
        Some(",")
    } else if blocks[block + 1..].iter().any(|block| !block.is_empty()) {
        Some("AND")
    } else {
        None
    }
}

/// What `tag_picker_content` hands the library's tag filter.
struct TagGroupView<'a> {
    groups: iad::primitives::tag_groups::TagGroups<String>,
    picked: Vec<w::ChipOption<String>>,
    before: Vec<Element<'a>>,
    after: Vec<Element<'a>>,
    separator: Box<dyn Fn(usize) -> String + 'a>,
    on_change: Box<dyn Fn(iad::primitives::tag_groups::GroupEvent<String>) -> Message + 'a>,
}

pub(super) fn editable_tag_group(group: &weave_player_lib::core::TagFilter) -> bool {
    (group.all.is_empty() || group.any.is_empty())
        && group.exclude.is_empty()
        && group.clauses.is_empty()
        && group.blocks.is_empty()
}

/// What the controls of a filter send: the jam's Tag filters and the playlist-filter editor show
/// the same fields and differ only in this.
#[derive(Clone, Copy)]
pub struct FilterMsgs {
    pub tag: fn(String) -> Message,
    pub artist: fn(String) -> Message,
    pub album: fn(String) -> Message,
    pub bpm: fn((f32, f32)) -> Message,
    pub target: FilterTarget,
}

impl FilterMsgs {
    pub(super) fn for_target(target: FilterTarget) -> Self {
        match target {
            FilterTarget::Jam => Self {
                tag: Message::ToggleTag,
                artist: Message::JamArtistToggle,
                album: Message::JamAlbumToggle,
                bpm: Message::Bpm,
                target,
            },
            FilterTarget::Draft => Self {
                tag: Message::DraftToggle,
                artist: Message::DraftArtistToggle,
                album: Message::DraftAlbumToggle,
                bpm: Message::DraftBpm,
                target,
            },
            FilterTarget::Page => Self {
                tag: Message::PageTag,
                artist: Message::PageArtist,
                album: Message::PageAlbum,
                bpm: Message::PageBpm,
                target,
            },
        }
    }
}

impl App {
    /// One tag picker shared by live filters, saved-filter drafts and track annotations.
    /// The engine filters the vocabulary; navigation changes presentation, never selection.
    pub fn tag_picker<'a>(
        &'a self,
        selected: &'a [String],
        on_toggle: fn(String) -> Message,
        picker: ChipPicker,
    ) -> Element<'a> {
        self.tag_picker_content(selected, on_toggle, picker, None)
    }

    fn filter_tag_picker<'a>(&'a self, filter: &'a Filter, msgs: FilterMsgs) -> Element<'a> {
        self.tag_picker_content(
            &filter.tags,
            msgs.tag,
            ChipPicker::Tags,
            Some((filter, msgs.target)),
        )
    }

    fn tag_picker_content<'a>(
        &'a self,
        selected: &'a [String],
        on_toggle: fn(String) -> Message,
        picker: ChipPicker,
        groups: Option<(&'a Filter, FilterTarget)>,
    ) -> Element<'a> {
        let all_tags = self.tag_browse == TagBrowse::All;
        let active = groups.and_then(|(_, target)| {
            self.active_tag_group
                .filter(|(context, _, _)| *context == target)
                .map(|(_, block, index)| (block, index))
        });
        let selected = active
            .and_then(|(block, index)| {
                groups
                    .and_then(|(filter, _)| filter.retained.tag_blocks.get(block))
                    .and_then(|b| b.get(index))
            })
            .map(|group| {
                if group.any.is_empty() {
                    group.all.as_slice()
                } else {
                    group.any.as_slice()
                }
            })
            .unwrap_or(selected);
        let choices: Result<Element<'a>, String> = match &self.tag_browse {
            TagBrowse::Categories => {
                self.backend
                    .tag_categories(&self.tag_query)
                    .map(|categories| {
                        let options = categories.into_iter().map(|key| {
                            let label = key.as_deref().unwrap_or("Custom").to_string();
                            let style = self.category_style(key.as_deref());
                            ChipOption::new(key, label)
                                .icon(crate::icons::for_key(&style.icon))
                                .color(style.color)
                        });
                        // The category list is deliberately unbounded. It is navigation, not a filter.
                        ChipGroup::new(options, &[], Message::TagCategory)
                            .empty_text("No matching categories")
                            .into()
                    })
            }
            mode => {
                let category = match mode {
                    TagBrowse::Category(key) => Some(key.as_deref().unwrap_or("")),
                    _ => None,
                };
                self.backend
                    .tag_choices(&self.tag_query, category)
                    .map(|tags| {
                        let options = tags.into_iter().map(|tag| {
                            let style = self.tag_style(&tag.name);
                            ChipOption::new(tag.name, tag.value)
                                .icon(crate::icons::for_key(&style.icon))
                                .color(style.color)
                        });
                        ChipGroup::new(options, selected, move |tag| match groups {
                            Some((_, target)) => Message::FilterTagToggle(target, tag),
                            None => on_toggle(tag),
                        })
                        .max_lines(self.chip_line_limit)
                        .expandable(self.chip_expanded[picker as usize], move |on| {
                            Message::ChipExpand(picker, on)
                        })
                        .empty_text("No matching tags")
                        .into()
                    })
            }
        };
        let choices: Element<'a> = match choices {
            Ok(choices) => choices,
            Err(error) => ty::caption(format!("Cannot load tags: {error}")).into(),
        };

        // The entry: picked tags as pills (the library's TagFilter), with the groups the filter
        // edits and, beside them, what it only shows.
        let view = self.tag_group_view(selected, groups, active, on_toggle);
        let mut back = None;
        if matches!(self.tag_browse, TagBrowse::Category(_)) {
            back = Some(
                crate::icons::icon_button(Icon::GoPrevious).on_press(Message::TagCategories),
            );
        }
        let toggle = w::tooltip(
            crate::icons::section_button(Icon::Tag, Message::TagBrowseToggle, all_tags),
            if all_tags {
                "Show categories"
            } else {
                "Show all tags"
            },
            w::TooltipPosition::Bottom,
        );
        let mut filter = iad::widgets::TagFilter::owned(
            view.groups,
            Vec::new(),
            &self.tag_query,
            view.on_change,
            Message::TagQuery,
        )
        .multiple(groups.is_some())
        .label("Tags")
        .placeholder("Find a tag or category")
        .flat(true)
        .picked(view.picked)
        .field_id(iced::widget::Id::new(TAG_QUERY_ID))
        .before(view.before)
        .after(view.after)
        .separator(view.separator)
        .trailing(toggle)
        .choices(choices);
        if let Some(back) = back {
            filter = filter.leading(back);
        }
        if let Some((_, target)) = groups {
            filter = filter.add_group_button(w::tooltip(
                crate::icons::section_button(Icon::ListAdd, Message::CommitTagGroup(target), false),
                "Finish this tag group and start a new one",
                w::TooltipPosition::Bottom,
            ));
        }
        filter.into()
    }

    /// What the library's tag filter needs of a filter's tags: the groups it edits (the editable
    /// ones, then a slot for the loose tags), how its events become messages, and the groups it
    /// can only show.
    fn tag_group_view<'a>(
        &'a self,
        selected: &'a [String],
        groups: Option<(&'a Filter, FilterTarget)>,
        active: Option<(usize, usize)>,
        on_toggle: fn(String) -> Message,
    ) -> TagGroupView<'a> {
        use iad::primitives::tag_groups::{GroupEvent, Step, TagGroups};
        let Some((filter, target)) = groups else {
            // No groups: the picked tags are one group, and toggling is the caller's.
            let picked = self.picked_options(selected.iter());
            return TagGroupView {
                groups: TagGroups::from_parts(vec![selected.to_vec()], 0),
                picked,
                before: Vec::new(),
                after: Vec::new(),
                separator: Box::new(|_| String::new()),
                on_change: Box::new(move |event| match event {
                    GroupEvent::Toggle(_, tag) => on_toggle(tag),
                    _ => Message::Noop,
                }),
            };
        };
        let blocks = &filter.retained.tag_blocks;
        let mut positions = Vec::new();
        let mut tags = Vec::new();
        let mut before: Vec<Element<'a>> = Vec::new();
        for (block, alternatives) in blocks.iter().enumerate() {
            for (index, group) in alternatives.iter().enumerate() {
                if editable_tag_group(group) {
                    positions.push((block, index));
                    tags.push(if group.any.is_empty() {
                        group.all.clone()
                    } else {
                        group.any.clone()
                    });
                } else {
                    before.push(self.tag_group_chip(group, None));
                    if let Some(separator) = tag_group_separator(blocks, block, index) {
                        before.push(ty::label(separator).into());
                    }
                }
            }
        }
        let slot = positions.len();
        // Loose tags are the first group until a group is committed; after that they are ANDed
        // with the committed alternatives, and only shown.
        let mut after: Vec<Element<'a>> = Vec::new();
        if blocks.is_empty() {
            tags.push(filter.tags.clone());
        } else {
            tags.push(Vec::new());
            if !filter.tags.is_empty() {
                after.push(ty::label("AND").into());
                after.extend(
                    filter
                        .tags
                        .iter()
                        .map(|tag| self.selection_tag_chip(tag, Some(on_toggle(tag.clone())))),
                );
            }
        }
        let active_group = active
            .and_then(|a| positions.iter().position(|p| *p == a))
            .unwrap_or(slot);
        let picked = self.picked_options(tags.iter().flatten());
        let block_of: Vec<usize> = positions.iter().map(|p| p.0).collect();
        let separator = Box::new(move |index: usize| match (block_of.get(index - 1), block_of.get(index)) {
            (Some(a), Some(b)) if a != b => "AND".to_string(),
            (Some(_), Some(_)) => ",".to_string(),
            _ => String::new(),
        });
        let place = move |group: usize| positions.get(group).copied();
        let on_change = Box::new(move |event: GroupEvent<String>| match event {
            GroupEvent::Toggle(group, tag) => match place(group) {
                Some(at) => Message::ToggleInTagGroup(target, at, tag),
                None => on_toggle(tag),
            },
            GroupEvent::Select(group) => Message::SelectTagGroup(target, place(group)),
            GroupEvent::Step(Step::Previous) => match active_group.checked_sub(1) {
                Some(group) => Message::SelectTagGroup(target, place(group)),
                None => Message::Noop,
            },
            GroupEvent::Step(Step::Next) if active_group < slot => {
                Message::SelectTagGroup(target, place(active_group + 1))
            }
            GroupEvent::Step(Step::Next) => Message::Noop,
            GroupEvent::AddGroup => Message::CommitTagGroup(target),
        });
        TagGroupView {
            groups: TagGroups::from_parts(tags, active_group),
            picked,
            before,
            after,
            separator,
            on_change,
        }
    }

    /// How the library's tag filter draws these tags: their value, with the tag's icon and colour.
    fn picked_options<'t>(
        &self,
        tags: impl Iterator<Item = &'t String>,
    ) -> Vec<w::ChipOption<String>> {
        tags.map(|tag| {
            let style = self.tag_style(tag);
            w::ChipOption::new(tag.clone(), crate::components::tag_badge::value_of(tag))
                .icon(crate::icons::for_key(&style.icon))
                .color(style.color)
        })
        .collect()
    }

    /// The fields of a filter, as chips: the tags (all of them are listed, with a search), the
    /// artists and the albums (any of the chosen ones; initially two lines of chips, found by
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
            self.filter_tag_picker(filter, msgs),
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
            self.bpm_section(filter, msgs.bpm),
        ]
        .spacing(16)
        .width(Length::Fill)
    }

    /// Artists or albums as chips with a search entry: most tracks first, then the "unknown"
    /// one; selection keeps that order. Clamped with an inline chevron, and typing is how the rest is
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
        // Keep choice positions stable when selection changes; only their color changes.
        let mut options: Vec<ChipOption<String>> = choices
            .iter()
            .map(|(name, _)| ChipOption::new(name.clone(), name.clone()))
            .collect();
        if unknown_count > 0 {
            options.push(ChipOption::new(data::UNKNOWN.to_string(), unknown_label));
        }
        for name in selected {
            if !choices.iter().any(|(choice, _)| choice == name)
                && (name != data::UNKNOWN || unknown_count == 0)
            {
                options.push(ChipOption::new(name.clone(), label_of(name)));
            }
        }
        column![
            row![
                ty::heading(title),
                ty::caption("any of").style(iad::styles::text::dim)
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            iced::widget::text_input(placeholder, query)
                .on_input(on_query)
                .padding(10)
                .style(iad::styles::text_input::default),
            ChipGroup::new(options, selected, on_toggle)
                .query(query)
                .max_lines(self.name_chip_line_limit)
                .expandable(self.chip_expanded[picker as usize], move |on| {
                    Message::ChipExpand(picker, on)
                })
                .empty_text("No match"),
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
        let mut spec = filter.search_spec();
        spec.bpm_min = None;
        spec.bpm_max = None;
        let mut bins = vec![0; 12];
        for index in self.backend.matching(&spec).unwrap_or_default() {
            if let Some(track) = self.lib.tracks.get(index)
                && (data::BPM.0..=data::BPM.1).contains(&track.bpm)
            {
                let bin = (((track.bpm - data::BPM.0) / (data::BPM.1 - data::BPM.0) * 12.0)
                    as usize)
                    .min(11);
                bins[bin] += 1;
            }
        }
        let histogram = Histogram::from_counts(data::BPM.0, data::BPM.1, bins);
        let band = |label: &'static str, r: (f32, f32)| -> Element<'a> {
            let b = w::flat_button(ty::label(label)).on_press(on_bpm(r));
            if filter.bpm == r
                && (r != data::BPM
                    || (filter.to_spec().bpm_min.is_none() && filter.to_spec().bpm_max.is_none()))
            {
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
                ty::body(filter.bpm_label()),
            ]
            .align_y(Alignment::Center),
            // One component: the bars and the thumbs edit the same range.
            w::HistogramRangeSlider::new(histogram, (lo, hi), on_bpm)
                .step(5.0)
                .height(48.0),
            row![
                band("Any", data::BPM),
                band("Slow · 60–90", (60.0, 90.0)),
                band("Mid · 90–120", (90.0, 120.0)),
                band("Fast · 120–180", (120.0, 180.0)),
            ]
            .spacing(6)
            .wrap(),
        ]
        .spacing(6)
        .into()
    }

    // ---- jam ----------------------------------------------------------------------------

    /// An icon button with a tiny accent dot when its filter is active.
    pub(super) fn active_filter_icon(
        &self,
        icon: Icon,
        tip: &'static str,
        active: bool,
        on_press: Message,
    ) -> Element<'_> {
        let dot = container(space::horizontal().width(6).height(6)).style(|theme: &iad::Theme| {
            iced::widget::container::Style {
                background: Some(theme.semantic.accent.bg.into()),
                border: iced::Border::default().rounded(3),
                ..Default::default()
            }
        });
        let metrics = iad::Metrics::desktop();
        let button =
            iad::layouts::Indicator::new(crate::icons::icon_button(icon).on_press(on_press), dot)
                .offset((metrics.button_min_height - metrics.icon_size) / 2.0)
                .show(active);
        w::tooltip(button, tip, w::TooltipPosition::Bottom)
    }

    /// Shared tag presentation for picker inputs and page summaries.
    /// Page summaries can omit removal actions while keeping identical labels and spacing.
    pub(super) fn tag_selection_chips<'a>(
        &'a self,
        selected: &[String],
        blocks: &[Vec<weave_player_lib::core::TagFilter>],
        on_toggle: Option<fn(String) -> Message>,
        target: Option<FilterTarget>,
    ) -> Vec<Element<'a>> {
        let mut chips = Vec::new();
        for (block_index, block) in blocks.iter().enumerate() {
            for (index, group) in block.iter().enumerate() {
                chips.push(
                    self.tag_group_chip(group, target.map(|target| (target, block_index, index))),
                );
                if let Some(separator) = tag_group_separator(blocks, block_index, index) {
                    chips.push(ty::label(separator).into());
                }
            }
        }
        if !chips.is_empty() && !selected.is_empty() {
            chips.push(ty::label("AND").into());
        }
        for tag in selected {
            chips.push(self.selection_tag_chip(tag, on_toggle.map(|toggle| toggle(tag.clone()))));
        }
        chips
    }

    fn selection_tag_chip<'a>(&'a self, name: &str, remove: Option<Message>) -> Element<'a> {
        let style = self.tag_style(name);
        let mut chip = w::Chip::new(crate::components::tag_badge::value_of(name))
            .size(ChipSize::Medium)
            .icon(crate::icons::for_key(&style.icon))
            .color(style.color);
        if let Some(remove) = remove {
            chip = chip.on_remove(remove);
        }
        chip.into()
    }

    fn tag_group_chip<'a>(
        &'a self,
        group: &weave_player_lib::core::TagFilter,
        remove: Option<(FilterTarget, usize, usize)>,
    ) -> Element<'a> {
        let parts = tag_group_parts(group)
            .into_iter()
            .filter(|part| {
                !matches!(part, TagGroupPart::Operator("(" | ")"))
                    && !(editable_tag_group(group) && matches!(part, TagGroupPart::Operator("+")))
            })
            .map(|part| match part {
                TagGroupPart::Tag(name) => {
                    let remove = remove.filter(|_| editable_tag_group(group)).map(
                        |(target, block, index)| {
                            Message::RemoveGroupedTag(target, block, index, name.clone())
                        },
                    );
                    self.selection_tag_chip(&name, remove)
                }
                TagGroupPart::Operator(",") if editable_tag_group(group) => ty::label("OR").into(),
                TagGroupPart::Operator(text) => ty::label(text).into(),
            });
        row(parts)
            .spacing(4)
            .align_y(Alignment::Center)
            .wrap()
            .into()
    }

    /// The active filters as chips, each clearing itself. `None` when nothing is active. The search
    /// text is not among them: it lives in the search bar.
    pub(super) fn active_chips(&self) -> Option<Element<'_>> {
        let f = &self.filter;
        let mut chips = self.tag_selection_chips(
            &f.tags,
            &f.retained.tag_blocks,
            Some(Message::ToggleTag),
            Some(FilterTarget::Jam),
        );
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

    /// Jam uses the shared detail header: Play left, filter/sort/save actions right.
    pub fn jam_page(&self) -> Element<'_> {
        let narrow = self.is_narrow();
        let n = self.rows.len();
        let saved = self
            .jam_saved
            .as_ref()
            .filter(|(_, f)| *f == self.filter)
            .map(|(i, _)| *i);
        let save: Element<'_> = match saved {
            Some(i) => w::tooltip(
                crate::icons::icon_button(Icon::Check)
                    .on_press(Message::Open(crate::Dest::Playlist(i))),
                format!("Saved as {}", self.lib.playlists[i].name),
                w::TooltipPosition::Bottom,
            ),
            None => {
                let can = self.filter.is_saveable() && n > 0;
                w::tooltip(
                    self.icon_action(Icon::ListAdd, can.then_some(Message::SaveJamAsPlaylist)),
                    if self.filter.is_saveable() {
                        "Save as playlist"
                    } else {
                        "Add a tag, tempo, artist or album to save"
                    },
                    w::TooltipPosition::Bottom,
                )
            }
        };
        let matches = ty::caption(format!("{n} matches")).style(iad::styles::text::dim);
        let chips = self.active_chips().unwrap_or_else(|| {
            container(
                ty::caption("No filters: every track matches.")
                    .size(iad::Metrics::desktop().space_m + 1.0)
                    .style(iad::styles::text::dim),
            )
            .height(ChipSize::Medium.height(&iad::Metrics::desktop()))
            .align_y(Alignment::Center)
            .into()
        });
        let play = w::suggested_circular_button(Icon::Play.view(20.0))
            .size(44.0)
            .on_press_maybe((n > 0).then_some(Message::StartJam));
        let tag_filters = self.active_filter_icon(
            Icon::Filter,
            "Tag filters",
            self.filter.tag_count() > 0,
            Message::JamFilters(true),
        );
        let cover: Element<'_> = w::avatar(96.0, "Jam")
            .icon(Icon::Sparkle)
            .shape(w::AvatarShape::Rounded)
            .into();
        let header = crate::components::MediaHeader::new("Jam")
            .kind("Tracks by filters")
            .cover(cover)
            .primary(row![play, matches].spacing(12).align_y(Alignment::Center))
            .action(tag_filters)
            .action(self.collection_sort_control())
            .action(save)
            .facts(chips)
            .narrow(narrow);
        let top: Element<'_> = if self.search_open {
            self.page_search_bar()
        } else {
            header.into()
        };
        let mut page = column![top].spacing(8).height(Length::Fill);
        if let Some(error) = &self.query_error {
            page = page.push(ty::caption(error.clone()));
        }
        page = page.push(self.virtual_tracks(RowOpts {
            album: true,
            bpm: true,
            menu: true,
        }));
        page.into()
    }

    // ---- jam filters --------------------------------------------------------------------

    pub(super) fn prepare_filter_editor(&mut self) {
        self.active_tag_group = None;
        self.chip_expanded = [false; 4];
        self.tag_browse = TagBrowse::Categories;
        self.tag_query.clear();
        self.artist_query.clear();
        self.album_query.clear();
    }

    /// Modal context is derived once; presentation never belongs to an individual page.
    pub(super) fn active_filter_context(&self) -> Option<FilterTarget> {
        if self.draft.is_some() {
            Some(FilterTarget::Draft)
        } else if self.page_filters_open {
            Some(FilterTarget::Page)
        } else if self.jam_open && self.jam_filters_open {
            Some(FilterTarget::Jam)
        } else {
            None
        }
    }

    pub(super) fn filter_close(&self, context: FilterTarget) -> Message {
        match context {
            FilterTarget::Jam => Message::JamFilters(false),
            FilterTarget::Page => Message::PageFilters(false),
            FilterTarget::Draft => Message::CancelFilter,
        }
    }

    pub(super) fn filter_editor(&self, context: FilterTarget) -> Element<'_> {
        use crate::components::filter_editor::FilterEditor;
        let close = self.filter_close(context);
        match context {
            FilterTarget::Jam => FilterEditor::new(
                "Tag filters",
                self.filter_fields(&self.filter, FilterMsgs::for_target(context))
                    .into(),
                close.clone(),
                "Done",
                close,
            )
            .subtitle(format!("{} matches", self.matching(&self.filter).len()))
            .action(w::tooltip(
                crate::icons::icon_button(Icon::Dice).on_press(Message::Surprise),
                "Surprise me",
                w::TooltipPosition::Bottom,
            ))
            .action(w::tooltip(
                self.icon_action(
                    Icon::Reset,
                    (self.filter.tag_count() > 0).then_some(Message::ClearTagFilters),
                ),
                "Clear tag filters",
                w::TooltipPosition::Bottom,
            ))
            .into(),
            FilterTarget::Page => FilterEditor::new(
                if self.dest == Dest::AboutMe {
                    "Listening filters"
                } else {
                    "Rediscover filters"
                },
                self.page_filter_fields(),
                close.clone(),
                "Done",
                close,
            )
            .into(),
            FilterTarget::Draft => {
                let draft = self.draft.as_ref().expect("active playlist filter draft");
                FilterEditor::new("Playlist filter", self.playlist_filter_editor(draft), close.clone(), "Save rules", Message::SaveFilter)
                    .subtitle(if self.manual_playlist().is_some() {
                        "Rules limit the tracks offered when adding. Saving changes asks before removing disqualified tracks."
                    } else { "The playlist holds every track that matches these rules, now and as the library changes." })
                    .cancel(close).into()
            }
        }
    }

    // ---- save as playlist --------------------------------------------------------------

    /// What Save as playlist asks: a name, and what will be kept (and what will not).
    fn save_playlist_form(&self) -> Element<'_> {
        let name = self.save_name.clone().unwrap_or_default();
        let taken = self.name_is_taken(&name);
        let can = !name.trim().is_empty() && !taken;
        let mut col = column![
            ty::title_3(if self.save_manual {
                "New playlist"
            } else {
                "Save as playlist"
            }),
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
        if self.save_manual {
            col = col
                .push(ty::caption(
                    "Rules limit the tracks offered when adding. Your playlist starts empty.",
                ))
                .push(self.playlist_filter_editor(&self.creation_filter));
        } else {
            col = col.push(
                ty::caption(format!("Saves: {}.", self.filter_sentence(&self.filter)))
                    .style(iad::styles::text::dim),
            );
        }
        if !self.save_manual && !self.filter.q.trim().is_empty() {
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
                w::tooltip(
                    crate::icons::icon_button(Icon::WindowClose).on_press(Message::CancelSaveJam),
                    "Cancel",
                    w::TooltipPosition::Bottom
                ),
                w::tooltip(
                    self.icon_action(Icon::Check, can.then_some(Message::ConfirmSaveJam)),
                    "Save playlist",
                    w::TooltipPosition::Bottom
                ),
            ]
            .spacing(8),
        );
        w::FocusScope::new(col)
            .trap(false)
            .initial_focus(SAVE_NAME_ID)
            .into()
    }

    pub fn create_playlist_page(&self) -> Element<'_> {
        iad::prelude::surface::scrolling(self.save_playlist_form())
    }

    /// Shared rules editor for creation and editing a playlist.
    fn playlist_filter_editor<'a>(&'a self, filter: &'a Filter) -> Element<'a> {
        column![
            ty::caption(self.filter_sentence(filter)),
            self.filter_fields(filter, FilterMsgs::for_target(FilterTarget::Draft)),
        ]
        .spacing(8)
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

    /// Uses the same engine search and row controls as the library, without changing page search.
    pub fn playlist_tracks_form(&self) -> Element<'_> {
        let Dest::Playlist(index) = self.dest else {
            return space().into();
        };
        let playlist = &self.lib.playlists[index];
        let results: Element<'_> = match self
            .backend
            .playlist_add_candidates(&playlist.name, &self.playlist_tracks_query)
        {
            Ok(ids) => {
                let mut rows = column![].spacing(4);
                if ids.is_empty() {
                    rows = rows.push(ty::caption("No tracks match these rules and search."));
                }
                for id in ids {
                    let track = self.track(id);
                    let present = playlist.tracks.contains(&id);
                    rows = rows.push(
                        w::action_row(track.title.clone())
                            .subtitle(track.by_line())
                            .prefix(cover(track.id, 40.0))
                            .suffix(w::tooltip(
                                crate::icons::icon_button(if present {
                                    Icon::Delete
                                } else {
                                    Icon::ListAdd
                                })
                                .on_press(Message::PlaylistTrackToggle(id)),
                                if present {
                                    "Remove from playlist"
                                } else {
                                    "Add to playlist"
                                },
                                w::TooltipPosition::Bottom,
                            )),
                    );
                }
                scrollable(rows).height(Length::Fill).into()
            }
            Err(error) => ty::body(format!("Cannot search: {error}")).into(),
        };
        column![
            row![
                ty::title_3("Playlist tracks"),
                space::horizontal(),
                w::tooltip(
                    crate::icons::icon_button(Icon::WindowClose)
                        .on_press(Message::PlaylistTracksOpen(false)),
                    "Done",
                    w::TooltipPosition::Bottom
                ),
            ]
            .align_y(Alignment::Center),
            w::search_bar(&self.playlist_tracks_query, Message::PlaylistTracksQuery)
                .placeholder("Search library tracks"),
            results,
        ]
        .spacing(12)
        .height(Length::Fill)
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
                w::EntryRow::new(
                    "Name",
                    w::EntryInput::Text(
                        w::entry("", name.as_str())
                            .on_input(Message::NameInput)
                            .on_submit(Message::SaveDetails),
                    ),
                )
                .into(),
            );
        }
        rows.push(
            w::EntryRow::new(
                "Description",
                w::EntryInput::Text(
                    w::entry("", draft.description.as_str())
                        .on_input(Message::DescInput)
                        .on_submit(Message::SaveDetails),
                ),
            )
            .into(),
        );
        let fields = container(w::boxed_list(rows)).width(Length::Fill);
        column![
            row![
                ty::title_3("Edit details"),
                space::horizontal(),
                crate::icons::icon_button(Icon::WindowClose).on_press(Message::CancelDetails),
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

    pub fn manual_filter_confirmation<'a>(
        &'a self,
        review: &'a crate::ManualFilterReview,
    ) -> Element<'a> {
        let n = review.removed.len();
        container(column![
            ty::title_3(if n == 0 { "Save rule changes?".to_string() } else { format!("Remove {n} track{}?", if n == 1 { "" } else { "s" }) }),
            ty::body(if n == 0 {
                "These changes disqualify 0 tracks. All curated tracks will stay in the playlist.".to_string()
            } else {
                format!("{n} track{} in “{}” will no longer qualify under these rules and will be removed from the playlist.", if n == 1 { "" } else { "s" }, review.name)
            }),
            row![
                space::horizontal(),
                w::tooltip(crate::icons::icon_button(Icon::WindowClose).on_press(Message::CancelManualFilterReview), "Keep editing", w::TooltipPosition::Bottom),
                w::tooltip(crate::icons::icon_button(Icon::Check).on_press(Message::ConfirmManualFilter), if n == 0 { "Save rules" } else { "Save rules and remove tracks" }, w::TooltipPosition::Bottom),
            ].spacing(8),
        ].spacing(12))
        .padding(20)
        .width(Length::Fixed((self.win_w - 40.0).clamp(200.0, 440.0)))
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
                    scrollable(self.tag_picker(
                        &e.tags,
                        Message::EditToggleTag,
                        ChipPicker::TrackTags
                    ))
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
            .map(|g| tag_pill(g, &self.tag_style(g), ChipSize::Small)))
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

pub(super) fn filter_scroll_direction() -> iced::widget::scrollable::Direction {
    iced::widget::scrollable::Direction::Vertical(
        iced::widget::scrollable::Scrollbar::new()
            .width(4)
            .scroller_width(4),
    )
}

#[derive(Debug, PartialEq)]
enum TagGroupPart {
    Tag(String),
    Operator(&'static str),
}

fn tag_group_parts(group: &weave_player_lib::core::TagFilter) -> Vec<TagGroupPart> {
    use TagGroupPart::{Operator, Tag};
    use weave_player_lib::core::TagOp;

    fn append(
        parts: &mut Vec<TagGroupPart>,
        names: impl IntoIterator<Item = String>,
        separator: &'static str,
        exact: bool,
    ) {
        for (index, name) in names.into_iter().enumerate() {
            if index > 0 {
                parts.push(Operator(separator));
            }
            if exact {
                parts.push(Operator("="));
            }
            parts.push(Tag(name));
        }
    }

    let mut parts = Vec::new();
    append(&mut parts, group.all.clone(), "+", false);
    for (names, prefix, suffix) in [(&group.any, "(", ")"), (&group.exclude, "without", "")] {
        if !names.is_empty() {
            if !parts.is_empty() {
                parts.push(Operator("+"));
            }
            parts.push(Operator(prefix));
            append(&mut parts, names.clone(), ",", false);
            if !suffix.is_empty() {
                parts.push(Operator(suffix));
            }
        }
    }
    for clause in &group.clauses {
        if !parts.is_empty() {
            parts.push(Operator("+"));
        }
        match clause.op {
            TagOp::Any => parts.push(Operator("(")),
            TagOp::Exclude => parts.push(Operator("without")),
            TagOp::All => {}
        }
        for (index, alternatives) in clause.names().into_iter().enumerate() {
            if index > 0 {
                parts.push(Operator(if clause.op == TagOp::All { "+" } else { "," }));
            }
            append(&mut parts, alternatives, ",", clause.exact);
        }
        if clause.op == TagOp::Any {
            parts.push(Operator(")"));
        }
    }
    parts
}

pub(super) fn tag_group_label(group: &weave_player_lib::core::TagFilter) -> String {
    let mut parts = group.all.clone();
    if !group.any.is_empty() {
        parts.push(format!("({})", group.any.join(", ")));
    }
    if !group.exclude.is_empty() {
        parts.push(format!("without {}", group.exclude.join(", ")));
    }
    for clause in &group.clauses {
        use weave_player_lib::core::TagOp;
        let values = clause
            .names()
            .into_iter()
            .map(|alternatives| {
                alternatives
                    .into_iter()
                    .map(|name| {
                        if clause.exact {
                            format!("={name}")
                        } else {
                            name
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .collect::<Vec<_>>();
        parts.push(match clause.op {
            TagOp::All => values.join(" + "),
            TagOp::Any => format!("({})", values.join(", ")),
            TagOp::Exclude => format!("without {}", values.join(", ")),
        });
    }
    parts.join(" + ")
}

impl App {
    /// One row for system, light, and dark appearance, using application-owned icons.
    pub(super) fn theme_mode_row(&self) -> Element<'_> {
        let metrics = iad::Metrics::desktop();
        let choice = |mode, icon: Icon, hint| {
            w::ToggleItem::custom(
                mode,
                w::tooltip(
                    icon.view(metrics.icon_size),
                    hint,
                    w::TooltipPosition::Bottom,
                ),
            )
        };
        let modes = w::toggle_group(
            vec![
                choice(None, Icon::Window, "Follow desktop appearance"),
                choice(Some(iad::Scheme::Light), Icon::Sun, "Light appearance"),
                choice(Some(iad::Scheme::Dark), Icon::Moon, "Dark appearance"),
            ],
            Some(self.theme_mode),
            Message::ThemeMode,
        );
        w::action_row("Theme mode")
            .suffix(modes)
            .wrap_suffix(false)
            .into()
    }

    /// One theme chooser shared by the desktop dialog and mobile bottom sheet.
    pub(super) fn themes_form(&self) -> Element<'_> {
        let mut choices = column![].spacing(6).width(Length::Fill);
        let mut desktop = w::action_row("Desktop theme")
            .subtitle("Follow the desktop appearance")
            .on_activate(Message::SelectTheme(None));
        if self.themes.selected.is_none() {
            desktop = desktop.suffix(Icon::Check.view(16.0));
        }
        choices = choices.push(desktop);
        for theme in &self.themes.themes {
            let accent = theme
                .config
                .light_dark(self.effective_scheme(), "accent", None)
                .unwrap_or_else(|| self.theme().semantic.accent.bg);
            let swatch = container(space::horizontal().width(22).height(22)).style(move |_| {
                iced::widget::container::Style {
                    background: Some(accent.into()),
                    border: iced::Border::default().rounded(11),
                    ..Default::default()
                }
            });
            let mut choice = w::action_row(theme.name.clone())
                .subtitle(theme.key.clone())
                .prefix(swatch)
                .on_activate(Message::SelectTheme(Some(theme.key.clone())));
            if self.themes.selected.as_deref() == Some(theme.key.as_str()) {
                choice = choice.suffix(Icon::Check.view(16.0));
            }
            choices = choices.push(choice);
        }
        if self.themes.themes.is_empty() {
            choices = choices.push(ty::caption("Import a theme file to add it here."));
        }
        let mut import = w::suggested_button(Icon::Document.view(16.0));
        if !self.theme_import_pending {
            import = import.on_press(Message::ImportTheme);
        }
        column![
            row![
                ty::title_3("Themes"),
                space::horizontal(),
                crate::icons::icon_button(Icon::WindowClose).on_press(Message::Themes(false))
            ]
            .align_y(Alignment::Center),
            ty::caption("Choose a theme to apply it immediately."),
            self.theme_mode_row(),
            scrollable(choices)
                .direction(filter_scroll_direction())
                .height(Length::Fill),
            row![
                w::tooltip(
                    import,
                    if self.theme_import_pending {
                        "Choosing theme…"
                    } else {
                        "Import theme…"
                    },
                    w::TooltipPosition::Top
                ),
                space::horizontal(),
                w::tooltip(
                    crate::icons::icon_button(Icon::Check).on_press(Message::Themes(false)),
                    "Done",
                    w::TooltipPosition::Top
                )
            ]
            .align_y(Alignment::Center),
        ]
        .spacing(12)
        .height(Length::Fill)
        .width(Length::Fill)
        .into()
    }
}

#[cfg(test)]
mod tag_group_tests {
    use super::*;
    use TagGroupPart::{Operator, Tag};
    use weave_player_lib::core::{TagClause, TagFilter, TagOp};

    #[test]
    fn separators_only_appear_between_groups() {
        let group = TagFilter {
            any: vec!["genre:bachata".into()],
            ..Default::default()
        };
        let single = vec![vec![group.clone()]];
        assert_eq!(tag_group_separator(&single, 0, 0), None);
        let alternatives = vec![vec![group.clone(), group.clone()]];
        assert_eq!(tag_group_separator(&alternatives, 0, 0), Some(","));
        assert_eq!(tag_group_separator(&alternatives, 0, 1), None);
        let blocks = vec![vec![group.clone()], vec![group]];
        assert_eq!(tag_group_separator(&blocks, 0, 0), Some("AND"));
        assert_eq!(tag_group_separator(&blocks, 1, 0), None);
    }

    #[test]
    fn or_groups_keep_individual_tag_identities_for_styled_pills() {
        let group = TagFilter {
            any: vec!["genre:bachata".into(), "mood:romantic".into()],
            ..Default::default()
        };
        assert_eq!(
            tag_group_parts(&group),
            vec![
                Operator("("),
                Tag("genre:bachata".into()),
                Operator(","),
                Tag("mood:romantic".into()),
                Operator(")"),
            ]
        );
    }

    #[test]
    fn grouped_clauses_preserve_operators_and_exact_matching() {
        let group = TagFilter {
            all: vec!["mood:romantic".into()],
            exclude: vec!["genre:rock".into()],
            clauses: vec![TagClause {
                op: TagOp::Any,
                keys: vec!["genre".into(), "style".into()],
                values: vec!["bachata".into()],
                exact: true,
            }],
            ..Default::default()
        };
        assert_eq!(
            tag_group_parts(&group),
            vec![
                Tag("mood:romantic".into()),
                Operator("+"),
                Operator("without"),
                Tag("genre:rock".into()),
                Operator("+"),
                Operator("("),
                Operator("="),
                Tag("genre:bachata".into()),
                Operator(","),
                Operator("="),
                Tag("style:bachata".into()),
                Operator(")"),
            ]
        );
    }
}
