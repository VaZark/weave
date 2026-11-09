//! Additive measured history. Opening an older library never fabricates elapsed time.
use crate::{Storage, db_err};
use rusqlite::{Connection, OptionalExtension, params};
use weave_player_core::{Error, ListenEnd, ListeningEvent, ListeningSegment, Result};

pub(crate) fn ensure(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS listening_events (
            id TEXT PRIMARY KEY, session TEXT NOT NULL,
            track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
            start_seq INTEGER NOT NULL, started_ms INTEGER NOT NULL,
            ended_ms INTEGER NOT NULL, listened_ms INTEGER NOT NULL DEFAULT 0,
            reported_ms INTEGER NOT NULL DEFAULT 0, reason TEXT
         );
         CREATE INDEX IF NOT EXISTS listening_event_track ON listening_events(track_id);
         CREATE INDEX IF NOT EXISTS listening_event_start ON listening_events(started_ms, id);
         CREATE TABLE IF NOT EXISTS listening_segments (
            event_id TEXT NOT NULL REFERENCES listening_events(id) ON DELETE CASCADE,
            started_ms INTEGER NOT NULL, ended_ms INTEGER NOT NULL, listened_ms INTEGER NOT NULL,
            PRIMARY KEY(event_id, ended_ms)
         );",
    )
    .map_err(db_err)
}

