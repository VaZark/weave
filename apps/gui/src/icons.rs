//! Music-specific symbolic icons, bundled as SVG until iced-adaptive-design has an icon theme.
//! Drawn like its own: 16 px grid, round strokes, tinted by the theme's foreground.

use iced::widget::svg::{Handle, Svg};
use iced::{Length, widget::svg};
use iced_adaptive_design::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    Play,
    Pause,
    Previous,
    Next,
    Shuffle,
    Repeat,
    Queue,
    Jam,
    Library,
    Note,
    Expand,
    Collapse,
    Heart,
    Moon,
    Bolt,
    Globe,
    Tag,
    Star,
    Target,
    Smile,
    Lyrics,
}

impl Glyph {
    fn body(self) -> &'static str {
        match self {
            Glyph::Play => r#"<path d="M5 3l8 5-8 5z" fill="black"/>"#,
            Glyph::Pause => r#"<path d="M5 3v10M11 3v10" stroke-width="2.6"/>"#,
            Glyph::Previous => r#"<path d="M4 3v10M13 3L6 8l7 5z" fill="black"/>"#,
            Glyph::Next => r#"<path d="M12 3v10M3 3l7 5-7 5z" fill="black"/>"#,
            Glyph::Shuffle => {
                r#"<path d="M2 4.5h2.5c3 0 4 7 7 7H14M2 11.5h2.5c1 0 1.8-1 2.6-2.2M9 6.2c.7-1 1.4-1.7 2.5-1.7H14M12 2.5l2 2-2 2M12 9.5l2 2-2 2"/>"#
            }
            Glyph::Repeat => {
                r#"<path d="M3 7V6a2 2 0 012-2h8M11 2l2 2-2 2M13 9v1a2 2 0 01-2 2H3M5 14l-2-2 2-2"/>"#
            }
            Glyph::Queue => r#"<path d="M2 4h9M2 8h9M2 12h5M12 10v4l3-2z" />"#,
            Glyph::Jam => r#"<path d="M8 1.5l1.5 4.5 4.5 1.5-4.5 1.5L8 13.5 6.5 9 2 7.5 6.5 6z"/>"#,
            Glyph::Library => r#"<path d="M3 3v10M7 3v10M10.5 3.8l3 8.8"/>"#,
            Glyph::Note => {
                r#"<path d="M6 11.5V3l7-1.5v8.5"/><circle cx="4.5" cy="11.5" r="1.8"/><circle cx="11.5" cy="10" r="1.8"/>"#
            }
            Glyph::Expand => r#"<path d="M3 10l5-5 5 5"/>"#,
            Glyph::Collapse => r#"<path d="M3 6l5 5 5-5"/>"#,
            Glyph::Heart => {
                r#"<path d="M8 13.5S2.3 10.2 2.3 6.2A3 3 0 018 4.6a3 3 0 015.7 1.6c0 4-5.7 7.3-5.7 7.3z"/>"#
            }
            Glyph::Moon => r#"<path d="M12.7 9.6A5.2 5.2 0 016.4 3.3a5.2 5.2 0 106.3 6.3z"/>"#,
            Glyph::Bolt => r#"<path d="M9.2 1.5L3.5 9h4.3l-1 5.5L12.5 7H8.2z"/>"#,
            Glyph::Globe => {
                r#"<circle cx="8" cy="8" r="6"/><path d="M2 8h12M8 2c2.2 2.2 2.2 9.8 0 12M8 2c-2.2 2.2-2.2 9.8 0 12"/>"#
            }
            Glyph::Tag => {
                r#"<path d="M2.5 2.5h5l6 6-5 5-6-6z"/><circle cx="5.3" cy="5.3" r=".8" fill="black"/>"#
            }
            Glyph::Star => {
                r#"<path d="M8 2l1.8 3.7 4 .6-2.9 2.8.7 4L8 11.2 4.4 13l.7-4L2.2 6.3l4-.6z"/>"#
            }
            Glyph::Lyrics => {
                r#"<path d="M2.5 3h11v7.5h-6.2L4.5 13v-2.5h-2z"/><path d="M5 6h6M5 8.2h4"/>"#
            }
            Glyph::Smile => {
                r#"<circle cx="8" cy="8" r="6"/><path d="M5.4 9.6a3.2 3.2 0 005.2 0"/><circle cx="5.9" cy="6.6" r=".8" fill="black"/><circle cx="10.1" cy="6.6" r=".8" fill="black"/>"#
            }
            Glyph::Target => {
                r#"<circle cx="8" cy="8" r="6"/><circle cx="8" cy="8" r="3"/><circle cx="8" cy="8" r=".8" fill="black"/>"#
            }
        }
    }

    /// The glyph for a tag's icon key (see `weave-player-lib`'s `Tag::icon`); an unknown key
    /// is the plain tag glyph.
    pub fn for_tag_icon(key: &str) -> Self {
        match key {
            "heart" => Glyph::Heart,
            "moon" => Glyph::Moon,
            "bolt" => Glyph::Bolt,
            "globe" => Glyph::Globe,
            "star" => Glyph::Star,
            "target" => Glyph::Target,
            "smile" => Glyph::Smile,
            "note" => Glyph::Note,
            _ => Glyph::Tag,
        }
    }

    fn handle(self) -> Handle {
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="black" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">{}</svg>"#,
            self.body()
        );
        Handle::from_memory(svg.into_bytes())
    }

    pub fn colored<'a>(self, size: f32, color: iced::Color) -> Svg<'a, Theme> {
        self.view(size)
            .style(move |_: &Theme, _| iced::widget::svg::Style { color: Some(color) })
    }

    pub fn view<'a>(self, size: f32) -> Svg<'a, Theme> {
        svg(self.handle())
            .width(Length::Fixed(size))
            .height(Length::Fixed(size))
    }
}
