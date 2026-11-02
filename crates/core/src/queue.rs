//! Playback session state: queue, cursor, shuffle, repeat, end-of-queue behavior and
//! sleep timer. Pure data and rules; persistence lives in storage and orchestration in
//! the engine. No audio is involved: a playback host observes this state.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::{Error, Result, Rng, TrackId};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayStatus {
    #[default]
    Stopped,
    Playing,
    Paused,
}
text_enum!(PlayStatus { Stopped => "stopped", Playing => "playing", Paused => "paused" });

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepeatMode {
    #[default]
    Off,
    /// Replay the current track when it ends.
    Song,
    /// Loop the whole queue (usually a playlist) when it ends.
    Playlist,
}
text_enum!(RepeatMode { Off => "off", Song => "song", Playlist => "playlist" });

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndOfQueue {
    #[default]
    Stop,
    /// Append similar tracks (shared tags, close to the average BPM) and keep going.
    Continue,
}
text_enum!(EndOfQueue { Stop => "stop", Continue => "continue" });

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SleepTimer {
    /// Unix milliseconds at which the timer comes due.
    pub at_ms: i64,
    /// Stop-after timer: when due during a track, let that track finish and stop after it
    /// instead of stopping immediately.
    pub stop_after: bool,
}

/// What the user asks for with `sleep`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SleepSetting {
    /// Cancel any timer and any scheduled stop.
    Off,
    /// Come due after `delay`; `stop_after` lets the track playing at that moment finish.
    In {
        delay: std::time::Duration,
        stop_after: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayerState {
    pub status: PlayStatus,
    pub repeat: RepeatMode,
    pub shuffle: bool,
    pub end_of_queue: EndOfQueue,
    /// A pending sleep timer.
    pub sleep: Option<SleepTimer>,
    /// A stop is scheduled for the end of the current track: set when a stop-after timer
    /// comes due during it. Follows the current track, so skipping keeps it.
    pub stop_after_track: bool,
    /// Index into `Queue::items` of the current track.
    pub current: Option<usize>,
    /// 0.0 (mute) to 1.0.
    pub volume: f32,
    /// Position anchor: where playback was at `position_at_ms` (unix milliseconds). While
    /// playing, the real position is this plus the time elapsed since the anchor.
    pub position_ms: u64,
    pub position_at_ms: i64,
    /// Bumped every time a track starts (or restarts); a playback host restarts on change.
    pub start_seq: u64,
    /// Bumped on every seek request; a playback host seeks to `position_ms` on change.
    pub seek_seq: u64,
}

impl Default for PlayerState {
    fn default() -> Self {
        Self {
            status: PlayStatus::default(),
            repeat: RepeatMode::default(),
            shuffle: false,
            end_of_queue: EndOfQueue::default(),
            sleep: None,
            stop_after_track: false,
            current: None,
            volume: 1.0,
            position_ms: 0,
            position_at_ms: 0,
            start_seq: 0,
            seek_seq: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdvanceReason {
    /// The user asked for the next track; "repeat song" does not apply.
    UserSkip,
    /// The current track finished by itself.
    TrackEnded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Advance {
    /// The cursor moved (or stayed, for repeat-song) and playback continues.
    Moved,
    /// Playback stopped.
    Stopped,
    /// The queue is exhausted and end-of-queue is `continue`: append tracks, then advance again.
    NeedsMore,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Queue {
    /// Tracks in list order (what the user sees and edits). Duplicates are allowed.
    pub items: Vec<TrackId>,
    /// Play order: indexes into `items`, always a permutation of `0..items.len()`.
    /// Identity unless shuffle is on.
    pub order: Vec<usize>,
    pub state: PlayerState,
}

impl Queue {
    /// Where playback is now, in milliseconds.
    pub fn position_ms(&self, now_ms: i64) -> u64 {
        let elapsed = if self.state.status == PlayStatus::Playing {
            (now_ms - self.state.position_at_ms).max(0) as u64
        } else {
            0
        };
        self.state.position_ms.saturating_add(elapsed)
    }

    /// Re-anchor the position (also used when a host reports the real one).
    pub fn set_position(&mut self, position_ms: u64, now_ms: i64) {
        self.state.position_ms = position_ms;
        self.state.position_at_ms = now_ms;
    }

    /// A track has just started: restart from zero and tell hosts to (re)start it.
    pub fn mark_started(&mut self, now_ms: i64) {
        self.state.start_seq += 1;
        self.set_position(0, now_ms);
    }

    /// Ask hosts to jump to `position_ms` in the current track.
    pub fn request_seek(&mut self, position_ms: u64, now_ms: i64) -> Result<()> {
        if self.state.current.is_none() {
            return Err(Error::InvalidInput("nothing is queued".into()));
        }
        self.state.seek_seq += 1;
        self.set_position(position_ms, now_ms);
        Ok(())
    }

    /// Stopped is also the end of any pending stop-after-track.
    fn halt(&mut self) {
        self.state.status = PlayStatus::Stopped;
        self.state.stop_after_track = false;
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn current_track(&self) -> Option<&TrackId> {
        self.state.current.and_then(|i| self.items.get(i))
    }

    fn order_pos(&self, index: usize) -> Option<usize> {
        self.order.iter().position(|&i| i == index)
    }

    /// Rebuild `order` for the current shuffle setting. When shuffled, the current
    /// track comes first so it is not skipped or repeated.
    fn rebuild_order(&mut self, rng: &mut Rng) {
        let n = self.items.len();
        if !self.state.shuffle {
            self.order = (0..n).collect();
            return;
        }
        let mut rest: Vec<usize> = (0..n).filter(|i| Some(*i) != self.state.current).collect();
        rng.shuffle(&mut rest);
        self.order = self.state.current.into_iter().chain(rest).collect();
    }

    pub fn set_shuffle(&mut self, on: bool, rng: &mut Rng) {
        self.state.shuffle = on;
        self.rebuild_order(rng);
    }

    /// Replace the queue and start it: at the first track, or at a random one when shuffle
    /// is on (so a shuffled queue does not always open with the same track).
    pub fn replace(&mut self, tracks: Vec<TrackId>, rng: &mut Rng) {
        self.items = tracks;
        let start = if self.state.shuffle && !self.items.is_empty() {
            rng.below(self.items.len())
        } else {
            0
        };
        self.state.current = (!self.items.is_empty()).then_some(start);
        if self.items.is_empty() {
            self.halt();
        } else {
            self.state.status = PlayStatus::Playing;
        }
        self.rebuild_order(rng);
    }

    /// Add to the end of the list. Shuffled queues place new tracks at random among
    /// those still to come.
    pub fn append(&mut self, tracks: Vec<TrackId>, rng: &mut Rng) {
        let start = self.items.len();
        self.items.extend(tracks);
        for index in start..self.items.len() {
            if self.state.shuffle {
                let first_upcoming = self
                    .state
                    .current
                    .and_then(|c| self.order_pos(c))
                    .map_or(0, |p| p + 1);
                let at = first_upcoming + rng.below(self.order.len() - first_upcoming + 1);
                self.order.insert(at, index);
            } else {
                self.order.push(index);
            }
        }
    }

    /// Insert right after the current track so they play next (in list and play order).
    pub fn insert_next(&mut self, tracks: Vec<TrackId>, rng: &mut Rng) {
        let Some(current) = self.state.current else {
            return self.append(tracks, rng);
        };
        let k = tracks.len();
        if k == 0 {
            return;
        }
        self.items.splice(current + 1..current + 1, tracks);
        for i in &mut self.order {
            if *i > current {
                *i += k;
            }
        }
        let at = self.order_pos(current).expect("current is in the order") + 1;
        self.order.splice(at..at, current + 1..current + 1 + k);
    }

    /// Remove by 0-based list positions. If the current track is removed, the cursor
    /// moves to what would have played next (or previously, at the end).
    pub fn remove(&mut self, positions: &[usize]) -> Result<()> {
        let removed: HashSet<usize> = positions.iter().copied().collect();
        if let Some(bad) = removed.iter().find(|&&p| p >= self.items.len()) {
            return Err(Error::InvalidInput(format!(
                "queue has no position {}",
                bad + 1
            )));
        }
        let survivor = self.state.current.and_then(|c| {
            if !removed.contains(&c) {
                return Some(c);
            }
            let p = self.order_pos(c).expect("current is in the order");
            let after = self.order[p + 1..].iter().copied();
            let before = self.order[..p].iter().rev().copied();
            after.chain(before).find(|i| !removed.contains(i))
        });
        let remap = |old: usize| old - removed.iter().filter(|&&r| r < old).count();

        self.order = self
            .order
            .iter()
            .filter(|i| !removed.contains(i))
            .map(|&i| remap(i))
            .collect();
        let mut index = 0;
        self.items.retain(|_| {
            let keep = !removed.contains(&index);
            index += 1;
            keep
        });
        self.state.current = survivor.map(remap);
        if self.state.current.is_none() {
            self.halt();
        }
        Ok(())
    }

    /// Move one track between 0-based list positions; the cursor follows its track.
    pub fn move_item(&mut self, from: usize, to: usize) -> Result<()> {
        let n = self.items.len();
        for p in [from, to] {
            if p >= n {
                return Err(Error::InvalidInput(format!(
                    "queue has no position {}",
                    p + 1
                )));
            }
        }
        let mut layout: Vec<usize> = (0..n).collect(); // new position -> old index
        let moved = layout.remove(from);
        layout.insert(to, moved);
        let mut new_index = vec![0; n]; // old index -> new position
        for (new, &old) in layout.iter().enumerate() {
            new_index[old] = new;
        }
        self.items = layout.iter().map(|&old| self.items[old].clone()).collect();
        self.state.current = self.state.current.map(|c| new_index[c]);
        self.order = if self.state.shuffle {
            self.order.iter().map(|&i| new_index[i]).collect()
        } else {
            (0..n).collect()
        };
        Ok(())
    }

    /// Empty the queue and stop; shuffle, repeat and end-of-queue settings are kept.
    pub fn clear(&mut self) {
        self.items.clear();
        self.order.clear();
        self.state.current = None;
        self.halt();
    }

    /// Make the track at a 0-based list position current and start playing.
    pub fn jump(&mut self, position: usize) -> Result<()> {
        if position >= self.items.len() {
            return Err(Error::InvalidInput(format!(
                "queue has no position {}",
                position + 1
            )));
        }
        self.state.current = Some(position);
        self.state.status = PlayStatus::Playing;
        Ok(())
    }

    pub fn advance(&mut self, reason: AdvanceReason, rng: &mut Rng) -> Advance {
        if self.items.is_empty() {
            self.halt();
            return Advance::Stopped;
        }
        let Some(current) = self.state.current else {
            self.state.current = Some(self.order[0]);
            self.state.status = PlayStatus::Playing;
            return Advance::Moved;
        };
        if reason == AdvanceReason::TrackEnded && self.state.stop_after_track {
            self.halt();
            return Advance::Stopped;
        }
        if reason == AdvanceReason::TrackEnded && self.state.repeat == RepeatMode::Song {
            self.state.status = PlayStatus::Playing;
            return Advance::Moved;
        }
        let pos = self.order_pos(current).expect("current is in the order");
        if pos + 1 < self.order.len() {
            self.state.current = Some(self.order[pos + 1]);
            self.state.status = PlayStatus::Playing;
            return Advance::Moved;
        }
        // End of the queue.
        if self.state.repeat == RepeatMode::Playlist {
            if self.state.shuffle {
                rng.shuffle(&mut self.order);
                // Do not start the new round with the track that just played.
                if self.order.len() > 1 && self.order[0] == current {
                    self.order.swap(0, 1);
                }
            }
            self.state.current = Some(self.order[0]);
            self.state.status = PlayStatus::Playing;
            return Advance::Moved;
        }
        if self.state.end_of_queue == EndOfQueue::Continue {
            return Advance::NeedsMore;
        }
        self.halt();
        Advance::Stopped
    }

    /// Nothing left to play (used when `continue` finds no similar tracks).
    pub fn stop_at_end(&mut self) {
        self.halt();
    }

    /// Previous track in play order. At the start it restarts the current track, or
    /// wraps to the end when repeating the playlist.
    pub fn previous(&mut self) -> Result<()> {
        let current = self
            .state
            .current
            .ok_or_else(|| Error::InvalidInput("nothing is queued".into()))?;
        let pos = self.order_pos(current).expect("current is in the order");
        let target = if pos > 0 {
            self.order[pos - 1]
        } else if self.state.repeat == RepeatMode::Playlist {
            self.order[self.order.len() - 1]
        } else {
            current
        };
        self.state.current = Some(target);
        self.state.status = PlayStatus::Playing;
        Ok(())
    }

    pub fn pause(&mut self) -> Result<()> {
        if self.state.status != PlayStatus::Playing {
            return Err(Error::InvalidInput("nothing is playing".into()));
        }
        self.state.status = PlayStatus::Paused;
        Ok(())
    }

    /// Resumes a paused session, or restarts the current track after a stop.
    pub fn resume(&mut self) -> Result<()> {
        match (self.state.status, self.state.current) {
            (PlayStatus::Paused, _) | (PlayStatus::Stopped, Some(_)) => {
                self.state.status = PlayStatus::Playing;
                Ok(())
            }
            (PlayStatus::Playing, _) => Ok(()),
            (PlayStatus::Stopped, None) => Err(Error::InvalidInput("nothing to resume".into())),
        }
    }

    pub fn stop(&mut self) {
        self.halt();
    }

    /// Apply a `sleep` request. A new request replaces any timer and any scheduled stop.
    pub fn set_sleep(&mut self, setting: SleepSetting, now_ms: i64) {
        self.state.sleep = match setting {
            SleepSetting::Off => None,
            SleepSetting::In { delay, stop_after } => Some(SleepTimer {
                at_ms: now_ms.saturating_add(delay.as_millis() as i64),
                stop_after,
            }),
        };
        self.state.stop_after_track = false;
    }

    /// Called when the timer may have come due (before any operation). A plain timer
    /// stops playback. A stop-after timer instead schedules a stop at the end of the
    /// current track and lets it play on. Returns whether the
    /// state changed.
    pub fn apply_sleep(&mut self, now_ms: i64) -> bool {
        let Some(timer) = self.state.sleep.filter(|t| t.at_ms <= now_ms) else {
            return false;
        };
        self.state.sleep = None;
        if timer.stop_after && self.state.status != PlayStatus::Stopped {
            self.state.stop_after_track = true;
        } else {
            self.halt();
        }
        true
    }

    /// Called whenever a track starts playing. If a stop-after timer comes due
    /// while this track plays, schedule the stop now: the track is allowed to finish
    /// and playback stops after it. Otherwise the timer is left alone. Needs the
    /// track's duration; with an unknown duration the timer is handled when it comes
    /// due (or at the next track change) instead. Returns whether a stop was scheduled.
    pub fn schedule_sleep_for_track(&mut self, now_ms: i64, duration_ms: Option<u64>) -> bool {
        let (Some(timer), Some(duration)) = (self.state.sleep, duration_ms) else {
            return false;
        };
        let ends_at = now_ms.saturating_add(duration.min(i64::MAX as u64) as i64);
        if timer.stop_after && timer.at_ms <= ends_at {
            self.state.sleep = None;
            self.state.stop_after_track = true;
            return true;
        }
        false
    }

    /// Checks the invariants every operation must preserve.
    #[cfg(test)]
    fn assert_valid(&self) {
        let mut sorted = self.order.clone();
        sorted.sort_unstable();
        assert_eq!(
            sorted,
            (0..self.items.len()).collect::<Vec<_>>(),
            "order is a permutation"
        );
        if let Some(c) = self.state.current {
            assert!(c < self.items.len(), "current in range");
        }
        if !self.state.shuffle {
            assert_eq!(
                self.order,
                (0..self.items.len()).collect::<Vec<_>>(),
                "identity order"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(names: &[&str]) -> Vec<TrackId> {
        names.iter().map(|n| TrackId(n.to_string())).collect()
    }

    fn names(q: &Queue) -> Vec<&str> {
        q.items.iter().map(|t| t.0.as_str()).collect()
    }

    fn current(q: &Queue) -> Option<&str> {
        q.current_track().map(|t| t.0.as_str())
    }

    fn started(tracks: &[&str]) -> (Queue, Rng) {
        let mut rng = Rng::new(7);
        let mut q = Queue::default();
        q.replace(ids(tracks), &mut rng);
        (q, rng)
    }

    #[test]
    fn replace_starts_playing_first_track() {
        let (q, _) = started(&["a", "b", "c"]);
        assert_eq!(
            (current(&q), q.state.status),
            (Some("a"), PlayStatus::Playing)
        );
        q.assert_valid();
    }

    #[test]
    fn advance_walks_then_stops_at_end() {
        let (mut q, mut rng) = started(&["a", "b"]);
        assert_eq!(q.advance(AdvanceReason::UserSkip, &mut rng), Advance::Moved);
        assert_eq!(current(&q), Some("b"));
        assert_eq!(
            q.advance(AdvanceReason::UserSkip, &mut rng),
            Advance::Stopped
        );
        assert_eq!(
            (current(&q), q.state.status),
            (Some("b"), PlayStatus::Stopped)
        );
    }

    #[test]
    fn repeat_song_only_applies_when_track_ends() {
        let (mut q, mut rng) = started(&["a", "b"]);
        q.state.repeat = RepeatMode::Song;
        assert_eq!(
            q.advance(AdvanceReason::TrackEnded, &mut rng),
            Advance::Moved
        );
        assert_eq!(current(&q), Some("a"));
        assert_eq!(q.advance(AdvanceReason::UserSkip, &mut rng), Advance::Moved);
        assert_eq!(current(&q), Some("b"));
    }

    #[test]
    fn repeat_playlist_wraps() {
        let (mut q, mut rng) = started(&["a", "b"]);
        q.state.repeat = RepeatMode::Playlist;
        q.advance(AdvanceReason::TrackEnded, &mut rng);
        assert_eq!(
            q.advance(AdvanceReason::TrackEnded, &mut rng),
            Advance::Moved
        );
        assert_eq!(current(&q), Some("a"));
        q.previous().unwrap();
        assert_eq!(
            current(&q),
            Some("b"),
            "previous wraps while repeating the playlist"
        );
    }

    #[test]
    fn continue_mode_asks_for_more_at_the_end() {
        let (mut q, mut rng) = started(&["a"]);
        q.state.end_of_queue = EndOfQueue::Continue;
        assert_eq!(
            q.advance(AdvanceReason::TrackEnded, &mut rng),
            Advance::NeedsMore
        );
        assert_eq!(
            q.state.status,
            PlayStatus::Playing,
            "unchanged until the engine decides"
        );
        q.append(ids(&["x"]), &mut rng);
        assert_eq!(
            q.advance(AdvanceReason::TrackEnded, &mut rng),
            Advance::Moved
        );
        assert_eq!(current(&q), Some("x"));
    }

    #[test]
    fn previous_restarts_at_the_start() {
        let (mut q, _) = started(&["a", "b"]);
        q.previous().unwrap();
        assert_eq!(current(&q), Some("a"));
        assert!(Queue::default().previous().is_err());
    }

    #[test]
    fn shuffle_visits_every_track_once_starting_with_current() {
        let (mut q, mut rng) = started(&["a", "b", "c", "d", "e", "f"]);
        q.jump(2).unwrap();
        q.set_shuffle(true, &mut rng);
        q.assert_valid();
        assert_eq!(q.order[0], 2, "current stays first");
        let mut seen = vec![current(&q).unwrap().to_string()];
        while q.advance(AdvanceReason::UserSkip, &mut rng) == Advance::Moved {
            seen.push(current(&q).unwrap().to_string());
        }
        seen.sort();
        assert_eq!(seen, ["a", "b", "c", "d", "e", "f"]);
    }

    #[test]
    fn shuffle_off_restores_list_order() {
        let (mut q, mut rng) = started(&["a", "b", "c", "d"]);
        q.set_shuffle(true, &mut rng);
        q.set_shuffle(false, &mut rng);
        q.assert_valid();
    }

    #[test]
    fn shuffled_repeat_playlist_starts_new_round_with_a_different_track() {
        for seed in 0..30 {
            let mut rng = Rng::new(seed);
            let mut q = Queue::default();
            q.state.repeat = RepeatMode::Playlist;
            q.state.shuffle = true;
            q.replace(ids(&["a", "b", "c"]), &mut rng);
            for _ in 0..2 {
                q.advance(AdvanceReason::TrackEnded, &mut rng);
            }
            let last = current(&q).unwrap().to_string();
            q.advance(AdvanceReason::TrackEnded, &mut rng);
            assert_ne!(current(&q).unwrap(), last);
            q.assert_valid();
        }
    }

    #[test]
    fn append_while_shuffled_lands_among_upcoming_tracks() {
        for seed in 0..30 {
            let mut rng = Rng::new(seed);
            let mut q = Queue::default();
            q.state.shuffle = true;
            q.replace(ids(&["a", "b", "c"]), &mut rng);
            q.append(ids(&["x", "y"]), &mut rng);
            q.assert_valid();
            assert_eq!(
                Some(q.order[0]),
                q.state.current,
                "the playing track is not displaced"
            );
            assert_eq!(names(&q), ["a", "b", "c", "x", "y"]);
        }
    }

    #[test]
    fn insert_next_plays_right_after_current() {
        let (mut q, mut rng) = started(&["a", "b", "c"]);
        q.insert_next(ids(&["x", "y"]), &mut rng);
        q.assert_valid();
        assert_eq!(names(&q), ["a", "x", "y", "b", "c"]);
        q.advance(AdvanceReason::UserSkip, &mut rng);
        assert_eq!(current(&q), Some("x"));

        q.set_shuffle(true, &mut rng);
        q.insert_next(ids(&["z"]), &mut rng);
        q.assert_valid();
        q.advance(AdvanceReason::UserSkip, &mut rng);
        assert_eq!(current(&q), Some("z"), "also in shuffle mode");
    }

    #[test]
    fn remove_before_current_keeps_cursor_on_same_track() {
        let (mut q, mut rng) = started(&["a", "b", "c", "d"]);
        q.jump(2).unwrap();
        q.remove(&[0, 1]).unwrap();
        q.assert_valid();
        assert_eq!((names(&q), current(&q)), (vec!["c", "d"], Some("c")));
        let _ = &mut rng;
    }

    #[test]
    fn remove_current_moves_to_next_in_play_order() {
        let (mut q, _) = started(&["a", "b", "c"]);
        q.jump(1).unwrap();
        q.remove(&[1]).unwrap();
        q.assert_valid();
        assert_eq!((names(&q), current(&q)), (vec!["a", "c"], Some("c")));
        q.remove(&[1]).unwrap();
        assert_eq!(
            current(&q),
            Some("a"),
            "falls back to the previous track at the end"
        );
        q.remove(&[0]).unwrap();
        assert_eq!((current(&q), q.state.status), (None, PlayStatus::Stopped));
    }

    #[test]
    fn remove_rejects_bad_positions_without_changing_anything() {
        let (mut q, _) = started(&["a", "b"]);
        let before = q.clone();
        assert!(q.remove(&[0, 5]).is_err());
        assert_eq!(q, before);
    }

    #[test]
    fn remove_in_shuffled_queue_keeps_a_valid_permutation() {
        for seed in 0..30 {
            let mut rng = Rng::new(seed);
            let mut q = Queue::default();
            q.state.shuffle = true;
            q.replace(ids(&["a", "b", "c", "d", "e"]), &mut rng);
            q.remove(&[1, 3]).unwrap();
            q.assert_valid();
            assert_eq!(names(&q), ["a", "c", "e"]);
        }
    }

    #[test]
    fn move_item_follows_cursor() {
        let (mut q, _) = started(&["a", "b", "c", "d"]);
        q.jump(0).unwrap();
        q.move_item(0, 2).unwrap();
        q.assert_valid();
        assert_eq!(
            (names(&q), current(&q)),
            (vec!["b", "c", "a", "d"], Some("a"))
        );
        assert_eq!(q.state.current, Some(2));
        q.move_item(3, 0).unwrap();
        assert_eq!(names(&q), ["d", "b", "c", "a"]);
        assert_eq!(current(&q), Some("a"));
        assert!(q.move_item(0, 9).is_err());
    }

    #[test]
    fn move_item_in_shuffled_queue_keeps_play_sequence() {
        let mut rng = Rng::new(3);
        let mut q = Queue::default();
        q.state.shuffle = true;
        q.replace(ids(&["a", "b", "c", "d"]), &mut rng);
        let sequence = |q: &Queue| {
            q.order
                .iter()
                .map(|&i| q.items[i].0.clone())
                .collect::<Vec<_>>()
        };
        let before = sequence(&q);
        q.move_item(0, 3).unwrap();
        q.assert_valid();
        assert_eq!(sequence(&q), before);
    }

    #[test]
    fn clear_keeps_settings() {
        let (mut q, mut rng) = started(&["a"]);
        q.state.repeat = RepeatMode::Playlist;
        q.set_shuffle(true, &mut rng);
        q.clear();
        assert!(q.is_empty() && q.state.current.is_none());
        assert_eq!(
            (q.state.repeat, q.state.shuffle),
            (RepeatMode::Playlist, true)
        );
        assert_eq!(
            q.advance(AdvanceReason::UserSkip, &mut rng),
            Advance::Stopped
        );
    }

    #[test]
    fn pause_resume_stop() {
        let (mut q, _) = started(&["a"]);
        q.pause().unwrap();
        assert!(q.pause().is_err());
        q.resume().unwrap();
        q.stop();
        assert_eq!(q.state.status, PlayStatus::Stopped);
        q.resume().unwrap();
        assert_eq!(q.state.status, PlayStatus::Playing);
        assert!(Queue::default().resume().is_err());
    }

    use std::time::Duration;

    fn timer(q: &mut Queue, now: i64, secs: u64, stop_after: bool) {
        q.set_sleep(
            SleepSetting::In {
                delay: Duration::from_secs(secs),
                stop_after,
            },
            now,
        );
    }

    #[test]
    fn plain_timer_stops_playback_once() {
        let (mut q, _) = started(&["a"]);
        timer(&mut q, 0, 1, false);
        assert!(!q.apply_sleep(999));
        assert_eq!(q.state.status, PlayStatus::Playing);
        assert!(q.apply_sleep(1_000));
        assert_eq!((q.state.status, q.state.sleep), (PlayStatus::Stopped, None));
        assert!(!q.apply_sleep(2_000));
    }

    #[test]
    fn stop_after_timer_coming_due_lets_the_track_finish_then_stops() {
        let (mut q, mut rng) = started(&["a", "b"]);
        timer(&mut q, 0, 60, true);
        assert!(q.apply_sleep(60_000));
        assert_eq!(q.state.status, PlayStatus::Playing, "the track plays on");
        assert!((q.state.sleep, q.state.stop_after_track) == (None, true));
        assert_eq!(
            q.advance(AdvanceReason::TrackEnded, &mut rng),
            Advance::Stopped
        );
        assert_eq!(
            (current(&q), q.state.status),
            (Some("a"), PlayStatus::Stopped)
        );
        assert!(
            !q.state.stop_after_track,
            "stopping clears the scheduled stop"
        );
    }

    #[test]
    fn stop_after_timer_with_nothing_playing_just_clears() {
        let (mut q, _) = started(&["a"]);
        q.stop();
        timer(&mut q, 0, 1, true);
        assert!(q.apply_sleep(1_000));
        assert!((q.state.sleep, q.state.stop_after_track) == (None, false));
    }

    #[test]
    fn scheduling_at_track_start_only_when_the_deadline_falls_inside_the_track() {
        let (mut q, _) = started(&["a"]);
        timer(&mut q, 0, 100, true); // due at 100_000 ms
        // a 60 s track starting at 0 ends at 60_000: the deadline is later, so leave it
        assert!(!q.schedule_sleep_for_track(0, Some(60_000)));
        assert!(q.state.sleep.is_some() && !q.state.stop_after_track);
        // a 60 s track starting at 50 s ends at 110 s: the deadline falls inside it
        assert!(q.schedule_sleep_for_track(50_000, Some(60_000)));
        assert!((q.state.sleep, q.state.stop_after_track) == (None, true));
    }

    #[test]
    fn deadline_exactly_at_the_end_of_the_track_counts_as_inside() {
        let (mut q, _) = started(&["a"]);
        timer(&mut q, 0, 60, true);
        assert!(q.schedule_sleep_for_track(0, Some(60_000)));
    }

    #[test]
    fn scheduling_ignores_plain_timers_and_unknown_durations() {
        let (mut q, _) = started(&["a"]);
        timer(&mut q, 0, 10, false);
        assert!(
            !q.schedule_sleep_for_track(0, Some(600_000)),
            "plain timers stop at the deadline"
        );
        timer(&mut q, 0, 10, true);
        assert!(
            !q.schedule_sleep_for_track(0, None),
            "unknown duration: decided when it comes due"
        );
        assert!(q.state.sleep.is_some());
        assert!(!Queue::default().schedule_sleep_for_track(0, Some(1)));
    }

    #[test]
    fn scheduled_stop_follows_the_track_across_skips_but_not_a_stop() {
        let (mut q, mut rng) = started(&["a", "b", "c"]);
        q.state.stop_after_track = true;
        q.advance(AdvanceReason::UserSkip, &mut rng);
        assert!(
            q.state.stop_after_track,
            "skipping keeps it for the new current track"
        );
        assert_eq!(
            q.advance(AdvanceReason::TrackEnded, &mut rng),
            Advance::Stopped
        );
        assert_eq!(current(&q), Some("b"));

        q.resume().unwrap();
        q.state.stop_after_track = true;
        q.stop();
        assert!(!q.state.stop_after_track);
    }

    #[test]
    fn scheduled_stop_beats_repeat_song() {
        let (mut q, mut rng) = started(&["a"]);
        q.state.repeat = RepeatMode::Song;
        q.state.stop_after_track = true;
        assert_eq!(
            q.advance(AdvanceReason::TrackEnded, &mut rng),
            Advance::Stopped
        );
    }

    #[test]
    fn a_new_request_replaces_timer_and_scheduled_stop() {
        let (mut q, _) = started(&["a"]);
        timer(&mut q, 0, 5, true);
        q.state.stop_after_track = true;
        q.set_sleep(SleepSetting::Off, 0);
        assert!((q.state.sleep, q.state.stop_after_track) == (None, false));
        timer(&mut q, 0, 5, false);
        q.state.stop_after_track = true;
        timer(&mut q, 0, 9, true);
        assert!(
            !q.state.stop_after_track
                && q.state
                    .sleep
                    .is_some_and(|t| t.at_ms == 9_000 && t.stop_after)
        );
    }

    #[test]
    fn position_advances_only_while_playing() {
        let (mut q, _) = started(&["a"]);
        q.mark_started(1_000);
        assert_eq!(q.position_ms(1_000), 0);
        assert_eq!(q.position_ms(3_500), 2_500, "elapsed since the anchor");
        q.set_position(2_500, 3_500);
        q.pause().unwrap();
        assert_eq!(q.position_ms(99_000), 2_500, "frozen while paused");
        assert_eq!(
            q.position_ms(0),
            2_500,
            "a clock behind the anchor never goes negative"
        );
    }

    #[test]
    fn mark_started_counts_starts_and_resets_the_position() {
        let (mut q, _) = started(&["a"]);
        q.set_position(5_000, 0);
        q.mark_started(10);
        q.mark_started(20);
        assert_eq!(
            (
                q.state.start_seq,
                q.state.position_ms,
                q.state.position_at_ms
            ),
            (2, 0, 20)
        );
    }

    #[test]
    fn seek_requests_need_a_current_track_and_are_counted() {
        let mut q = Queue::default();
        assert!(q.request_seek(1_000, 0).is_err());
        let (mut q, _) = started(&["a"]);
        q.request_seek(42_000, 7).unwrap();
        q.request_seek(43_000, 8).unwrap();
        assert_eq!(
            (
                q.state.seek_seq,
                q.state.position_ms,
                q.state.position_at_ms
            ),
            (2, 43_000, 8)
        );
        assert_eq!(q.state.start_seq, 0, "a seek is not a restart");
    }

    #[test]
    fn a_fresh_session_is_stopped_at_full_volume() {
        let s = PlayerState::default();
        assert_eq!(
            (s.status, s.volume, s.start_seq, s.seek_seq),
            (PlayStatus::Stopped, 1.0, 0, 0)
        );
    }

    #[test]
    fn a_shuffled_replace_starts_at_a_random_track_and_visits_each_once() {
        let mut starts = std::collections::HashSet::new();
        for seed in 0..40 {
            let mut rng = Rng::new(seed);
            let mut q = Queue::default();
            q.state.shuffle = true;
            q.replace(ids(&["a", "b", "c", "d"]), &mut rng);
            q.assert_valid();
            assert_eq!(Some(q.order[0]), q.state.current);
            starts.insert(q.state.current.unwrap());
        }
        assert_eq!(starts.len(), 4, "every track can open a shuffled queue");

        let mut q = Queue::default();
        q.replace(ids(&["a", "b"]), &mut Rng::new(1));
        assert_eq!(
            q.state.current,
            Some(0),
            "an unshuffled queue starts at the top"
        );
    }

    #[test]
    fn text_enums_round_trip() {
        for r in RepeatMode::ALL {
            assert_eq!(r.as_str().parse::<RepeatMode>().unwrap(), *r);
        }
        assert_eq!(
            "CONTINUE".parse::<EndOfQueue>().unwrap(),
            EndOfQueue::Continue
        );
        assert!(
            "loop"
                .parse::<RepeatMode>()
                .unwrap_err()
                .contains("off, song, playlist")
        );
    }
}
