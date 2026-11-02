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
mod tag;
mod track;

pub use backend::{
    Availability, Backend, CapabilityDescriptor, CapabilityGroup, CapabilityState, ScannedTrack,
};
pub use duration::{parse_duration, parse_span};
pub use error::{Error, Result};
pub use filter::{FilterSpec, Sort, SortField, TagClause, TagOp, TagRange};
pub use lyrics::{LyricLine, Lyrics};
pub use playback::{Playback, SourceRef};
pub use playlist::{Playlist, PlaylistKind};
pub use queue::{
    Advance, AdvanceReason, EndOfQueue, PlayStatus, PlayerState, Queue, RepeatMode, SleepSetting,
    SleepTimer,
};
pub use rng::Rng;
pub use slug::{playlist_slug, slugify, track_slug};
pub use tag::{
    BPM, ChildPolicy, EXACT_PREFIX, GENRE, Tag, TagName, YEAR, is_numeric_key, metadata_tags,
    normalize_tag, parse_tag_ref,
};
pub use track::{Metadata, Page, SearchResult, Track, TrackId};
