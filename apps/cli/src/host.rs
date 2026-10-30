//! The playback host: a resident process that plays the persisted session.
//!
//! Commands only edit the session (queue, status, seek requests, volume); the host follows
//! it, drives the audio, tells the engine when a track ends, publishes the real position,
//! applies the sleep timer on time and answers to `quit`. Commands that leave the session
//! playing start a host if none is running.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use weave_core::{Error, PlayStatus, Playback, PlayerState};
use weave_engine::{Library, SessionView};

use crate::Failure;
use crate::mpris::{Mpris, Remote, Snapshot, TrackInfo};

/// How often the host re-reads the session.
pub const TICK: Duration = Duration::from_millis(100);
/// How often the real position is published for other commands.
const POSITION_EVERY: Duration = Duration::from_secs(1);

// --- one host per library ------------------------------------------------------------

fn sibling(db: &Path, suffix: &str) -> PathBuf {
    let mut name = db.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

pub fn lock_path(db: &Path) -> PathBuf {
    sibling(db, ".host.lock")
}

pub fn log_path(db: &Path) -> PathBuf {
    sibling(db, ".host.log")
}

/// Held for as long as a host runs; released by the OS even if the process dies.
pub struct HostLock(#[allow(dead_code)] File);

impl HostLock {
    /// `None` when another host holds the lock.
    pub fn acquire(db: &Path) -> std::io::Result<Option<HostLock>> {
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path(db))?;
        match file.try_lock() {
            Ok(()) => {
                file.set_len(0)?;
                write!(file, "{}", std::process::id())?;
                Ok(Some(HostLock(file)))
            }
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(e)) => Err(e),
        }
    }
}

/// Whether a host currently holds the lock.
pub fn is_running(db: &Path) -> bool {
    let Ok(file) = File::open(lock_path(db)) else {
        return false;
    };
    match file.try_lock_shared() {
        Ok(()) => false, // nobody holds the exclusive lock
        Err(TryLockError::WouldBlock) => true,
        Err(TryLockError::Error(_)) => false,
    }
}

/// Start a detached host for `db` unless one is running. Returns the log file's path when
/// it started one.
pub fn ensure_running(db: &Path) -> std::io::Result<Option<PathBuf>> {
    if is_running(db) {
        return Ok(None);
    }
    let log = log_path(db);
    let out = File::create(&log)?;
    let mut cmd = std::process::Command::new(std::env::current_exe()?);
    cmd.arg("--db")
        .arg(db)
        .arg("host")
        .stdin(std::process::Stdio::null())
        .stdout(out.try_clone()?)
        .stderr(out);
    #[cfg(unix)]
    {
        // Its own process group, so Ctrl-C in the terminal that started it does not kill it.
        std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    }
    cmd.spawn()?;
    Ok(Some(log))
}

// --- following the session ---------------------------------------------------------------

/// What the audio has to do to match the session.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    /// Load the current track and begin at this position (milliseconds).
    Start(u64),
    Pause,
    Resume,
    Stop,
    Seek(u64),
    Volume(f32),
}

/// Turns changes of the persisted session into audio actions. Pure, so it is tested
/// without any sound device.
#[derive(Debug, Default)]
pub struct Follower {
    loaded: Option<u64>, // the start_seq of the track the audio has loaded
    seek_seq: u64,
    status: PlayStatus,
    volume: Option<f32>,
}

impl Follower {
    /// `position_ms` is where playback is right now according to the session.
    pub fn step(&mut self, st: &PlayerState, position_ms: u64) -> Vec<Action> {
        let mut actions = Vec::new();
        if self.volume != Some(st.volume) {
            self.volume = Some(st.volume);
            actions.push(Action::Volume(st.volume));
        }

        if st.status == PlayStatus::Stopped || st.current.is_none() {
            if self.loaded.take().is_some() {
                actions.push(Action::Stop);
            }
            self.seek_seq = st.seek_seq;
            self.status = PlayStatus::Stopped;
            return actions;
        }

        if self.loaded != Some(st.start_seq) {
            self.loaded = Some(st.start_seq);
            self.seek_seq = st.seek_seq;
            actions.push(Action::Start(position_ms));
            // A new track begins playing; if the session is paused it must not.
            self.status = PlayStatus::Playing;
        } else if self.seek_seq != st.seek_seq {
            self.seek_seq = st.seek_seq;
            actions.push(Action::Seek(position_ms));
        }

        match (self.status, st.status) {
            (PlayStatus::Playing, PlayStatus::Paused) => actions.push(Action::Pause),
            (PlayStatus::Paused, PlayStatus::Playing) => actions.push(Action::Resume),
            _ => {}
        }
        self.status = st.status;
        actions
    }
}

// --- the host loop -------------------------------------------------------------------------

/// Something that happened, for the log.
#[derive(Debug, PartialEq)]
pub struct Event {
    pub name: &'static str,
    pub text: String,
    pub track: Option<String>,
}

