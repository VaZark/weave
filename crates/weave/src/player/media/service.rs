//! The souvlaki side: creates the OS media controls, translates session snapshots into souvlaki
//! calls and souvlaki events into `Remote` requests.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, OsMediaControls,
    Repeat, SeekDirection,
};
use weave_player_core::{PlayStatus, RepeatMode};

use super::{Remote, Snapshot, TrackInfo};

/// souvlaki's `set_playback` also emits MPRIS `Seeked`, so the position is only pushed this
/// often while playing (and at once on a state change, a new track or a seek).
const POSITION_EVERY: Duration = Duration::from_secs(1);
/// What a `Seek` without an amount (media keys on some platforms) skips.
const SEEK_STEP_MS: i64 = 5_000;
/// What fast-forward and rewind keys skip.
const WIND_STEP_MS: i64 = 10_000;

pub struct MediaService {
    controls: OsMediaControls,
    remote: mpsc::Receiver<Remote>,
    previous: Option<Snapshot>,
    last_playback: Instant,
}

impl MediaService {
    /// Register with the OS media controls as `name`. On Linux that claims
    /// `org.mpris.MediaPlayer2.<name>` on the session bus; it fails (with the reason) when there
    /// is no session bus or the name is taken. Other platforms are not wired yet.
    pub fn start(name: &str) -> Result<MediaService, String> {
        let mut controls = new_controls(name)?;
        let (tx, remote) = mpsc::channel();
        controls
            .attach(move |event| {
                if let Some(request) = remote_from(&event) {
                    let _ = tx.send(request);
                }
            })
            .map_err(|e| e.to_string())?;
        set_permissions(&mut controls, false)?;
        Ok(MediaService {
            controls,
            remote,
            previous: None,
            last_playback: Instant::now(),
        })
    }

    /// Push what differs from the previous snapshot to the OS.
    pub fn send(&mut self, snapshot: Snapshot) {
        let previous = self.previous.take();
        let first = previous.is_none();
        let changed =
            |f: fn(&Snapshot) -> bool| previous.as_ref().is_none_or(|p| f(p) != f(&snapshot));
        let track_changed = previous.as_ref().is_none_or(|p| p.track != snapshot.track);

        if track_changed {
            let metadata = snapshot
                .track
                .as_ref()
                .map_or_else(MediaMetadata::default, metadata_of);
            let _ = self.controls.set_metadata(metadata);
        }
        if changed(|s| s.track.is_some()) {
            let _ = set_permissions(&mut self.controls, snapshot.track.is_some());
        }
        if previous
            .as_ref()
            .is_none_or(|p| p.volume != snapshot.volume)
        {
            let _ = self.controls.set_volume(f64::from(snapshot.volume));
        }
        if changed(|s| s.shuffle) {
            let _ = self.controls.set_shuffle(snapshot.shuffle);
        }
        if previous
            .as_ref()
            .is_none_or(|p| p.repeat != snapshot.repeat)
        {
            let _ = self.controls.set_repeat(repeat_of(snapshot.repeat));
        }

        let due = first
            || track_changed
            || previous
                .as_ref()
                .is_some_and(|p| p.status != snapshot.status || p.seek_seq != snapshot.seek_seq)
            || (snapshot.status == PlayStatus::Playing
                && self.last_playback.elapsed() >= POSITION_EVERY);
        if due {
            let _ = self.controls.set_playback(playback_of(&snapshot));
            self.last_playback = Instant::now();
        }
        self.previous = Some(snapshot);
    }

    pub fn try_recv(&self) -> Option<Remote> {
        self.remote.try_recv().ok()
    }
}

// --- the platform ------------------------------------------------------------------------------

#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "ios", target_os = "android"))
))]
fn new_controls(name: &str) -> Result<OsMediaControls, String> {
    use souvlaki::platform::mpris::MprisConfig;
    ensure_name_is_free(name)?;
    OsMediaControls::new(MprisConfig {
        dbus_name: name.to_string(),
        identity: "Weave".into(),
        desktop_entry: "weave".into(),
    })
    .map_err(|e| e.to_string())
}

#[cfg(not(all(
    unix,
    not(any(target_os = "macos", target_os = "ios", target_os = "android"))
)))]
fn new_controls(_name: &str) -> Result<OsMediaControls, String> {
    Err("OS media controls are only wired for Linux so far".into())
}

#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "ios", target_os = "android"))
))]
fn set_permissions(controls: &mut OsMediaControls, has_track: bool) -> Result<(), String> {
    use souvlaki::platform::mpris::MprisPermissions;
    controls
        .set_permissions(MprisPermissions {
            can_quit: true,
            can_play: has_track,
            can_pause: has_track,
            can_go_next: has_track,
            can_go_previous: has_track,
            can_seek: has_track,
            can_control: true,
            ..MprisPermissions::none()
        })
        .map_err(|e| e.to_string())
}

