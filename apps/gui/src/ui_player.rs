//! The player: the bar (wide), the mini player (narrow), Now Playing and the queue sheet.

use iad::prelude::{
    self as w, ChipSize, Menu, MenuItem, ReorderableList, SyncedText, typography as ty,
};
use iad::styles as style;
use iced::widget::{button, column, container, mouse_area, row, space, vertical_slider};
use iced::{Alignment, Color, Length};
use iced_adaptive_design as iad;
use weave_icons::Icon;

use crate::components::tag_badge::tag_pill;
use crate::data::{Track, clock};
use crate::icons::cover;
use crate::{App, Element, LYRICS, Message};
use iad::prelude::surface;

impl App {
    fn play_pause(&self, size: f32) -> Element<'_> {
        let g = if self.player.playing {
            Icon::Pause
        } else {
            Icon::Play
        };
        w::suggested_button(g.view(size))
            .on_press(Message::TogglePlay)
            .into()
    }

    /// Same geometry as the toolkit icon button, with a dimmed icon when unavailable.
    pub(super) fn icon_action(&self, icon: Icon, press: Option<Message>) -> Element<'_> {
        let metrics = iad::Metrics::desktop();
        button(
            container(icon.view(metrics.icon_size))
                .center(Length::Fixed(metrics.button_min_height)),
        )
        .padding(0)
        .style(style::button::flat)
        .on_press_maybe(press)
        .into()
    }

    pub(crate) fn controls(&self, size: f32) -> Element<'_> {
        row![
            self.icon_action(
                Icon::SkipBackward,
                self.can_previous().then_some(Message::Previous)
            ),
            self.play_pause(size),
            self.icon_action(Icon::SkipForward, self.can_next().then_some(Message::Next)),
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

    /// The volume button (mute and back) with its slider.
    fn volume_control(&self) -> Element<'_> {
        let volume = self.player.volume;
        let icon = if volume == 0.0 {
            Icon::VolumeMute
        } else {
            Icon::Volume
        };
        let percent = (volume * 100.0).round() as u32;
        // A vertical slider; hovering it (and dragging) shows the percentage it is at.
        let slider = w::tooltip(
            vertical_slider(0.0..=1.0, volume, Message::SetVolume)
                .step(0.01f32)
                .height(Length::Fixed(120.0)),
            format!("{percent}%"),
            w::TooltipPosition::Left,
        );
        let panel = container(slider)
            .padding(8) // gutter-ok: a component's own padding, not a page gutter
            .style(style::container::popover);
        let metrics = iad::Metrics::desktop();
        let trigger = container(icon.view(16.0)).center_y(Length::Fixed(
            metrics.button_min_height - metrics.button_padding.y(),
        ));
        w::Popover::new(trigger, panel)
            .appearance(w::PopoverAppearance::Flat)
            .fit_content()
            .align(w::PopoverAlign::Center)
            .open_initially(std::env::var("WEAVE_GUI_START").as_deref() == Ok("volume"))
            .into()
    }

    /// The heart of a track: in the Favourites playlist or not.
    fn heart(&self, id: usize) -> Element<'_> {
        w::IconToggleButton::new(Icon::Heart, self.is_favourite(id))
            .size(16.0)
            .on_toggle(move |_| Message::ToggleFavoriteTrack(id))
            .into()
    }

    /// Wide layout: the player bar.
    pub fn player_bar(&self) -> Element<'_> {
        let Some(t) = self.now() else {
            return space::vertical().height(0).into();
        };
        // Equal side regions keep transport centered independently of the metadata/actions.
        let scrubber_width = (self.win_w - 720.0).max(200.0);
        let metadata_width = ((self.win_w - scrubber_width - 400.0) / 2.0).clamp(64.0, 200.0);
        let track = row![
            mouse_area(
                row![
                    cover(t.id, 48.0),
                    container(column![
                        ty::body(t.title.clone()),
                        ty::caption(t.artist.clone())
                    ])
                    .max_width(metadata_width),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            )
            .on_press(Message::NowPlaying(!self.now_playing)),
            self.heart(t.id),
        ]
        .spacing(4)
        .align_y(Alignment::Center);
        let actions = row![
            w::IconToggleButton::new(Icon::Shuffle, self.player.shuffle)
                .size(16.0)
                .on_toggle(move |_| Message::Shuffle),
            w::IconToggleButton::new(Icon::Repeat, self.player.repeat)
                .size(16.0)
                .on_toggle(move |_| Message::Repeat),
            self.volume_control(),
            w::IconToggleButton::new(Icon::Queue, self.queue_open)
                .size(16.0)
                .on_toggle(move |_| Message::ToggleQueue),
        ]
        .spacing(4)
        .align_y(Alignment::Center);
        container(
            row![
                container(track).align_left(Length::FillPortion(1)),
                row![
                    self.controls(16.0),
                    container(self.seek(t)).width(scrubber_width),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
                container(actions).align_right(Length::FillPortion(1)),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .padding(surface::bar_padding())
        .style(style::container::headerbar)
        .into()
    }

    /// Narrow layout: the mini player. Tapping it expands Now Playing.
    pub fn mini_player(&self) -> Element<'_> {
        let Some(t) = self.now() else {
            return space::vertical().height(0).into();
        };
        let frac = self.player.pos / t.secs as f32;
        container(column![
            row![
                mouse_area(
                    row![
                        cover(t.id, 40.0),
                        container(column![
                            ty::body(t.title.clone()),
                            ty::caption(t.artist.clone())
                        ])
                        .max_width(180.0),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center)
                )
                .on_press(Message::NowPlaying(true)),
                self.heart(t.id),
                space::horizontal(),
                self.play_pause(16.0),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .padding([6, 12]), // gutter-ok: a component's own padding, not a page gutter
            w::progress_bar(0.0..=1.0, frac),
        ])
        .style(style::container::headerbar)
        .into()
    }

    // ---- the queue ------------------------------------------------------------------------

    /// The queue page, settled: the now-playing bar on top (the mini player on narrow, the full
    /// bar on wide) and the queue's body below it.
    pub fn queue_page(&self) -> Element<'_> {
        column![self.now_bar(), self.queue_body()]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// The now-playing bar of this layout: the full bar on wide, the mini player on narrow.
    pub fn now_bar(&self) -> Element<'_> {
        if self.is_narrow() {
            self.mini_player()
        } else {
            self.player_bar()
        }
    }

    pub fn sleep_menu(&self, wrap: impl Fn(Message) -> Message) -> Menu<Message> {
        let option = |minutes: Option<u32>, label: &'static str| {
            MenuItem::check(
                label,
                self.sleep_choice == minutes,
                wrap(Message::SleepTimer(minutes)),
            )
        };
        Menu::new("Sleep timer")
            .check_style(w::MenuCheckStyle::Mark)
            .section([
                option(None, "Off"),
                option(Some(15), "In 15 minutes"),
                option(Some(30), "In 30 minutes"),
                option(Some(60), "In an hour"),
            ])
    }

    /// The body of the queue: the sleep timer, shuffle and repeat where the bar lacks them, and
    /// "Up next", which is only what follows the playing track and is always editable (a delete
    /// button before the drag handle).
    pub fn queue_body(&self) -> Element<'_> {
        let narrow = self.is_narrow();
        let timer = if narrow {
            crate::icons::icon_button(Icon::Clock)
                .on_press(Message::OpenSheet(Some(crate::Sheet::Sleep)))
                .into()
        } else {
            self.menu_button(Icon::Clock, "Sleep timer", self.sleep_menu(|m| m))
        };
        let sleep = match self.sleep_left {
            Some(left) => ty::caption(format!("Stops in {}", clock(left))),
            None => ty::caption(""),
        };
        let list = ReorderableList::of(&self.player.up_next, |id: &usize| {
            let t = &self.lib.tracks[*id];
            row![
                column![ty::body(t.title.clone()), ty::caption(t.artist.clone())]
                    .width(Length::Fill),
                ty::caption(t.duration()),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .padding([4, 8]) // gutter-ok: a component's own padding, not a page gutter
            .into()
        })
        // The delete button, before the drag handle.
        .row_action(|id: &usize| {
            let at = self
                .player
                .up_next
                .items()
                .iter()
                .position(|x| x == id)
                .unwrap_or(0);
            crate::icons::icon_button(Icon::Delete)
                .on_press(Message::QueueRemove(at))
                .into()
        })
        .on_event(Message::Queue);
        // No title: the queue is what is under the player. The close chevron is at the right edge.
        let mut controls = row![sleep, space::horizontal()]
            .spacing(8)
            .align_y(Alignment::Center);
        if narrow {
            controls = controls
                .push(
                    w::IconToggleButton::new(Icon::Shuffle, self.player.shuffle)
                        .size(16.0)
                        .on_toggle(move |_| Message::Shuffle),
                )
                .push(
                    w::IconToggleButton::new(Icon::Repeat, self.player.repeat)
                        .size(16.0)
                        .on_toggle(move |_| Message::Repeat),
                );
        }
        controls = controls
            .push(timer)
            .push(self.icon_action(
                Icon::Delete,
                (!self.player.up_next.items().is_empty()).then_some(Message::ClearQueue),
            ))
            .push(crate::icons::icon_button(Icon::GoDown).on_press(Message::ToggleQueue));
        // The playing track is not listed: the player is right above.
        let played = self
            .player
            .current
            .unwrap_or(0)
            .min(self.player.queue.len());
        let mut history = column![];
        for (at, id) in self.player.queue[..played].iter().enumerate() {
            let t = &self.lib.tracks[*id];
            history = history.push(
                button(
                    row![
                        column![ty::body(t.title.clone()), ty::caption(t.artist.clone())]
                            .width(Length::Fill),
                        ty::caption(t.duration()),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                )
                .style(style::button::flat)
                .width(Length::Fill)
                .on_press(Message::QueueJump(at)),
            );
        }
        let history = w::Collapsible::new(
            w::SectionDivider::new("History")
                .detail(format!("{played} tracks"))
                .open(self.history_open)
                .on_toggle(Message::ToggleQueueHistory),
            history,
            self.history_open,
        );
        let up_next = w::Collapsible::new(
            w::SectionDivider::new("Up next")
                .detail(format!("{} tracks", self.player.up_next.items().len()))
                .open(self.up_next_open)
                .on_toggle(Message::ToggleUpNext),
            list,
            self.up_next_open,
        );
        let stack = column![history, up_next].spacing(8);
        column![surface::toolbar(controls), surface::scrolling(stack)]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    // ---- Now Playing ----------------------------------------------------------------------

    pub(super) fn radial_visualizer_visible(&self) -> bool {
        self.now_playing
            && self.now().is_some()
            && self.theme_config.text("player", "artwork") == Some("disc")
            && self.theme_config.bool("player", "radial-visualizer") == Some(true)
            && !(self.show_lyrics && self.is_narrow())
            && !iad::config::current().reduced_motion
    }

    fn player_artwork(&self, track: &Track, size: f32) -> Element<'_> {
        if self.theme_config.text("player", "artwork") == Some("disc") {
            let silent = &[];
            let bands = if iad::config::current().reduced_motion {
                silent
            } else {
                &self.spectrum[..]
            };
            crate::components::disc_artwork::disc(
                track.id,
                size,
                bands,
                self.theme_config.bool("player", "radial-visualizer") == Some(true),
            )
        } else {
            cover(track.id, size)
        }
    }

    /// Now Playing: the cover, or the lyrics in its place. The transport controls are only
    /// here when the layout has no player bar (narrow): they are not drawn twice.
    pub fn now_playing_view(&self) -> Element<'_> {
        let Some(t) = self.now() else {
            return container(ty::body("Nothing playing"))
                .center(Length::Fill)
                .into();
        };
        let narrow = self.is_narrow();
        let synced = || -> Element<'_> {
            SyncedText::new(LYRICS, &self.lyrics, Message::Seek)
                .current(self.lyric_line)
                .placeholder("No lyrics for this track.")
                .into()
        };
        // Lyrics sit beside the cover when there is room, and replace it when there is not.
        let beside = !narrow && self.show_lyrics;
        let tags = row(t
            .tags
            .iter()
            .map(|g| tag_pill(g, &self.tag_style(g), ChipSize::Medium)))
        .spacing(6)
        .wrap();
        let info = column![
            row![ty::title_2(t.title.clone()), self.heart(t.id)]
                .spacing(8)
                .align_y(Alignment::Center),
            ty::body(t.by_line()),
            tags,
        ]
        .spacing(6)
        .align_x(Alignment::Center);
        // The window's player: there is no bar under Now Playing, so the transport is here, in
        // both layouts (the volume is the bar's, not Now Playing's).
        let transport = column![
            self.seek(t),
            self.controls(20.0),
            row![
                w::IconToggleButton::new(Icon::Shuffle, self.player.shuffle)
                    .size(16.0)
                    .on_toggle(move |_| Message::Shuffle),
                w::IconToggleButton::new(Icon::Repeat, self.player.repeat)
                    .size(16.0)
                    .on_toggle(move |_| Message::Repeat),
                w::IconToggleButton::new(Icon::Lyrics, self.show_lyrics)
                    .size(16.0)
                    .on_toggle(move |_| Message::ToggleLyrics),
                w::IconToggleButton::new(Icon::Queue, self.queue_open)
                    .size(16.0)
                    .on_toggle(move |_| Message::ToggleQueue),
            ]
            .spacing(8),
        ]
        .spacing(12)
        .align_x(Alignment::Center);
        let stage: Element<'_> = if beside {
            // The cover and what it is on the left, the lyrics on the right.
            row![
                column![
                    container(self.player_artwork(t, 300.0)).center(Length::Fill),
                    info
                ]
                .spacing(12)
                .align_x(Alignment::Center)
                .width(Length::FillPortion(1)),
                container(synced())
                    .width(Length::FillPortion(1))
                    .height(Length::Fill),
            ]
            .spacing(48)
            .height(Length::Fill)
            .into()
        } else {
            let art: Element<'_> = if self.show_lyrics {
                synced()
            } else {
                container(self.player_artwork(t, 300.0))
                    .center(Length::Fill)
                    .into()
            };
            column![
                container(art).height(Length::Fill).width(Length::Fill),
                info
            ]
            .spacing(12)
            .align_x(Alignment::Center)
            .height(Length::Fill)
            .into()
        };
        let body = container(
            column![stage, transport]
                .spacing(12)
                .align_x(Alignment::Center),
        )
        .max_width(if beside { 1040 } else { 640 })
        .height(Length::Fill);
        // The top row is also the window's handle: with the header gone it is what drags it.
        let close = row![
            w::flat_button(Icon::GoDown.view(16.0)).on_press(Message::NowPlaying(false)),
            mouse_area(space::horizontal().height(Length::Fixed(32.0)))
                .on_press(Message::Window(iad::windows::Action::Drag)),
        ]
        .align_y(Alignment::Center);
        container(column![
            close,
            surface::full(container(body).center_x(Length::Fill))
        ])
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|theme: &iad::Theme| iced::widget::container::Style {
            background: Some(
                Color {
                    a: 1.0,
                    ..theme.semantic.window.bg
                }
                .into(),
            ),
            ..Default::default()
        })
        .into()
    }
}
