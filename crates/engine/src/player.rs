//! Playback session orchestration: queue edits, transport state, shuffle/repeat,
//! end-of-queue behavior and the sleep timer. The session is persisted after every
//! operation, so separate CLI invocations share it. No audio is produced here; the
//! playback host (a resident process) follows `SessionView` and plays it.
//!
//! The sleep timer is checked whenever the session is read or changed (the host does so
//! every 100 ms), and playback is stopped at that point if it ran out.

use std::time::Duration;

use weave_core::{
    Advance, AdvanceReason, Backend, CapabilityGroup, EndOfQueue, Error, FilterSpec, Lyrics, Page,
    PlayStatus, Playback, PlayerState, Queue, QueueSource, RepeatMode, Result, SleepSetting,
    SourceRef, Track, TrackId,
};

use crate::{Library, autoplay};

#[derive(Debug, Clone)]
pub struct SessionView {
    pub state: PlayerState,
    /// Queue contents in list order.
    pub tracks: Vec<Track>,
    /// Time left on the sleep timer, if one is set.
    pub sleep_remaining: Option<Duration>,
    /// Where playback is in the current track.
    pub position: Duration,
    /// The display name of the playlist when the queue's source is one.
    pub source_name: Option<String>,
}

impl SessionView {
    pub fn current(&self) -> Option<&Track> {
        self.state.current.and_then(|i| self.tracks.get(i))
    }
}

fn to_index(position: usize) -> Result<usize> {
    position
        .checked_sub(1)
        .ok_or_else(|| Error::InvalidInput("queue positions start at 1".into()))
}

impl Library {
    fn now_ms(&self) -> i64 {
        (self.clock)()
    }

    /// Load the session, first handling a sleep timer that came due (see
    /// `Queue::apply_sleep`). Also reports whether it did.
    fn load_session_checked(&mut self) -> Result<(Queue, bool)> {
        let mut queue = self.storage.load_queue()?;
        let now = self.now_ms();
        // If the timer stops playback, the position freezes where it was at the deadline.
        let at_deadline = queue.state.sleep.map(|t| {
            let at = t.at_ms.min(now);
            (queue.position_ms(at), at)
        });
        let fired = queue.apply_sleep(now);
        if fired {
            if let (PlayStatus::Stopped, Some((pos, at))) = (queue.state.status, at_deadline) {
                queue.set_position(pos, at);
            }
            self.storage.save_queue(&queue)?;
        }
        Ok((queue, fired))
    }

    fn load_session(&mut self) -> Result<Queue> {
        Ok(self.load_session_checked()?.0)
    }

    /// Every time a track starts playing: if a stop-after timer comes due while it
    /// plays, schedule the stop for the end of this track; otherwise leave things be.
    fn schedule_stop(&self, queue: &mut Queue) -> Result<()> {
        if queue.state.status != PlayStatus::Playing || queue.state.sleep.is_none() {
            return Ok(());
        }
        if let Some(id) = queue.current_track() {
            let duration = self.storage.track(&id.0)?.metadata.duration_ms;
            queue.schedule_sleep_for_track(self.now_ms(), duration);
        }
        Ok(())
    }

    /// Save after an operation that started a track.
    fn commit_started(&mut self, queue: &mut Queue) -> Result<SessionView> {
        if queue.state.status != PlayStatus::Stopped && queue.state.current.is_some() {
            queue.mark_started(self.now_ms());
        }
        self.schedule_stop(queue)?;
        self.commit(queue)
    }

    fn view(&self, queue: &Queue) -> Result<SessionView> {
        let slugs: Vec<String> = queue.items.iter().map(|t| t.0.clone()).collect();
        let sleep_remaining = queue
            .state
            .sleep
            .map(|t| Duration::from_millis((t.at_ms - self.now_ms()).max(0) as u64));
        Ok(SessionView {
            state: queue.state.clone(),
            tracks: self.storage.tracks_by_slugs(&slugs)?,
            sleep_remaining,
            position: Duration::from_millis(queue.position_ms(self.now_ms())),
            source_name: self.source_name(&queue.state.source),
        })
    }

    fn source_name(&self, source: &QueueSource) -> Option<String> {
        match source {
            QueueSource::Playlist { id } => self.storage.playlist(id).ok().map(|p| p.name),
            _ => None,
        }
    }

    fn commit(&mut self, queue: &Queue) -> Result<SessionView> {
        self.storage.save_queue(queue)?;
        self.view(queue)
    }

    fn resolve(&self, ids: &[String]) -> Result<Vec<TrackId>> {
        ids.iter()
            .map(|id| Ok(self.storage.track(id)?.id))
            .collect()
    }

    // --- reading -----------------------------------------------------------

    pub fn session(&mut self) -> Result<SessionView> {
        let queue = self.load_session()?;
        self.view(&queue)
    }

    /// The track the session is on (for "tag the current track").
    pub fn current_track(&mut self) -> Result<Track> {
        let queue = self.load_session()?;
        let id = queue.current_track().ok_or_else(|| {
            Error::InvalidInput("no current track; play something or pass a track id".into())
        })?;
        self.storage.track(&id.0)
    }

    /// How long until the session next needs attention, i.e. until a sleep timer comes
    /// due while something is playing or paused. `None` means nothing is scheduled.
    ///
    /// A host process (the `watch` command today, the audio player in 0.a4) sleeps for
    /// this long, then calls `session()`, which applies the timer. The deadline is the
    /// whole schedule: a plain timer fires at it whichever track is playing, and a
    /// stop-after timer is turned into a stop at the end of the track playing then.
    /// Hosts should also re-check now and then, because other commands can change it.
    pub fn next_wakeup(&mut self) -> Result<Option<Duration>> {
        let queue = self.load_session()?;
        if queue.state.status == PlayStatus::Stopped {
            return Ok(None);
        }
        Ok(queue
            .state
            .sleep
            .map(|t| Duration::from_millis((t.at_ms - self.now_ms()).max(0) as u64)))
    }

    // --- hosts: what a playback host reads and reports ---------------------------

    /// The session state, after applying a sleep timer that came due. Cheap enough to poll.
    pub fn player_state(&mut self) -> Result<PlayerState> {
        Ok(self.load_session()?.state)
    }

    /// Where playback is in the current track according to the session, in milliseconds.
    pub fn session_position_ms(&mut self) -> Result<u64> {
        let queue = self.load_session()?;
        Ok(queue.position_ms(self.now_ms()))
    }

    /// The current track, for a host that is about to start it.
    pub fn current_track_if_any(&mut self) -> Result<Option<Track>> {
        let queue = self.load_session()?;
        queue
            .current_track()
            .map(|id| self.storage.track(&id.0))
            .transpose()
    }

    /// The source a playback backend can play for a track.
    pub fn playback_source(&self, track_id: &str) -> Result<SourceRef> {
        let playable = self.local.capabilities().iter().any(|c| {
            c.group == CapabilityGroup::Playback && c.state.supported && c.state.available
        });
        self.storage
            .sources(track_id)?
            .into_iter()
            .find(|(backend, _)| playable && backend == self.local.id())
            .map(|(backend_id, external_id)| SourceRef {
                backend_id,
                external_id,
            })
            .ok_or_else(|| Error::InvalidInput(format!("no playable source for \"{track_id}\"")))
    }

    /// A player for the host to drive (needs the `audio` feature and an output device).
    pub fn open_playback(&self) -> Result<Box<dyn Playback>> {
        #[cfg(feature = "audio")]
        {
            self.local.playback()
        }
        #[cfg(not(feature = "audio"))]
        {
            Err(Error::Backend("this build has no audio support".into()))
        }
    }

    /// Ask the playback host to exit. Only meaningful while one is running.
    pub fn request_host_quit(&mut self) -> Result<()> {
        self.storage.set_host_quit(true)
    }

    pub fn host_quit_requested(&self) -> Result<bool> {
        self.storage.host_quit_requested()
    }

