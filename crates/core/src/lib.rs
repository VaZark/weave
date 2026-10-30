//! Domain types, filters and backend contracts. No SQL, no UI, no SDKs.

#[macro_use]
mod text_enum;

mod backend;
mod duration;
mod error;
mod filter;
mod lyrics;
mod playback;
mod playlist;
mod queue;
mod rng;
mod slug;
mod track;

pub use backend::{
    Availability, Backend, CapabilityDescriptor, CapabilityGroup, CapabilityState, ScannedTrack,
};
pub use duration::{parse_duration, parse_span};
pub use error::{Error, Result};
pub use filter::{FilterSpec, Sort, SortField, normalize_tag};
pub use lyrics::{LyricLine, Lyrics};
pub use playback::{Playback, SourceRef};
pub use playlist::{Playlist, PlaylistKind};
pub use queue::{
    Advance, AdvanceReason, EndOfQueue, PlayStatus, PlayerState, Queue, QueueSource, RepeatMode,
    SleepSetting, SleepTimer,
};
pub use rng::Rng;
pub use slug::{playlist_slug, slugify, track_slug};
pub use track::{Metadata, Page, SearchResult, Tag, Track, TrackId};
