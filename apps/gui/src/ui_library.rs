//! Library collections, Explore and measured listening overview. Charts are deliberately deferred.
use crate::ui_dialogs::FilterMsgs;
use crate::{App, Dest, Element, Message};
use iad::prelude::{self as w, surface, typography as ty};
use iced::widget::{column, container, row, scrollable, space};
use iced::{Alignment, Length};
use iced_adaptive_design::{self as iad, Icon};
use weave_player_lib::core::{AnalyticsPeriod, AnalyticsQuery, ListeningAnalytics, SortField};

impl App {
    pub fn collection_order_index(&self) -> Option<usize> {
        match self.dest {
            Dest::Tracks => Some(0),
            Dest::Artists => Some(1),
            Dest::Albums => Some(2),
            Dest::Playlists => Some(3),
            _ => None,
        }
    }
    pub fn collection_order(&self) -> weave_player_lib::core::CollectionOrder {
        self.collection_order_index()
            .and_then(|i| self.collection_orders[i])
            .unwrap_or_default()
    }
    pub fn collection_artist_indices(
        &self,
        query: &str,
        order: weave_player_lib::core::CollectionOrder,
    ) -> Result<Vec<usize>, String> {
        let artists = self.lib.artists();
        let mut indices: Vec<_> = self
            .backend
            .artist_names(
                query,
                if self.collection_order_index().is_some() {
                    self.selected_collection_order()
                } else {
                    Some(order)
                },
                self.collection_descending(),
            )?
            .iter()
            .filter_map(|name| artists.iter().position(|a| &a.0 == name))
            .collect();
        let mut seen = std::collections::HashSet::new();
        indices.retain(|i| seen.insert(*i));
        Ok(indices)
    }
    pub fn collection_album_indices(
        &self,
        query: &str,
        order: weave_player_lib::core::CollectionOrder,
    ) -> Result<Vec<usize>, String> {
        let albums = self.lib.albums();
        let mut indices: Vec<_> = self
            .backend
            .album_names(
                query,
                if self.collection_order_index().is_some() {
                    self.selected_collection_order()
                } else {
                    Some(order)
                },
                self.collection_descending(),
            )?
            .iter()
            .filter_map(|name| {
                albums
                    .iter()
                    .position(|a| (&a.0, &a.1) == (&name.0, &name.1))
            })
            .collect();
        let mut seen = std::collections::HashSet::new();
        indices.retain(|i| seen.insert(*i));
        Ok(indices)
    }
    pub fn collection_playlist_indices(
        &self,
        query: &str,
        order: weave_player_lib::core::CollectionOrder,
    ) -> Result<Vec<usize>, String> {
        Ok(self
            .backend
            .playlist_names(
                query,
                if self.collection_order_index().is_some() {
                    self.selected_collection_order()
                } else {
                    Some(order)
                },
                self.collection_descending(),
            )?
            .iter()
            .filter_map(|name| self.lib.playlists.iter().position(|p| &p.name == name))
            .collect())
    }
    pub fn collection_descending(&self) -> bool {
        if self.jam_open {
            self.jam_descending
        } else if let Some(index) = self.collection_order_index() {
            self.collection_descending[index]
        } else {
            self.detail_descending
        }
    }
    pub fn selected_collection_order(&self) -> Option<weave_player_lib::core::CollectionOrder> {
        if self.jam_open {
            self.jam_order
        } else if self.collection_order_index().is_some() {
            self.collection_orders[self.collection_order_index().unwrap()]
        } else {
            self.detail_order
        }
    }
    pub fn collection_sort_menu(&self, wrap: impl Fn(Message) -> Message) -> w::Menu<Message> {
        use weave_player_lib::core::CollectionOrder;
        let selected = self.selected_collection_order();
        w::Menu::new("Sort")
            .check_style(w::MenuCheckStyle::Mark)
            .section(
                [
                    CollectionOrder::Recent,
                    CollectionOrder::ReleaseDate,
                    CollectionOrder::AddedDate,
                    CollectionOrder::Rating,
                ]
                .map(|order| {
                    w::MenuItem::check(
                        if selected == Some(order) {
                            format!(
                                "{order} {}",
                                if self.collection_descending() {
                                    "↓"
                                } else {
                                    "↑"
                                }
                            )
                        } else {
                            order.to_string()
                        },
                        selected == Some(order),
                        wrap(Message::CollectionOrder(order)),
                    )
                }),
            )
    }
    pub fn collection_sort_control(&self) -> Element<'_> {
        if self.playlist_edit.is_some() {
            return w::tooltip(
                self.icon_action(Icon::GoUpDown, None),
                "Finish editing to sort",
                w::TooltipPosition::Bottom,
            );
        }
        if self.is_narrow() {
            w::icon_button(Icon::GoUpDown)
                .on_press(Message::OpenSheet(Some(crate::Sheet::Sort)))
                .into()
        } else {
            self.menu_button(Icon::GoUpDown, "Sort", self.collection_sort_menu(|m| m))
        }
    }
    pub fn collection_header(&self, title: &str, count: usize) -> Element<'_> {
        if self.search_open {
            return self.page_search_bar();
        }
        let icon = match self.dest {
            Dest::Artists => Icon::MusicNote,
            Dest::Albums => Icon::Document,
            Dest::Playlists => Icon::Folder,
            _ => Icon::Queue,
        };
        let mut header = crate::components::MediaHeader::new(title)
            .kind("Your library")
            .cover(
                w::avatar(
                    if self.is_narrow() { 80.0 } else { 112.0 },
                    title.to_string(),
                )
                .icon(icon),
            )
            .facts(ty::caption(if self.collection_query.trim().is_empty() {
                format!("{count} items")
            } else {
                format!("{count} items · Search: {}", self.collection_query)
            }))
            .narrow(self.is_narrow());
        if self.dest == Dest::Playlists {
            header = header.action(w::tooltip(
                w::icon_button(Icon::ListAdd).on_press(Message::NewManualPlaylist),
                "New playlist",
                w::TooltipPosition::Bottom,
            ));
        }
        header = header.action(self.collection_sort_control());
        if self.dest == Dest::Tracks {
            header = header.primary(self.play_all()).action(
                w::IconToggleButton::new(Icon::Shuffle, self.player.shuffle)
                    .size(20.0)
                    .tooltip("Shuffle")
                    .on_toggle(|_| Message::Shuffle),
            );
        }
        header.into()
    }
    pub fn collection_toolbar(&self, title: &str, count: usize) -> Element<'_> {
        if self.search_open {
            return self.page_search_bar();
        }
        surface::toolbar(
            row![
                ty::heading(title.to_string()),
                space::horizontal(),
                ty::caption(format!("{count} items")),
            ]
            .align_y(Alignment::Center),
        )
    }
    pub fn collection_list(&self, kind: Dest) -> Element<'_> {
        let order = self.collection_order();
        let result: Result<Vec<Element<'_>>, String> = match kind {
            Dest::Artists => self
                .collection_artist_indices(&self.collection_query, order)
                .map(|indices| {
                    indices
                        .into_iter()
                        .map(|i| self.artist_card(i, None))
                        .collect()
                }),
            Dest::Albums => self
                .collection_album_indices(&self.collection_query, order)
                .map(|indices| {
                    indices
                        .into_iter()
                        .map(|i| self.album_card(i, true))
                        .collect()
                }),
            _ => self
                .collection_playlist_indices(&self.collection_query, order)
                .map(|indices| indices.into_iter().map(|i| self.playlist_card(i)).collect()),
        };
        let title = match kind {
            Dest::Artists => "All artists",
            Dest::Albums => "All albums",
            _ => "All playlists",
        };
        match result {
            Ok(items) => {
                let n = items.len();
                let body: Element<'_> = if n == 0 {
                    ty::body(if self.collection_query.is_empty() {
                        "Nothing here yet"
                    } else {
                        "No matching results"
                    })
                    .into()
                } else {
                    self.media_grid(items)
                };
                column![self.collection_header(title, n), surface::scrolling(body)]
                    .height(Length::Fill)
                    .into()
            }
            Err(error) => column![
                self.collection_header(title, 0),
                ty::body(format!("Cannot search: {error}"))
            ]
            .into(),
        }
    }
    pub fn explore_group<'a>(
        &'a self,
        index: usize,
        title: &str,
        body: Element<'a>,
    ) -> Element<'a> {
        self.section_group(
            title,
            self.explore_open[index],
            Message::ExploreSection(index),
            body,
        )
    }
    pub(super) fn section_group<'a>(
        &'a self,
        title: &str,
        open: bool,
        on_toggle: Message,
        body: Element<'a>,
    ) -> Element<'a> {
        let mut group = column![
            w::SectionDivider::new(title.to_string())
                .collapsible(true)
                .default_collapse_state(w::CollapseState::Expanded)
                .open(open)
                .chevron(true)
                .on_toggle(on_toggle)
        ]
        .spacing(8);
        if open {
            group = group.push(body);
        }
        group.into()
    }
    pub fn explore_page(&self) -> Element<'_> {
        let link = |label: &str, description: &str, icon: Icon, dest| -> Element<'_> {
            w::action_row(label.to_string())
                .subtitle(description.to_string())
                .prefix(icon.view(20.0))
                .chevron(true)
                .on_activate(Message::Open(dest))
                .into()
        };
        let library = column![
            link("All tracks", "Browse your music", Icon::Queue, Dest::Tracks),
            link(
                "All artists",
                "Browse artist collections",
                Icon::MusicNote,
                Dest::Artists
            ),
            link("All albums", "Browse albums", Icon::Document, Dest::Albums),
            link(
                "All playlists",
                "Manual and dynamic collections",
                Icon::Folder,
                Dest::Playlists
            ),
        ]
        .spacing(4)
        .into();
        let explore = column![
            link(
                "About me",
                "Your measured listening history",
                Icon::Home,
                Dest::AboutMe
            ),
            link(
                "Rediscover",
                "Previously played music, inactive for 180 days by default",
                Icon::Clock,
                Dest::Rediscover,
            ),
        ]
        .spacing(4)
        .into();
        let tracks: Vec<_> = self
            .lib
            .most_listened_tracks(5)
            .into_iter()
            .cloned()
            .collect();
        let track_body = if tracks.is_empty() {
            ty::body("Play music to see your most listened tracks").into()
        } else {
            self.plain_list(
                &tracks,
                crate::ui_lists::RowOpts {
                    album: true,
                    bpm: false,
                    menu: true,
                },
            )
        };
        let albums = self.backend.top_albums(6);
        let mut album_rows = column![].spacing(4);
        match albums {
            Ok(albums) => {
                if albums.is_empty() {
                    album_rows =
                        album_rows.push(ty::body("Play music to see your most listened albums"));
                }
                for (name, artist, plays) in albums {
                    if let Some(index) = self
                        .lib
                        .albums()
                        .iter()
                        .position(|a| a.0 == name && a.1 == artist)
                    {
                        album_rows = album_rows.push(
                            w::action_row(name)
                                .subtitle(format!("{artist} · {plays} starts"))
                                .chevron(true)
                                .on_activate(Message::Open(Dest::Album(index))),
                        );
                    }
                }
            }
            Err(error) => album_rows = album_rows.push(ty::caption(error)),
        }
        let mut artists = column![].spacing(4);
        let popular = self.lib.most_listened_artists(6);
        if popular.is_empty() {
            artists = artists.push(ty::body("Play music to see your most listened artists"));
        }
        for (index, plays) in popular {
            if let Some((name, _)) = self.lib.artists().get(index) {
                artists = artists.push(
                    w::action_row(name.clone())
                        .subtitle(format!("{plays} starts"))
                        .chevron(true)
                        .on_activate(Message::Open(Dest::Artist(index))),
                );
            }
        }
        surface::scrolling(
            column![
                self.explore_group(0, "Library", library),
                self.explore_group(1, "Explore", explore),
                self.explore_group(2, "Most listened tracks", track_body),
                self.explore_group(3, "Most listened albums", album_rows.into()),
                self.explore_group(4, "Most listened artists", artists.into()),
            ]
            .spacing(20),
        )
    }
    pub fn rediscover_page(&self) -> Element<'_> {
        let header = crate::components::MediaHeader::new("Rediscover")
            .kind("Your library")
            .cover(w::avatar(80.0, "Rediscover").icon(Icon::Clock))
            .description(ty::body(format!(
                "Previously played, inactive for {} days",
                self.while_days
            )))
            .facts(ty::caption(format!("{} matches", self.rows.len())))
            .primary(
                w::suggested_circular_button(self.action_icon(
                    Icon::Play,
                    20.0,
                    !self.rows.is_empty(),
                ))
                .on_press_maybe(
                    (!self.rows.is_empty())
                        .then(|| Message::PlayAll(self.rows.iter().map(|t| t.id).collect())),
                ),
            )
            .action(self.active_filter_icon(
                Icon::Filter,
                "Filters",
                self.rediscover_filter.tag_count() > 0 || self.while_days != 180,
                Message::PageFilters(true),
            ))
            .narrow(self.is_narrow());
        let header: Element<'_> = if self.search_open {
            self.page_search_bar()
        } else {
            header.into()
        };
        let mut page = column![header].spacing(8);
        if let Some(error) = &self.query_error {
            page = page.push(ty::body(error.clone()));
        } else if self.rows.is_empty() {
            page = page.push(ty::body(
                "No tracks match yet. Change the inactivity period or filters.",
            ));
        }
        page.push(self.virtual_tracks(crate::ui_lists::RowOpts {
            album: true,
            bpm: false,
            menu: true,
        }))
        .height(Length::Fill)
        .into()
    }
    pub fn page_filters_view(&self) -> Element<'_> {
        let about = self.dest == Dest::AboutMe;
        let filter = if about {
            &self.analytics_filter
        } else {
            &self.rediscover_filter
        };
        let msgs = FilterMsgs {
            tag: Message::PageTag,
            artist: Message::PageArtist,
            album: Message::PageAlbum,
            bpm: Message::PageBpm,
            target: crate::FilterTarget::Page,
        };
        let mut fields = self.filter_fields(filter, msgs);
        fields = fields
            .push(ty::heading("Minimum rating"))
            .push(w::toggle_group(
                (0u8..=5)
                    .map(|rating| {
                        w::ToggleItem::new(
                            rating,
                            if rating == 0 {
                                "Any".into()
                            } else {
                                format!("{rating}★")
                            },
                        )
                    })
                    .collect(),
                Some(filter.retained.rating_min.unwrap_or(0)),
                Message::PageRating,
            ));
        if !about {
            fields = fields.push(ty::heading("Inactive for (days)")).push(
                w::entry("180", self.rediscover_days.clone()).on_input(Message::RediscoverDays),
            );
            if self.rediscover_days.parse::<u32>().is_err() {
                fields = fields.push(ty::caption("Enter a non-negative whole number"));
            }
            fields = fields.push(ty::heading("Order by")).push(w::toggle_group(
                vec![
                    w::ToggleItem::new(SortField::Preference, "Preference"),
                    w::ToggleItem::new(SortField::Rating, "Rating"),
                    w::ToggleItem::new(SortField::PlayCount, "Starts"),
                ],
                Some(self.rediscover_sort),
                Message::RediscoverSort,
            ));
        }
        column![
            row![
                ty::title_3(if about {
                    "Listening filters"
                } else {
                    "Rediscover filters"
                }),
                space::horizontal(),
                w::suggested_button(ty::label("Done")).on_press(Message::PageFilters(false))
            ]
            .align_y(Alignment::Center),
            scrollable(fields)
                .direction(crate::ui_dialogs::filter_scroll_direction())
                .height(Length::Fill)
        ]
        .spacing(12)
        .into()
    }
    pub fn analytics_query(&self) -> Result<AnalyticsQuery, String> {
        let mut query = AnalyticsQuery {
            period: self.analytics_period,
            filter: self.analytics_filter.search_spec(),
            ..Default::default()
        };
        if self.analytics_period.is_none() {
            let (Some(start), Some(end)) = (self.analytics_range.start, self.analytics_range.end)
            else {
                return Err("Choose both dates for the custom range".into());
            };
            query.since_ms = Some(start.to_days() * 86_400_000);
            query.until_ms = Some(end.add_days(1).to_days() * 86_400_000);
        }
        Ok(query)
    }
    pub fn about_page(&self) -> Element<'_> {
        let header = w::ProfileHeader::new(
            "About me",
            w::avatar(80.0, "Me"),
            ty::body("Your measured listening history"),
        )
        .top_right(self.active_filter_icon(
            Icon::Filter,
            "Filters",
            self.analytics_filter.tag_count() > 0,
            Message::PageFilters(true),
        ))
        .narrow(self.is_narrow());
        let periods = w::toggle_group(
            vec![
                w::ToggleItem::new(Some(AnalyticsPeriod::Day), "24 hours"),
                w::ToggleItem::new(Some(AnalyticsPeriod::Week), "7 days"),
                w::ToggleItem::new(Some(AnalyticsPeriod::Month), "Month"),
                w::ToggleItem::new(Some(AnalyticsPeriod::Year), "Year"),
                w::ToggleItem::new(None, "Custom"),
            ],
            self.analytics_period.map(Some).or(Some(None)),
            Message::AnalyticsPeriod,
        );
        let mut page = column![header, periods].spacing(16);
        if self.analytics_period.is_none() {
            page = page
                .push(
                    w::DateRangeInput::new(
                        &self.analytics_date_text,
                        self.analytics_date_view,
                        Message::AnalyticsDateText,
                    )
                    .selected(self.analytics_range)
                    .on_pick(Message::AnalyticsDatePick)
                    .on_view(Message::AnalyticsDateView)
                    .width(Length::Fill),
                )
                .push(ty::caption(
                    "UTC calendar dates, including the selected end date",
                ));
        }
        if self.analytics_loading {
            page = page.push(ty::body("Loading listening statistics…"));
        }
        match &self.analytics_result {
            Some(Ok(stats)) => {
                if stats.listens == 0 {
                    page=page.push(ty::body("No measured listening in this range. Statistics accumulate while audio plays; old start counts are not listening time."));
                }
                let value = |share: &Option<weave_player_lib::core::ListeningShare>| {
                    share
                        .as_ref()
                        .map(|s| s.name.clone())
                        .unwrap_or_else(|| "—".into())
                };
                let metrics = vec![
                    (
                        "Listening hours",
                        format!("{:.2}", stats.listened_ms as f64 / 3_600_000.0),
                    ),
                    ("Unique tracks", stats.unique_tracks.to_string()),
                    (
                        "Favorite artist (mean rating)",
                        value(&stats.favorite_artist),
                    ),
                    ("Favorite track (rating)", value(&stats.favorite_track)),
                    ("Most listened artist", value(&stats.most_listened_artist)),
                    ("Most listened track", value(&stats.most_listened_track)),
                    ("Most listened tag", value(&stats.most_listened_tag)),
                    (
                        "Completion rate",
                        stats
                            .completion_rate
                            .map(|r| format!("{:.1}%", r * 100.0))
                            .unwrap_or_else(|| "—".into()),
                    ),
                    (
                        "Replay rate",
                        stats
                            .replay_rate
                            .map(|r| format!("{:.1}%", r * 100.0))
                            .unwrap_or_else(|| "—".into()),
                    ),
                    (
                        "Repeat frequency",
                        format!("{:.2} per day", stats.repeats_per_day),
                    ),
                    ("Completed listens", stats.completed_listens.to_string()),
                    (
                        "Longest session",
                        format!(
                            "{:.1} min",
                            stats.longest_session.as_ref().map_or(0, |s| s.listened_ms) as f64
                                / 60_000.0
                        ),
                    ),
                    (
                        "Most replayed track",
                        stats
                            .most_replayed_track
                            .as_ref()
                            .map(|s| {
                                self.lib
                                    .tracks
                                    .iter()
                                    .find(|t| t.slug == s.track)
                                    .map(|t| t.title.clone())
                                    .unwrap_or_else(|| s.track.clone())
                            })
                            .unwrap_or_else(|| "—".into()),
                    ),
                ];
                page = page.push(
                    w::FlowGrid::new(
                        metrics
                            .into_iter()
                            .map(|(label, value)| {
                                container(
                                    column![ty::caption(label), ty::title_3(value)].spacing(4),
                                )
                                .padding(12)
                                .style(iad::styles::container::card)
                                .into()
                            })
                            .collect::<Vec<Element<'_>>>(),
                    )
                    .max_width(260.0)
                    .spacing(12.0),
                );
                page = page.push(ty::caption(
                    "Favorites use explicit ratings; most listened uses measured duration.",
                ));
            }
            Some(Err(error)) => {
                page = page.push(ty::body(format!("Cannot show statistics: {error}")))
            }
            None => {}
        }
        surface::scrolling(page)
    }
    pub fn refresh_analytics(&mut self) -> iced::Task<Message> {
        self.analytics_request += 1;
        let request = self.analytics_request;
        self.last_analytics = std::time::Instant::now();
        let query = match self.analytics_query() {
            Ok(q) => q,
            Err(e) => {
                self.analytics_loading = false;
                self.analytics_result = Some(Err(e));
                return iced::Task::none();
            }
        };
        if self.analytics_last_query.as_ref() != Some(&query) {
            self.analytics_result = None;
        }
        self.analytics_last_query = Some(query.clone());
        if let Some(db) = self.backend.db().map(|p| p.to_path_buf()) {
            self.analytics_loading = true;
            iced::Task::perform(load_analytics(db, query), move |result| {
                Message::AnalyticsLoaded(request, Box::new(result))
            })
        } else {
            self.analytics_result = Some(self.backend.analytics(&query));
            self.analytics_loading = false;
            iced::Task::none()
        }
    }
}
async fn load_analytics(
    db: std::path::PathBuf,
    query: AnalyticsQuery,
) -> Result<ListeningAnalytics, String> {
    let (tx, rx) = iced::futures::channel::oneshot::channel();
    std::thread::spawn(move || {
        let result = weave_player_lib::Library::open(&db)
            .and_then(|lib| lib.listening_analytics(&query))
            .map_err(|e| e.to_string());
        let _ = tx.send(result);
    });
    rx.await
        .unwrap_or_else(|_| Err("Statistics loading stopped".into()))
}
