//! Domain types, filters and backend contracts. No SQL, no UI, no SDKs.

mod backend;
mod error;
mod filter;
mod track;

pub use backend::{
    Availability, Backend, CapabilityDescriptor, CapabilityGroup, CapabilityState, ScannedTrack,
};
pub use error::{Error, Result};
pub use filter::{FilterSpec, normalize_tag};
pub use track::{Metadata, Page, SearchResult, Tag, Track, TrackId};
