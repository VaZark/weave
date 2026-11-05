//! Minimal mode: the window is the player bar and nothing else, in two views. The first is the
//! cover and the spectrum of what plays. Pressing the cover switches to the second: the cover,
//! the song and artist, and the transport controls, as in the player bar. Both end with a chevron
//! that opens the full window again.

use iad::Icon;
use iad::prelude::{self as w, typography as ty};
use iced::font::Weight;
use iced::widget::{column, container, mouse_area, row, space};
use iced::{Alignment, Font, Length, mouse};
use iced_adaptive_design as iad;

use crate::icons::cover;
use crate::{App, Element, Message};

/// A little heavier than the body text: the strip is small and read at a glance.
const SEMIBOLD: Font = Font {
    weight: Weight::Semibold,
    ..Font::DEFAULT
};

impl App {
    /// The one-line player. Whatever is not a button drags the window.
    pub fn minimal_view(&self) -> Element<'_> {
        let chevron = w::icon_button(Icon::GoUp).on_press(Message::Minimal(false));
        let line: Element<'_> = match self.now() {
            Some(t) => {
                // The cover switches the view.
                let art = mouse_area(cover(t.id, 44.0))
                    .on_press(Message::MinimalDetails)
                    .interaction(mouse::Interaction::Pointer);
                if self.minimal_details {
                    row![
                        art,
                        column![
                            ty::body(t.title.clone()).font(SEMIBOLD),
                            ty::caption(t.artist.clone()).font(SEMIBOLD),
                        ]
                        .width(Length::Fill),
                        self.controls(14.0),
                        chevron,
                    ]
                } else {
                    row![
                        art,
                        container(w::spectrum_bars(&self.spectrum, 32.0))
                            .width(Length::Fill)
                            .padding([0, 6]), // gutter-ok: a component's own padding
                        chevron,
                    ]
                }
                .spacing(10)
                .align_y(Alignment::Center)
                .into()
            }
            None => row![
                ty::body("Nothing playing").font(SEMIBOLD),
                space::horizontal(),
                chevron
            ]
            .align_y(Alignment::Center)
            .into(),
        };
        // The whole strip is a drag handle; the buttons take their own presses first.
        mouse_area(
            container(line)
                .padding([0, 12]) // gutter-ok: the strip's own padding
                .center_y(Length::Fill),
        )
        .on_press(Message::Window(iad::windows::Action::Drag))
        .into()
    }
}
