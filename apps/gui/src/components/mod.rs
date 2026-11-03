//! The one component left here: [`tag_badge`], which is weave's tag-to-icon-and-colour lookup over
//! iced-adaptive-design's `Chip`. Everything else that used to live here is upstream now
//! (`SyncedText`, `VirtualList`, `EditableText`, `DetailHeader`, `HistogramRangeSlider`,
//! `IconToggleButton`, `Avatar` icons and shapes, `MediaRow`, `SortHeader`, `ChipGroup`,
//! `CardGrid`, `SectionHeading`, `Badge`, `RoundBadge`, the circular play button, the menus with
//! end icons, and `primitives::history`).

pub mod tag_badge;
