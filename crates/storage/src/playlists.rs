//! Dynamic playlists (saved filters) and manual playlists (ordered track lists).

use rusqlite::{OptionalExtension, params};
use weave_core::{
    Error, FilterSpec, Page, Playlist, PlaylistKind, Result, SearchResult, normalize_tag,
    playlist_slug, slugify,
};

use crate::{Storage, db_err, query, unique_slug, window};

fn spec_json(f: &FilterSpec) -> Result<String> {
    serde_json::to_string(f).map_err(|e| Error::Storage(e.to_string()))
}

fn row_to_playlist(r: &rusqlite::Row) -> rusqlite::Result<Result<Playlist>> {
    let kind: String = r.get(4)?;
    let json: String = r.get(2)?;
    let (id, name, description) = (r.get(0)?, r.get(1)?, r.get(3)?);
    Ok((|| {
        let filter = serde_json::from_str::<FilterSpec>(&json)
            .map_err(|e| Error::Storage(format!("corrupt filter_spec: {e}")))?;
        let kind = kind
            .parse::<PlaylistKind>()
            .map_err(|e| Error::Storage(format!("corrupt playlist kind: {e}")))?;
        Ok(Playlist {
            id,
            name,
            description,
            kind,
            filter,
        })
    })())
}

fn ordered_track_ids(tx: &rusqlite::Transaction, playlist_id: &str) -> Result<Vec<String>> {
    let mut stmt = tx
        .prepare("SELECT track_id FROM playlist_tracks WHERE playlist_id = ?1 ORDER BY play_order")
        .map_err(db_err)?;
    stmt.query_map([playlist_id], |r| r.get(0))
        .map_err(db_err)?
        .collect::<std::result::Result<_, _>>()
        .map_err(db_err)
}

/// Rewrite the play order as 0..n in the given sequence.
fn write_order(tx: &rusqlite::Transaction, playlist_id: &str, ids: &[String]) -> Result<()> {
    for (order, id) in ids.iter().enumerate() {
        tx.execute(
            "UPDATE playlist_tracks SET play_order = ?1 WHERE playlist_id = ?2 AND track_id = ?3",
            params![order as i64, playlist_id, id],
        )
        .map_err(db_err)?;
    }
    Ok(())
}

/// Close the gaps left by removals, keeping the relative order.
fn renumber(tx: &rusqlite::Transaction, playlist_id: &str) -> Result<()> {
    let ids = ordered_track_ids(tx, playlist_id)?;
    write_order(tx, playlist_id, &ids)
}

const COLUMNS: &str = "id, name, filter_spec, description, kind";

impl Storage {
    pub fn create_playlist(
        &mut self,
        name: &str,
        description: Option<&str>,
        filter: &FilterSpec,
    ) -> Result<Playlist> {
        let filter = filter.clone().normalized();
        filter.validate()?;
        self.insert_playlist(name, description, PlaylistKind::Dynamic, filter)
    }

    pub fn create_manual_playlist(
        &mut self,
        name: &str,
        description: Option<&str>,
    ) -> Result<Playlist> {
        self.insert_playlist(
            name,
            description,
            PlaylistKind::Manual,
            FilterSpec::default(),
        )
    }

