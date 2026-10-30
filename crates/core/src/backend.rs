use serde::{Deserialize, Serialize};

use crate::Metadata;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityGroup {
    Catalog,
    Library,
    Metadata,
    Playback,
    Playlists,
    Assets,
}

/// `supported`, `authorized` and `available` are tracked separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityState {
    pub supported: bool,
    pub authorized: bool,
    pub available: bool,
}

impl CapabilityState {
    pub const READY: Self = Self { supported: true, authorized: true, available: true };
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityDescriptor {
    pub group: CapabilityGroup,
    /// Operation name within the group, e.g. `read`, `stream`.
    pub name: String,
    pub state: CapabilityState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    Available,
    Unavailable,
}

/// Backend identity and capability discovery. Each capability group has its own
/// typed trait (e.g. a library reader) layered on top of this.
pub trait Backend: Send + Sync {
    fn id(&self) -> &str;
    fn capabilities(&self) -> Vec<CapabilityDescriptor>;
}

/// A track as seen by a backend, before the engine assigns identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScannedTrack {
    /// Stable backend-specific reference (for `local`, the canonical file path).
    pub external_id: String,
    pub metadata: Metadata,
}
