//! Our own widgets. Everything that is not in iced-adaptive-design lives here and nowhere else,
//! so that what is generic can be upstreamed.
//!
//! * **Generic** (nothing about music; candidates for upstreaming): [`synced_text`], [`badge`],
//!   [`detail_header`], [`histogram_range`],
//!   [`editable_text`].
//! * **App-specific** (stay here): [`cover`], [`transport`], [`tag_badge`], [`play_button`],
//!   [`playlist_icon`].
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
pub mod synced_text;
pub mod tag_badge;
pub mod transport;
