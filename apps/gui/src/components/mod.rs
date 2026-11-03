//! Our own widgets. Everything that is not in iced-adaptive-design lives here and nowhere else,
//! so that what is generic can be upstreamed.
//!
//! What is left here is what iced-adaptive-design does not have:
//!
//! * [`histogram_range`]: iad's `ComposedComponent` filled in (a recipe, not a widget).
//! * [`badge`]: a round badge in any colour (iad's `RoundBadge` only takes a role), used by
//!   [`playlist_icon`].
//! * App-specific: [`cover`], [`transport`], [`tag_badge`] (iad's `Chip` with a tag's icon and
//!   colour), [`playlist_icon`].
//!
//! Upstreamed and removed from here: `SyncedText`, `VirtualList`, `EditableText`, `DetailHeader`,
//! the round suggested play button, the count badge and the menu with end icons.
//!
pub mod badge;
pub mod cover;
pub mod histogram_range;
pub mod playlist_icon;
pub mod tag_badge;
pub mod transport;
