//! Filtered, measured analytics; no lifetime play count is used as elapsed time.
use crate::Library;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use weave_player_core::*;

const DAY: i64 = 86_400_000;
fn ranked(mut shares: Vec<ListeningShare>) -> Vec<ListeningShare> {
    shares.sort_by(|a, b| b.listened_ms.cmp(&a.listened_ms).then(a.id.cmp(&b.id)));
    shares
}
fn buckets(events: &[ListeningEvent], width: i64) -> Vec<ActivityBucket> {
    let mut bins = BTreeMap::new();
    for segment in events.iter().flat_map(|e| &e.segments) {
        let mut at = segment.started_ms;
        while at < segment.ended_ms {
            let start = at / width * width;
            let end = segment.ended_ms.min(start.saturating_add(width));
            *bins.entry(start).or_insert(0u64) += (end - at) as u64;
            at = end;
        }
    }
    bins.into_iter()
        .map(|(started_ms, listened_ms)| ActivityBucket {
            started_ms,
            listened_ms,
        })
        .collect()
}
impl Library {
    /// Called only once audio output successfully starts. No event exists for failed output.
    pub fn begin_listening(&mut self, start_seq: u64) -> Result<String> {
        let state = self.storage.load_queue()?.state;
        if state.start_seq != start_seq || state.status == PlayStatus::Stopped {
            return Err(Error::InvalidInput("stale playback start".into()));
        }
        let queue = self.storage.load_queue()?;
        let track = queue
            .current_track()
            .ok_or_else(|| Error::InvalidInput("nothing queued".into()))?;
        self.storage
            .begin_listening(&track.0, start_seq, (self.clock)())
    }
    pub fn report_listening(&mut self, id: &str, cumulative_ms: u64) -> Result<bool> {
        self.storage
            .report_listening(id, (self.clock)(), cumulative_ms)
    }
    pub fn end_listening(&mut self, id: &str, reason: ListenEnd) -> Result<()> {
        self.storage.end_listening(id, reason, (self.clock)())
    }
    /// Called once by a replacement audio host after it acquires ownership.
    pub fn recover_listening(&mut self) -> Result<()> {
        self.storage.recover_listening()
    }
    pub fn listening_events(&self, query: &AnalyticsQuery) -> Result<Vec<ListeningEvent>> {
        let (since, until) = query.range((self.clock)())?;
        let allowed: BTreeSet<_> = self
            .search(&query.filter, Page::ALL)?
            .tracks
            .into_iter()
            .map(|t| t.id.0)
            .collect();
        Ok(self
            .storage
            .listening_events(since, until)?
            .into_iter()
            .filter(|e| allowed.contains(&e.track))
            .collect())
    }
    pub fn listening_analytics(&self, query: &AnalyticsQuery) -> Result<ListeningAnalytics> {
        let (since, until) = query.range((self.clock)())?;
        let tracking_since = self.storage.listening_tracking_since()?;
        let since = if query.since_ms.is_none() && query.period.is_none() {
            tracking_since.unwrap_or(since).min(until - 1)
        } else {
            since
        };
        let tracks: HashMap<_, _> = self
            .search(&query.filter, Page::ALL)?
            .tracks
            .into_iter()
            .map(|t| (t.id.0.clone(), t))
            .collect();
        let events: Vec<_> = self
            .storage
            .listening_events(since, until)?
            .into_iter()
            .filter(|e| tracks.contains_key(&e.track) && e.listened_ms > 0)
            .collect();
        let mut result = ListeningAnalytics {
            since_ms: since,
            until_ms: until,
            tracking_since_ms: tracking_since,
            ..Default::default()
        };
        let mut loyalty: BTreeMap<String, ListeningLoyalty> = BTreeMap::new();
        let mut ended: HashMap<String, usize> = HashMap::new();
        let mut sessions: BTreeMap<String, ListeningSession> = BTreeMap::new();
        for event in &events {
            result.listened_ms += event.listened_ms;
            result.listens += 1;
            let entry = loyalty
                .entry(event.track.clone())
                .or_insert_with(|| ListeningLoyalty {
                    track: event.track.clone(),
                    ..Default::default()
                });
            entry.listens += 1;
            entry.listened_ms += event.listened_ms;
            let end_in_range =
                event.ended_ms >= since && event.ended_ms < until && event.reason.is_some();
            if end_in_range {
                *ended.entry(event.track.clone()).or_default() += 1;
            }
            if end_in_range && event.reason == Some(ListenEnd::Completed) {
                entry.completed_listens += 1;
                result.completed_listens += 1;
            }
            let session =
                sessions
                    .entry(event.session.clone())
                    .or_insert_with(|| ListeningSession {
                        id: event.session.clone(),
                        started_ms: event.segments[0].started_ms,
                        ended_ms: event.segments.last().unwrap().ended_ms,
                        listened_ms: 0,
                        tags: Vec::new(),
                    });
            session.started_ms = session.started_ms.min(event.segments[0].started_ms);
            session.ended_ms = session
                .ended_ms
                .max(event.segments.last().unwrap().ended_ms);
            session.listened_ms += event.listened_ms;
            session
                .tags
                .extend(tracks[&event.track].tags.iter().cloned());
        }
        result.unique_tracks = loyalty.len();
        let mut track_shares = Vec::new();
        let mut artist_shares: BTreeMap<String, ListeningShare> = BTreeMap::new();
        let mut artist_ratings: HashMap<String, (u64, u64)> = HashMap::new();
        let mut tag_shares: BTreeMap<String, ListeningShare> = BTreeMap::new();
        let tag_tree: HashMap<_, _> = self
            .list_tags()?
            .into_iter()
            .map(|t| (normalize_tag(&t.name), t))
            .collect();
        let mut genre_totals: BTreeMap<String, u64> = BTreeMap::new();
        for (slug, entry) in &mut loyalty {
            let t = &tracks[slug];
            let completed_denominator = ended.get(slug).copied().unwrap_or(0);
            entry.ended_listens = completed_denominator;
            entry.completion_rate = (completed_denominator > 0)
                .then(|| entry.completed_listens as f64 / completed_denominator as f64);
            entry.replay_rate = Some(entry.listens.saturating_sub(1) as f64 / entry.listens as f64);
            entry.repeats_per_day =
                entry.listens.saturating_sub(1) as f64 / ((until - since) as f64 / DAY as f64);
            track_shares.push(ListeningShare {
                id: slug.clone(),
                name: t.metadata.title.clone(),
                listened_ms: entry.listened_ms,
            });
            let (_, artist, _) = self.storage.track_links(slug)?;
            if let Some(artist) = artist {
                let id = artist.id;
                artist_shares
                    .entry(id.clone())
                    .or_insert_with(|| ListeningShare {
                        id: id.clone(),
                        name: artist.name,
                        listened_ms: 0,
                    })
                    .listened_ms += entry.listened_ms;
                if let Some(rating) = t.rating {
                    let v = artist_ratings.entry(id).or_default();
                    v.0 += u64::from(rating);
                    v.1 += 1;
                }
            } else {
                result.artist_unknown_ms += entry.listened_ms;
            }
            for tag in &t.tags {
                let key = normalize_tag(tag);
                tag_shares
                    .entry(key.clone())
                    .or_insert_with(|| ListeningShare {
                        id: key,
                        name: tag.clone(),
                        listened_ms: 0,
                    })
                    .listened_ms += entry.listened_ms;
            }
            // Ignore redundant ancestor assignments; divide the track's time across genre leaves.
            let genres: Vec<_> = t
                .tags
                .iter()
                .filter(|name| TagName::parse_lossy(name).key.as_deref() == Some("genre"))
                .map(|name| normalize_tag(name))
                .collect();
            let mut ancestors = BTreeSet::new();
            for genre in &genres {
                let mut next = tag_tree
                    .get(genre)
                    .and_then(|t| t.parent.as_ref())
                    .map(|s| normalize_tag(s));
                while let Some(key) = next {
                    if !ancestors.insert(key.clone()) {
                        break;
                    }
                    next = tag_tree
                        .get(&key)
                        .and_then(|t| t.parent.as_ref())
                        .map(|s| normalize_tag(s));
                }
            }
            let mut leaves: Vec<_> = genres
                .into_iter()
                .filter(|g| !ancestors.contains(g))
                .collect();
            leaves.sort();
            leaves.dedup();
            if leaves.is_empty() {
                *genre_totals.entry("(unknown genre)".into()).or_default() += entry.listened_ms;
            }
            for (i, leaf) in leaves.iter().enumerate() {
                let share = entry.listened_ms / leaves.len() as u64
                    + u64::from((i as u64) < entry.listened_ms % leaves.len() as u64);
                let mut key = Some(leaf.clone());
                let mut visited = BTreeSet::new();
                while let Some(k) = key {
                    if !visited.insert(k.clone()) {
                        break;
                    }
                    *genre_totals.entry(k.clone()).or_default() += share;
                    key = tag_tree
                        .get(&k)
                        .and_then(|t| t.parent.as_ref())
                        .map(|s| normalize_tag(s));
                }
            }
        }
        let track_shares = ranked(track_shares);
        let all_artists = ranked(artist_shares.into_values().collect());
        let tag_shares = ranked(tag_shares.into_values().collect());
        result.most_listened_track = track_shares.first().cloned();
        result.most_listened_artist = all_artists.first().cloned();
        result.most_listened_tag = tag_shares.first().cloned();
        result.favorite_track = track_shares
            .iter()
            .filter(|s| tracks[&s.id].rating.is_some())
            .max_by(|a, b| {
                tracks[&a.id]
                    .rating
                    .cmp(&tracks[&b.id].rating)
                    .then(a.listened_ms.cmp(&b.listened_ms))
                    .then(b.id.cmp(&a.id))
            })
            .cloned();
        result.favorite_artist = all_artists
            .iter()
            .filter(|a| artist_ratings.contains_key(&a.id))
            .max_by(|a, b| {
                let (a_sum, a_count) = artist_ratings[&a.id];
                let (b_sum, b_count) = artist_ratings[&b.id];
                (a_sum * b_count)
                    .cmp(&(b_sum * a_count))
                    .then(a.listened_ms.cmp(&b.listened_ms))
                    .then(b.id.cmp(&a.id))
            })
            .cloned();
        result.artist_other_ms = all_artists
            .iter()
            .skip(query.top_n)
            .map(|a| a.listened_ms)
            .sum();
        result.artists = all_artists.into_iter().take(query.top_n).collect();
        result.genres = genre_totals
            .into_iter()
            .map(|(key, listened_ms)| {
                let tag = tag_tree.get(&key);
                GenreShare {
                    name: tag.map(|t| t.name.clone()).unwrap_or(key),
                    parent: tag.and_then(|t| t.parent.clone()),
                    listened_ms,
                }
            })
            .collect();
        result.loyalty = loyalty.into_values().collect();
        result.most_replayed_track = result
            .loyalty
            .iter()
            .filter(|l| l.listens > 1)
            .max_by(|a, b| {
                a.listens
                    .cmp(&b.listens)
                    .then(a.listened_ms.cmp(&b.listened_ms))
                    .then(b.track.cmp(&a.track))
            })
            .cloned();
        result.sessions = sessions
            .into_values()
            .map(|mut s| {
                s.tags.sort();
                s.tags.dedup();
                s
            })
            .collect();
        result
            .sessions
            .sort_by(|a, b| a.started_ms.cmp(&b.started_ms).then(a.id.cmp(&b.id)));
        result.longest_session = result
            .sessions
            .iter()
            .max_by(|a, b| a.listened_ms.cmp(&b.listened_ms).then(b.id.cmp(&a.id)))
            .cloned();
        result.daily = buckets(&events, DAY);
        let span = until - since;
        let width = if span <= 2 * DAY {
            3_600_000
        } else if span <= 90 * DAY {
            DAY
        } else {
            7 * DAY
        };
        result.activity_bucket_ms = width as u64;
        result.activity = buckets(&events, width);
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, rc::Rc};
    fn fixture() -> (Library, Rc<Cell<i64>>) {
        let clock = Rc::new(Cell::new(DAY - 1000));
        let c = clock.clone();
        let mut lib = Library::open_in_memory()
            .unwrap()
            .with_determinism(1, move || c.get());
        lib.import_scanned(&[
            ScannedTrack {
                path: "/a".into(),
                metadata: Metadata {
                    title: "A".into(),
                    artist: Some("Alice".into()),
                    duration_ms: Some(100_000),
                    ..Default::default()
                },
            },
            ScannedTrack {
                path: "/b".into(),
                metadata: Metadata {
                    title: "B".into(),
                    artist: Some("Bob".into()),
                    duration_ms: Some(100_000),
                    ..Default::default()
                },
            },
        ])
        .unwrap();
        (lib, clock)
    }
    fn event(
        lib: &mut Library,
        clock: &Cell<i64>,
        slug: &str,
        start: i64,
        duration: u64,
        reason: ListenEnd,
    ) {
        clock.set(start);
        let state = lib.play_track(slug).unwrap().state;
        let id = lib.begin_listening(state.start_seq).unwrap();
        clock.set(start + duration as i64);
        assert!(lib.report_listening(&id, duration).unwrap());
        lib.end_listening(&id, reason).unwrap();
    }
    #[test]
    fn measured_overview_buckets_distributions_and_loyalty_use_the_same_filters() {
        let (mut lib, clock) = fixture();
        lib.create_tag("genre:electronic", None).unwrap();
        lib.create_tag("genre:ambient", Some("genre:electronic"))
            .unwrap();
        lib.add_tags(
            "alice-a",
            &[
                "genre:electronic".into(),
                "genre:ambient".into(),
                "mood:calm".into(),
            ],
        )
        .unwrap();
        lib.set_rating("alice-a", Some(3)).unwrap();
        lib.set_rating("bob-b", Some(5)).unwrap();
        event(
            &mut lib,
            &clock,
            "alice-a",
            DAY - 1000,
            2000,
            ListenEnd::Completed,
        );
        event(
            &mut lib,
            &clock,
            "alice-a",
            DAY + 3000,
            1000,
            ListenEnd::Completed,
        );
        event(
            &mut lib,
            &clock,
            "bob-b",
            DAY + 5000,
            500,
            ListenEnd::Skipped,
        );
        clock.set(DAY + 6000);
        let query = AnalyticsQuery {
            since_ms: Some(DAY - 2000),
            until_ms: Some(DAY + 6000),
            top_n: 1,
            ..Default::default()
        };
        let result = lib.listening_analytics(&query).unwrap();
        assert_eq!(
            (
                result.listened_ms,
                result.unique_tracks,
                result.listens,
                result.completed_listens
            ),
            (3500, 2, 3, 2)
        );
        assert_eq!(result.most_listened_track.as_ref().unwrap().id, "alice-a");
        assert_eq!(result.favorite_track.as_ref().unwrap().id, "bob-b");
        assert_eq!(result.favorite_artist.as_ref().unwrap().name, "Bob");
        assert_eq!(result.artists.len(), 1);
        assert_eq!(result.artist_other_ms, 500);
        assert_eq!(
            result
                .daily
                .iter()
                .map(|d| d.listened_ms)
                .collect::<Vec<_>>(),
            [1000, 2500]
        );
        assert_eq!(
            result.activity.iter().map(|d| d.listened_ms).sum::<u64>(),
            3500
        );
        assert_eq!(result.longest_session.as_ref().unwrap().listened_ms, 3500);
        assert_eq!(result.sessions.len(), 1);
        assert!(result.sessions[0].tags.contains(&"mood:calm".into()));
        let loyalty = result
            .loyalty
            .iter()
            .find(|l| l.track == "alice-a")
            .unwrap();
        assert_eq!(loyalty.completion_rate, Some(1.0));
        assert_eq!(loyalty.replay_rate, Some(0.5));
        assert_eq!(loyalty.completed_listens, 2);
        assert!(loyalty.repeats_per_day > 0.0);
        assert_eq!(result.most_replayed_track.unwrap().track, "alice-a");
        let genre = |name| {
            result
                .genres
                .iter()
                .find(|g| g.name == name)
                .unwrap()
                .listened_ms
        };
        assert_eq!(genre("genre:ambient"), 3000);
        assert_eq!(
            genre("genre:electronic"),
            3000,
            "redundant parent assignment is not double counted"
        );
        assert_eq!(genre("(unknown genre)"), 500);
        let excluded = AnalyticsQuery {
            filter: FilterSpec {
                tag_exclude: vec!["genre:electronic".into()],
                ..Default::default()
            },
            ..query.clone()
        };
        let result = lib.listening_analytics(&excluded).unwrap();
        assert_eq!(
            (
                result.listened_ms,
                result.unique_tracks,
                result.completed_listens
            ),
            (500, 1, 0)
        );
        assert_eq!(lib.listening_events(&excluded).unwrap().len(), 1);
        let clipped = lib
            .listening_analytics(&AnalyticsQuery {
                since_ms: Some(DAY - 500),
                until_ms: Some(DAY + 500),
                ..query
            })
            .unwrap();
        assert_eq!(clipped.listened_ms, 1000);
        assert_eq!(
            clipped.completed_listens, 0,
            "completion outside range does not count"
        );
        assert_eq!(clipped.loyalty[0].completion_rate, None);
    }
    #[test]
    fn queue_time_seeks_and_pauses_are_not_invented_as_listening() {
        let (mut lib, clock) = fixture();
        let state = lib.play_track("alice-a").unwrap().state;
        clock.set(DAY + 5000);
        assert_eq!(
            lib.listening_analytics(&AnalyticsQuery::default())
                .unwrap()
                .listened_ms,
            0
        );
        let id = lib.begin_listening(state.start_seq).unwrap();
        clock.set(DAY + 6000);
        lib.report_listening(&id, 1000).unwrap();
        lib.pause().unwrap();
        clock.set(DAY + 60_000);
        lib.report_listening(&id, 1000).unwrap();
        lib.seek(std::time::Duration::from_secs(90)).unwrap();
        lib.resume().unwrap();
        clock.set(DAY + 61_000);
        lib.report_listening(&id, 2000).unwrap();
        lib.end_listening(&id, ListenEnd::Stopped).unwrap();
        clock.set(DAY + 62_000);
        let result = lib.listening_analytics(&AnalyticsQuery::default()).unwrap();
        assert_eq!(result.listened_ms, 2000);
        assert_eq!(result.completed_listens, 0);
        assert!(lib.begin_listening(state.start_seq + 1).is_err());
        let paused_range = AnalyticsQuery {
            since_ms: Some(DAY + 7000),
            until_ms: Some(DAY + 59_000),
            ..Default::default()
        };
        let paused = lib.listening_analytics(&paused_range).unwrap();
        assert_eq!(paused.listened_ms, 0);
        assert_eq!(paused.unique_tracks, 0);
    }
}