pub struct Host {
    follower: Follower,
    playback: Option<Box<dyn Playback>>,
    started: Instant,
    last_report: Instant,
    /// How often the real position is published (a field so tests can speed it up).
    pub report_every: Duration,
    last_view: Option<SessionView>,
    /// Consecutive tracks skipped because they could not be played.
    skipped: usize,
    mpris: Option<Mpris>,
    last_snapshot: Option<(Snapshot, Instant)>,
    /// Plain lyrics of the track that started with this `start_seq`, for `xesam:asText`.
    lyrics_cache: Option<(u64, Option<String>)>,
    quit: bool,
}

impl Host {
    pub fn new(playback: Option<Box<dyn Playback>>) -> Self {
        let now = Instant::now();
        Self {
            follower: Follower::default(),
            playback,
            started: now,
            last_report: now,
            report_every: POSITION_EVERY,
            last_view: None,
            skipped: 0,
            mpris: None,
            last_snapshot: None,
            lyrics_cache: None,
            quit: false,
        }
    }

    pub fn with_mpris(mut self, mpris: Option<Mpris>) -> Self {
        self.mpris = mpris;
        self
    }

    /// A desktop controller asked the host to exit.
    pub fn should_quit(&self) -> bool {
        self.quit
    }

    /// One pass: follow the session, detect a finished track, publish the position.
    pub fn tick(&mut self, lib: &mut Library) -> Result<Vec<Event>, Failure> {
        let mut events = Vec::new();
        while let Some(remote) = self.mpris.as_ref().and_then(Mpris::try_recv) {
            match apply_remote(lib, &remote) {
                Ok(true) => self.quit = true,
                Ok(false) => {}
                Err(e) => events.push(Event {
                    name: "error",
                    text: format!("{remote:?}: {e}"),
                    track: None,
                }),
            }
        }
        let state = lib.player_state()?;
        let position = lib.session_position_ms()?;

        for action in self.follower.step(&state, position) {
            if let Err(e) = self.apply(lib, action) {
                events.push(Event {
                    name: "error",
                    text: e.to_string(),
                    track: None,
                });
                self.skip_unplayable(lib, &mut events)?;
            }
        }

        if let Some(p) = self.playback.as_mut() {
            if state.status == PlayStatus::Playing && p.finished() {
                lib.track_ended()?;
            } else if state.status == PlayStatus::Playing
                && self.last_report.elapsed() >= self.report_every
            {
                self.last_report = Instant::now();
                lib.report_position(state.start_seq, p.position())?;
            }
        }

        let view = lib.session()?;
        self.publish_to_mpris(lib, &view)?;
        if let Some(before) = &self.last_view {
            events.extend(session_events(before, &view));
        }
        self.last_view = Some(view);
        Ok(events)
    }

    /// Send the session to MPRIS when something besides the position changed, and the
    /// position itself about twice a second.
    fn publish_to_mpris(&mut self, lib: &Library, view: &SessionView) -> Result<(), Failure> {
        let Some(mpris) = &self.mpris else {
            return Ok(());
        };
        let key = view.state.start_seq;
        if self.lyrics_cache.as_ref().map(|(k, _)| *k) != Some(key) {
            let text = match view.current() {
                Some(t) => lib.lyrics(&t.id.0)?.map(|l| l.plain_text()),
                None => None,
            };
            self.lyrics_cache = Some((key, text));
        }
        let lyrics = self.lyrics_cache.as_ref().and_then(|(_, t)| t.clone());
        let snapshot = snapshot_of(view, lyrics);
        let due = match &self.last_snapshot {
            None => true,
            Some((prev, at)) => {
                !prev.same_except_position(&snapshot) || at.elapsed() >= Duration::from_millis(500)
            }
        };
        if due {
            mpris.send(snapshot.clone());
            self.last_snapshot = Some((snapshot, Instant::now()));
        }
        Ok(())
    }

    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    fn apply(&mut self, lib: &mut Library, action: Action) -> Result<(), Error> {
        let Some(p) = self.playback.as_mut() else {
            return Ok(());
        };
        match action {
            Action::Start(from_ms) => {
                let track = lib
                    .current_track_if_any()?
                    .ok_or_else(|| Error::InvalidInput("nothing is queued".into()))?;
                let source = lib.playback_source(&track.id.0).map_err(|e| {
                    Error::Backend(format!("cannot play \"{}\": {e}", track.metadata.title))
                })?;
                p.start(&source, Duration::from_millis(from_ms))?;
                self.skipped = 0;
            }
            Action::Pause => p.pause(),
            Action::Resume => p.resume(),
            Action::Stop => p.stop(),
            Action::Seek(ms) => p.seek(Duration::from_millis(ms))?,
            Action::Volume(v) => p.set_volume(v),
        }
        Ok(())
    }

