//! The player: the bar (wide), the mini player (narrow), Now Playing and the queue sheet.

use iad::prelude::{
    self as w, ChipSize, Menu, MenuItem, ReorderableList, SyncedText, typography as ty,
};
use iad::{Icon, styles as style};
use iced::widget::{button, column, container, mouse_area, row, space};
use iced::{Alignment, Background, Color, Length};
use iced_adaptive_design as iad;

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

    /// A skip button. Without a press (nothing to skip to) it is drawn faded and does nothing:
    /// an icon does not follow its button's disabled state, so it is tinted here.
    fn skip_button(&self, icon: Icon, press: Option<Message>) -> Element<'_> {
        let Some(msg) = press else {
            let m = iad::Metrics::desktop();
            // The solid dim colour of the theme: an icon tint keeps no transparency.
            let faded = self.theme().semantic.dim_solid();
            return button(
                container(icon.colored(m.icon_size, faded))
                    .center(Length::Fixed(m.button_min_height)),
            )
            .padding(0)
            .style(style::button::flat)
            .into();
        };
        w::icon_button(icon).on_press(msg).into()
    }

    fn controls(&self, size: f32) -> Element<'_> {
        row![
            self.skip_button(
                Icon::SkipBackward,
                self.can_previous().then_some(Message::Previous)
            ),
            self.play_pause(size),
            self.skip_button(Icon::SkipForward, self.can_next().then_some(Message::Next)),
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
    pub fn player_bar(&self) -> Element<'_> {
        let Some(t) = self.now() else {
            // Nothing playing: no bar at all.
            return space::vertical().height(0).into();
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
                w::IconToggleButton::new(Icon::Shuffle, self.player.shuffle)
                    .size(16.0)
                    .on_toggle(move |_| Message::Shuffle),
                w::IconToggleButton::new(Icon::Repeat, self.player.repeat)
                    .size(16.0)
                    .on_toggle(move |_| Message::Repeat),
                w::IconToggleButton::new(Icon::Queue, self.queue_open)
                    .size(16.0)
                    .on_toggle(move |_| Message::ToggleQueue),
                w::IconToggleButton::new(
                    if self.now_playing {
                        Icon::GoDown
                    } else {
                        Icon::GoUp
                    },
                    false
                )
                .size(16.0)
                .on_toggle(move |_| Message::NowPlaying(!self.now_playing)),
            ]
            .spacing(14)
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
            .padding([6, 12]), // gutter-ok: a component's own padding, not a page gutter
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

    /// The body of the queue: the sleep timer, shuffle and repeat where the bar lacks them, and
    /// "Up next", which is only what follows the playing track and is always editable (a delete
    /// button before the drag handle).
    pub fn queue_body(&self) -> Element<'_> {
        let narrow = self.is_narrow();
        let timer = {
            let opt = |m: Option<u32>, label: &'static str| {
                MenuItem::check(label, self.sleep_choice == m, Message::SleepTimer(m))
            };
            self.menu_button(
                Icon::Clock,
                "Sleep timer",
                Menu::new("Sleep timer")
                    .check_style(w::MenuCheckStyle::Mark)
                    .section([
                        opt(None, "Off"),
                        opt(Some(15), "In 15 minutes"),
                        opt(Some(30), "In 30 minutes"),
                        opt(Some(60), "In an hour"),
                    ]),
            )
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
            w::icon_button(Icon::Delete)
                .on_press(Message::QueueRemove(at))
                .into()
        })
        .on_event(Message::Queue);
        let mut controls = row![
            w::icon_button(Icon::GoDown).on_press(Message::ToggleQueue),
            ty::heading("Up next"),
            ty::caption(format!("{} tracks", self.player.up_next.items().len())),
            space::horizontal(),
            sleep,
            timer,
        ]
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
        column![surface::toolbar(controls), surface::scrolling(list)]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    // ---- Now Playing ----------------------------------------------------------------------

    /// Now Playing: the cover, or the lyrics in its place. The transport controls are only
    /// here when the layout has no player bar (narrow): they are not drawn twice.
    pub fn now_playing_view(&self) -> Element<'_> {
        let Some(t) = self.now() else {
            return container(ty::body("Nothing playing"))
                .center(Length::Fill)
                .into();
        };
        let narrow = self.is_narrow();
        let art: Element<'_> = if self.show_lyrics {
            SyncedText::new(LYRICS, &self.lyrics, Message::Seek)
                .current(self.lyric_line)
                .placeholder("No lyrics for this track.")
                .into()
        } else {
            container(cover(t.id, 260.0)).center(Length::Fill).into()
        };
        let tags = row(t
            .tags
            .iter()
            .map(|g| tag_pill(g, &self.lib.style_of(g), ChipSize::Medium)))
        .spacing(6)
        .wrap();
        let mut body = column![
            container(art).height(Length::Fill).width(Length::Fill),
            column![
                ty::title_2(t.title.clone()),
                ty::body(format!("{} · {}", t.artist, t.album)),
                tags,
            ]
            .spacing(6)
            .align_x(Alignment::Center),
        ]
        .spacing(12)
        .align_x(Alignment::Center)
        .max_width(640)
        .height(Length::Fill);
        let lyrics = w::IconToggleButton::new(Icon::Lyrics, self.show_lyrics)
            .size(16.0)
            .on_toggle(move |_| Message::ToggleLyrics);
        if narrow {
            body = body.push(self.seek(t)).push(self.controls(20.0)).push(
                row![
                    w::IconToggleButton::new(Icon::Shuffle, self.player.shuffle)
                        .size(16.0)
                        .on_toggle(move |_| Message::Shuffle),
                    w::IconToggleButton::new(Icon::Repeat, self.player.repeat)
                        .size(16.0)
                        .on_toggle(move |_| Message::Repeat),
                    lyrics,
                    w::IconToggleButton::new(Icon::Queue, self.queue_open)
                        .size(16.0)
                        .on_toggle(move |_| Message::ToggleQueue),
                ]
                .spacing(8),
            );
        } else {
            body = body.push(lyrics);
        }
        let close = row![
            w::flat_button(Icon::GoDown.view(16.0)).on_press(Message::NowPlaying(false)),
            space::horizontal(),
        ];
        let backdrop = iad::color::from_index(t.id);
        container(column![
            close,
            surface::full(container(body).center_x(Length::Fill))
        ])
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |t: &iad::Theme| {
            // Opaque: the window colour with a wash of the cover's colour.
            let base = t.palette.window_bg_color;
            let k = 0.14;
            iced::widget::container::Style {
                background: Some(Background::Color(Color {
                    r: base.r + (backdrop.r - base.r) * k,
                    g: base.g + (backdrop.g - base.g) * k,
                    b: base.b + (backdrop.b - base.b) * k,
                    a: 1.0,
                })),
                ..Default::default()
            }
        })
        .into()
    }
}
