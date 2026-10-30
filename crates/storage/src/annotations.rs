//! User annotations: favorites and ratings. Kept apart from source-derived metadata.

use rusqlite::params;
use weave_core::{Error, Result};

use crate::{Storage, db_err};

impl Storage {
    /// Mark tracks as favorite (or not). Returns how many actually changed.
    /// Every ID is resolved first, so an unknown ID changes nothing.
    pub fn set_favorite(&mut self, ids: &[String], favorite: bool) -> Result<usize> {
        let internal = ids
            .iter()
            .map(|id| self.resolve_id(id))
            .collect::<Result<Vec<_>>>()?;
        let tx = self.conn.transaction().map_err(db_err)?;
        let mut changed = 0;
        for id in &internal {
            tx.execute(
                "INSERT OR IGNORE INTO track_annotations (track_id) VALUES (?1)",
                [id],
            )
            .map_err(db_err)?;
            changed += tx
                .execute(
                    "UPDATE track_annotations SET favorite = ?1 WHERE track_id = ?2 AND favorite != ?1",
                    params![favorite, id],
                )
                .map_err(db_err)?;
        }
        tx.commit().map_err(db_err)?;
        Ok(changed)
    }

    /// Set a 1 to 5 rating, or clear it with `None`.
    pub fn set_rating(&mut self, id: &str, rating: Option<u8>) -> Result<()> {
        if let Some(r) = rating
            && !(1..=5).contains(&r)
        {
            return Err(Error::InvalidInput("rating must be between 1 and 5".into()));
        }
        let internal = self.resolve_id(id)?;
        let tx = self.conn.transaction().map_err(db_err)?;
        tx.execute(
            "INSERT OR IGNORE INTO track_annotations (track_id) VALUES (?1)",
            [&internal],
        )
        .map_err(db_err)?;
        tx.execute(
            "UPDATE track_annotations SET rating = ?1 WHERE track_id = ?2",
            params![rating, internal],
        )
        .map_err(db_err)?;
        tx.commit().map_err(db_err)
    }
}
