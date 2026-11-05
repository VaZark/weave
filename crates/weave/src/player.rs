//! The embedded player: plays the persisted session inside your process.
//!
//! A [`Player`] runs on its own thread with its own connection to the library database. It
//! follows the session that any [`Library`](crate::Library) handle edits (the same way the
//! `weave-player` CLI and its host process do): it starts tracks, pauses, seeks, applies
//! volume, reports the real position, advances when a track ends, enforces the sleep timer
//! and (with the `media-controls` feature) answers the OS media controls. Only one player can run per
//! library at a time, across processes.

mod host;
mod lock;
mod media;

use std::fmt;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread::JoinHandle;
use std::time::Duration;

use weave_player_core::{Error, PlayStatus, Playback, Result};
use weave_player_engine::Library;

use host::Host;
use lock::HostLock;
use media::MediaService;

pub use host::TICK as DEFAULT_TICK;
pub use lock::lock_path;

/// Builds the audio output. It runs on the player's thread, so the output it creates does not
/// have to be `Send` (audio devices usually are not).
pub type PlaybackFactory = Box<dyn FnOnce() -> Result<Box<dyn Playback>> + Send>;

/// Where the sound goes.
#[derive(Default)]
pub enum Audio {
    /// The default output device (needs the `audio` feature). Starting fails if there is none.
    #[default]
    Device,
    /// Follow the session without producing sound (servers, tests, UIs that play elsewhere).
    Off,
    /// Your own output.
    Custom(PlaybackFactory),
}

impl fmt::Debug for Audio {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Audio::Device => "Device",
            Audio::Off => "Off",
            Audio::Custom(_) => "Custom(..)",
        })
    }
}

#[derive(Debug)]
pub struct PlayerOptions {
    pub audio: Audio,
    /// Register with the OS media controls: media keys and now-playing widgets (needs the
    /// `media-controls` feature; wired for Linux, where it needs a session bus). If that fails the
    /// player carries on and reports a [`PlayerEvent::Error`].
    pub media_controls: bool,
    /// The name to register as. On Linux the MPRIS bus name `org.mpris.MediaPlayer2.<name>`.
    pub media_controls_name: String,
    /// How often the session is re-read.
    pub tick: Duration,
}

impl Default for PlayerOptions {
    fn default() -> Self {
        Self {
            audio: Audio::Device,
            media_controls: cfg!(feature = "media-controls"),
            media_controls_name: "weave".into(),
            tick: DEFAULT_TICK,
        }
    }
}

impl PlayerOptions {
    /// No sound and no desktop integration: only follows the session.
    pub fn silent() -> Self {
        Self {
            audio: Audio::Off,
            media_controls: false,
            ..Self::default()
        }
    }
}

/// Something that happened while following the session.
#[derive(Debug, Clone, PartialEq)]
pub enum PlayerEvent {
    /// A track started (or restarted).
    TrackStarted {
        title: String,
        /// The track's ID.
        track: Option<String>,
    },
    StatusChanged(PlayStatus),
    /// The sleep timer stopped playback.
    SleepStopped,
    /// A stop is scheduled for the end of the current track.
    StopScheduled {
        title: String,
    },
    /// Something failed; the player keeps going.
    Error(String),
}

impl PlayerEvent {
    /// A short stable name, for logs and machine-readable output.
    pub fn name(&self) -> &'static str {
        match self {
            PlayerEvent::TrackStarted { .. } => "track",
            PlayerEvent::StatusChanged(_) => "status",
            PlayerEvent::SleepStopped => "sleep_stopped",
            PlayerEvent::StopScheduled { .. } => "stop_scheduled",
            PlayerEvent::Error(_) => "error",
        }
    }

    /// The ID of the track this is about, if it is about one.
    pub fn track(&self) -> Option<&str> {
        match self {
            PlayerEvent::TrackStarted { track, .. } => track.as_deref(),
            _ => None,
        }
    }
}