    fn insert_playlist(
        &mut self,
        name: &str,
        description: Option<&str>,
        kind: PlaylistKind,
        filter: FilterSpec,
    ) -> Result<Playlist> {
        let name = name.trim();
        let description = description.map(str::trim).filter(|d| !d.is_empty());
        if name.is_empty() {
            return Err(Error::InvalidInput("playlist name cannot be empty".into()));
        }
        let tx = self.conn.transaction().map_err(db_err)?;
        let exists = tx
            .query_row(
                "SELECT 1 FROM playlists WHERE normalized_name = ?1",
                [normalize_tag(name)],
                |_| Ok(()),
            )
            .optional()
            .map_err(db_err)?
            .is_some();
        if exists {
            return Err(Error::AlreadyExists(format!("playlist \"{name}\"")));
        }
        // The slug is the playlist's public ID (the `id` column).
        let id = unique_slug(&tx, "playlists", "id", &playlist_slug(name))?;
        tx.execute(
            "INSERT INTO playlists (id, name, normalized_name, description, kind, filter_spec)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                id,
                name,
                normalize_tag(name),
                description,
                kind.as_str(),
                spec_json(&filter)?
            ],
        )
        .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(Playlist {
            id,
            name: name.into(),
            description: description.map(Into::into),
            kind,
            filter,
        })
    }

    /// Look up by name (case-insensitive) or by slug ID (normalized like `slugify`).
    pub fn playlist(&self, name: &str) -> Result<Playlist> {
        self.conn
            .query_row(
                &format!(
                    "SELECT {COLUMNS} FROM playlists
                     WHERE normalized_name = ?1 OR id = ?2
                     ORDER BY normalized_name = ?1 DESC LIMIT 1"
                ),
                params![normalize_tag(name), slugify(name)],
                row_to_playlist,
            )
            .optional()
            .map_err(db_err)?
            .ok_or_else(|| Error::NotFound(format!("playlist \"{name}\"")))?
    }

    pub fn playlists(&self) -> Result<Vec<Playlist>> {
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT {COLUMNS} FROM playlists ORDER BY normalized_name"
            ))
            .map_err(db_err)?;
        let rows = stmt.query_map([], row_to_playlist).map_err(db_err)?;
        rows.map(|r| r.map_err(db_err)?).collect()
    }

    pub fn update_playlist(
        &mut self,
        id: &str,
        description: Option<&str>,
        filter: &FilterSpec,
    ) -> Result<()> {
        filter.validate()?;
        self.conn
            .execute(
                "UPDATE playlists SET description = ?1, filter_spec = ?2 WHERE id = ?3",
                params![description, spec_json(filter)?, id],
            )
            .map_err(db_err)?;
        Ok(())
    }

    /// Resolve a name or slug to a manual playlist's ID.
    fn require_manual(&self, name: &str) -> Result<String> {
        let p = self.playlist(name)?;
        if p.kind != PlaylistKind::Manual {
            return Err(Error::InvalidInput(format!(
                "playlist \"{}\" is dynamic; its tracks come from its filters",
                p.name
            )));
        }
        Ok(p.id)
    }

    /// Append tracks to a manual playlist, skipping ones already in it. Returns how many were added.
    pub fn playlist_add(&mut self, playlist: &str, track_ids: &[String]) -> Result<usize> {
        let playlist_id = &self.require_manual(playlist)?;
        let internal = track_ids
            .iter()
            .map(|id| self.resolve_id(id))
            .collect::<Result<Vec<_>>>()?;
        let tx = self.conn.transaction().map_err(db_err)?;
        // The play order is contiguous (0..n), so the next free number is the count.
        let mut next: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM playlist_tracks WHERE playlist_id = ?1",
                [playlist_id],
                |r| r.get(0),
            )
            .map_err(db_err)?;
        let mut added = 0;
        for id in &internal {
            let n = tx
                .execute(
                    "INSERT OR IGNORE INTO playlist_tracks (playlist_id, track_id, play_order)
                     VALUES (?1, ?2, ?3)",
                    params![playlist_id, id, next],
                )
                .map_err(db_err)?;
            next += n as i64;
            added += n;
        }
        tx.commit().map_err(db_err)?;
        Ok(added)
    }

    /// Remove tracks from a manual playlist. Returns how many were removed.
    pub fn playlist_remove(&mut self, playlist: &str, track_ids: &[String]) -> Result<usize> {
        let playlist_id = &self.require_manual(playlist)?;
        let internal = track_ids
            .iter()
            .map(|id| self.resolve_id(id))
            .collect::<Result<Vec<_>>>()?;
        let tx = self.conn.transaction().map_err(db_err)?;
        let mut removed = 0;
        for id in &internal {
            removed += tx
                .execute(
                    "DELETE FROM playlist_tracks WHERE playlist_id = ?1 AND track_id = ?2",
                    params![playlist_id, id],
                )
                .map_err(db_err)?;
        }
        renumber(&tx, playlist_id)?;
        tx.commit().map_err(db_err)?;
        Ok(removed)
    }

    /// Move one track of a manual playlist to another place in its play order (0-based).
    pub fn playlist_move(&mut self, playlist: &str, from: usize, to: usize) -> Result<()> {
        let playlist_id = &self.require_manual(playlist)?;
        let tx = self.conn.transaction().map_err(db_err)?;
        let mut ids = ordered_track_ids(&tx, playlist_id)?;
        for p in [from, to] {
            if p >= ids.len() {
                return Err(Error::InvalidInput(format!(
                    "the playlist has no position {} ({} track(s))",
                    p + 1,
                    ids.len()
                )));
            }
        }
        let moved = ids.remove(from);
        ids.insert(to, moved);
        write_order(&tx, playlist_id, &ids)?;
        tx.commit().map_err(db_err)
    }

    /// A manual playlist's tracks in their play order.
    pub fn manual_playlist_tracks(&self, playlist_id: &str, page: Page) -> Result<SearchResult> {
        let total: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM playlist_tracks WHERE playlist_id = ?1",
                [playlist_id],
                |r| r.get(0),
            )
            .map_err(db_err)?;
        let total = total as usize;
        let (offset, limit) = window(total, page);
        if limit == 0 {
            return Ok(SearchResult {
                total,
                tracks: Vec::new(),
            });
        }
        let sql = format!(
            "SELECT {}{} JOIN playlist_tracks p ON p.track_id = m.track_id
             WHERE p.playlist_id = ? ORDER BY p.play_order LIMIT ? OFFSET ?",
            query::COLUMNS,
            query::FROM
        );
        let tracks = self.fetch(
            &sql,
            vec![
                playlist_id.to_string().into(),
                (limit as i64).into(),
                (offset as i64).into(),
            ],
        )?;
        Ok(SearchResult { total, tracks })
    }
}
