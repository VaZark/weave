//! Per-album, per-artist and per-playlist shuffle preferences: play this one shuffled, or in order.
//!
//! Like the listening history, the table is created when the library is opened, outside the
//! squashed schema, so a library made before it keeps working (nothing has a preference).

use rusqlite::{Connection, params};
use weave_player_core::{EntityKind, Error, Result, ShufflePref};

use crate::{Storage, db_err};

/// Run when a library is opened; harmless to run again.
pub(crate) fn ensure(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS shuffle_prefs (
             kind    TEXT NOT NULL,           -- album, artist or playlist
             entity  TEXT NOT NULL,           -- its slug
             shuffle INTEGER NOT NULL,        -- 1 shuffled, 0 in order
             PRIMARY KEY (kind, entity)
         );",
    )
    .map_err(db_err)
}

impl Storage {
    /// Set what to do when the album, artist or playlist with this slug is played: `Some(true)`
    /// shuffled, `Some(false)` in order, `None` to follow the player again. The slug is stored
    /// as given: the caller resolves it first.
    pub fn set_shuffle_pref(
        &mut self,
        kind: EntityKind,
        id: &str,
        shuffle: Option<bool>,
    ) -> Result<()> {
        if kind == EntityKind::Track {
            return Err(Error::InvalidInput(
                "shuffle is set for an album, an artist or a playlist".into(),
            ));
        }
        match shuffle {
            Some(on) => self.conn.execute(
                "INSERT INTO shuffle_prefs (kind, entity, shuffle) VALUES (?1, ?2, ?3)
                 ON CONFLICT (kind, entity) DO UPDATE SET shuffle = excluded.shuffle",
                params![kind.as_str(), id, on],
            ),
            None => self.conn.execute(
                "DELETE FROM shuffle_prefs WHERE kind = ?1 AND entity = ?2",
                params![kind.as_str(), id],
            ),
        }
        .map_err(db_err)?;
        Ok(())
    }

    /// Every preference that is set.
    pub fn shuffle_prefs(&self) -> Result<Vec<ShufflePref>> {
        let mut stmt = self
            .conn
            .prepare("SELECT kind, entity, shuffle FROM shuffle_prefs ORDER BY kind, entity")
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, bool>(2)?,
                ))
            })
            .map_err(db_err)?;
        let mut out = Vec::new();
        for row in rows {
            let (kind, id, shuffle) = row.map_err(db_err)?;
            // A kind this version does not know is left out rather than failing the lot.
            if let Ok(kind) = kind.parse::<EntityKind>() {
                out.push(ShufflePref { kind, id, shuffle });
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_preference_is_set_changed_and_cleared_and_never_for_a_track() {
        let mut st = Storage::open_in_memory().unwrap();
        assert!(st.shuffle_prefs().unwrap().is_empty());
        st.set_shuffle_pref(EntityKind::Playlist, "road-trip", Some(true))
            .unwrap();
        st.set_shuffle_pref(EntityKind::Album, "abbey-road", Some(false))
            .unwrap();
        // Setting it again replaces it.
        st.set_shuffle_pref(EntityKind::Playlist, "road-trip", Some(false))
            .unwrap();
        st.set_shuffle_pref(EntityKind::Playlist, "road-trip", Some(true))
            .unwrap();
        let prefs = st.shuffle_prefs().unwrap();
        assert_eq!(
            prefs,
            vec![
                ShufflePref {
                    kind: EntityKind::Album,
                    id: "abbey-road".into(),
                    shuffle: false
                },
                ShufflePref {
                    kind: EntityKind::Playlist,
                    id: "road-trip".into(),
                    shuffle: true
                },
            ]
        );
        st.set_shuffle_pref(EntityKind::Album, "abbey-road", None)
            .unwrap();
        assert_eq!(st.shuffle_prefs().unwrap().len(), 1);
        assert!(
            st.set_shuffle_pref(EntityKind::Track, "x", Some(true))
                .is_err()
        );
    }
}