impl fmt::Display for PlayerEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlayerEvent::TrackStarted { title, .. } => write!(f, "now on \"{title}\""),
            PlayerEvent::StatusChanged(status) => write!(f, "{status}"),
            PlayerEvent::SleepStopped => f.write_str("sleep timer: playback stopped"),
            PlayerEvent::StopScheduled { title } => write!(f, "stopping after \"{title}\""),
            PlayerEvent::Error(message) => f.write_str(message),
        }
    }
}

/// A running player. Dropping it stops it and releases the library's player lock.
pub struct Player {
    events: Receiver<PlayerEvent>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Player {
    /// Start a player for the library at `db`. Errors, synchronously, if another player is
    /// already running for it ([`Error::AlreadyExists`]), the library cannot be opened, or the
    /// audio output cannot be created.
    pub fn start(db: impl AsRef<Path>, options: PlayerOptions) -> Result<Player> {
        let db = db.as_ref().to_path_buf();
        let lock = HostLock::acquire(&db)
            .map_err(|e| Error::Player(format!("cannot take the player lock: {e}")))?
            .ok_or_else(|| {
                Error::AlreadyExists("a player is already running for this library".into())
            })?;

        let (events_tx, events) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::Builder::new()
            .name("weave-player".into())
            .spawn(move || {
                let _lock = lock;
                run(&db, options, &events_tx, &ready_tx, &flag);
            })
            .map_err(|e| Error::Player(format!("cannot start the player thread: {e}")))?;

        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Player {
                events,
                stop,
                thread: Some(thread),
            }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => {
                let _ = thread.join();
                Err(Error::Player("the player stopped while starting".into()))
            }
        }
    }

    /// Whether a player (in this or another process) is running for the library at `db`.
    pub fn is_running(db: impl AsRef<Path>) -> bool {
        lock::is_running(db.as_ref())
    }

    /// What the player reports. The channel closes when the player ends.
    pub fn events(&self) -> &Receiver<PlayerEvent> {
        &self.events
    }

    /// Whether the player has ended by itself (a `quit` request, or a desktop "Quit").
    pub fn is_finished(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }

    /// Stop the player and wait for it to end.
    pub fn stop(mut self) {
        self.shut_down();
    }

    fn shut_down(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.shut_down();
    }
}

fn run(
    db: &Path,
    options: PlayerOptions,
    events: &mpsc::Sender<PlayerEvent>,
    ready: &mpsc::Sender<Result<()>>,
    stop: &AtomicBool,
) {
    let setup = || -> Result<(Library, Host)> {
        let mut lib = Library::open(db)?;
        lib.ensure_playback_allowed()?; // a remote-only instance never plays
        lib.clear_host_quit()?; // an old quit request must not end this player at once
        let playback = match options.audio {
            Audio::Off => None,
            Audio::Device => Some(lib.open_playback()?),
            Audio::Custom(factory) => Some(factory()?),
        };
        let media = if options.media_controls {
            match MediaService::start(&options.media_controls_name) {
                Ok(m) => Some(m),
                Err(e) => {
                    let _ = events.send(PlayerEvent::Error(format!(
                        "media controls are unavailable ({e}); continuing without them"
                    )));
                    None
                }
            }
        } else {
            None
        };
        Ok((lib, Host::new(playback).with_media(media)))
    };
    let (mut lib, mut host) = match setup() {
        Ok(ready_parts) => ready_parts,
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    let _ = ready.send(Ok(()));

    loop {
        let wait = match host.tick(&mut lib) {
            Ok(batch) => {
                for event in batch {
                    let _ = events.send(event); // nobody listening is fine
                }
                options.tick
            }
            Err(e) => {
                let _ = events.send(PlayerEvent::Error(e.to_string()));
                options.tick.max(Duration::from_secs(1)) // do not spam a persistent failure
            }
        };
        if stop.load(Ordering::SeqCst)
            || host.should_quit()
            || lib.host_quit_requested().unwrap_or(false)
        {
            break;
        }
        std::thread::sleep(wait);
    }
    if let Err(e) = host.shutdown(&mut lib) {
        let _ = events.send(PlayerEvent::Error(e.to_string()));
    }
    let _ = lib.clear_host_quit();
}
