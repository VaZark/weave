//! Components that are not upstream (yet). [`media_list`] is generic and meant for
//! iced-adaptive-design; [`tag_badge`] is weave's tag-to-icon-and-colour lookup over its `Chip`.
//! Everything else that used to live here is upstream now
//! (`SyncedText`, `VirtualList`, `EditableText`, `DetailHeader`, `HistogramRangeSlider`,
//! `IconToggleButton`, `Avatar` icons and shapes, `ChipGroup`,
//! `CardGrid`, `SectionHeading`, `Badge`, `RoundBadge`, the circular play button, the menus with
//! end icons, and `primitives::history`).

pub mod media_list;
pub mod tag_badge;
