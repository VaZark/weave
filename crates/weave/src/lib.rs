//! Weave: a local-first music library, queue and playback engine to embed in players.
//!
//! One dependency gives a player (a GUI, a TUI, a service) everything the `weave-player` CLI has:
//! the library of tracks with tags, playlists, ratings, filters and sorting; a persisted
//! now-playing queue with shuffle, repeat, recommendations and a sleep timer; lyrics; and an
//! embedded [`Player`] that plays the session in your process, with optional OS media
//! controls (media keys and now-playing widgets, through souvlaki). The CLI is only a frontend over this crate.
//!
//! # Shape
//!
//! Everything goes through [`Library`], a handle on one library database. Several handles (even
//! in different processes) can be open at once: the session lives in the database, and the
//! [`Player`] follows it.
//!
//! ```no_run
//! use weave_player_lib::{Library, Player, PlayerOptions};
//!
//! # fn main() -> weave_player_lib::core::Result<()> {
//! let db = std::path::Path::new("library.db");
//! let mut library = Library::open(db)?;
//! library.import(std::path::Path::new("/home/me/Music"))?;
//!
//! // Sound, in this process, until `player` is dropped.
//! let player = Player::start(db, PlayerOptions::default())?;
//! library.play_random(Default::default())?; // everything, shuffled
//!
//! for event in player.events().iter().take(5) {
//!     println!("{event}");
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # Cargo features
//!
//! None are on by default; pick what your player needs.
//!
//! - `audio`: sound output through rodio (needs the ALSA development files on Linux).
//! - `media-controls`: register with the OS media controls (media keys, now-playing widgets)
//!   through souvlaki's in-progress 1.0 branch. Linux (MPRIS) only for now.
//! - `sqlite-from-source`: compile and statically link SQLite instead of using the system one
//!   (3.14.0 or newer).
//!
//! The API is not stable before 0.1.

mod player;

/// A mock library for frontend work. Debug builds only: it is not in release builds.
#[cfg(debug_assertions)]
pub mod mock;
pub mod paths;

/// The domain types: tracks, filters, playlists, the queue and its state, errors.
pub use weave_player_core as core;
pub use weave_player_engine::{ImportStats, Library, SessionView};

pub use player::{
    Audio, DEFAULT_TICK, PlaybackFactory, Player, PlayerEvent, PlayerOptions, lock_path,
};