    /// Withdraw a quit request (a starting host does this, so an old request cannot kill it).
    pub fn clear_host_quit(&mut self) -> Result<()> {
        self.storage.set_host_quit(false)
    }

    /// Where the host really is. Ignored (`false`) if the track was started again since.
    pub fn report_position(&mut self, start_seq: u64, position: Duration) -> Result<bool> {
        let now = self.now_ms();
        self.storage
            .report_position(start_seq, position.as_millis() as u64, now)
    }

    /// Synchronized or plain lyrics for a track, if any of its sources has them.
    pub fn lyrics(&self, track_id: &str) -> Result<Option<Lyrics>> {
        for (backend, external_id) in self.storage.sources(track_id)? {
            if backend == self.local.id()
                && let Some(l) = self.local.lyrics(&external_id)?
            {
                return Ok(Some(l));
            }
        }
        Ok(None)
    }

    // --- starting playback -------------------------------------------------

    /// Replace the queue with one track and start it (a manual queue).
    pub fn play_track(&mut self, id: &str) -> Result<SessionView> {
        let track = self.resolve(&[id.to_string()])?;
        let mut queue = self.load_session()?;
        queue.replace(track, &mut self.rng);
        queue.state.source = QueueSource::Manual;
        self.commit_started(&mut queue)
    }

    /// Replace the queue with a playlist's current tracks and start it, in order or (`shuffle`)
    /// shuffled from a random track. The queue then belongs to that playlist: tracks cannot be
    /// added to it by hand (it can still be reordered and trimmed). Without `shuffle` the current
    /// shuffle mode is left as it is.
    pub fn play_playlist(&mut self, name: &str, shuffle: bool) -> Result<SessionView> {
        let (playlist, result) = self.playlist_tracks(name, Page::ALL)?;
        if result.tracks.is_empty() {
            return Err(Error::InvalidInput(format!(
                "playlist \"{}\" has no tracks",
                playlist.name
            )));
        }
        let mut queue = self.load_session()?;
        if shuffle {
            queue.state.shuffle = true;
        }
        queue.replace(
            result.tracks.into_iter().map(|t| t.id).collect(),
            &mut self.rng,
        );
        queue.state.source = QueueSource::Playlist { id: playlist.id };
        self.commit_started(&mut queue)
    }

    /// Replace the queue with what a filter matches right now (the filter's own sort and limit
    /// apply first) and start it shuffled, from a random track.
    pub fn play_shuffle(&mut self, filter: FilterSpec) -> Result<SessionView> {
        let filter = filter.normalized();
        let matches = self.storage.search(&filter, Page::ALL)?;
        if matches.tracks.is_empty() {
            return Err(Error::InvalidInput(
                "nothing in the library matches those filters".into(),
            ));
        }
        let mut queue = self.load_session()?;
        queue.state.shuffle = true;
        queue.replace(
            matches.tracks.into_iter().map(|t| t.id).collect(),
            &mut self.rng,
        );
        queue.state.source = QueueSource::Search {
            filter: Box::new(filter),
        };
        self.commit_started(&mut queue)
    }

    // --- queue editing -----------------------------------------------------

    /// Add tracks, and/or the current tracks of a playlist (manual or dynamic), to the end of the
    /// queue or right after the current one (`next`). A playlist is only a way to pick tracks:
    /// they are added as plain tracks and the queue does not become that playlist's. Everything
    /// is resolved first, so a bad ID or playlist name changes nothing. Returns how many tracks
    /// were added. A queue started from a playlist or a search does not take additions: clear
    /// it, or play a track, to start a manual queue.
    pub fn queue_add(
        &mut self,
        ids: &[String],
        playlist: Option<&str>,
        next: bool,
    ) -> Result<usize> {
        let mut tracks = self.resolve(ids)?;
        if let Some(name) = playlist {
            let (p, result) = self.playlist_tracks(name, Page::ALL)?;
            if result.tracks.is_empty() {
                return Err(Error::InvalidInput(format!(
                    "playlist \"{}\" has no tracks",
                    p.name
                )));
            }
            tracks.extend(result.tracks.into_iter().map(|t| t.id));
        }
        let n = tracks.len();
        let mut queue = self.load_session()?;
        if queue.state.source != QueueSource::Manual && !queue.is_empty() {
            return Err(Error::InvalidInput(format!(
                "the queue is playing {}; adding tracks would mix sources \
                 (run `queue clear`, or play a track to start a manual queue)",
                self.describe_source(&queue.state.source)
            )));
        }
        if next {
            queue.insert_next(tracks, &mut self.rng);
        } else {
            queue.append(tracks, &mut self.rng);
        }
        self.commit(&queue)?;
        Ok(n)
    }

    fn describe_source(&self, source: &QueueSource) -> String {
        match source {
            QueueSource::Manual => "a manual queue".into(),
            QueueSource::Playlist { id } => {
                format!(
                    "playlist \"{}\"",
                    self.source_name(source).unwrap_or_else(|| id.clone())
                )
            }
            QueueSource::Search { .. } => "a search".into(),
        }
    }

    /// Remove by 1-based positions.
    pub fn queue_remove(&mut self, positions: &[usize]) -> Result<SessionView> {
        let indexes = positions
            .iter()
            .map(|&p| to_index(p))
            .collect::<Result<Vec<_>>>()?;
        let mut queue = self.load_session()?;
        let before = queue.current_track().cloned();
        queue.remove(&indexes)?;
        if queue.current_track().cloned() != before {
            // The current track was removed and the next one took over.
            return self.commit_started(&mut queue);
        }
        self.commit(&queue)
    }

    /// Move a track between 1-based positions.
    pub fn queue_move(&mut self, from: usize, to: usize) -> Result<SessionView> {
        let (from, to) = (to_index(from)?, to_index(to)?);
        let mut queue = self.load_session()?;
        queue.move_item(from, to)?;
        self.commit(&queue)
    }

    pub fn queue_clear(&mut self) -> Result<SessionView> {
        let mut queue = self.load_session()?;
        queue.clear();
        self.commit(&queue)
    }

    /// Make the track at a 1-based position current and start it.
    pub fn queue_jump(&mut self, position: usize) -> Result<SessionView> {
        let index = to_index(position)?;
        let mut queue = self.load_session()?;
        queue.jump(index)?;
        self.commit_started(&mut queue)
    }

    // --- transport ---------------------------------------------------------

    pub fn pause(&mut self) -> Result<SessionView> {
        let mut queue = self.load_session()?;
        let (now, pos) = (self.now_ms(), queue.position_ms(self.now_ms()));
        queue.pause()?;
        queue.set_position(pos, now);
        self.commit(&queue)
    }

    pub fn resume(&mut self) -> Result<SessionView> {
        let mut queue = self.load_session()?;
        let restarting = queue.state.status == PlayStatus::Stopped;
        queue.resume()?;
        if restarting {
            return self.commit_started(&mut queue);
        }
        queue.state.position_at_ms = self.now_ms(); // the position was frozen while paused
        self.commit(&queue)
    }

    pub fn stop(&mut self) -> Result<SessionView> {
        let mut queue = self.load_session()?;
        let (now, pos) = (self.now_ms(), queue.position_ms(self.now_ms()));
        queue.stop();
        queue.set_position(pos, now);
        self.commit(&queue)
    }

    /// The user skips to the next track ("repeat song" does not apply).
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<SessionView> {
        self.advance(AdvanceReason::UserSkip)
    }

    pub fn previous(&mut self) -> Result<SessionView> {
        let mut queue = self.load_session()?;
        queue.previous()?;
        self.commit_started(&mut queue)
    }

