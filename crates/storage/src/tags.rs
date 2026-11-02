//! The tag vocabulary: creation, the optional parent and assignments to tracks, artists, albums
//! and playlists.

use rusqlite::{OptionalExtension, Transaction, params};
use weave_core::{ChildPolicy, EntityKind, Error, Result, Tag, TagName, normalize_tag};

use crate::{Storage, db_err};

const TAG_SELECT: &str = "SELECT g.name, g.key, g.value, p.name,
        (SELECT COUNT(*) FROM track_tags tt WHERE tt.tag_id = g.id),
        (SELECT COUNT(*) FROM artist_tags tt WHERE tt.tag_id = g.id),
        (SELECT COUNT(*) FROM album_tags tt WHERE tt.tag_id = g.id),
        (SELECT COUNT(*) FROM playlist_tags tt WHERE tt.tag_id = g.id)
     FROM tags g LEFT JOIN tags p ON p.id = g.parent_id";

fn tag_row(r: &rusqlite::Row) -> rusqlite::Result<Tag> {
    Ok(Tag {
        name: r.get(0)?,
        key: r.get(1)?,
        value: r.get(2)?,
        parent: r.get(3)?,
        tracks: r.get::<_, i64>(4)? as usize,
        artists: r.get::<_, i64>(5)? as usize,
        albums: r.get::<_, i64>(6)? as usize,
        playlists: r.get::<_, i64>(7)? as usize,
    })
}

/// The tag with this name, created (as a root) when missing.
pub(crate) fn ensure_tag(tx: &Transaction, name: &TagName) -> Result<i64> {
    let norm = name.normalized();
    tx.execute(
        "INSERT OR IGNORE INTO tags (name, normalized_name, key, value)
         VALUES (?1, ?2, ?3, ?4)",
        params![name.display(), norm, name.key, name.value],
    )
    .map_err(db_err)?;
    tx.query_row(
        "SELECT id FROM tags WHERE normalized_name = ?1",
        [norm],
        |r| r.get(0),
    )
    .map_err(db_err)
}

/// The assignment table of a kind of thing and its key column.
fn assignments(kind: EntityKind) -> (&'static str, &'static str) {
    match kind {
        EntityKind::Track => ("track_tags", "track_id"),
        EntityKind::Artist => ("artist_tags", "artist_id"),
        EntityKind::Album => ("album_tags", "album_id"),
        EntityKind::Playlist => ("playlist_tags", "playlist_id"),
    }
}

/// Assign a tag to something (by internal key). Returns whether the assignment is new.
pub(crate) fn assign(
    tx: &Transaction,
    kind: EntityKind,
    owner: &str,
    name: &TagName,
) -> Result<bool> {
    let (table, col) = assignments(kind);
    let tag_id = ensure_tag(tx, name)?;
    tx.execute(
        &format!("INSERT OR IGNORE INTO {table} ({col}, tag_id) VALUES (?1, ?2)"),
        params![owner, tag_id],
    )
    .map(|n| n > 0)
    .map_err(db_err)
}

fn tag_id(tx: &Transaction, name: &str) -> Result<i64> {
    tx.query_row(
        "SELECT id FROM tags WHERE normalized_name = ?1",
        [normalize_tag(name)],
        |r| r.get(0),
    )
    .optional()
    .map_err(db_err)?
    .ok_or_else(|| Error::NotFound(format!("tag {}", name.trim())))
}

impl Storage {
    /// Add tags to a track (deduplicated, case-insensitive). Returns how many were newly applied.
    pub fn add_tags(&mut self, track_id: &str, names: &[String]) -> Result<usize> {
        self.add_tags_to(EntityKind::Track, track_id, names)
    }

    /// Remove tag assignments from a track. Returns how many were removed.
    pub fn remove_tags(&mut self, track_id: &str, names: &[String]) -> Result<usize> {
        self.remove_tags_from(EntityKind::Track, track_id, names)
    }

    /// A track's tags, in display spelling.
    pub fn tags(&self, track_id: &str) -> Result<Vec<String>> {
        self.tags_of(EntityKind::Track, track_id)
    }

    /// The internal key of a track, artist, album or playlist given its public ID.
    pub(crate) fn resolve_entity(&self, kind: EntityKind, key: &str) -> Result<String> {
        match kind {
            EntityKind::Track => self.resolve_id(key),
            EntityKind::Artist => self.resolve_artist(key),
            EntityKind::Album => self.resolve_album(key),
            EntityKind::Playlist => self.playlist(key).map(|p| p.id),
        }
    }

    /// Add tags to a track, artist, album or playlist. Returns how many were newly applied.
    pub fn add_tags_to(&mut self, kind: EntityKind, id: &str, names: &[String]) -> Result<usize> {
        let names = names
            .iter()
            .map(|n| TagName::parse(n))
            .collect::<Result<Vec<_>>>()?;
        let owner = self.resolve_entity(kind, id)?;
        let tx = self.conn.transaction().map_err(db_err)?;
        let mut added = 0;
        for name in &names {
            added += usize::from(assign(&tx, kind, &owner, name)?);
        }
        tx.commit().map_err(db_err)?;
        Ok(added)
    }

