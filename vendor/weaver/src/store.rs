//! The tag vocabulary in SQLite: creation, the optional parent and assignments to owners.

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::error::db_err;
use crate::{ChildPolicy, Error, Result, Tag, TagName, normalize_tag};

/// One kind of thing that can carry tags, and where its assignments live.
///
/// The assignment table `table` has the columns `col` (the owner's key) and `tag_id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Owner {
    /// Short label, e.g. `track`; names this kind in [`Tag::counts`].
    pub name: &'static str,
    /// The assignment table, e.g. `track_tags`.
    pub table: &'static str,
    /// The owner's key column in the assignment table, e.g. `track_id`.
    pub col: &'static str,
    /// The table of the owners, e.g. `tracks`, and its key column, e.g. `id`.
    pub owner_table: &'static str,
    pub owner_key: &'static str,
    /// The SQL type of that key, e.g. `TEXT`.
    pub key_type: &'static str,
}

/// The tag store over a set of owner kinds. Cheap to copy; holds no connection.
#[derive(Debug, Clone, Copy)]
pub struct Tags {
    owners: &'static [Owner],
}

impl Tags {
    pub const fn new(owners: &'static [Owner]) -> Self {
        Self { owners }
    }

    /// DDL for the `tags` table and one assignment table per owner. Run it once, inside the
    /// application's own schema setup (foreign keys to the owner tables are resolved lazily).
    pub fn schema_sql(&self) -> String {
        let mut sql = String::from(
            "CREATE TABLE tags (
                 id              INTEGER PRIMARY KEY,
                 name            TEXT NOT NULL,        -- display spelling, `key:value`
                 normalized_name TEXT NOT NULL UNIQUE, -- lookup identity
                 key             TEXT,                 -- lowercase; NULL for an unkeyed tag
                 value           TEXT NOT NULL,
                 parent_id       INTEGER REFERENCES tags(id)
             );
             CREATE INDEX tags_parent ON tags(parent_id);
             CREATE INDEX tags_key ON tags(key);\n",
        );
        for o in self.owners {
            let Owner {
                table,
                col,
                owner_table,
                owner_key,
                key_type,
                ..
            } = o;
            sql += &format!(
                "CREATE TABLE {table} (
                     {col} {key_type} NOT NULL REFERENCES {owner_table}({owner_key}) ON DELETE CASCADE,
                     tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
                     PRIMARY KEY ({col}, tag_id)
                 );
                 CREATE INDEX {table}_tag ON {table}(tag_id, {col});\n"
            );
        }
        sql
    }

    fn select(&self) -> String {
        let counts: String = self
            .owners
            .iter()
            .map(|o| {
                format!(
                    ",\n(SELECT COUNT(*) FROM {} tt WHERE tt.tag_id = g.id)",
                    o.table
                )
            })
            .collect();
        format!(
            "SELECT g.name, g.key, g.value, p.name{counts}
             FROM tags g LEFT JOIN tags p ON p.id = g.parent_id"
        )
    }

    fn row(&self, r: &rusqlite::Row) -> rusqlite::Result<Tag> {
        let mut counts = Vec::new();
        for (i, o) in self.owners.iter().enumerate() {
            counts.push((o.name.to_string(), r.get::<_, i64>(4 + i)? as usize));
        }
        Ok(Tag {
            name: r.get(0)?,
            key: r.get(1)?,
            value: r.get(2)?,
            parent: r.get(3)?,
            counts,
        })
    }

    /// The tag with this name, created (as a root) when missing.
    pub fn ensure(&self, tx: &Transaction, name: &TagName) -> Result<i64> {
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

    /// Assign a tag to something (by its key). Returns whether the assignment is new.
    pub fn assign(&self, tx: &Transaction, o: &Owner, key: &str, name: &TagName) -> Result<bool> {
        let (table, col) = (o.table, o.col);
        let tag_id = self.ensure(tx, name)?;
        tx.execute(
            &format!("INSERT OR IGNORE INTO {table} ({col}, tag_id) VALUES (?1, ?2)"),
            params![key, tag_id],
        )
        .map(|n| n > 0)
        .map_err(db_err)
    }

    /// Add tags (parsed, deduplicated, case-insensitive) to something in one transaction.
    /// Returns how many were newly applied.
    pub fn add(
        &self,
        conn: &mut Connection,
        o: &Owner,
        key: &str,
        names: &[String],
    ) -> Result<usize> {
        let names = names
            .iter()
            .map(|n| TagName::parse(n))
            .collect::<Result<Vec<_>>>()?;
        let tx = conn.transaction().map_err(db_err)?;
        let mut added = 0;
        for name in &names {
            added += usize::from(self.assign(&tx, o, key, name)?);
        }
        tx.commit().map_err(db_err)?;
        Ok(added)
    }

    /// Remove tag assignments. Returns how many were removed.
    pub fn remove(
        &self,
        conn: &mut Connection,
        o: &Owner,
        key: &str,
        names: &[String],
    ) -> Result<usize> {
        let (table, col) = (o.table, o.col);
        let tx = conn.transaction().map_err(db_err)?;
        let mut removed = 0;
        for name in names {
            removed += tx
                .execute(
                    &format!(
                        "DELETE FROM {table} WHERE {col} = ?1 AND tag_id IN
                           (SELECT id FROM tags WHERE normalized_name = ?2)"
                    ),
                    params![key, normalize_tag(name)],
                )
                .map_err(db_err)?;
        }
        tx.commit().map_err(db_err)?;
        Ok(removed)
    }

    /// The tags on something, in display spelling.
    pub fn of(&self, conn: &Connection, o: &Owner, key: &str) -> Result<Vec<String>> {
        let (table, col) = (o.table, o.col);
        let mut stmt = conn
            .prepare(&format!(
                "SELECT g.name FROM {table} tt JOIN tags g ON g.id = tt.tag_id
                 WHERE tt.{col} = ?1 ORDER BY g.normalized_name"
            ))
            .map_err(db_err)?;
        stmt.query_map([key], |r| r.get(0))
            .map_err(db_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(db_err)
    }

    /// Give `to` every tag `from` has (a merge). Returns how many were newly applied.
    pub fn copy(&self, tx: &Transaction, o: &Owner, from: &str, to: &str) -> Result<usize> {
        let (table, col) = (o.table, o.col);
        tx.execute(
            &format!(
                "INSERT OR IGNORE INTO {table} ({col}, tag_id)
                 SELECT ?1, tag_id FROM {table} WHERE {col} = ?2"
            ),
            params![to, from],
        )
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

    /// Create a tag, optionally under an existing parent.
    pub fn create(&self, conn: &mut Connection, name: &str, parent: Option<&str>) -> Result<Tag> {
        let name = TagName::parse(name)?;
        let tx = conn.transaction().map_err(db_err)?;
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
        let parent_id = parent.map(|p| Self::tag_id(&tx, p)).transpose()?;
        let id = self.ensure(&tx, &name)?;
        tx.execute(
            "UPDATE tags SET parent_id = ?1 WHERE id = ?2",
            params![parent_id, id],
        )
        .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        self.get(conn, &name.display())
    }

    pub fn get(&self, conn: &Connection, name: &str) -> Result<Tag> {
        conn.query_row(
            &format!("{} WHERE g.normalized_name = ?1", self.select()),
            [normalize_tag(name)],
            |r| self.row(r),
        )
        .optional()
        .map_err(db_err)?
        .ok_or_else(|| Error::NotFound(format!("tag {}", name.trim())))
    }

    /// All tags by name, with their parents and direct counts.
    pub fn list(&self, conn: &Connection) -> Result<Vec<Tag>> {
        let mut stmt = conn
            .prepare(&format!("{} ORDER BY g.normalized_name", self.select()))
            .map_err(db_err)?;
        stmt.query_map([], |r| self.row(r))
            .map_err(db_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(db_err)
    }

    /// Set or clear (`None`) a tag's parent. Rejects self-parenting and any cycle; assignments
    /// are untouched, only what a non-exact filter reaches changes.
    pub fn set_parent(
        &self,
        conn: &mut Connection,
        name: &str,
        parent: Option<&str>,
    ) -> Result<Tag> {
        let tx = conn.transaction().map_err(db_err)?;
        let id = Self::tag_id(&tx, name)?;
        let parent_id = parent.map(|p| Self::tag_id(&tx, p)).transpose()?;
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
        self.get(conn, name)
    }

    /// Delete a tag and its assignments. `Detach` turns its children into roots; `Delete` also
    /// deletes every descendant. Returns how many tags were deleted.
    pub fn delete(
        &self,
        conn: &mut Connection,
        name: &str,
        children: ChildPolicy,
    ) -> Result<usize> {
        let tx = conn.transaction().map_err(db_err)?;
        let id = Self::tag_id(&tx, name)?;
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
