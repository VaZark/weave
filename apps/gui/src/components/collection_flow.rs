//! Shared flow policy, independent of collection data and item rendering.
use iced::Length;
use iced::widget::column;
use iced_adaptive_design::{Element, prelude::FlowGrid};

#[derive(Clone, Copy)]
pub struct CollectionFlow {
    narrow: bool,
    compact: bool,
    cards_only: bool,
}

impl CollectionFlow {
    pub fn new(narrow: bool, compact: bool) -> Self {
        Self {
            narrow,
            compact,
            cards_only: false,
        }
    }

    /// Recents keep cards at every width while sharing grid sizing and density.
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
                .max_width(if self.compact { 220.0 } else { 260.0 })
                .spacing(if self.compact { 8.0 } else { 12.0 })
                .into()
        }
    }
}