impl Storage {
    /// One output host owns the current event. Replacement hosts end abandoned events at their
    /// last report; no wall time between shutdown/crash and restart is added.
    pub fn begin_listening(&mut self, track: &str, start_seq: u64, now: i64) -> Result<String> {
        if now < 0 {
            return Err(Error::InvalidInput("negative listening timestamp".into()));
        }
        let track = self.resolve_id(track)?;
        let tx = self.conn.transaction().map_err(db_err)?;
        let existing: Option<String> = tx.query_row(
            "SELECT id FROM listening_events WHERE track_id = ?1 AND start_seq = ?2 AND reason IS NULL", params![track, start_seq as i64], |r| r.get(0)).optional().map_err(db_err)?;
        if let Some(id) = existing {
            return Ok(id);
        }
        let prior: Option<(String, i64)> = tx
            .query_row(
                "SELECT e.session, COALESCE((SELECT MAX(s.ended_ms) FROM listening_segments s WHERE s.event_id = e.id), e.started_ms) AS last FROM listening_events e ORDER BY e.started_ms DESC, e.id DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(db_err)?;
        let session = prior
            .filter(|(_, last)| now >= *last && now - *last < 1_800_000)
            .map(|(session, _)| session)
            .unwrap_or_else(|| format!("session-{}", uuid::Uuid::new_v4().simple()));
        tx.execute(
            "UPDATE listening_events SET reason = 'interrupted' WHERE reason IS NULL",
            [],
        )
        .map_err(db_err)?;
        let id = format!("listen-{}", uuid::Uuid::new_v4().simple());
        tx.execute("INSERT INTO listening_events (id, session, track_id, start_seq, started_ms, ended_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            params![id, session, track, start_seq as i64, now]).map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(id)
    }

    /// Cumulative audible wall milliseconds measured by the host's monotonic clock.
    /// Duplicate/stale reports add nothing; elapsed wall time caps new time, never a seek offset.
    pub fn report_listening(&mut self, id: &str, now: i64, cumulative_ms: u64) -> Result<bool> {
        if cumulative_ms > i64::MAX as u64 {
            return Err(Error::InvalidInput("listening duration too large".into()));
        }
        let tx = self.conn.transaction().map_err(db_err)?;
        let prior: Option<(i64, i64)> = tx.query_row(
            "SELECT ended_ms, reported_ms FROM listening_events WHERE id = ?1 AND reason IS NULL", [id],
            |r| Ok((r.get(0)?, r.get(1)?))).optional().map_err(db_err)?;
        let Some((last, reported)) = prior else {
            return Ok(false);
        };
        if now <= last || cumulative_ms <= reported as u64 {
            return Ok(false);
        }
        let delta = (cumulative_ms - reported as u64).min((now - last) as u64);
        if delta > 0 {
            tx.execute("INSERT INTO listening_segments (event_id, started_ms, ended_ms, listened_ms) VALUES (?1, ?2, ?3, ?4)",
                params![id, now - delta as i64, now, delta as i64]).map_err(db_err)?;
        }
        tx.execute("UPDATE listening_events SET ended_ms = ?2, reported_ms = ?3, listened_ms = listened_ms + ?4 WHERE id = ?1",
            params![id, now, cumulative_ms as i64, delta as i64]).map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(true)
    }

    /// End at the known transport endpoint, retaining only measured listening duration.
    pub fn end_listening(&mut self, id: &str, reason: ListenEnd, now: i64) -> Result<()> {
        self.conn
            .execute(
                "UPDATE listening_events SET reason = ?2, ended_ms = MAX(ended_ms, ?3) WHERE id = ?1 AND reason IS NULL",
                params![id, reason.as_str(), now],
            )
            .map_err(db_err)?;
        Ok(())
    }

    /// A newly acquired audio host interrupts the previous owner's abandoned event.
    pub fn recover_listening(&mut self) -> Result<()> {
        self.conn
            .execute(
                "UPDATE listening_events SET reason = 'interrupted' WHERE reason IS NULL",
                [],
            )
            .map_err(db_err)?;
        Ok(())
    }

    pub fn listening_tracking_since(&self) -> Result<Option<i64>> {
        self.conn
            .query_row("SELECT MIN(started_ms) FROM listening_events", [], |r| {
                r.get(0)
            })
            .map_err(db_err)
    }

    /// Events overlapping a half-open range. Original event bounds are kept; segments and
    /// listened_ms are clipped, so paused gaps contribute zero even within long events.
    pub fn listening_events(&self, since: i64, until: i64) -> Result<Vec<ListeningEvent>> {
        if since < 0 || until <= since {
            return Err(Error::InvalidInput("invalid listening range".into()));
        }
        let mut stmt = self
            .conn
            .prepare(
                "SELECT e.id, e.session, t.slug, e.started_ms, e.ended_ms, e.reason
             FROM listening_events e JOIN tracks t ON t.id = e.track_id
             WHERE e.started_ms < ?2 AND e.ended_ms >= ?1 ORDER BY e.started_ms, e.id",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map(params![since, until], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, Option<String>>(5)?,
                ))
            })
            .map_err(db_err)?;
        let mut events = Vec::new();
        for row in rows {
            let (id, session, track, started_ms, ended_ms, reason) = row.map_err(db_err)?;
            let mut segments = self.conn.prepare("SELECT MAX(started_ms, ?2), MIN(ended_ms, ?3) FROM listening_segments WHERE event_id = ?1 AND ended_ms > ?2 AND started_ms < ?3 ORDER BY started_ms").map_err(db_err)?;
            let segments: Vec<ListeningSegment> = segments
                .query_map(params![id, since, until], |r| {
                    let (start, end): (i64, i64) = (r.get(0)?, r.get(1)?);
                    Ok(ListeningSegment {
                        started_ms: start,
                        ended_ms: end,
                        listened_ms: (end - start) as u64,
                    })
                })
                .map_err(db_err)?
                .collect::<std::result::Result<_, _>>()
                .map_err(db_err)?;
            let reason = reason
                .map(|r| r.parse::<ListenEnd>().map_err(Error::Storage))
                .transpose()?;
            let listened_ms = segments.iter().map(|s| s.listened_ms).sum();
            events.push(ListeningEvent {
                id,
                session,
                track,
                started_ms,
                ended_ms,
                listened_ms,
                reason,
                segments,
            });
        }
        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use weave_player_core::{EntityKind, Metadata, ScannedTrack};
    fn library() -> Storage {
        let mut s = Storage::open_in_memory().unwrap();
        s.import_tracks(&[
            ScannedTrack {
                path: "/a".into(),
                metadata: Metadata {
                    title: "A".into(),
                    artist: Some("X".into()),
                    ..Default::default()
                },
            },
            ScannedTrack {
                path: "/b".into(),
                metadata: Metadata {
                    title: "B".into(),
                    artist: Some("X".into()),
                    ..Default::default()
                },
            },
        ])
        .unwrap();
        s
    }
    #[test]
    fn reports_are_idempotent_ranges_clip_segments_and_pauses_are_empty() {
        let mut s = library();
        let id = s.begin_listening("x-a", 1, 1000).unwrap();
        assert!(s.report_listening(&id, 2000, 1000).unwrap());
        assert!(!s.report_listening(&id, 2000, 1000).unwrap());
        assert!(!s.report_listening(&id, 999, 500).unwrap());
        // A repeated paused report doesn't move the event's endpoint.
        assert!(!s.report_listening(&id, 90_000, 1000).unwrap());
        s.report_listening(&id, 100_000, 2000).unwrap();
        s.end_listening(&id, ListenEnd::Completed, 100_000).unwrap();
        s.end_listening(&id, ListenEnd::Error, 101_000).unwrap();
        assert!(!s.report_listening(&id, 101_000, 3000).unwrap());
        let all = s.listening_events(0, 200_000).unwrap();
        assert_eq!(all[0].reason, Some(ListenEnd::Completed));
        assert_eq!(all[0].listened_ms, 2000);
        assert_eq!(s.listening_events(2000, 99_000).unwrap()[0].listened_ms, 0);
        assert_eq!(
            s.listening_events(1500, 99_500).unwrap()[0].listened_ms,
            1000
        );
        assert_eq!(
            s.listening_events(100_000, 200_000).unwrap()[0].listened_ms,
            0
        );
        assert!(s.listening_events(0, 1000).unwrap().is_empty());
        assert!(s.listening_events(2, 1).is_err());
    }
    #[test]
    fn restart_marks_abandoned_event_and_new_session_after_inactivity() {
        let mut s = library();
        let first = s.begin_listening("x-a", 1, 1000).unwrap();
        s.report_listening(&first, 2000, 1000).unwrap();
        let next = s.begin_listening("x-b", 2, 3000).unwrap();
        s.report_listening(&next, 4000, 1000).unwrap();
        let events = s.listening_events(0, 5000).unwrap();
        assert_eq!(events[0].reason, Some(ListenEnd::Interrupted));
        assert_eq!(events[0].session, events[1].session);
        assert!(!s.report_listening(&first, 5000, 4000).unwrap());
        let third = s.begin_listening("x-a", 3, 2_000_000).unwrap();
        s.report_listening(&third, 2_001_000, 1000).unwrap();
        let events = s.listening_events(0, 3_000_000).unwrap();
        assert_ne!(events[1].session, events[2].session);
        assert_eq!(events[1].listened_ms, 1000);
    }
    #[test]
    fn explicit_merges_preserve_measured_segments_and_legacy_history() {
        let mut s = library();
        let id = s.begin_listening("x-a", 1, 1000).unwrap();
        s.report_listening(&id, 2000, 1000).unwrap();
        s.record_play("x-a", 1).unwrap();
        s.merge_duplicates(EntityKind::Track, "x-a", "x-b").unwrap();
        let events = s.listening_events(0, 5000).unwrap();
        assert_eq!(events[0].track, "x-b");
        assert_eq!(events[0].listened_ms, 1000);
        assert_eq!(s.play_history().unwrap()[0].track, "x-b");
    }
    #[test]
    fn open_adds_measured_tables_without_backfilling_start_counts() {
        let mut s = library();
        s.record_play("x-a", 100).unwrap();
        s.conn
            .execute_batch("DROP TABLE listening_segments; DROP TABLE listening_events;")
            .unwrap();
        ensure(&s.conn).unwrap();
        assert!(s.listening_events(0, 200_000).unwrap().is_empty());
        assert_eq!(s.play_history().unwrap()[0].plays, 1);
    }
    #[test]
    fn persisted_events_survive_reopen_and_repeated_begins_reuse_the_active_event() {
        let dir =
            std::env::temp_dir().join(format!("weave-listening-{}", uuid::Uuid::new_v4().simple()));
        let path = dir.join("library.db");
        let id;
        {
            let mut s = Storage::open(&path).unwrap();
            s.import_tracks(&[ScannedTrack {
                path: "/song".into(),
                metadata: Metadata {
                    title: "Song".into(),
                    ..Default::default()
                },
            }])
            .unwrap();
            id = s.begin_listening("unknown-song", 1, 1000).unwrap();
            s.report_listening(&id, 2000, 1000).unwrap();
            assert_eq!(s.begin_listening("unknown-song", 1, 3000).unwrap(), id);
        }
        let mut s = Storage::open(&path).unwrap();
        s.recover_listening().unwrap();
        assert!(!s.report_listening(&id, 50_000, 49_000).unwrap());
        let replacement = s.begin_listening("unknown-song", 1, 50_000).unwrap();
        assert_ne!(replacement, id);
        s.report_listening(&replacement, 51_000, 1000).unwrap();
        let events = s.listening_events(0, 100_000).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].reason, Some(ListenEnd::Interrupted));
        assert_eq!(events.iter().map(|e| e.listened_ms).sum::<u64>(), 2000);
        drop(s);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
