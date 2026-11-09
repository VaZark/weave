//! Shared flow policy, independent of collection data and item rendering.
use iced::Length;
use iced::widget::column;
use iced_adaptive_design::{Element, prelude::FlowGrid};

#[derive(Clone, Copy)]
pub struct CollectionFlow {
    narrow: bool,
    cards_only: bool,
}

impl CollectionFlow {
    pub fn new(narrow: bool) -> Self {
        Self {
            narrow,
            cards_only: false,
        }
    }

    /// Recents keep cards at every width while sharing the grid sizing.
    pub fn cards(mut self) -> Self {
        self.cards_only = true;
        self
    }

    pub fn is_list(self) -> bool {
        self.narrow && !self.cards_only
    }

    pub fn view<'a, M: 'a>(self, items: Vec<Element<'a, M>>) -> Element<'a, M> {
        if self.is_list() {
            column(items).spacing(4).width(Length::Fill).into()
        } else {
            FlowGrid::new(items)
                .max_width(220.0)
                .spacing(8.0)
                .into()
        }
    }
}
