//! Weave's use of the tag vocabulary (see the `weaver` crate): the genre tag and the tag view
//! the API returns.

use serde::{Deserialize, Serialize};

use crate::Metadata;
pub use weaver::{ChildPolicy, EXACT_PREFIX, TagName, normalize_tag, parse_tag_ref};

/// The one key the importer fills: a track's genre. Every other key is the user's own.
pub const GENRE: &str = "genre";

/// A tag as stored: its name, parent and how many things are assigned it directly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tag {
    /// Display name, e.g. `genre:Bachata`; also the tag's handle in the API.
    pub name: String,
    pub key: Option<String>,
    pub value: String,
    /// Display name of the parent tag; `None` for a root.
    pub parent: Option<String>,
    /// Things assigned this tag explicitly (descendants not counted).
    pub tracks: usize,
    pub artists: usize,
    pub albums: usize,
    pub playlists: usize,
}

impl From<weaver::Tag> for Tag {
    fn from(t: weaver::Tag) -> Self {
        Self {
            tracks: t.count("track"),
            artists: t.count("artist"),
            albums: t.count("album"),
            playlists: t.count("playlist"),
            name: t.name,
            key: t.key,
            value: t.value,
            parent: t.parent,
        }
    }
}

/// Tags that imported metadata maps to: the genre. Year, BPM, title, artist and album stay
/// columns of the track; they are not descriptive tags.
pub fn metadata_tags(m: &Metadata) -> Vec<TagName> {
    let genre = m.genre.as_deref().map(str::trim).filter(|g| !g.is_empty());
    genre
        .map(|value| TagName {
            key: Some(GENRE.into()),
            value: value.to_string(),
        })
        .filter(|name| TagName::parse(&name.display()).is_ok_and(|p| p == *name))
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_maps_only_the_genre() {
        let m = Metadata {
            title: "t".into(),
            genre: Some("Bachata".into()),
            artist: Some("Aventura".into()),
            year: Some(1999),
            bpm: Some(90.0),
            ..Default::default()
        };
        let names: Vec<_> = metadata_tags(&m).iter().map(TagName::display).collect();
        assert_eq!(names, ["genre:Bachata"], "only the genre becomes a tag");
        assert!(metadata_tags(&Metadata::default()).is_empty());
    }
}