    /// A playback backend reports that the current track finished by itself.
    pub fn track_ended(&mut self) -> Result<SessionView> {
        let (queue, fired) = self.load_session_checked()?;
        if queue.state.status != PlayStatus::Playing {
            if fired {
                // A timer stopped playback just before this report: nothing left to do.
                return self.view(&queue);
            }
            return Err(Error::InvalidInput("nothing is playing".into()));
        }
        self.advance(AdvanceReason::TrackEnded)
    }

    fn advance(&mut self, reason: AdvanceReason) -> Result<SessionView> {
        let mut queue = self.load_session()?;
        if queue.advance(reason, &mut self.rng) == Advance::NeedsMore {
            let slugs: Vec<String> = queue.items.iter().map(|t| t.0.clone()).collect();
            let queued = self.storage.tracks_by_slugs(&slugs)?;
            let picks = autoplay::suggest(&self.storage, &queued)?;
            if picks.is_empty() {
                queue.stop_at_end();
            } else {
                queue.append(picks.into_iter().map(|t| t.id).collect(), &mut self.rng);
                queue.advance(reason, &mut self.rng);
            }
        }
        // A track change: check whether a stop needs scheduling (no-op without a timer).
        self.commit_started(&mut queue)
    }

    // --- modes and timer ---------------------------------------------------

    pub fn set_shuffle(&mut self, on: bool) -> Result<SessionView> {
        let mut queue = self.load_session()?;
        queue.set_shuffle(on, &mut self.rng);
        self.commit(&queue)
    }

    pub fn set_repeat(&mut self, mode: RepeatMode) -> Result<SessionView> {
        let mut queue = self.load_session()?;
        queue.state.repeat = mode;
        self.commit(&queue)
    }

    pub fn set_end_of_queue(&mut self, mode: EndOfQueue) -> Result<SessionView> {
        let mut queue = self.load_session()?;
        queue.state.end_of_queue = mode;
        self.commit(&queue)
    }

    /// Jump to a position in the current track (clamped to its length when known).
    pub fn seek(&mut self, to: Duration) -> Result<SessionView> {
        let mut queue = self.load_session()?;
        let mut ms = to.as_millis() as u64;
        if let Some(id) = queue.current_track()
            && let Some(len) = self.storage.track(&id.0)?.metadata.duration_ms
        {
            ms = ms.min(len);
        }
        queue.request_seek(ms, self.now_ms())?;
        self.commit(&queue)
    }

    /// 0.0 (mute) to 1.0.
    pub fn set_volume(&mut self, volume: f32) -> Result<SessionView> {
        if !volume.is_finite() || !(0.0..=1.0).contains(&volume) {
            return Err(Error::InvalidInput("volume must be between 0 and 1".into()));
        }
        let mut queue = self.load_session()?;
        queue.state.volume = volume;
        self.commit(&queue)
    }

