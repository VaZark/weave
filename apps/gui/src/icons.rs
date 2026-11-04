//! The icons iced-adaptive-design's `Icon` does not have, and the mapping from an icon *key* (what
//! a tag or a playlist stores) to an `Icon`.

use iced_adaptive_design::color::from_index;
use iced_adaptive_design::prelude::{self as w, AvatarShape};
use iced_adaptive_design::{Element, Icon, IconSource};

/// Albums stacked on a shelf: the library.
pub fn library() -> IconSource {
    IconSource::svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="black" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M3 3v10M7 3v10M10.5 3.8l3 8.8"/></svg>"#,
    )
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
