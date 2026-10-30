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

/// Source-neutral descriptive metadata, as reported by a backend.
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
    /// The slug, e.g. `romeo-santos-propuesta-indecente-local`.
    pub id: TrackId,
    #[serde(flatten)]
    pub metadata: Metadata,
    /// Custom tags, in display spelling.
    pub tags: Vec<String>,
}

/// A user tag. `name` keeps the spelling it was first created with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tag {
    pub name: String,
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
