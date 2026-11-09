//! Shared presentation for recent and full artist, album and playlist collections: tight tiles
//! with a small picture and the name beside it, whatever the density. (Density changes the track
//! lists only.)
use super::collection_flow::CollectionFlow;
use iad::prelude::{self as w};
use iced_adaptive_design::{self as iad, Element};

/// One responsive decision for every collection and its items.
#[derive(Clone, Copy)]
pub struct CollectionLayout {
    flow: CollectionFlow,
}

impl CollectionLayout {
    pub fn new(narrow: bool) -> Self {
        Self {
            flow: CollectionFlow::new(narrow),
        }
    }

    pub fn cards(mut self) -> Self {
        self.flow = self.flow.cards();
        self
    }

    pub fn view<'a, M: 'a>(self, items: Vec<Element<'a, M>>) -> Element<'a, M> {
        self.flow.view(items)
    }

    pub fn item<'a, M: Clone + 'a>(
        self,
        title: String,
        subtitle: String,
        picture: impl Fn(f32) -> Element<'a, M>,
        on_press: M,
    ) -> Element<'a, M> {
        if self.flow.is_list() {
            let mut entry = w::action_row(title)
                .prefix(picture(40.0))
                .chevron(true)
                .on_activate(on_press);
            if !subtitle.is_empty() {
                entry = entry.subtitle(subtitle);
            }
            return entry.into();
        }
        w::ItemCard::new(title)
            .subtitle(subtitle)
            .picture(picture(36.0))
            .density(iad::config::Density::Compact)
            .on_press(on_press)
            .into()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn recent_cards_keep_the_same_presentation_at_both_width_modes() {
        use super::CollectionLayout;
        use iced::advanced::{layout, renderer::Headless, widget::Tree};
        use iced::{Font, Pixels, Size};
        let renderer = iced::futures::executor::block_on(iced::Renderer::new(
            Font::DEFAULT,
            Pixels(14.0),
            Some("tiny-skia"),
        ))
        .unwrap();
        {
            for width in [390.0, 900.0] {
                let mut layouts = Vec::new();
                for narrow in [false, true] {
                    let collection = CollectionLayout::new(narrow).cards();
                    let items = (0..6)
                        .map(|index| {
                            collection.item(
                                format!("Recent {index}"),
                                "3 tracks".into(),
                                |size| iced::widget::space().width(size).height(size).into(),
                                (),
                            )
                        })
                        .collect();
                    let mut element = collection.view(items);
                    let mut tree = Tree::new(&element);
                    let node = element.as_widget_mut().layout(
                        &mut tree,
                        &renderer,
                        &layout::Limits::new(Size::ZERO, Size::new(width, 1000.0)),
                    );
                    layouts.push(
                        node.children()
                            .iter()
                            .map(|child| child.bounds())
                            .collect::<Vec<_>>(),
                    );
                }
                assert_eq!(
                    layouts[0], layouts[1],
                    "recent cards must not turn into list rows in narrow mode"
                );
            }
        }
    }

    #[test]
    fn collections_follow_the_same_wide_and_narrow_policy() {
        use super::CollectionLayout;
        use iced::advanced::{layout, renderer::Headless, widget::Tree};
        use iced::{Font, Pixels, Size};
        let renderer = iced::futures::executor::block_on(iced::Renderer::new(
            Font::DEFAULT,
            Pixels(14.0),
            Some("tiny-skia"),
        ))
        .unwrap();
        for narrow in [false, true] {
            {
                let collection = CollectionLayout::new(narrow);
                let items = (0..6)
                    .map(|index| {
                        collection.item(
                            format!("Collection {index}"),
                            "3 tracks".into(),
                            |size| iced::widget::space().width(size).height(size).into(),
                            (),
                        )
                    })
                    .collect();
                let mut element = collection.view(items);
                let mut tree = Tree::new(&element);
                let node = element.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(Size::ZERO, Size::new(900.0, 1000.0)),
                );
                assert_eq!(node.size().width, 900.0);
                let children = node.children();
                assert_eq!(children.len(), 6);
                if narrow {
                    assert!(children.iter().all(|child| child.size().width == 900.0));
                    assert!(children.windows(2).all(
                        |pair| pair[1].bounds().y >= pair[0].bounds().y + pair[0].size().height
                    ));
                } else {
                    let columns = children
                        .iter()
                        .take_while(|child| child.bounds().y == 0.0)
                        .count();
                    assert!(columns > 1, "wide mode must flow into multiple columns");
                    assert!(children.iter().all(|child| child.size().width < 900.0));
                }
            }
        }
    }
}
