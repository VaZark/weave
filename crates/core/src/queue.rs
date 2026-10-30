//! Playback session state: queue, cursor, shuffle, repeat, end-of-queue behavior and
//! sleep timer. Pure data and rules; persistence lives in storage and orchestration in
//! the engine. No audio is involved: a future playback backend observes this state.

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

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PlayerState {
    pub status: PlayStatus,
    pub repeat: RepeatMode,
    pub shuffle: bool,
    pub end_of_queue: EndOfQueue,
    /// Unix milliseconds at which the sleep timer stops playback.
    pub sleep_until_ms: Option<i64>,
    /// Index into `Queue::items` of the current track.
    pub current: Option<usize>,
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

    /// Replace the queue and start at its first track in play order.
    pub fn replace(&mut self, tracks: Vec<TrackId>, rng: &mut Rng) {
        self.items = tracks;
        self.state.current = (!self.items.is_empty()).then_some(0);
        self.state.status = if self.items.is_empty() {
            PlayStatus::Stopped
        } else {
            PlayStatus::Playing
        };
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
            self.state.status = PlayStatus::Stopped;
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
        self.state.status = PlayStatus::Stopped;
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
            self.state.status = PlayStatus::Stopped;
            return Advance::Stopped;
        }
        let Some(current) = self.state.current else {
            self.state.current = Some(self.order[0]);
            self.state.status = PlayStatus::Playing;
            return Advance::Moved;
        };
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
        self.state.status = PlayStatus::Stopped;
        Advance::Stopped
    }

    /// Nothing left to play (used when `continue` finds no similar tracks).
    pub fn stop_at_end(&mut self) {
        self.state.status = PlayStatus::Stopped;
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
        self.state.status = PlayStatus::Stopped;
    }

    pub fn set_sleep_until(&mut self, at_ms: Option<i64>) {
        self.state.sleep_until_ms = at_ms;
    }

    /// Stops playback if the sleep timer has run out. Returns whether it fired.
    pub fn apply_sleep(&mut self, now_ms: i64) -> bool {
        match self.state.sleep_until_ms {
            Some(at) if at <= now_ms => {
                self.state.sleep_until_ms = None;
                self.state.status = PlayStatus::Stopped;
                true
            }
            _ => false,
        }
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
            assert_eq!(q.order[0], 0, "the playing track is not displaced");
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

    #[test]
    fn sleep_timer_stops_playback_once() {
        let (mut q, _) = started(&["a"]);
        q.set_sleep_until(Some(1_000));
        assert!(!q.apply_sleep(999));
        assert_eq!(q.state.status, PlayStatus::Playing);
        assert!(q.apply_sleep(1_000));
        assert_eq!(
            (q.state.status, q.state.sleep_until_ms),
            (PlayStatus::Stopped, None)
        );
        assert!(!q.apply_sleep(2_000));
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
