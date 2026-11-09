//! Domain types, filters and backend contracts. No SQL, no UI, no SDKs.

#[macro_use]
mod text_enum;

mod analytics;
mod credits;
mod duration;
mod entity;
mod error;
mod filter;
mod import;
mod instance;
mod lyrics;
mod playback;
mod playlist;
mod queue;
mod rng;
mod slug;
pub mod spectrum;
mod tag;
mod track;

pub use analytics::*;
pub use credits::{Credits, parse_credits};
pub use duration::{parse_duration, parse_span};
pub use entity::{
    AlbumEntry, AlbumProfile, ArtistEntry, ArtistProfile, Count, DedupMark, Dependent,
    DuplicateCandidate, EntityKind, EntityRef, MergeStats, ShufflePref, SimilarPair, Stats,
    TrackDetail, entity_norm, loose_key,
};
pub use error::{Error, Result};
pub use filter::{
    CollectionOrder, FilterJoin, FilterSpec, Sort, SortField, TagClause, TagFilter, TagOp,
};
pub use import::{ImportIssue, ImportReport};
pub use instance::{Instance, default_instance_name, parse_instance_name};
pub use lyrics::{LyricLine, Lyrics};
pub use playback::Playback;
pub use playlist::{Playlist, PlaylistKind};
pub use queue::{
    Advance, AdvanceReason, EndOfQueue, PlayStatus, PlayerState, Queue, RepeatMode, SleepSetting,
    SleepTimer,
};
pub use rng::Rng;
pub use slug::{album_slug, artist_slug, playlist_slug, slugify, track_slug};
pub use tag::{
    ChildPolicy, EXACT_PREFIX, GENRE, Tag, TagName, metadata_tags, normalize_tag, parse_tag_ref,
};
pub use track::{Metadata, Page, PlayStat, ScannedTrack, SearchResult, Track, TrackId};
