use serde::{Deserialize, Serialize};

/// Weave's own track identity: a readable slug assigned at import and never
/// changed afterwards. Never a path or a service ID.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TrackId(pub String);

impl std::fmt::Display for TrackId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Descriptive metadata read from a file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Metadata {
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub genre: Option<String>,
    pub composer: Option<String>,
    pub year: Option<i32>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub duration_ms: Option<u64>,
    pub bpm: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    /// The slug, e.g. `romeo-santos-propuesta-indecente`.
    pub id: TrackId,
    #[serde(flatten)]
    pub metadata: Metadata,
    /// Tags in display spelling: those from imported metadata (`genre`, `year`, `bpm`) and the user's own.
    pub tags: Vec<String>,
    /// 1 to 5 stars; `None` means unrated.
    pub rating: Option<u8>,
    /// When first indexed, in Unix seconds. Reimports retain this timestamp.
    #[serde(default)]
    pub added_at: i64,
}

/// A file found by a scan, before the engine assigns identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScannedTrack {
    /// Canonical path of the file.
    pub path: String,
    pub metadata: Metadata,
}

/// Offset/limit window over an ordered result set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Page {
    pub offset: usize,
    /// `None` returns everything from `offset` on.
    pub limit: Option<usize>,
}

impl Page {
    pub const ALL: Page = Page {
        offset: 0,
        limit: None,
    };

    /// 1-based page number; page 0 is treated as 1.
    pub fn numbered(page: usize, per_page: usize) -> Page {
        Page {
            offset: page.saturating_sub(1) * per_page,
            limit: Some(per_page),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResult {
    /// Matches across all pages.
    pub total: usize,
    pub tracks: Vec<Track>,
}

/// How often, and when last, a track was played: the listening history of one track.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayStat {
    /// The track's slug.
    pub track: String,
    /// Times it started playing.
    pub plays: u32,
    /// Unix seconds when it last started.
    pub last_played_at: i64,
}
