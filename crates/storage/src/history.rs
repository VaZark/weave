//! Listening history: one row per time a track started playing.
//!
//! The table is created when the library is opened (`CREATE TABLE IF NOT EXISTS`), outside the
//! squashed schema of `migrations`, so a library made before the history existed keeps working:
//! nothing is recorded for the past, and it fills as tracks are played.

use rusqlite::Connection;
use weave_player_core::{PlayStat, Result};

use crate::{Storage, db_err};

/// Run when a library is opened; harmless to run again.
pub(crate) fn ensure(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS play_history (
             track_id  TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
             played_at INTEGER NOT NULL        -- unix seconds
         );
         CREATE INDEX IF NOT EXISTS play_history_track ON play_history(track_id);",
    )
    .map_err(db_err)
}

impl Storage {
    /// Record that the track with this slug started playing at `at` (unix seconds). A slug that
    /// is not in the library records nothing.
    pub fn record_play(&mut self, slug: &str, at: i64) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO play_history (track_id, played_at)
                 SELECT id, ?2 FROM tracks WHERE slug = ?1",
                rusqlite::params![slug, at],
            )
            .map_err(db_err)?;
        Ok(())
    }

    /// The history of every track that was played: how often and when last, most recent first.
    pub fn play_history(&self) -> Result<Vec<PlayStat>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT t.slug, COUNT(*), MAX(h.played_at) AS last
                 FROM play_history h JOIN tracks t ON t.id = h.track_id
                 GROUP BY t.id ORDER BY last DESC, t.slug",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |r| {
                Ok(PlayStat {
                    track: r.get(0)?,
                    plays: r.get(1)?,
                    last_played_at: r.get(2)?,
                })
            })
            .map_err(db_err)?;
        rows.collect::<std::result::Result<_, _>>().map_err(db_err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use weave_player_core::{Metadata, ScannedTrack};

    fn track(title: &str) -> ScannedTrack {
        ScannedTrack {
            path: format!("/m/{title}.mp3"),
            metadata: Metadata {
                title: title.into(),
                artist: Some("x".into()),
                ..Default::default()
            },
        }
    }

    #[test]
    fn plays_are_counted_per_track_and_unknown_tracks_record_nothing() {
        let mut st = Storage::open_in_memory().unwrap();
        st.import_tracks(&[track("a"), track("b")]).unwrap();
        st.record_play("x-a", 100).unwrap();
        st.record_play("x-a", 300).unwrap();
        st.record_play("x-b", 200).unwrap();
        st.record_play("x-nothing", 400).unwrap();
        let h = st.play_history().unwrap();
        assert_eq!(h.len(), 2);
        // Most recent first.
        assert_eq!(
            (h[0].track.as_str(), h[0].plays, h[0].last_played_at),
            ("x-a", 2, 300)
        );
        assert_eq!(
            (h[1].track.as_str(), h[1].plays, h[1].last_played_at),
            ("x-b", 1, 200)
        );
    }

    #[test]
    fn a_library_made_before_the_history_gains_the_table_when_opened() {
        let dir = std::env::temp_dir().join(format!("weave-history-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("old.db");
        {
            let mut st = Storage::open(&path).unwrap();
            st.import_tracks(&[track("a")]).unwrap();
            // As if it was made by a version without the history.
            st.conn.execute("DROP TABLE play_history", []).unwrap();
        }
        let mut st = Storage::open(&path).unwrap();
        assert!(st.play_history().unwrap().is_empty());
        st.record_play("x-a", 5).unwrap();
        assert_eq!(st.play_history().unwrap()[0].plays, 1);
        drop(st);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
