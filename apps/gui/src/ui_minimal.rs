//! Minimal mode: the window is the player bar and nothing else. The cover, the title, the
//! spectrum of what plays in place of a seek bar, the transport controls, and a chevron that
//! opens the full window again.

use iad::Icon;
use iad::prelude::{self as w, typography as ty};
use iced::widget::{column, container, mouse_area, row, space};
use iced::{Alignment, Length};
use iced_adaptive_design as iad;

use crate::icons::cover;
use crate::{App, Element, Message};

/// `e` as a drag handle for the window.
fn drag<'a>(e: Element<'a>) -> Element<'a> {
    mouse_area(e)
        .on_press(Message::Window(iad::windows::Action::Drag))
        .into()
}

impl App {
    /// The one-line player. Its cover, title and bars are a drag handle for the window.
    pub fn minimal_view(&self) -> Element<'_> {
        let chevron = w::icon_button(Icon::GoUp).on_press(Message::Minimal(false));
        let line: Element<'_> = match self.now() {
            Some(t) => row![
                drag(
                    row![
                        cover(t.id, 44.0),
                        column![ty::body(t.title.clone()), ty::caption(t.artist.clone())]
                            .width(Length::Fixed(150.0)),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center)
                    .into()
                ),
                container(drag(w::spectrum_bars(&self.spectrum, 32.0).into()))
                    .width(Length::Fill)
                    .padding([0, 6]), // gutter-ok: a component's own padding, not a page gutter
                self.controls(14.0),
                chevron,
            ]
            .spacing(10)
            .align_y(Alignment::Center)
            .into(),
            None => row![
                drag(ty::body("Nothing playing").into()),
                space::horizontal(),
                chevron,
            ]
            .align_y(Alignment::Center)
            .into(),
        };
        container(line)
            .padding([0, 12]) // gutter-ok: the strip's own padding
            .center_y(Length::Fill)
            .into()
    }
}
