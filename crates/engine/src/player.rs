//! Playback session orchestration: queue edits, transport state, shuffle/repeat,
//! end-of-queue behavior and the sleep timer. The session is persisted after every
//! operation, so separate CLI invocations share it. No audio is produced here; a
//! playback backend will follow `SessionView` (0.a4).
//!
//! There is no background process: the sleep timer is checked whenever the session is
//! read or changed, and playback is stopped at that point if it ran out.

use std::time::Duration;

use weave_core::{
    Advance, AdvanceReason, EndOfQueue, Error, Page, PlayStatus, PlayerState, Queue, RepeatMode,
    Result, SleepSetting, Track, TrackId,
};

use crate::{Library, autoplay};

#[derive(Debug, Clone)]
pub struct SessionView {
    pub state: PlayerState,
    /// Queue contents in list order.
    pub tracks: Vec<Track>,
    /// Time left on the sleep timer, if one is set.
    pub sleep_remaining: Option<Duration>,
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
        let fired = queue.apply_sleep(self.now_ms());
        if fired {
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
        })
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

    fn playlist_slugs(&self, name: &str) -> Result<Vec<TrackId>> {
        let (p, result) = self.playlist_tracks(name, Page::ALL)?;
        if result.tracks.is_empty() {
            return Err(Error::InvalidInput(format!(
                "playlist \"{}\" has no tracks",
                p.name
            )));
        }
        Ok(result.tracks.into_iter().map(|t| t.id).collect())
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

    // --- starting playback -------------------------------------------------

    /// Replace the queue with one track and start it.
    pub fn play_track(&mut self, id: &str) -> Result<SessionView> {
        let track = self.resolve(&[id.to_string()])?;
        let mut queue = self.load_session()?;
        queue.replace(track, &mut self.rng);
        self.commit_started(&mut queue)
    }

    /// Replace the queue with a playlist's tracks and start it, or (`append`) add
    /// them to the end without disturbing playback.
    pub fn play_playlist(&mut self, name: &str, append: bool) -> Result<SessionView> {
        let tracks = self.playlist_slugs(name)?;
        let mut queue = self.load_session()?;
        if append {
            queue.append(tracks, &mut self.rng);
            self.commit(&queue)
        } else {
            queue.replace(tracks, &mut self.rng);
            self.commit_started(&mut queue)
        }
    }

    // --- queue editing -----------------------------------------------------

    /// Add tracks to the end of the queue, or right after the current one (`next`).
    /// Returns how many were added.
    pub fn queue_add(&mut self, ids: &[String], next: bool) -> Result<usize> {
        let tracks = self.resolve(ids)?;
        let n = tracks.len();
        let mut queue = self.load_session()?;
        if next {
            queue.insert_next(tracks, &mut self.rng);
        } else {
            queue.append(tracks, &mut self.rng);
        }
        self.commit(&queue)?;
        Ok(n)
    }

    /// Add tracks and/or a playlist's tracks in one step: everything is resolved first, so a
    /// bad ID or playlist name changes nothing. Returns how many were added.
    pub fn queue_add_all(
        &mut self,
        ids: &[String],
        playlist: Option<&str>,
        next: bool,
    ) -> Result<usize> {
        let mut tracks = self.resolve(ids)?;
        if let Some(name) = playlist {
            tracks.extend(self.playlist_slugs(name)?);
        }
        let n = tracks.len();
        let mut queue = self.load_session()?;
        if next {
            queue.insert_next(tracks, &mut self.rng);
        } else {
            queue.append(tracks, &mut self.rng);
        }
        self.commit(&queue)?;
        Ok(n)
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
        queue.pause()?;
        self.commit(&queue)
    }

    pub fn resume(&mut self) -> Result<SessionView> {
        let mut queue = self.load_session()?;
        let restarting = queue.state.status == PlayStatus::Stopped;
        queue.resume()?;
        if restarting {
            return self.commit_started(&mut queue);
        }
        self.commit(&queue)
    }

    pub fn stop(&mut self) -> Result<SessionView> {
        let mut queue = self.load_session()?;
        queue.stop();
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
        lib.queue_add(&[id(2), id(3)], false).unwrap();
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
            lib.queue_add(&[id(1), id(2)], false).unwrap();
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
    fn playlist_play_replaces_or_appends_and_rejects_empty() {
        let (mut lib, _) = library(3);
        lib.create_manual_playlist("Mix", None).unwrap();
        assert!(matches!(
            lib.play_playlist("mix", false),
            Err(Error::InvalidInput(_))
        ));
        lib.playlist_add("mix", &[id(3), id(1)]).unwrap();
        let v = lib.play_playlist("mix", false).unwrap();
        assert_eq!((titles(&v), now(&v)), (vec!["t3", "t1"], Some("t3")));
        let v = lib.play_playlist("mix", true).unwrap();
        assert_eq!(
            (titles(&v), now(&v)),
            (vec!["t3", "t1", "t3", "t1"], Some("t3"))
        );
    }

    #[test]
    fn queue_add_all_is_all_or_nothing() {
        let (mut lib, _) = library(3);
        lib.create_manual_playlist("Mix", None).unwrap();
        lib.playlist_add("mix", &[id(3)]).unwrap();
        assert_eq!(lib.queue_add_all(&[id(1)], Some("mix"), false).unwrap(), 2);
        assert_eq!(titles(&lib.session().unwrap()), ["t1", "t3"]);

        assert!(lib.queue_add_all(&[id(2)], Some("nope"), false).is_err());
        assert!(
            lib.queue_add_all(&[id(2), "nope".into()], Some("mix"), false)
                .is_err()
        );
        assert_eq!(
            titles(&lib.session().unwrap()),
            ["t1", "t3"],
            "failed adds change nothing"
        );
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
        lib.queue_add(&[id(1), id(2), id(3)], false).unwrap();
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
        lib.queue_add(&[id(3), id(4)], false).unwrap();
        lib.queue_add(&[id(2)], true).unwrap();
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
        lib.queue_add(&[id(1), id(2)], false).unwrap();
        lib.next().unwrap();
        lib.set_repeat(RepeatMode::Song).unwrap();
        assert_eq!(now(&lib.track_ended().unwrap()), Some("t1"));
        assert_eq!(now(&lib.track_ended().unwrap()), Some("t1"));
        assert_eq!(now(&lib.next().unwrap()), Some("t2"));
    }

    #[test]
    fn repeat_playlist_loops_the_queue() {
        let (mut lib, _) = library(2);
        lib.queue_add(&[id(1), id(2)], false).unwrap();
        lib.next().unwrap();
        lib.set_repeat(RepeatMode::Playlist).unwrap();
        assert_eq!(now(&lib.track_ended().unwrap()), Some("t2"));
        let v = lib.track_ended().unwrap();
        assert_eq!((now(&v), v.state.status), (Some("t1"), PlayStatus::Playing));
    }

    #[test]
    fn shuffle_plays_everything_once() {
        let (mut lib, _) = library(6);
        lib.queue_add(&(1..=6).map(id).collect::<Vec<_>>(), false)
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
        lib.queue_add(&[id(1), id(2)], false).unwrap();
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
        lib.queue_add(&[id(1), id(2)], false).unwrap();
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
        lib.queue_add(&[id(1), id(2)], false).unwrap();
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
        lib.queue_add(&[id(1), id(2), id(3)], false).unwrap();
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
        lib.queue_add(&(1..=4).map(id).collect::<Vec<_>>(), false)
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
        lib.queue_add(&[id(1), id(2)], false).unwrap();
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
        lib.queue_add(&[id(1), id(2)], false).unwrap();
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
        lib.queue_add(&[id(1), id(2), id(3)], false).unwrap();
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
            lib.queue_add(&[id(1), id(2), id(3)], false).unwrap();
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
        lib.queue_add(&[id(1), id(2)], false).unwrap();
        lib.next().unwrap();
        lib.set_sleep(timer(600, true)).unwrap(); // beyond the 3 minute track
        lib.pause().unwrap();
        let v = lib.resume().unwrap();
        assert!(!v.state.stop_after_track && v.state.sleep.is_some());
    }

    #[test]
    fn timer_state_survives_reopening_the_database() {
        let dir = std::env::temp_dir().join(format!("weave-sleep-{}", uuid_like()));
        let path = dir.join("lib.db");
        {
            let mut lib = Library::open(&path).unwrap();
            lib.import_scanned("fake", &[scanned("t1", None), scanned("t2", None)])
                .unwrap();
            lib.queue_add(&[id(1), id(2)], false).unwrap();
            lib.next().unwrap();
            lib.set_sleep(timer(600, true)).unwrap();
        }
        let mut lib = Library::open(&path).unwrap();
        let v = lib.session().unwrap();
        assert!(v.state.sleep.is_some_and(|t| t.stop_after) && v.sleep_remaining.is_some());
        std::fs::remove_dir_all(dir).unwrap();
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
