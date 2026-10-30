//! Domain types, filters and backend contracts. No SQL, no UI, no SDKs.

#[macro_use]
mod text_enum;

mod backend;
mod duration;
mod error;
mod filter;
mod playlist;
mod queue;
mod rng;
mod slug;
mod track;

pub use backend::{
    Availability, Backend, CapabilityDescriptor, CapabilityGroup, CapabilityState, ScannedTrack,
};
pub use duration::parse_duration;
pub use error::{Error, Result};
pub use filter::{FilterSpec, Sort, SortField, normalize_tag};
pub use playlist::{Playlist, PlaylistKind};
pub use queue::{Advance, AdvanceReason, EndOfQueue, PlayStatus, PlayerState, Queue, RepeatMode};
pub use rng::Rng;
pub use slug::{playlist_slug, slugify, track_slug};
pub use track::{Metadata, Page, SearchResult, Tag, Track, TrackId};
