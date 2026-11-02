//! Following the persisted session: turns its changes into audio actions, detects finished
//! tracks, publishes the real position, applies the sleep timer and carries out desktop
//! (media-key and widget) requests. `Player` runs this on a thread; the logic is kept apart from threads and
//! devices so it is tested against a fake `Playback`.

use std::time::{Duration, Instant};

use weave_core::{Error, PlayStatus, Playback, PlayerState, Result};
use weave_engine::{Library, SessionView};

use super::PlayerEvent;
use super::media::{MediaService, Remote, Snapshot, TrackInfo};

/// How often the player re-reads the session.
pub const TICK: Duration = Duration::from_millis(100);
/// A track that started less than this long ago is played from its beginning: the player only
/// noticed it a tick late, and skipping the opening of a song would be worse than replaying a
/// second. Only a player that joins a session well into a track resumes in place.
const RESUME_AFTER_MS: u64 = 1_000;
/// How often the real position is published for other processes.
const POSITION_EVERY: Duration = Duration::from_secs(1);

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
            let from = if position_ms < RESUME_AFTER_MS {
                0
            } else {
                position_ms
            };
            actions.push(Action::Start(from));
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

pub struct Host {
    follower: Follower,
    playback: Option<Box<dyn Playback>>,
    last_report: Instant,
    /// How often the real position is published (a field so tests can speed it up).
    pub report_every: Duration,
    last_view: Option<SessionView>,
    /// Consecutive tracks skipped because they could not be played.
    skipped: usize,
    media: Option<MediaService>,
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
            last_report: now,
            report_every: POSITION_EVERY,
            last_view: None,
            skipped: 0,
            media: None,
            last_snapshot: None,
            lyrics_cache: None,
            quit: false,
        }
    }

    pub fn with_media(mut self, media: Option<MediaService>) -> Self {
        self.media = media;
        self
    }

    /// A desktop controller asked the host to exit.
    pub fn should_quit(&self) -> bool {
        self.quit
    }

    /// One pass: follow the session, detect a finished track, publish the position.
    pub fn tick(&mut self, lib: &mut Library) -> Result<Vec<PlayerEvent>> {
        let mut events = Vec::new();
        while let Some(remote) = self.media.as_ref().and_then(MediaService::try_recv) {
            match apply_remote(lib, &remote) {
                Ok(true) => self.quit = true,
                Ok(false) => {}
                Err(e) => events.push(PlayerEvent::Error(format!("{remote:?}: {e}"))),
            }
        }
        let state = lib.player_state()?;
        let position = lib.session_position_ms()?;

        for action in self.follower.step(&state, position) {
            if let Err(e) = self.apply(lib, action) {
                events.push(PlayerEvent::Error(e.to_string()));
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
        self.publish_to_media(lib, &view)?;
        if let Some(before) = &self.last_view {
            events.extend(session_events(before, &view));
        }
        self.last_view = Some(view);
        Ok(events)
    }

    /// Send the session to the OS media controls when something besides the position changed,
    /// and the position itself about twice a second.
    fn publish_to_media(&mut self, lib: &Library, view: &SessionView) -> Result<()> {
        let Some(media) = self.media.as_mut() else {
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
            media.send(snapshot.clone());
            self.last_snapshot = Some((snapshot, Instant::now()));
        }
        Ok(())
    }

    fn apply(&mut self, lib: &mut Library, action: Action) -> Result<()> {
        let Some(p) = self.playback.as_mut() else {
            return Ok(());
        };
        match action {
            Action::Start(from_ms) => {
                let track = lib
                    .current_track_if_any()?
                    .ok_or_else(|| Error::InvalidInput("nothing is queued".into()))?;
                let cannot = |e: Error| {
                    Error::Player(format!("cannot play \"{}\": {e}", track.metadata.title))
                };
                let path = lib.playback_path(&track.id.0).map_err(cannot)?;
                p.start(&path, Duration::from_millis(from_ms))
                    .map_err(|e| {
                        Error::Player(format!("cannot play \"{}\": {e}", track.metadata.title))
                    })?;
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
    fn skip_unplayable(&mut self, lib: &mut Library, events: &mut Vec<PlayerEvent>) -> Result<()> {
        self.skipped += 1;
        let len = lib.session()?.tracks.len().max(1);
        if self.skipped >= len {
            lib.stop()?;
            self.skipped = 0;
            events.push(PlayerEvent::Error(
                "nothing in the queue could be played; stopped".into(),
            ));
        } else {
            lib.next()?;
        }
        Ok(())
    }
}

/// What the OS media controls show for a session.
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
pub fn apply_remote(lib: &mut Library, remote: &Remote) -> Result<bool> {
    let play = |lib: &mut Library| -> Result<()> {
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
pub fn session_events(before: &SessionView, after: &SessionView) -> Vec<PlayerEvent> {
    let mut out = Vec::new();
    let timer_gone = before.state.sleep.is_some() && after.state.sleep.is_none();
    if timer_gone && after.state.status == PlayStatus::Stopped {
        out.push(PlayerEvent::SleepStopped);
    } else if (timer_gone && after.state.stop_after_track)
        || (!before.state.stop_after_track && after.state.stop_after_track)
    {
        out.push(PlayerEvent::StopScheduled {
            title: label(after),
        });
    }
    if (track_id(before) != track_id(after) || before.state.start_seq != after.state.start_seq)
        && after.state.current.is_some()
        && after.state.status != PlayStatus::Stopped
    {
        out.push(PlayerEvent::TrackStarted {
            title: label(after),
            track: track_id(after),
        });
    }
    if before.state.status != after.state.status && !timer_gone {
        out.push(PlayerEvent::StatusChanged(after.state.status));
    }
    out
}

#[cfg(test)]
mod tests;