    pub fn remove_tags_from(
        &mut self,
        kind: EntityKind,
        id: &str,
        names: &[String],
    ) -> Result<usize> {
        let (table, col) = assignments(kind);
        let owner = self.resolve_entity(kind, id)?;
        let tx = self.conn.transaction().map_err(db_err)?;
        let mut removed = 0;
        for name in names {
            removed += tx
                .execute(
                    &format!(
                        "DELETE FROM {table} WHERE {col} = ?1 AND tag_id IN
                           (SELECT id FROM tags WHERE normalized_name = ?2)"
                    ),
                    params![owner, normalize_tag(name)],
                )
                .map_err(db_err)?;
        }
        tx.commit().map_err(db_err)?;
        Ok(removed)
    }

    /// The tags on a track, artist, album or playlist, in display spelling.
    pub fn tags_of(&self, kind: EntityKind, id: &str) -> Result<Vec<String>> {
        let (table, col) = assignments(kind);
        let owner = self.resolve_entity(kind, id)?;
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT g.name FROM {table} tt JOIN tags g ON g.id = tt.tag_id
                 WHERE tt.{col} = ?1 ORDER BY g.normalized_name"
            ))
            .map_err(db_err)?;
        stmt.query_map([owner], |r| r.get(0))
            .map_err(db_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(db_err)
    }

    /// Create a tag, optionally under an existing parent.
    pub fn create_tag(&mut self, name: &str, parent: Option<&str>) -> Result<Tag> {
        let name = TagName::parse(name)?;
        let tx = self.conn.transaction().map_err(db_err)?;
        let exists = tx
            .query_row(
                "SELECT 1 FROM tags WHERE normalized_name = ?1",
                [name.normalized()],
                |_| Ok(()),
            )
            .optional()
            .map_err(db_err)?
            .is_some();
        if exists {
            return Err(Error::AlreadyExists(format!("tag {}", name.display())));
        }
        let parent_id = parent.map(|p| tag_id(&tx, p)).transpose()?;
        let id = ensure_tag(&tx, &name)?;
        tx.execute(
            "UPDATE tags SET parent_id = ?1 WHERE id = ?2",
            params![parent_id, id],
        )
        .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        self.tag(&name.display())
    }

    pub fn tag(&self, name: &str) -> Result<Tag> {
        self.conn
            .query_row(
                &format!("{TAG_SELECT} WHERE g.normalized_name = ?1"),
                [normalize_tag(name)],
                tag_row,
            )
            .optional()
            .map_err(db_err)?
            .ok_or_else(|| Error::NotFound(format!("tag {}", name.trim())))
    }

    /// All tags by name, with their parents and direct track counts.
    pub fn list_tags(&self) -> Result<Vec<Tag>> {
        let mut stmt = self
            .conn
            .prepare(&format!("{TAG_SELECT} ORDER BY g.normalized_name"))
            .map_err(db_err)?;
        stmt.query_map([], tag_row)
            .map_err(db_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(db_err)
    }

    /// Set or clear (`None`) a tag's parent. Rejects self-parenting and any cycle; assignments
    /// are untouched, only what `has(parent)` reaches changes.
    pub fn set_tag_parent(&mut self, name: &str, parent: Option<&str>) -> Result<Tag> {
        let tx = self.conn.transaction().map_err(db_err)?;
        let id = tag_id(&tx, name)?;
        let parent_id = parent.map(|p| tag_id(&tx, p)).transpose()?;
        if let Some(pid) = parent_id {
            // Walk from the new parent up to its root: the tag must not be on that path.
            let cycle = tx
                .query_row(
                    "WITH RECURSIVE up(id) AS (
                         SELECT ?1
                         UNION
                         SELECT t.parent_id FROM tags t JOIN up ON t.id = up.id
                         WHERE t.parent_id IS NOT NULL)
                     SELECT 1 FROM up WHERE id = ?2",
                    params![pid, id],
                    |_| Ok(()),
                )
                .optional()
                .map_err(db_err)?
                .is_some();
            if cycle {
                let what = if pid == id {
                    "a tag cannot be its own parent"
                } else {
                    "that parent is a descendant of the tag, which would make a cycle"
                };
                return Err(Error::InvalidInput(what.into()));
            }
        }
        tx.execute(
            "UPDATE tags SET parent_id = ?1 WHERE id = ?2",
            params![parent_id, id],
        )
        .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        self.tag(name)
    }

    /// Delete a tag and its assignments. `Detach` turns its children into roots; `Delete` also
    /// deletes every descendant. Returns how many tags were deleted.
    pub fn delete_tag(&mut self, name: &str, children: ChildPolicy) -> Result<usize> {
        let tx = self.conn.transaction().map_err(db_err)?;
        let id = tag_id(&tx, name)?;
        let deleted = match children {
            ChildPolicy::Detach => {
                tx.execute(
                    "UPDATE tags SET parent_id = NULL WHERE parent_id = ?1",
                    [id],
                )
                .map_err(db_err)?;
                tx.execute("DELETE FROM tags WHERE id = ?1", [id])
                    .map_err(db_err)?
            }
            ChildPolicy::Delete => tx
                .execute(
                    "DELETE FROM tags WHERE id IN (
                         WITH RECURSIVE d(id) AS (
                             SELECT ?1
                             UNION
                             SELECT c.id FROM tags c JOIN d ON c.parent_id = d.id)
                         SELECT id FROM d)",
                    [id],
                )
                .map_err(db_err)?,
        };
        tx.commit().map_err(db_err)?;
        Ok(deleted)
    }
}
