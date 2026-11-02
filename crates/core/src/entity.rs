//! The things a library is made of besides tracks: artists and albums, and the profiles that
//! describe them. Tags can be put on tracks, artists, albums and playlists alike.

use serde::{Deserialize, Serialize};

use crate::Track;

/// What a tag, or an ID, can belong to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    Track,
    Artist,
    Album,
    Playlist,
}
text_enum!(EntityKind {
    Track => "track",
    Artist => "artist",
    Album => "album",
    Playlist => "playlist",
});

/// Identity of an artist or album as the same text is spelled by different files: trimmed,
/// whitespace collapsed, lowercased. Two spellings with the same identity are one entity.
pub fn entity_norm(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// A looser key for *suggesting* duplicates (never for merging by itself): letters and digits
/// only, without a leading "the", so `Daft-Punk`, `daft punk` and `The Daft Punk` collide.
pub fn loose_key(name: &str) -> String {
    let norm = entity_norm(name);
    let norm = norm.strip_prefix("the ").unwrap_or(&norm);
    norm.chars().filter(|c| c.is_alphanumeric()).collect()
}

/// An artist, album or playlist as other things refer to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityRef {
    /// The slug used to address it.
    pub id: String,
    pub name: String,
}

/// A name and how many times it occurs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Count {
    pub name: String,
    pub count: usize,
}

/// Everything about one track, including what it is linked to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackDetail {
    pub track: Track,
    /// The audio file.
    pub path: Option<String>,
    /// Unix seconds when it was first indexed.
    pub added_at: i64,
    pub artist: Option<EntityRef>,
    pub album: Option<EntityRef>,
    pub playlists: Vec<EntityRef>,
    /// 1-based positions in the now-playing queue (a track can be queued more than once).
    pub queue_positions: Vec<usize>,
}

/// One row of the artist list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtistEntry {
    pub id: String,
    pub name: String,
    pub tracks: usize,
    pub albums: usize,
    pub tags: Vec<String>,
}

/// One row of the album list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlbumEntry {
    pub id: String,
    pub title: String,
    pub artist: Option<EntityRef>,
    /// Earliest year among its tracks.
    pub year: Option<i32>,
    pub tracks: usize,
    pub tags: Vec<String>,
}

/// Figures over a set of tracks (an artist's, an album's).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    pub tracks: usize,
    pub duration_ms: u64,
    pub year_min: Option<i32>,
    pub year_max: Option<i32>,
    pub bpm_min: Option<f64>,
    pub bpm_max: Option<f64>,
    pub bpm_avg: Option<f64>,
    pub rated: usize,
    pub rating_avg: Option<f64>,
    /// Unix seconds.
    pub first_added: Option<i64>,
    pub last_added: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtistProfile {
    pub id: String,
    pub name: String,
    /// Other spellings that were merged into this artist; a re-import maps them back here.
    pub aliases: Vec<String>,
    /// Tags put on the artist itself.
    pub tags: Vec<String>,
    pub stats: Stats,
    pub albums: Vec<AlbumEntry>,
    /// Tracks that belong to no album.
    pub loose_tracks: usize,
    /// The `genre:` tags across the artist's tracks, most common first.
    pub genres: Vec<Count>,
    /// The other tags across the artist's tracks (numbers like bpm and year left out).
    pub track_tags: Vec<Count>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlbumProfile {
    pub id: String,
    pub title: String,
    /// The artist profile it is linked to.
    pub artist: Option<EntityRef>,
    pub tags: Vec<String>,
    pub stats: Stats,
    pub genres: Vec<Count>,
    pub track_tags: Vec<Count>,
    /// In disc and track order.
    pub tracks: Vec<Track>,
}

/// Two artists whose names look alike and might be one person (or not: the user decides).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimilarArtists {
    pub a: ArtistEntry,
    pub b: ArtistEntry,
}

/// What a merge moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeStats {
    pub tracks: usize,
    pub albums: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_ignores_case_and_spacing_only() {
        assert_eq!(entity_norm("  Romeo   SANTOS "), "romeo santos");
        assert_ne!(entity_norm("Daft-Punk"), entity_norm("Daft Punk"));
    }

    #[test]
    fn loose_keys_collide_on_punctuation_and_a_leading_the() {
        assert_eq!(loose_key("Daft-Punk"), loose_key("daft punk"));
        assert_eq!(loose_key("The Daft Punk"), loose_key("Daft Punk"));
        assert_ne!(loose_key("Theory"), loose_key("ory"));
    }
}
