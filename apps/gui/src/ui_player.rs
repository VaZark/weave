//! The player: the bar (wide), the mini player (narrow), Now Playing and the queue sheet.

use iad::widget::{
    self as w, ChipSize, Menu, MenuItem, ReorderableList, SyncedText, typography as ty,
};
use iad::{Icon, style};
use iced::widget::{column, container, mouse_area, row, scrollable, space};
use iced::{Alignment, Background, Color, Length};
use iced_adaptive_design as iad;

use crate::components::tag_badge::tag_pill;
use crate::data::{Track, clock};
use crate::icons::cover;
use crate::ui::{BAR_H, HEADER_H, TAB_H};
use crate::{App, Element, LYRICS, Message};

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

    fn controls(&self, size: f32) -> Element<'_> {
        row![
            w::icon_button(Icon::SkipBackward).on_press(Message::Previous),
            self.play_pause(size),
            w::icon_button(Icon::SkipForward).on_press(Message::Next),
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
        .padding([8, 16])
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
            .padding([6, 12]),
        ])
        .style(style::container::headerbar)
        .into()
    }

    // ---- the queue ------------------------------------------------------------------------

    /// The queue page. The now-playing bar is moved to the top of the page (above the list, on
    /// both layouts) and the queue pushes the rest of the page's content out. "Up next" is only
    /// what follows the playing track, always editable: a delete button before the drag handle.
    /// Shuffle, repeat and the sleep timer are here too (the wide bar already has the first two).
    pub fn queue_page(&self) -> Element<'_> {
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
            let at = self
                .player
                .up_next
                .items()
                .iter()
                .position(|x| x == id)
                .unwrap_or(0);
            row![
                column![ty::body(t.title.clone()), ty::caption(t.artist.clone())]
                    .width(Length::Fill),
                ty::caption(t.duration()),
                w::icon_button(Icon::Delete).on_press(Message::QueueRemove(at)),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .padding([4, 8])
            .into()
        })
        .controls(false)
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
        // The now-playing bar sits on top: the mini player on narrow, the full bar on wide.
        let now = if narrow {
            self.mini_player()
        } else {
            self.player_bar()
        };
        // The list slides up into place when the queue opens and back down when it closes.
        let e = self.queue_slide.eased();
        let scroller = scrollable(container(list).padding(12));
        let list_area: Element<'_> = if e >= 0.999 {
            scroller.height(Length::Fill).into()
        } else {
            let fixed = BAR_H + 46.0 + HEADER_H + if narrow { TAB_H } else { 0.0 } + 2.0;
            let avail = (self.win_h - fixed).max(120.0);
            container(column![
                space::vertical().height(Length::Fixed((1.0 - e) * avail)),
                container(scroller).height(Length::Fixed(avail)),
            ])
            .width(Length::Fill)
            .height(Length::Fill)
            .clip(true)
            .into()
        };
        column![now, container(controls).padding([6, 12]), list_area]
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
        .padding(16)
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
            container(body).center_x(Length::Fill).height(Length::Fill)
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