    /// The track could not be played: move on, but never loop forever over a queue that
    /// has nothing playable in it.
    fn skip_unplayable(
        &mut self,
        lib: &mut Library,
        events: &mut Vec<Event>,
    ) -> Result<(), Failure> {
        self.skipped += 1;
        let len = lib.session()?.tracks.len().max(1);
        if self.skipped >= len {
            lib.stop()?;
            self.skipped = 0;
            events.push(Event {
                name: "error",
                text: "nothing in the queue could be played; stopped".into(),
                track: None,
            });
        } else {
            lib.next()?;
        }
        Ok(())
    }
}

/// What MPRIS shows for a session.
pub fn snapshot_of(v: &SessionView, lyrics: Option<String>) -> Snapshot {
    Snapshot {
        status: v.state.status,
        track: v.current().map(|t| TrackInfo {
            key: v.state.start_seq,
            title: t.metadata.title.clone(),
            artist: t.metadata.artist.clone(),
            album: t.metadata.album.clone(),
            length_ms: t.metadata.duration_ms,
            lyrics,
        }),
        position_ms: v.position.as_millis() as u64,
        volume: v.state.volume,
        shuffle: v.state.shuffle,
        repeat: v.state.repeat,
        seek_seq: v.state.seek_seq,
    }
}

/// Carry out a desktop controller's request through the same engine calls the CLI uses.
/// Returns `true` when the host should exit.
pub fn apply_remote(lib: &mut Library, remote: &Remote) -> Result<bool, Error> {
    let play = |lib: &mut Library| -> Result<(), Error> {
        let st = lib.player_state()?;
        match (st.status, st.current) {
            (PlayStatus::Playing, _) => {}
            (PlayStatus::Paused, _) | (PlayStatus::Stopped, Some(_)) => {
                lib.resume()?;
            }
            // Nothing selected yet: start the queue from its first track.
            (PlayStatus::Stopped, None) => {
                lib.next()?;
            }
        }
        Ok(())
    };
    match remote {
        Remote::Play => play(lib)?,
        Remote::Pause => {
            if lib.player_state()?.status == PlayStatus::Playing {
                lib.pause()?;
            }
        }
        Remote::PlayPause => {
            if lib.player_state()?.status == PlayStatus::Playing {
                lib.pause()?;
            } else {
                play(lib)?;
            }
        }
        Remote::Stop => {
            lib.stop()?;
        }
        Remote::Next => {
            lib.next()?;
        }
        Remote::Previous => {
            lib.previous()?;
        }
        Remote::SeekBy(ms) => {
            let now = lib.session_position_ms()? as i64;
            lib.seek(Duration::from_millis((now + ms).max(0) as u64))?;
        }
        Remote::SetPosition(ms) => {
            lib.seek(Duration::from_millis(*ms))?;
        }
        Remote::SetVolume(v) => {
            lib.set_volume(*v)?;
        }
        Remote::SetShuffle(on) => {
            lib.set_shuffle(*on)?;
        }
        Remote::SetRepeat(mode) => {
            lib.set_repeat(*mode)?;
        }
        Remote::Quit => return Ok(true),
    }
    Ok(false)
}

fn label(v: &SessionView) -> String {
    v.current()
        .map_or_else(|| "(nothing)".into(), |t| t.metadata.title.clone())
}

fn track_id(v: &SessionView) -> Option<String> {
    v.current().map(|t| t.id.0.clone())
}

/// What changed between two snapshots of the session.
pub fn session_events(before: &SessionView, after: &SessionView) -> Vec<Event> {
    let mut out = Vec::new();
    let mut push = |name, text: String| {
        out.push(Event {
            name,
            text,
            track: track_id(after),
        })
    };

    let timer_gone = before.state.sleep.is_some() && after.state.sleep.is_none();
    if timer_gone && after.state.status == PlayStatus::Stopped {
        push("sleep_stopped", "sleep timer: playback stopped".into());
    } else if timer_gone && after.state.stop_after_track {
        push(
            "stop_scheduled",
            format!("sleep timer: stopping after \"{}\"", label(after)),
        );
    } else if !before.state.stop_after_track && after.state.stop_after_track {
        push(
            "stop_scheduled",
            format!("stop scheduled: stopping after \"{}\"", label(after)),
        );
    }
    if (track_id(before) != track_id(after) || before.state.start_seq != after.state.start_seq)
        && after.state.current.is_some()
        && after.state.status != PlayStatus::Stopped
    {
        push("track", format!("now on \"{}\"", label(after)));
    }
    if before.state.status != after.state.status && !timer_gone {
        push("status", format!("{}", after.state.status));
    }
    out
}

/// Print one event as a log line (JSON lines with `--json`).
pub fn emit(json: bool, elapsed: Duration, e: &Event) {
    if json {
        outln!(
            "{}",
            serde_json::json!({
                "at_secs": elapsed.as_secs(),
                "event": e.name,
                "message": e.text,
                "track": e.track,
            })
        );
    } else {
        outln!("[+{}s] {}", elapsed.as_secs(), e.text);
    }
}

#[cfg(test)]
mod tests;
