//! Components that are not upstream (yet). [`media_list`] is generic and meant for
//! iced-adaptive-design; [`tag_badge`] is weave's tag-to-icon-and-colour lookup over its `Chip`.
//! Generic widgets that used to live here are upstream; MediaHeader remains the app-owned media composition
//! (`SyncedText`, `VirtualList`, `EditableText`, `DetailHeader`, `HistogramRangeSlider`,
//! `IconToggleButton`, `Avatar` icons and shapes, `ChipGroup`,
//! `FlowGrid`, `SectionHeading`, `Badge`, `RoundBadge`, the circular play button, the menus with
//! end icons, and `primitives::history`).

pub mod media_list;
pub mod tag_badge;

pub mod media_header;
pub use media_header::MediaHeader;

pub mod tags_input;

pub mod collection;

pub mod collection_flow;

pub mod disc_artwork;
