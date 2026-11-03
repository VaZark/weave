//! The icons iced-adaptive-design's `Icon` does not have, and the mapping from an icon *key* (what
//! a tag or a playlist stores) to an `Icon`.

use iced_adaptive_design::{Icon, IconSource};

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
