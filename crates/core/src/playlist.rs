use serde::{Deserialize, Serialize};

use crate::FilterSpec;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaylistKind {
    /// Saved filter (plus sort and limit); membership is evaluated on demand.
    Dynamic,
    /// Explicit, ordered list of tracks.
    Manual,
}

text_enum!(PlaylistKind { Dynamic => "dynamic", Manual => "manual" });

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Playlist {
    /// Slug, the public ID.
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub kind: PlaylistKind,
    /// Rules for membership (dynamic) or finding eligible additions (manual).
    /// Manual membership and order are curated independently of these rules.
    pub filter: FilterSpec,
}
