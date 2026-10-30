//! Desktop media controls (MPRIS over D-Bus). The host sends snapshots of the session to a
//! thread that owns the D-Bus connection, and receives the commands desktop widgets, media
//! keys and `playerctl` send. Without the `mpris` feature, `start` reports it is unavailable.

use weave_core::{PlayStatus, RepeatMode};

/// A request from a desktop media controller. (Only the D-Bus service constructs these.)
#[cfg_attr(not(feature = "mpris"), allow(dead_code))]
#[derive(Debug, Clone, PartialEq)]
pub enum Remote {
    Play,
    Pause,
    PlayPause,
    Stop,
    Next,
    Previous,
    /// Relative seek in milliseconds (negative goes back).
    SeekBy(i64),
    SetPosition(u64),
    SetVolume(f32),
    SetShuffle(bool),
    SetRepeat(RepeatMode),
    Quit,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrackInfo {
    /// Changes whenever the track (re)starts; becomes the MPRIS track id.
    pub key: u64,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub length_ms: Option<u64>,
    /// Plain lyrics text (`xesam:asText`).
    pub lyrics: Option<String>,
}

/// Everything MPRIS shows about the session.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub status: PlayStatus,
    pub track: Option<TrackInfo>,
    pub position_ms: u64,
    pub volume: f32,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    /// Changes on every seek request, so clients get a `Seeked` signal.
    pub seek_seq: u64,
}

impl Snapshot {
    /// Equal apart from the position, which moves constantly.
    pub fn same_except_position(&self, other: &Snapshot) -> bool {
        Snapshot {
            position_ms: 0,
            ..self.clone()
        } == Snapshot {
            position_ms: 0,
            ..other.clone()
        }
    }
}

#[cfg(feature = "mpris")]
mod service;
#[cfg(feature = "mpris")]
pub use service::Mpris;

#[cfg(not(feature = "mpris"))]
pub struct Mpris;

#[cfg(not(feature = "mpris"))]
impl Mpris {
    pub fn start(_bus_name: &str) -> Result<Mpris, String> {
        Err("this build has no MPRIS support".into())
    }

    pub fn send(&self, _snapshot: Snapshot) {}

    pub fn try_recv(&self) -> Option<Remote> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> Snapshot {
        Snapshot {
            status: PlayStatus::Playing,
            track: None,
            position_ms: 1_000,
            volume: 1.0,
            shuffle: false,
            repeat: RepeatMode::Off,
            seek_seq: 0,
        }
    }

    #[test]
    fn position_alone_does_not_count_as_a_change() {
        let a = snapshot();
        let b = Snapshot {
            position_ms: 9_000,
            ..snapshot()
        };
        assert!(a.same_except_position(&b));
        let c = Snapshot {
            status: PlayStatus::Paused,
            ..snapshot()
        };
        assert!(!a.same_except_position(&c));
        let d = Snapshot {
            seek_seq: 1,
            ..snapshot()
        };
        assert!(!a.same_except_position(&d), "a seek is a change");
    }
}
