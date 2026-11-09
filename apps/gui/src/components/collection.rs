//! Shared presentation for recent and full artist, album and playlist collections.
use super::collection_flow::CollectionFlow;
use iad::prelude::{self as w, typography as ty};
use iced::widget::{button, container, row};
use iced::{Alignment, Length};
use iced_adaptive_design::{self as iad, Element};

/// The most characters a collection tile shows: about two lines.
const MEDIA_NAME_MAX: usize = 25;

/// `text` cut to `max` characters, with `…` where it was cut.
fn limit_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let kept: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}

/// One responsive/density decision for every collection and its items.
#[derive(Clone, Copy)]
pub struct CollectionLayout {
    flow: CollectionFlow,
    compact: bool,
}

impl CollectionLayout {
    pub fn new(narrow: bool, compact: bool) -> Self {
        Self {
            flow: CollectionFlow::new(narrow, compact),
            compact,
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
        card_picture: f32,
        raised: bool,
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
        let compact = self.compact;
        if !compact {
            // The picture
            // at the left and the title beside it, centered on it, the name cut at
            // `MEDIA_NAME_MAX` characters (about two lines). (The kind and the
            // count are left out; compact rows still show them.) Text wraps at words
            // (`heading` alone never wraps).
            let wrap = iced::widget::text::Wrapping::WordOrGlyph;
            let body = row![
                picture(card_picture),
                ty::heading(limit_chars(&title, MEDIA_NAME_MAX))
                    .wrapping(wrap)
                    .width(Length::Fill)
            ]
            .spacing(12)
            .align_y(Alignment::Center);
            let surface = container(body)
                .padding(12) // gutter-ok: the entry's own padding
                .width(Length::Fill)
                .height(Length::Fill);
            let surface = if raised {
                surface.style(iad::styles::container::card)
            } else {
                surface
            };
            return button(surface)
                .padding(0)
                .style(move |theme, status| {
                    if raised {
                        iad::styles::button::events(theme, status)
                    } else {
                        iad::styles::button::flat(theme, status)
                    }
                })
                .width(Length::Fill)
                .height(Length::Fill)
                .on_press(on_press)
                .into();
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
    use super::limit_chars;

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
        for compact in [false, true] {
            for width in [390.0, 900.0] {
                let mut layouts = Vec::new();
                for narrow in [false, true] {
                    let collection = CollectionLayout::new(narrow, compact).cards();
                    let items = (0..6)
                        .map(|index| {
                            collection.item(
                                format!("Recent {index}"),
                                "3 tracks".into(),
                                |size| iced::widget::space().width(size).height(size).into(),
                                40.0,
                                true,
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
    fn collections_follow_the_same_wide_narrow_and_density_policy() {
        use super::CollectionLayout;
        use iced::advanced::{layout, renderer::Headless, widget::Tree};
        use iced::{Font, Pixels, Size};
        let renderer = iced::futures::executor::block_on(iced::Renderer::new(
            Font::DEFAULT,
            Pixels(14.0),
            Some("tiny-skia"),
        ))
        .unwrap();
        let mut wide_columns = Vec::new();
        for compact in [false, true] {
            for narrow in [false, true] {
                let collection = CollectionLayout::new(narrow, compact);
                let items = (0..6)
                    .map(|index| {
                        collection.item(
                            format!("Collection {index}"),
                            "3 tracks".into(),
                            |size| iced::widget::space().width(size).height(size).into(),
                            40.0,
                            true,
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
                    wide_columns.push(columns);
                }
            }
        }
        assert!(
            wide_columns[1] >= wide_columns[0],
            "compact mode keeps the denser grid"
        );
    }

    #[test]
    fn a_long_name_is_cut_with_an_ellipsis_and_a_short_one_is_kept() {
        assert_eq!(limit_chars("Road trip", 50), "Road trip");
        let fifty = "x".repeat(50);
        assert_eq!(limit_chars(&fifty, 50), fifty, "exactly the limit is kept");
        let long = format!("{} tail", "word ".repeat(20));
        let cut = limit_chars(&long, 50);
        assert!(cut.chars().count() <= 50 && cut.ends_with('…'), "{cut}");
        // Characters, not bytes: accents do not shorten the allowance.
        assert_eq!(limit_chars(&"é".repeat(60), 50).chars().count(), 50);
    }
}
