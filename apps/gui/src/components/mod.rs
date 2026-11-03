//! Our own widgets. Everything that is not in iced-adaptive-design lives here and nowhere else,
//! so that what is generic can be upstreamed.
//!
//! * **Generic** (nothing about music; candidates for upstreaming): [`badge`], [`detail_header`],
//!   [`editable_text`], [`virtual_list`]. [`histogram_range`] is iad's `ComposedComponent` filled
//!   in, so it is a recipe more than a widget.
//! * **App-specific** (stay here): [`cover`], [`transport`], [`tag_badge`], [`play_button`],
//!   [`playlist_icon`].
//!
//! Already upstreamed and removed from here: `SyncedText` (with `primitives::synced`).
//!
//! A generic component takes plain data and messages, uses only iced and `iad`, and keeps its
//! headless logic in `crate::primitives`. It does not import `data`, `Message` or `App`.

pub mod badge;
pub mod cover;
pub mod detail_header;
pub mod editable_text;
pub mod histogram_range;
pub mod play_button;
pub mod playlist_icon;
pub mod tag_badge;
pub mod transport;
pub mod virtual_list;
