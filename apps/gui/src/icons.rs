//! Application-owned icons, toolkit button adapters, and stable stored-key mappings.

use iced_adaptive_design::color::from_index;
use iced_adaptive_design::prelude::{self as w, AvatarShape};
use iced_adaptive_design::{Element, IconSource};
pub use weave_icons::Icon;

/// The application's library mark, shipped by `weave-icons`.
pub fn library() -> IconSource {
    Icon::Library.into()
}

/// The toolkit's icon-button geometry with application-owned artwork.
pub fn icon_button<'a, M: 'a>(
    icon: Icon,
) -> iced::widget::Button<'a, M, iced_adaptive_design::Theme> {
    let m = iced_adaptive_design::Metrics::desktop();
    iced::widget::button(
        iced::widget::container(icon.view(m.icon_size))
            .center(iced::Length::Fixed(m.button_min_height)),
    )
    .padding(0)
    .style(iced_adaptive_design::styles::button::flat)
}

/// A compact entry action using the application's own icon set.
pub fn section_button<'a, M: Clone + 'a>(
    icon: Icon,
    message: M,
    active: bool,
) -> iced::widget::Button<'a, M, iced_adaptive_design::Theme> {
    let m = iced_adaptive_design::Metrics::desktop();
    let side = w::section_size(&m);
    iced::widget::button(
        iced::widget::container(icon.view(m.icon_size - 2.0)).center(iced::Length::Fixed(side)),
    )
    .padding(0)
    .style(move |theme, status| {
        let mut style = iced_adaptive_design::styles::button::flat(theme, status);
        if active {
            style.text_color = theme.semantic.accent.color;
            style.background = Some(iced::Background::Color(iced_adaptive_design::color::alpha(
                theme.semantic.accent.color,
                0.18,
            )));
        }
        style
    })
    .on_press(message)
}

/// The icon for a stored key (`heart`, `moon`, ...); an unknown key is the plain tag icon.
pub fn for_key(key: &str) -> Icon {
    match key {
        "heart" => Icon::Heart,
        "moon" => Icon::Moon,
        "bolt" => Icon::Bolt,
        "globe" => Icon::Globe,
        "star" => Icon::Star,
        "target" => Icon::Target,
        "smile" => Icon::Smile,
        "note" => Icon::MusicNote,
        _ => Icon::Tag,
    }
}

/// A cover until the engine provides art: a rounded square with a note, coloured by the track's
/// (or album's) id from iad's avatar palette.
pub fn cover<'a, M: 'a>(id: usize, size: f32) -> Element<'a, M> {
    w::avatar(size, id.to_string())
        .icon(Icon::MusicNote)
        .shape(AvatarShape::Rounded)
        .color(from_index(id))
        .into()
}

/// A playlist's picture: its icon on a colour taken from its name, or the initials when it has
/// no icon.
pub fn playlist_avatar<'a, M: 'a>(name: &str, icon: Option<&str>, size: f32) -> Element<'a, M> {
    let a = w::avatar(size, name.to_string());
    match icon {
        Some(key) => a.icon(for_key(key)).into(),
        None => a.into(),
    }
}
