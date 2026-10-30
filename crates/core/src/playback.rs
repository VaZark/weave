//! The playback capability: what a backend must offer to play one track at a time.

use std::time::Duration;

use crate::Result;

/// A backend's reference to one playable source of a track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRef {
    pub backend_id: String,
    pub external_id: String,
}

/// Plays one source at a time. The host follows the session and calls these; the
/// session itself (queue, shuffle, repeat) is decided by the engine, never here.
pub trait Playback {
    /// Replace whatever is playing with `source`, starting at `from`.
    fn start(&mut self, source: &SourceRef, from: Duration) -> Result<()>;
    fn pause(&mut self);
    fn resume(&mut self);
    fn stop(&mut self);
    fn seek(&mut self, to: Duration) -> Result<()>;
    fn position(&self) -> Duration;
    /// The started source played to its end (false while it plays, is paused or was stopped).
    fn finished(&self) -> bool;
    /// 0.0 (mute) to 1.0.
    fn set_volume(&mut self, volume: f32);
}