#[cfg(not(all(
    unix,
    not(any(target_os = "macos", target_os = "ios", target_os = "android"))
)))]
fn set_permissions(_controls: &mut OsMediaControls, _has_track: bool) -> Result<(), String> {
    Ok(())
}

/// Registering a name another player already owns does not fail: the request is queued and the
/// desktop keeps talking to the first owner. Look first, so the player can say so. (It also
/// fails here, early and clearly, when there is no session bus.)
#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "ios", target_os = "android"))
))]
fn ensure_name_is_free(bus_name: &str) -> Result<(), String> {
    use zbus::blocking::{Connection, fdo::DBusProxy};
    use zbus::names::BusName;
    let full = format!("org.mpris.MediaPlayer2.{bus_name}");
    let connection = Connection::session().map_err(|e| e.to_string())?;
    let proxy = DBusProxy::new(&connection).map_err(|e| e.to_string())?;
    let name = BusName::try_from(full.as_str()).map_err(|e| e.to_string())?;
    if proxy.name_has_owner(name).map_err(|e| e.to_string())? {
        return Err(format!(
            "{full} is already owned by another player (is another weave player running?)"
        ));
    }
    Ok(())
}

// --- pure mappings -------------------------------------------------------------------------------

fn signed(direction: &SeekDirection, ms: i64) -> i64 {
    match direction {
        SeekDirection::Forward => ms,
        SeekDirection::Backward => -ms,
    }
}

/// What the OS asked for, in our terms. `None` for requests with no meaning here.
fn remote_from(event: &MediaControlEvent) -> Option<Remote> {
    Some(match event {
        MediaControlEvent::Play => Remote::Play,
        MediaControlEvent::Pause => Remote::Pause,
        MediaControlEvent::Toggle => Remote::PlayPause,
        MediaControlEvent::Stop => Remote::Stop,
        MediaControlEvent::Next => Remote::Next,
        MediaControlEvent::Previous => Remote::Previous,
        MediaControlEvent::Seek(direction) => Remote::SeekBy(signed(direction, SEEK_STEP_MS)),
        MediaControlEvent::SeekBy(direction, amount) => Remote::SeekBy(signed(
            direction,
            amount.as_millis().min(i64::MAX as u128) as i64,
        )),
        MediaControlEvent::SetPosition(MediaPosition(at)) => {
            Remote::SetPosition(at.as_millis().min(u128::from(u64::MAX)) as u64)
        }
        MediaControlEvent::SetVolume(v) if v.is_finite() => {
            Remote::SetVolume(v.clamp(0.0, 1.0) as f32)
        }
        MediaControlEvent::SetShuffle(on) => Remote::SetShuffle(*on),
        MediaControlEvent::SetRepeat(repeat) => Remote::SetRepeat(repeat_from(*repeat)),
        MediaControlEvent::FastForward => Remote::SeekBy(WIND_STEP_MS),
        MediaControlEvent::Rewind => Remote::SeekBy(-WIND_STEP_MS),
        MediaControlEvent::Quit => Remote::Quit,
        _ => return None,
    })
}

fn repeat_of(r: RepeatMode) -> Repeat {
    match r {
        RepeatMode::Off => Repeat::None,
        RepeatMode::Song => Repeat::Track,
        RepeatMode::Playlist => Repeat::Playlist,
    }
}

fn repeat_from(r: Repeat) -> RepeatMode {
    match r {
        Repeat::None => RepeatMode::Off,
        Repeat::Track => RepeatMode::Song,
        Repeat::Playlist => RepeatMode::Playlist,
    }
}

fn playback_of(s: &Snapshot) -> MediaPlayback {
    let progress = s
        .track
        .as_ref()
        .map(|_| MediaPosition(Duration::from_millis(s.position_ms)));
    match s.status {
        PlayStatus::Stopped => MediaPlayback::Stopped,
        PlayStatus::Paused => MediaPlayback::Paused { progress },
        PlayStatus::Playing => MediaPlayback::Playing { progress },
    }
}

fn metadata_of(t: &TrackInfo) -> MediaMetadata {
    MediaMetadata {
        title: Some(t.title.clone()),
        artist: t.artist.clone(),
        artists: t.artist.clone().map(|a| vec![a]),
        album_title: t.album.clone(),
        duration: t.length_ms.map(Duration::from_millis),
        lyrics: t.lyrics.clone(),
        ..MediaMetadata::default()
    }
}

#[cfg(test)]
mod tests;