    /// Set, replace or cancel the sleep timer (see `SleepSetting`). A timer with
    /// `stop_after` waits for the track playing when it comes due to finish.
    pub fn set_sleep(&mut self, setting: SleepSetting) -> Result<SessionView> {
        let mut queue = self.load_session()?;
        queue.set_sleep(setting, self.now_ms());
        self.commit(&queue)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use weave_core::{FilterSpec, Metadata, ScannedTrack};

    use super::*;

    fn scanned(title: &str, bpm: Option<f64>) -> ScannedTrack {
        ScannedTrack {
            external_id: title.into(),
            metadata: Metadata {
                title: title.into(),
                artist: Some("x".into()),
                bpm,
                duration_ms: Some(TRACK_MS),
                ..Default::default()
            },
        }
    }

    /// Every test track is three minutes long unless a test says otherwise.
    const TRACK_MS: u64 = 180_000;

    /// Library with tracks t1..tN (bpm 100), a controllable clock, and a fixed seed.
    fn library(n: usize) -> (Library, Rc<Cell<i64>>) {
        let clock = Rc::new(Cell::new(1_000_000));
        let c = clock.clone();
        let mut lib = Library::open_in_memory()
            .unwrap()
            .with_determinism(5, move || c.get());
        let tracks: Vec<_> = (1..=n)
            .map(|i| scanned(&format!("t{i}"), Some(100.0)))
            .collect();
        lib.import_scanned("fake", &tracks).unwrap();
        (lib, clock)
    }

    fn timer(secs: u64, stop_after: bool) -> SleepSetting {
        SleepSetting::In {
            delay: Duration::from_secs(secs),
            stop_after,
        }
    }

    fn id(n: usize) -> String {
        format!("x-t{n}-fake")
    }

    fn titles(v: &SessionView) -> Vec<&str> {
        v.tracks.iter().map(|t| t.metadata.title.as_str()).collect()
    }

    fn now(v: &SessionView) -> Option<&str> {
        v.current().map(|t| t.metadata.title.as_str())
    }

    #[test]
    fn play_track_replaces_the_queue() {
        let (mut lib, _) = library(3);
        lib.queue_add(&[id(2), id(3)], None, false).unwrap();
        let v = lib.play_track(&id(1)).unwrap();
        assert_eq!(
            (titles(&v), now(&v), v.state.status),
            (vec!["t1"], Some("t1"), PlayStatus::Playing)
        );
        assert!(matches!(lib.play_track("nope"), Err(Error::NotFound(_))));
        assert_eq!(
            titles(&lib.session().unwrap()),
            ["t1"],
            "a failed play changes nothing"
        );
    }

    #[test]
    fn session_survives_reopening_the_database() {
        let dir = std::env::temp_dir().join(format!("weave-engine-{}", uuid_like()));
        let path = dir.join("lib.db");
        {
            let mut lib = Library::open(&path).unwrap();
            lib.import_scanned("fake", &[scanned("t1", None), scanned("t2", None)])
                .unwrap();
            lib.queue_add(&[id(1), id(2)], None, false).unwrap();
            lib.queue_jump(2).unwrap();
            lib.set_repeat(RepeatMode::Playlist).unwrap();
            lib.set_shuffle(true).unwrap();
        }
        let mut lib = Library::open(&path).unwrap();
        let v = lib.session().unwrap();
        assert_eq!(
            (now(&v), v.state.repeat, v.state.shuffle),
            (Some("t2"), RepeatMode::Playlist, true)
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn uuid_like() -> String {
        format!(
            "{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )
    }

    #[test]
    fn playlist_play_replaces_the_queue_and_rejects_an_empty_playlist() {
        let (mut lib, _) = library(3);
        lib.queue_add(&[id(2)], None, false).unwrap();
        lib.create_manual_playlist("Mix", None).unwrap();
        assert!(matches!(
            lib.play_playlist("mix", false),
            Err(Error::InvalidInput(_))
        ));
        assert_eq!(
            titles(&lib.session().unwrap()),
            ["t2"],
            "a failed play changes nothing"
        );

        lib.playlist_add("mix", &[id(3), id(1)]).unwrap();
        let v = lib.play_playlist("Mix", false).unwrap();
        assert_eq!((titles(&v), now(&v)), (vec!["t3", "t1"], Some("t3")));
        assert_eq!(v.state.source, QueueSource::Playlist { id: "mix".into() });
        assert_eq!(v.source_name.as_deref(), Some("Mix"));
    }

    #[test]
    fn a_queue_has_one_source_and_hand_picked_tracks_do_not_mix_in() {
        let (mut lib, _) = library(3);
        lib.create_manual_playlist("Mix", None).unwrap();
        lib.playlist_add("mix", &[id(1), id(2)]).unwrap();
        lib.play_playlist("mix", false).unwrap();

        for next in [false, true] {
            let err = lib.queue_add(&[id(3)], None, next).unwrap_err().to_string();
            assert!(
                err.contains("playlist \"Mix\"") && err.contains("queue clear"),
                "{err}"
            );
        }
        assert_eq!(
            titles(&lib.session().unwrap()),
            ["t1", "t2"],
            "nothing was added"
        );

        // starting a track makes it a manual queue again; so does clearing
        lib.play_track(&id(3)).unwrap();
        assert_eq!(lib.session().unwrap().state.source, QueueSource::Manual);
        lib.queue_add(&[id(1)], None, false).unwrap();
        lib.play_playlist("mix", false).unwrap();
        lib.queue_clear().unwrap();
        lib.queue_add(&[id(3)], None, false).unwrap();
        assert_eq!(titles(&lib.session().unwrap()), ["t3"]);

        // removing every track also ends the playlist's hold on the queue
        lib.play_playlist("mix", false).unwrap();
        lib.queue_remove(&[1, 2]).unwrap();
        assert_eq!(lib.session().unwrap().state.source, QueueSource::Manual);
        lib.queue_add(&[id(3)], None, false).unwrap();
    }

    #[test]
    fn a_unknown_id_added_to_a_manual_queue_changes_nothing() {
        let (mut lib, _) = library(2);
        lib.queue_add(&[id(1)], None, false).unwrap();
        assert!(lib.queue_add(&[id(2), "nope".into()], None, false).is_err());
        assert_eq!(titles(&lib.session().unwrap()), ["t1"]);
    }

    #[test]
    fn dynamic_playlist_play_honors_sort_and_limit() {
        let (mut lib, _) = library(4);
        for (i, r) in [(1, 2u8), (2, 5), (3, 4), (4, 1)] {
            lib.set_rating(&id(i), Some(r)).unwrap();
        }
        let filter = FilterSpec {
            sort: Some(weave_core::Sort {
                field: weave_core::SortField::Rating,
                descending: true,
            }),
            limit: Some(2),
            ..Default::default()
        };
        lib.create_playlist("Top", None, filter).unwrap();
        let v = lib.play_playlist("top", false).unwrap();
        assert_eq!(titles(&v), ["t2", "t3"]);
    }

    #[test]
    fn next_and_previous_walk_the_queue_then_stop() {
        let (mut lib, _) = library(3);
        lib.queue_add(&[id(1), id(2), id(3)], None, false).unwrap();
        assert_eq!(
            now(&lib.next().unwrap()),
            Some("t1"),
            "next starts an idle queue"
        );
        assert_eq!(now(&lib.next().unwrap()), Some("t2"));
        assert_eq!(now(&lib.previous().unwrap()), Some("t1"));
        lib.next().unwrap();
        lib.next().unwrap();
        let v = lib.next().unwrap();
        assert_eq!((now(&v), v.state.status), (Some("t3"), PlayStatus::Stopped));
    }

    #[test]
    fn queue_editing_through_one_based_positions() {
        let (mut lib, _) = library(4);
        lib.play_track(&id(1)).unwrap();
        lib.queue_add(&[id(3), id(4)], None, false).unwrap();
        lib.queue_add(&[id(2)], None, true).unwrap();
        assert_eq!(titles(&lib.session().unwrap()), ["t1", "t2", "t3", "t4"]);
        lib.queue_move(4, 2).unwrap();
        assert_eq!(titles(&lib.session().unwrap()), ["t1", "t4", "t2", "t3"]);
        let v = lib.queue_remove(&[2]).unwrap();
        assert_eq!(titles(&v), ["t1", "t2", "t3"]);
        let v = lib.queue_jump(3).unwrap();
        assert_eq!(now(&v), Some("t3"));
        assert!(lib.queue_jump(0).is_err() && lib.queue_jump(4).is_err());
        assert!(lib.queue_remove(&[0]).is_err() && lib.queue_move(1, 9).is_err());
        let v = lib.queue_clear().unwrap();
        assert_eq!((v.tracks.len(), v.state.status), (0, PlayStatus::Stopped));
    }

    #[test]
    fn transport_errors_when_there_is_nothing_to_do() {
        let (mut lib, _) = library(1);
        assert!(lib.pause().is_err() && lib.resume().is_err() && lib.previous().is_err());
        assert!(lib.track_ended().is_err());
        assert!(lib.current_track().is_err());
        lib.play_track(&id(1)).unwrap();
        assert_eq!(lib.pause().unwrap().state.status, PlayStatus::Paused);
        assert!(lib.track_ended().is_err(), "a paused track cannot end");
        assert_eq!(lib.resume().unwrap().state.status, PlayStatus::Playing);
        assert_eq!(lib.stop().unwrap().state.status, PlayStatus::Stopped);
        assert_eq!(lib.current_track().unwrap().metadata.title, "t1");
    }

    #[test]
    fn repeat_song_replays_on_end_but_not_on_skip() {
        let (mut lib, _) = library(2);
        lib.queue_add(&[id(1), id(2)], None, false).unwrap();
        lib.next().unwrap();
        lib.set_repeat(RepeatMode::Song).unwrap();
        assert_eq!(now(&lib.track_ended().unwrap()), Some("t1"));
        assert_eq!(now(&lib.track_ended().unwrap()), Some("t1"));
        assert_eq!(now(&lib.next().unwrap()), Some("t2"));
    }

    #[test]
    fn repeat_playlist_loops_the_queue() {
        let (mut lib, _) = library(2);
        lib.queue_add(&[id(1), id(2)], None, false).unwrap();
        lib.next().unwrap();
        lib.set_repeat(RepeatMode::Playlist).unwrap();
        assert_eq!(now(&lib.track_ended().unwrap()), Some("t2"));
        let v = lib.track_ended().unwrap();
        assert_eq!((now(&v), v.state.status), (Some("t1"), PlayStatus::Playing));
    }

    #[test]
    fn shuffle_plays_everything_once() {
        let (mut lib, _) = library(6);
        lib.queue_add(&(1..=6).map(id).collect::<Vec<_>>(), None, false)
            .unwrap();
        lib.next().unwrap();
        lib.set_shuffle(true).unwrap();
        let mut seen = vec![now(&lib.session().unwrap()).unwrap().to_string()];
        loop {
            let v = lib.track_ended().unwrap();
            if v.state.status == PlayStatus::Stopped {
                break;
            }
            seen.push(now(&v).unwrap().to_string());
        }
        seen.sort();
        assert_eq!(seen, ["t1", "t2", "t3", "t4", "t5", "t6"]);
    }

    #[test]
    fn sleep_timer_stops_playback_when_it_runs_out() {
        let (mut lib, clock) = library(2);
        lib.queue_add(&[id(1), id(2)], None, false).unwrap();
        lib.next().unwrap();
        let v = lib.set_sleep(timer(30 * 60, false)).unwrap();
        assert_eq!(v.sleep_remaining, Some(Duration::from_secs(1800)));

        clock.set(clock.get() + 29 * 60_000);
        let v = lib.session().unwrap();
        assert_eq!(
            (v.state.status, v.sleep_remaining),
            (PlayStatus::Playing, Some(Duration::from_secs(60)))
        );

        clock.set(clock.get() + 2 * 60_000);
        let v = lib.session().unwrap();
        assert_eq!(
            (v.state.status, v.sleep_remaining),
            (PlayStatus::Stopped, None)
        );
        assert_eq!(now(&v), Some("t1"), "the position is kept");
        assert_eq!(
            lib.session().unwrap().state.status,
            PlayStatus::Stopped,
            "and it was saved"
        );
    }

    #[test]
    fn sleep_timer_can_be_cancelled_and_stops_any_operation() {
        let (mut lib, clock) = library(2);
        lib.queue_add(&[id(1), id(2)], None, false).unwrap();
        lib.next().unwrap();
        lib.set_sleep(timer(60, false)).unwrap();
        assert_eq!(
            lib.set_sleep(SleepSetting::Off).unwrap().sleep_remaining,
            None
        );
        clock.set(clock.get() + 3_600_000);
        assert_eq!(lib.session().unwrap().state.status, PlayStatus::Playing);

        lib.set_sleep(timer(60, false)).unwrap();
        clock.set(clock.get() + 120_000);
        assert!(
            lib.pause().is_err(),
            "the timer already stopped playback before pause ran"
        );
    }

    fn advance_clock(clock: &Cell<i64>, secs: i64) {
        clock.set(clock.get() + secs * 1000);
    }

    #[test]
    fn plain_timer_after_the_deadline_makes_a_track_end_report_a_no_op() {
        let (mut lib, clock) = library(3);
        lib.queue_add(&[id(1), id(2)], None, false).unwrap();
        lib.next().unwrap();
        lib.set_sleep(timer(60, false)).unwrap();
        advance_clock(&clock, 180); // the track ends well after the deadline
        let v = lib.track_ended().unwrap();
        assert_eq!((now(&v), v.state.status), (Some("t1"), PlayStatus::Stopped));
        assert!(
            lib.track_ended().is_err(),
            "a second report has nothing to stop"
        );
    }

    #[test]
    fn stop_after_schedules_the_stop_when_the_deadline_falls_inside_the_starting_track() {
        let (mut lib, clock) = library(3);
        lib.queue_add(&[id(1), id(2), id(3)], None, false).unwrap();
        lib.set_sleep(timer(100, true)).unwrap(); // due 100 s from now
        let v = lib.next().unwrap(); // t1 starts: 0..180 s, so the deadline falls inside it
        assert_eq!(now(&v), Some("t1"));
        assert!(v.state.stop_after_track && v.state.sleep.is_none() && v.sleep_remaining.is_none());

        advance_clock(&clock, 180);
        let v = lib.track_ended().unwrap();
        assert_eq!(
            (now(&v), v.state.status),
            (Some("t1"), PlayStatus::Stopped),
            "stopped after t1, t2 never started"
        );
        assert!(!v.state.stop_after_track);
    }

    #[test]
    fn stop_after_is_left_alone_while_the_deadline_is_beyond_the_track() {
        let (mut lib, clock) = library(4);
        lib.queue_add(&(1..=4).map(id).collect::<Vec<_>>(), None, false)
            .unwrap();
        lib.set_sleep(timer(600, true)).unwrap(); // 10 minutes
        let v = lib.next().unwrap(); // t1: 0..180
        assert!(!v.state.stop_after_track && v.sleep_remaining.is_some());
        advance_clock(&clock, 180);
        let v = lib.track_ended().unwrap(); // t2: 180..360
        assert_eq!(now(&v), Some("t2"));
        assert!(!v.state.stop_after_track);
        advance_clock(&clock, 180);
        let v = lib.track_ended().unwrap(); // t3: 360..540, still before 600
        assert!(!v.state.stop_after_track && v.sleep_remaining.is_some());
        advance_clock(&clock, 180);
        let v = lib.track_ended().unwrap(); // t4: 540..720 contains 600
        assert_eq!(now(&v), Some("t4"));
        assert!(v.state.stop_after_track && v.state.sleep.is_none());
        advance_clock(&clock, 180);
        assert_eq!(lib.track_ended().unwrap().state.status, PlayStatus::Stopped);
    }

    #[test]
    fn stop_after_with_unknown_duration_is_decided_when_it_comes_due() {
        let clock = Rc::new(Cell::new(1_000_000));
        let c = clock.clone();
        let mut lib = Library::open_in_memory()
            .unwrap()
            .with_determinism(5, move || c.get());
        let no_duration = |t: &str| ScannedTrack {
            external_id: t.into(),
            metadata: Metadata {
                title: t.into(),
                artist: Some("x".into()),
                ..Default::default()
            },
        };
        lib.import_scanned("fake", &[no_duration("t1"), no_duration("t2")])
            .unwrap();
        lib.queue_add(&[id(1), id(2)], None, false).unwrap();
        lib.set_sleep(timer(60, true)).unwrap();
        let v = lib.next().unwrap();
        assert!(
            !v.state.stop_after_track && v.sleep_remaining.is_some(),
            "nothing to compare against yet"
        );

        advance_clock(&clock, 90);
        let v = lib.session().unwrap(); // the deadline passed while t1 plays
        assert_eq!(v.state.status, PlayStatus::Playing);
        assert!(v.state.stop_after_track && v.state.sleep.is_none());
        let v = lib.track_ended().unwrap();
        assert_eq!((now(&v), v.state.status), (Some("t1"), PlayStatus::Stopped));
    }

    #[test]
    fn stop_after_timer_that_comes_due_while_paused_waits_for_the_track_to_end() {
        let (mut lib, clock) = library(2);
        lib.queue_add(&[id(1), id(2)], None, false).unwrap();
        lib.set_sleep(timer(600, true)).unwrap();
        lib.next().unwrap();
        lib.pause().unwrap();
        advance_clock(&clock, 900);
        let v = lib.resume().unwrap();
        assert_eq!(
            v.state.status,
            PlayStatus::Playing,
            "a paused track is not stopped by the timer"
        );
        assert!(v.state.stop_after_track);
        assert_eq!(lib.track_ended().unwrap().state.status, PlayStatus::Stopped);
    }

    #[test]
    fn scheduled_stop_follows_a_skip_and_is_cleared_by_stop_and_by_a_new_timer() {
        let (mut lib, _) = library(3);
        lib.queue_add(&[id(1), id(2), id(3)], None, false).unwrap();
        lib.set_sleep(timer(60, true)).unwrap();
        assert!(lib.next().unwrap().state.stop_after_track); // t1 scheduled
        let v = lib.next().unwrap(); // skip to t2
        assert_eq!(now(&v), Some("t2"));
        assert!(
            v.state.stop_after_track,
            "the stop now applies to the new current track"
        );

        assert!(!lib.stop().unwrap().state.stop_after_track);
        lib.resume().unwrap();
        lib.set_sleep(timer(60, true)).unwrap();
        lib.queue_jump(3).unwrap(); // scheduled again for t3
        assert!(lib.session().unwrap().state.stop_after_track);
        let v = lib.set_sleep(SleepSetting::Off).unwrap();
        assert!(!v.state.stop_after_track && v.state.sleep.is_none());
    }

    #[test]
    fn every_way_of_starting_a_track_checks_the_timer() {
        // resume after a stop, previous, jump, play, playlist play, and removing the current track.
        type Starter = Box<dyn Fn(&mut Library)>;
        let starters: Vec<(&str, Starter)> = vec![
            (
                "play_track",
                Box::new(|l| {
                    l.play_track(&id(2)).unwrap();
                }),
            ),
            (
                "queue_jump",
                Box::new(|l| {
                    l.queue_jump(2).unwrap();
                }),
            ),
            (
                "previous",
                Box::new(|l| {
                    l.queue_jump(2).unwrap();
                    l.set_sleep(timer(60, true)).unwrap();
                    l.previous().unwrap();
                }),
            ),
            (
                "resume after stop",
                Box::new(|l| {
                    l.stop().unwrap();
                    l.resume().unwrap();
                }),
            ),
            (
                "remove current",
                Box::new(|l| {
                    l.queue_remove(&[1]).unwrap();
                }),
            ),
            (
                "playlist play",
                Box::new(|l| {
                    l.play_playlist("mix", false).unwrap();
                }),
            ),
        ];
        for (name, start) in starters {
            let (mut lib, _) = library(3);
            lib.create_manual_playlist("Mix", None).unwrap();
            lib.playlist_add("mix", &[id(1), id(2)]).unwrap();
            lib.queue_add(&[id(1), id(2), id(3)], None, false).unwrap();
            lib.next().unwrap();
            lib.set_sleep(timer(60, true)).unwrap();
            start(&mut lib);
            let v = lib.session().unwrap();
            assert!(
                v.state.stop_after_track,
                "{name}: a stop should be scheduled"
            );
        }
    }

    #[test]
    fn pausing_and_resuming_do_not_reschedule_or_disturb_a_pending_timer() {
        let (mut lib, _) = library(2);
        lib.queue_add(&[id(1), id(2)], None, false).unwrap();
        lib.next().unwrap();
        lib.set_sleep(timer(600, true)).unwrap(); // beyond the 3 minute track
        lib.pause().unwrap();
        let v = lib.resume().unwrap();
        assert!(!v.state.stop_after_track && v.state.sleep.is_some());
    }

    #[test]
    fn next_wakeup_reports_the_timer_only_while_something_plays() {
        let (mut lib, clock) = library(2);
        lib.queue_add(&[id(1), id(2)], None, false).unwrap();
        assert_eq!(lib.next_wakeup().unwrap(), None, "no timer");
        lib.set_sleep(timer(600, false)).unwrap();
        assert_eq!(
            lib.next_wakeup().unwrap(),
            None,
            "stopped: nothing to stop yet"
        );

        lib.next().unwrap();
        assert_eq!(lib.next_wakeup().unwrap(), Some(Duration::from_secs(600)));
        advance_clock(&clock, 100);
        assert_eq!(lib.next_wakeup().unwrap(), Some(Duration::from_secs(500)));
        lib.pause().unwrap();
        assert_eq!(
            lib.next_wakeup().unwrap(),
            Some(Duration::from_secs(500)),
            "a paused session still has a deadline"
        );

        advance_clock(&clock, 500);
        assert_eq!(
            lib.next_wakeup().unwrap(),
            None,
            "the timer came due and was applied"
        );
        assert_eq!(lib.session().unwrap().state.status, PlayStatus::Stopped);
    }

    #[test]
    fn next_wakeup_after_a_stop_after_timer_fires_is_a_scheduled_stop_not_a_deadline() {
        let (mut lib, clock) = library(2);
        lib.queue_add(&[id(1), id(2)], None, false).unwrap();
        lib.set_sleep(timer(600, true)).unwrap();
        lib.next().unwrap();
        advance_clock(&clock, 600);
        assert_eq!(lib.next_wakeup().unwrap(), None);
        let v = lib.session().unwrap();
        assert!(v.state.stop_after_track && v.state.status == PlayStatus::Playing);
    }

    #[test]
    fn timer_state_survives_reopening_the_database() {
        let dir = std::env::temp_dir().join(format!("weave-sleep-{}", uuid_like()));
        let path = dir.join("lib.db");
        {
            let mut lib = Library::open(&path).unwrap();
            lib.import_scanned("fake", &[scanned("t1", None), scanned("t2", None)])
                .unwrap();
            lib.queue_add(&[id(1), id(2)], None, false).unwrap();
            lib.next().unwrap();
            lib.set_sleep(timer(600, true)).unwrap();
        }
        let mut lib = Library::open(&path).unwrap();
        let v = lib.session().unwrap();
        assert!(v.state.sleep.is_some_and(|t| t.stop_after) && v.sleep_remaining.is_some());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn position_follows_the_clock_and_freezes_when_paused_or_stopped() {
        let (mut lib, clock) = library(2);
        lib.queue_add(&[id(1), id(2)], None, false).unwrap();
        lib.next().unwrap();
        advance_clock(&clock, 10);
        assert_eq!(lib.session().unwrap().position, Duration::from_secs(10));

        lib.pause().unwrap();
        advance_clock(&clock, 60);
        assert_eq!(
            lib.session().unwrap().position,
            Duration::from_secs(10),
            "frozen while paused"
        );

        lib.resume().unwrap();
        advance_clock(&clock, 5);
        assert_eq!(lib.session().unwrap().position, Duration::from_secs(15));

        lib.stop().unwrap();
        advance_clock(&clock, 60);
        assert_eq!(
            lib.session().unwrap().position,
            Duration::from_secs(15),
            "kept after a stop"
        );
    }

    #[test]
    fn every_track_start_bumps_the_start_counter_and_resets_the_position() {
        let (mut lib, clock) = library(3);
        let seq = |lib: &mut Library| lib.player_state().unwrap().start_seq;
        lib.queue_add(&[id(1), id(2), id(3)], None, false).unwrap();
        assert_eq!(seq(&mut lib), 0, "queueing starts nothing");

        type Start = Box<dyn Fn(&mut Library)>;
        let starts: Vec<(&str, Start)> = vec![
            (
                "next",
                Box::new(|l| {
                    l.next().unwrap();
                }),
            ),
            (
                "next again",
                Box::new(|l| {
                    l.next().unwrap();
                }),
            ),
            (
                "previous",
                Box::new(|l| {
                    l.previous().unwrap();
                }),
            ),
            (
                "jump",
                Box::new(|l| {
                    l.queue_jump(3).unwrap();
                }),
            ),
            (
                "play",
                Box::new(|l| {
                    l.play_track(&id(1)).unwrap();
                }),
            ),
            (
                "resume after stop",
                Box::new(|l| {
                    l.stop().unwrap();
                    l.resume().unwrap();
                }),
            ),
            (
                "repeat-song replay",
                Box::new(|l| {
                    l.set_repeat(RepeatMode::Song).unwrap();
                    l.track_ended().unwrap();
                }),
            ),
        ];
        let mut expected = 0;
        for (name, start) in starts {
            advance_clock(&clock, 30);
            start(&mut lib);
            expected += 1;
            let v = lib.session().unwrap();
            assert_eq!(v.state.start_seq, expected, "{name}");
            assert_eq!(
                v.position,
                Duration::ZERO,
                "{name}: a new track starts at zero"
            );
        }

        let before = seq(&mut lib);
        lib.pause().unwrap();
        lib.resume().unwrap();
        lib.seek(Duration::from_secs(5)).unwrap();
        lib.set_volume(0.5).unwrap();
        lib.stop().unwrap();
        assert_eq!(
            seq(&mut lib),
            before,
            "pause, resume, seek, volume and stop are not starts"
        );
    }

    #[test]
    fn a_stop_at_the_end_of_the_queue_is_not_a_start() {
        let (mut lib, _) = library(1);
        lib.play_track(&id(1)).unwrap();
        let before = lib.player_state().unwrap().start_seq;
        let v = lib.track_ended().unwrap();
        assert_eq!(v.state.status, PlayStatus::Stopped);
        assert_eq!(v.state.start_seq, before);
    }

    #[test]
    fn seek_moves_the_position_counts_and_clamps_to_the_track_length() {
        let (mut lib, clock) = library(1);
        assert!(lib.seek(Duration::from_secs(5)).is_err(), "nothing queued");
        lib.play_track(&id(1)).unwrap();
        let v = lib.seek(Duration::from_secs(42)).unwrap();
        assert_eq!((v.position, v.state.seek_seq), (Duration::from_secs(42), 1));
        advance_clock(&clock, 3);
        assert_eq!(
            lib.session().unwrap().position,
            Duration::from_secs(45),
            "keeps running from there"
        );

        let v = lib.seek(Duration::from_secs(10_000)).unwrap();
        assert_eq!(
            v.position,
            Duration::from_millis(TRACK_MS),
            "clamped to the 3 minute track"
        );
        assert_eq!(v.state.seek_seq, 2);
    }

    #[test]
    fn volume_is_validated_and_persisted() {
        let (mut lib, _) = library(1);
        assert_eq!(lib.player_state().unwrap().volume, 1.0);
        assert_eq!(lib.set_volume(0.3).unwrap().state.volume, 0.3);
        for bad in [-0.1, 1.1, f32::NAN, f32::INFINITY] {
            assert!(lib.set_volume(bad).is_err(), "{bad}");
        }
        assert_eq!(
            lib.player_state().unwrap().volume,
            0.3,
            "bad values change nothing"
        );
    }

    #[test]
    fn a_host_position_report_is_ignored_once_the_track_started_again() {
        let (mut lib, clock) = library(2);
        lib.queue_add(&[id(1), id(2)], None, false).unwrap();
        lib.next().unwrap();
        let seq = lib.player_state().unwrap().start_seq;
        advance_clock(&clock, 4);
        assert!(
            lib.report_position(seq, Duration::from_millis(3_900))
                .unwrap()
        );
        assert_eq!(
            lib.session().unwrap().position,
            Duration::from_millis(3_900)
        );

        lib.next().unwrap();
        assert!(
            !lib.report_position(seq, Duration::from_secs(99)).unwrap(),
            "stale start"
        );
        assert_eq!(lib.session().unwrap().position, Duration::ZERO);
    }

    #[test]
    fn a_sleep_stop_freezes_the_position_at_the_deadline() {
        let (mut lib, clock) = library(1);
        lib.play_track(&id(1)).unwrap();
        lib.set_sleep(timer(60, false)).unwrap();
        advance_clock(&clock, 600);
        let v = lib.session().unwrap();
        assert_eq!(v.state.status, PlayStatus::Stopped);
        assert_eq!(v.position, Duration::from_secs(60), "not 600 s");
    }

    fn real_files() -> (Library, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("weave-real-{}", uuid_like()));
        std::fs::create_dir_all(&dir).unwrap();
        // header + 1 s of silence, as a valid mono 16-bit 8 kHz WAV
        let data = 16_000u32;
        let mut wav = Vec::new();
        wav.extend(b"RIFF");
        wav.extend((36 + data).to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16u32.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(8000u32.to_le_bytes());
        wav.extend(16_000u32.to_le_bytes());
        wav.extend(2u16.to_le_bytes());
        wav.extend(16u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend(data.to_le_bytes());
        wav.extend(std::iter::repeat_n(0u8, data as usize));
        std::fs::write(dir.join("song.wav"), &wav).unwrap();
        std::fs::write(dir.join("other.wav"), &wav).unwrap();
        std::fs::write(dir.join("song.lrc"), "[00:01.00]first\n[00:02.00]second").unwrap();
        let mut lib = Library::open_in_memory().unwrap();
        assert_eq!(lib.import(&dir).unwrap().added, 2);
        (lib, dir)
    }

    #[test]
    fn lyrics_and_playback_sources_come_from_the_local_backend() {
        let (lib, dir) = real_files();
        let all = lib
            .search(&FilterSpec::default(), Page::ALL)
            .unwrap()
            .tracks;
        let song = all.iter().find(|t| t.metadata.title == "song").unwrap();
        let other = all.iter().find(|t| t.metadata.title == "other").unwrap();

        let l = lib.lyrics(&song.id.0).unwrap().unwrap();
        assert!(l.synced && l.lines.len() == 2);
        assert_eq!(lib.lyrics(&other.id.0).unwrap(), None);
        assert!(matches!(lib.lyrics("nope"), Err(Error::NotFound(_))));

        let source = lib.playback_source(&song.id.0);
        if cfg!(feature = "audio") {
            let s = source.unwrap();
            assert!(s.backend_id == "local" && s.external_id.ends_with("song.wav"));
        } else {
            assert!(
                matches!(source, Err(Error::InvalidInput(_))),
                "no audio support, nothing is playable"
            );
            assert!(matches!(lib.open_playback(), Err(Error::Backend(_))));
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn sources_of_other_backends_are_not_playable_here() {
        let (lib, _) = library(1);
        assert!(
            matches!(lib.playback_source(&id(1)), Err(Error::InvalidInput(_))),
            "a fake backend"
        );
        assert_eq!(lib.lyrics(&id(1)).unwrap(), None);
    }

    /// t1-t3 tagged bachata, t4 zouk; every library gets its own seed.
    fn library_with_tags(seed: u64) -> Library {
        let clock = Rc::new(Cell::new(1_000_000));
        let mut lib = Library::open_in_memory()
            .unwrap()
            .with_determinism(seed, move || clock.get());
        let tracks: Vec<_> = (1..=4)
            .map(|i| scanned(&format!("t{i}"), Some(100.0)))
            .collect();
        lib.import_scanned("fake", &tracks).unwrap();
        for i in 1..=3 {
            lib.add_tags(&id(i), &["bachata".to_string()]).unwrap();
        }
        lib.add_tags(&id(4), &["zouk".to_string()]).unwrap();
        lib
    }

    fn bachata() -> FilterSpec {
        FilterSpec {
            tag_all: vec!["bachata".into()],
            ..Default::default()
        }
    }

    #[test]
    fn play_shuffle_queues_every_match_in_random_order_from_a_random_start() {
        let mut starts = std::collections::HashSet::new();
        let mut orders = std::collections::HashSet::new();
        for seed in 0..40 {
            let mut lib = library_with_tags(seed);
            let v = lib.play_shuffle(bachata()).unwrap();
            let mut queued = titles(&v);
            queued.sort();
            assert_eq!(queued, ["t1", "t2", "t3"], "only what the filter matches");
            assert!(v.state.shuffle && v.state.status == PlayStatus::Playing);
            assert!(
                matches!(&v.state.source, QueueSource::Search { filter } if filter.tag_all == ["bachata"])
            );
            starts.insert(now(&v).unwrap().to_string());

            // one round plays each track exactly once
            let mut played = vec![now(&v).unwrap().to_string()];
            loop {
                let v = lib.track_ended().unwrap();
                if v.state.status == PlayStatus::Stopped {
                    break;
                }
                played.push(now(&v).unwrap().to_string());
            }
            orders.insert(played.clone());
            played.sort();
            assert_eq!(played, ["t1", "t2", "t3"]);
        }
        assert_eq!(starts.len(), 3, "any match can open the shuffle");
        assert!(orders.len() > 2, "the order really varies");
    }

    #[test]
    fn play_shuffle_applies_the_filters_sort_and_limit_first() {
        let mut lib = library_with_tags(3);
        let filter = FilterSpec {
            sort: Some(weave_core::Sort {
                field: weave_core::SortField::Title,
                descending: true,
            }),
            limit: Some(2),
            ..bachata()
        };
        let v = lib.play_shuffle(filter).unwrap();
        let mut queued = titles(&v);
        queued.sort();
        assert_eq!(queued, ["t2", "t3"], "the top two by title, descending");
    }

    #[test]
    fn a_search_that_matches_nothing_is_an_error_and_changes_nothing() {
        let mut lib = library_with_tags(1);
        lib.queue_add(&[id(4)], None, false).unwrap();
        let none = FilterSpec {
            tag_all: vec!["metal".into()],
            ..Default::default()
        };
        assert!(matches!(
            lib.play_shuffle(none),
            Err(Error::InvalidInput(_))
        ));
        let bad = FilterSpec {
            bpm_min: Some(9.0),
            bpm_max: Some(1.0),
            ..Default::default()
        };
        assert!(matches!(
            lib.play_shuffle(bad),
            Err(Error::InvalidFilter(_))
        ));
        assert_eq!(titles(&lib.session().unwrap()), ["t4"]);
    }

    #[test]
    fn a_search_queue_does_not_take_hand_picked_tracks_either() {
        let mut lib = library_with_tags(1);
        lib.play_shuffle(bachata()).unwrap();
        let err = lib
            .queue_add(&[id(4)], None, false)
            .unwrap_err()
            .to_string();
        assert!(err.contains("a search"), "{err}");
    }

    #[test]
    fn a_playlist_may_fall_through_to_recommendations_without_changing_its_source() {
        let mut lib = tagged_library();
        lib.create_manual_playlist("Mix", None).unwrap();
        lib.playlist_add("mix", &["x-seed-fake".into()]).unwrap();
        lib.play_playlist("mix", false).unwrap();
        lib.set_end_of_queue(EndOfQueue::Continue).unwrap();

        let v = lib.track_ended().unwrap();
        assert_eq!(v.state.status, PlayStatus::Playing);
        assert!(
            v.tracks.len() > 1,
            "similar tracks were appended: {:?}",
            titles(&v)
        );
        assert_eq!(v.state.source, QueueSource::Playlist { id: "mix".into() });
        assert!(
            lib.queue_add(&["x-untagged-fake".into()], None, false)
                .is_err(),
            "still not open to hand-picked tracks"
        );
    }

    fn playlist_lib(seed: u64) -> Library {
        let mut lib = library_with_tags(seed);
        lib.create_manual_playlist("Mix", None).unwrap();
        lib.playlist_add("mix", &[id(3), id(1), id(2)]).unwrap();
        lib
    }

    #[test]
    fn the_shuffle_flag_shuffles_a_playlist_from_a_random_start_and_its_absence_changes_nothing() {
        let mut starts = std::collections::HashSet::new();
        for seed in 0..40 {
            let mut lib = playlist_lib(seed);
            let v = lib.play_playlist("mix", true).unwrap();
            assert!(v.state.shuffle);
            assert_eq!(v.state.source, QueueSource::Playlist { id: "mix".into() });
            let mut queued = titles(&v);
            queued.sort();
            assert_eq!(queued, ["t1", "t2", "t3"]);
            starts.insert(now(&v).unwrap().to_string());
        }
        assert_eq!(starts.len(), 3, "any track can open it");

        let mut lib = playlist_lib(1);
        let v = lib.play_playlist("mix", false).unwrap();
        assert_eq!(
            (v.state.shuffle, titles(&v), now(&v)),
            (false, vec!["t3", "t1", "t2"], Some("t3"))
        );
        lib.set_shuffle(true).unwrap();
        assert!(
            lib.play_playlist("mix", false).unwrap().state.shuffle,
            "the mode you set stays on"
        );
        lib.set_shuffle(false).unwrap();
        assert!(
            !lib.play_playlist("mix", false).unwrap().state.shuffle,
            "and off"
        );
    }

    #[test]
    fn a_playlist_queue_can_be_reordered_trimmed_and_jumped_around() {
        let mut lib = playlist_lib(1);
        lib.play_playlist("mix", false).unwrap();
        let source = QueueSource::Playlist { id: "mix".into() };

        let v = lib.queue_move(3, 1).unwrap();
        assert_eq!(titles(&v), ["t2", "t3", "t1"]);
        assert_eq!(v.state.source, source);
        let v = lib.queue_jump(3).unwrap();
        assert_eq!(
            (now(&v), v.state.source.clone()),
            (Some("t1"), source.clone())
        );
        let v = lib.queue_remove(&[1]).unwrap();
        assert_eq!(titles(&v), ["t3", "t1"]);
        assert_eq!(v.state.source, source, "still that playlist's queue");
    }

    #[test]
    fn adding_a_playlist_appends_its_tracks_as_plain_tracks_to_a_manual_queue() {
        let mut lib = playlist_lib(1);
        lib.queue_add(&[id(4)], None, false).unwrap();
        let n = lib.queue_add(&[], Some("mix"), false).unwrap();
        assert_eq!(n, 3);
        let v = lib.session().unwrap();
        assert_eq!(
            titles(&v),
            ["t4", "t3", "t1", "t2"],
            "in the playlist's own order, after what was there"
        );
        assert_eq!(
            v.state.source,
            QueueSource::Manual,
            "the queue is not that playlist's"
        );

        // tracks and a playlist together, and after the current track
        lib.play_track(&id(4)).unwrap();
        lib.queue_add(&[id(1)], Some("mix"), true).unwrap();
        assert_eq!(
            titles(&lib.session().unwrap()),
            ["t4", "t1", "t3", "t1", "t2"]
        );
    }

    #[test]
    fn a_dynamic_playlist_adds_whatever_it_matches_right_now_in_its_own_order() {
        let mut lib = library_with_tags(1);
        let filter = FilterSpec {
            sort: Some(weave_core::Sort {
                field: weave_core::SortField::Title,
                descending: true,
            }),
            limit: Some(2),
            ..bachata()
        };
        lib.create_playlist("Top", None, filter).unwrap();
        assert_eq!(lib.queue_add(&[], Some("top"), false).unwrap(), 2);
        assert_eq!(titles(&lib.session().unwrap()), ["t3", "t2"]);

        // "right now": a track tagged afterwards shows up the next time it is added
        lib.add_tags(&id(4), &["bachata".to_string()]).unwrap();
        lib.queue_clear().unwrap();
        lib.queue_add(&[], Some("top"), false).unwrap();
        assert_eq!(titles(&lib.session().unwrap()), ["t4", "t3"]);
        assert_eq!(lib.session().unwrap().state.source, QueueSource::Manual);
    }

    #[test]
    fn adding_a_bad_or_empty_playlist_changes_nothing() {
        let mut lib = playlist_lib(1);
        lib.create_manual_playlist("Empty", None).unwrap();
        lib.queue_add(&[id(4)], None, false).unwrap();
        assert!(matches!(
            lib.queue_add(&[], Some("empty"), false),
            Err(Error::InvalidInput(_))
        ));
        assert!(matches!(
            lib.queue_add(&[], Some("nope"), false),
            Err(Error::NotFound(_))
        ));
        assert!(lib.queue_add(&["nope".into()], Some("mix"), false).is_err());
        assert_eq!(titles(&lib.session().unwrap()), ["t4"]);
    }

    #[test]
    fn adding_anything_to_a_playlist_or_search_queue_is_still_refused() {
        let mut lib = playlist_lib(1);
        lib.play_playlist("mix", false).unwrap();
        for next in [false, true] {
            assert!(lib.queue_add(&[id(4)], None, next).is_err());
            assert!(
                lib.queue_add(&[], Some("mix"), next).is_err(),
                "not even a playlist"
            );
        }
        assert_eq!(titles(&lib.session().unwrap()), ["t3", "t1", "t2"]);
        lib.play_shuffle(bachata()).unwrap();
        assert!(lib.queue_add(&[], Some("mix"), false).is_err());
    }

    fn tagged_library() -> Library {
        let mut lib = Library::open_in_memory().unwrap();
        let tracks: Vec<_> = [
            ("seed", 100.0),
            ("match-a", 104.0),
            ("match-b", 96.0),
            ("too-fast", 140.0),
            ("other-genre", 100.0),
            ("untagged", 100.0),
        ]
        .into_iter()
        .map(|(t, b)| scanned(t, Some(b)))
        .collect();
        lib.import_scanned("fake", &tracks).unwrap();
        let tag = |lib: &mut Library, t: &str, tag: &str| {
            lib.add_tags(&format!("x-{t}-fake"), &[tag.to_string()])
                .unwrap();
        };
        for t in ["seed", "match-a", "match-b", "too-fast"] {
            tag(&mut lib, t, "bachata");
        }
        tag(&mut lib, "other-genre", "metal");
        lib
    }

    #[test]
    fn continue_appends_similar_tracks_and_keeps_playing() {
        let mut lib = tagged_library();
        lib.play_track("x-seed-fake").unwrap();
        lib.set_end_of_queue(EndOfQueue::Continue).unwrap();
        let v = lib.track_ended().unwrap();
        assert_eq!(v.state.status, PlayStatus::Playing);
        assert_eq!(now(&v), Some("match-a"), "closest bpm first");
        assert_eq!(
            titles(&v),
            ["seed", "match-a", "match-b"],
            "shares the tag, near the average bpm, not queued"
        );
    }

    #[test]
    fn continue_stops_when_nothing_is_similar() {
        let mut lib = tagged_library();
        lib.play_track("x-other-genre-fake").unwrap();
        lib.set_end_of_queue(EndOfQueue::Continue).unwrap();
        let v = lib.track_ended().unwrap();
        assert_eq!((v.state.status, v.tracks.len()), (PlayStatus::Stopped, 1));
    }

    #[test]
    fn stop_mode_stops_at_the_end() {
        let mut lib = tagged_library();
        lib.play_track("x-seed-fake").unwrap();
        let v = lib.track_ended().unwrap();
        assert_eq!((v.state.status, v.tracks.len()), (PlayStatus::Stopped, 1));
    }
}
