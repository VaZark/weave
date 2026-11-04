//! Dynamic playlists (saved filters) and manual playlists (ordered track lists).

use rusqlite::{OptionalExtension, params, types::Value};
use weave_player_core::{
    Dependent, EntityRef, Error, FilterSpec, Page, Playlist, PlaylistKind, Result, SearchResult,
    normalize_tag, playlist_slug, slugify,
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
pub(crate) fn renumber(tx: &rusqlite::Transaction, playlist_id: &str) -> Result<()> {
    let ids = ordered_track_ids(tx, playlist_id)?;
    write_order(tx, playlist_id, &ids)
}

/// Playlists may draw on playlists that draw on playlists, this many levels deep.
const MAX_SOURCE_DEPTH: usize = 8;

/// A source with a limit is bound as a list of tracks; this many at most.
const MAX_LIMITED_SOURCE: usize = 500;

fn too_deep() -> Error {
    Error::InvalidInput(format!(
        "playlists draw on each other more than {MAX_SOURCE_DEPTH} levels deep"
    ))
}

const COLUMNS: &str = "id, name, filter_spec, description, kind";

impl Storage {
    pub fn create_playlist(
        &mut self,
        name: &str,
        description: Option<&str>,
        filter: &FilterSpec,
    ) -> Result<Playlist> {
        let mut filter = filter.clone().normalized();
        filter.validate()?;
        // A new playlist has no ID yet, so nothing can draw on it: no loop is possible.
        filter.sources = self.canonical_sources(&filter, None)?;
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

    /// Save a playlist's description and filter. Returns the filter as saved, with its sources
    /// replaced by the playlists' IDs. Sources must exist and must not lead back to `id`.
    pub fn update_playlist(
        &mut self,
        id: &str,
        description: Option<&str>,
        filter: &FilterSpec,
    ) -> Result<FilterSpec> {
        let mut filter = filter.clone().normalized();
        filter.validate()?;
        filter.sources = self.canonical_sources(&filter, Some(id))?;
        self.conn
            .execute(
                "UPDATE playlists SET description = ?1, filter_spec = ?2 WHERE id = ?3",
                params![description, spec_json(&filter)?, id],
            )
            .map_err(db_err)?;
        Ok(filter)
    }

    /// Change a playlist's name. Its ID (the slug) stays as it was: it is assigned once, and
    /// other playlists that draw on this one, and everything else that refers to it, keep
    /// working. The new name must be new (case-insensitively) and not empty.
    pub fn rename_playlist(&mut self, name: &str, new_name: &str) -> Result<Playlist> {
        let mut p = self.playlist(name)?;
        let new_name = new_name.trim();
        if new_name.is_empty() {
            return Err(Error::InvalidInput("playlist name cannot be empty".into()));
        }
        let taken = self
            .conn
            .query_row(
                "SELECT 1 FROM playlists WHERE normalized_name = ?1 AND id != ?2",
                params![normalize_tag(new_name), p.id],
                |_| Ok(()),
            )
            .optional()
            .map_err(db_err)?
            .is_some();
        if taken {
            return Err(Error::AlreadyExists(format!("playlist \"{new_name}\"")));
        }
        self.conn
            .execute(
                "UPDATE playlists SET name = ?1, normalized_name = ?2 WHERE id = ?3",
                params![new_name, normalize_tag(new_name), p.id],
            )
            .map_err(db_err)?;
        p.name = new_name.into();
        Ok(p)
    }

    /// The dynamic playlists that draw on this playlist.
    pub fn playlist_dependents(&self, name: &str) -> Result<Vec<Dependent>> {
        let p = self.playlist(name)?;
        Ok(self
            .playlists()?
            .into_iter()
            .filter(|q| q.kind == PlaylistKind::Dynamic && q.filter.sources.contains(&p.id))
            .map(|q| Dependent {
                only_source: q.filter.sources.len() == 1,
                playlist: EntityRef {
                    id: q.id,
                    name: q.name,
                },
            })
            .collect())
    }

    /// Delete a playlist (its tracks and tags with it). If other playlists draw on it this is
    /// refused unless `detach` says to remove it from their sources; returns those playlists.
    pub fn delete_playlist(&mut self, name: &str, detach: bool) -> Result<Vec<EntityRef>> {
        let p = self.playlist(name)?;
        let dependents = self.playlist_dependents(name)?;
        if !dependents.is_empty() && !detach {
            let names: Vec<String> = dependents
                .iter()
                .map(|d| format!("\"{}\"", d.playlist.name))
                .collect();
            return Err(Error::InvalidInput(format!(
                "{} draw(s) on \"{}\"; deleting it means removing it from their sources",
                names.join(", "),
                p.name
            )));
        }
        // Read what is needed before the transaction borrows the connection.
        let mut changes = Vec::new();
        for d in dependents {
            let mut filter = self.playlist(&d.playlist.id)?.filter;
            filter.sources.retain(|s| s != &p.id);
            changes.push((d.playlist, spec_json(&filter)?));
        }
        let mut detached = Vec::new();
        let tx = self.conn.transaction().map_err(db_err)?;
        for (playlist, json) in changes {
            tx.execute(
                "UPDATE playlists SET filter_spec = ?1 WHERE id = ?2",
                params![json, playlist.id],
            )
            .map_err(db_err)?;
            detached.push(playlist);
        }
        tx.execute("DELETE FROM playlists WHERE id = ?1", [&p.id])
            .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(detached)
    }

    /// The sources of a filter as playlist IDs. Each must exist, and none may be `owner` or lead
    /// back to it through its own sources.
    fn canonical_sources(&self, filter: &FilterSpec, owner: Option<&str>) -> Result<Vec<String>> {
        let mut ids: Vec<String> = Vec::new();
        for name in &filter.sources {
            let p = self.playlist(name)?;
            if owner == Some(p.id.as_str()) {
                return Err(Error::InvalidInput(
                    "a playlist cannot draw on itself".into(),
                ));
            }
            if let Some(owner) = owner
                && self.draws_on(&p.id, owner, 0)?
            {
                return Err(Error::InvalidInput(format!(
                    "\"{}\" already draws on this playlist, so this would make a loop",
                    p.name
                )));
            }
            if !ids.contains(&p.id) {
                ids.push(p.id);
            }
        }
        Ok(ids)
    }

    /// Does the dynamic playlist `from` draw on `target`, directly or through its sources?
    fn draws_on(&self, from: &str, target: &str, depth: usize) -> Result<bool> {
        if depth > MAX_SOURCE_DEPTH {
            return Err(too_deep());
        }
        let p = self.playlist(from)?;
        for s in &p.filter.sources {
            if s == target || self.draws_on(s, target, depth + 1)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// ` AND (…)` limiting a search to the filter's source playlists (any one of them), with the
    /// values to bind. Empty when there are no sources.
    ///
    /// A manual playlist limits to its tracks. A dynamic one without a limit is inlined as its own
    /// conditions, sources included. One with a limit contributes exactly the tracks it yields.
    pub(crate) fn source_terms(
        &self,
        filter: &FilterSpec,
        depth: usize,
    ) -> Result<(String, Vec<Value>)> {
        if filter.sources.is_empty() {
            return Ok((String::new(), Vec::new()));
        }
        if depth > MAX_SOURCE_DEPTH {
            return Err(too_deep());
        }
        let mut terms: Vec<String> = Vec::new();
        let mut args: Vec<Value> = Vec::new();
        for name in &filter.sources {
            let p = self.playlist(name)?;
            match (p.kind, p.filter.limit) {
                (PlaylistKind::Manual, _) => {
                    terms.push(
                        "m.track_id IN (SELECT track_id FROM playlist_tracks WHERE playlist_id = ?)"
                            .into(),
                    );
                    args.push(Value::Text(p.id));
                }
                (PlaylistKind::Dynamic, None) => {
                    let (conditions, own) = query::conditions(&p.filter);
                    let (nested, more) = self.source_terms(&p.filter, depth + 1)?;
                    terms.push(format!("(1=1{conditions}{nested})"));
                    args.extend(own);
                    args.extend(more);
                }
                (PlaylistKind::Dynamic, Some(_)) => {
                    let slugs: Vec<String> = self
                        .search(&p.filter, Page::ALL)?
                        .tracks
                        .into_iter()
                        .map(|t| t.id.0)
                        .collect();
                    if slugs.len() > MAX_LIMITED_SOURCE {
                        return Err(Error::InvalidInput(format!(
                            "playlist \"{}\" has a limit above {MAX_LIMITED_SOURCE}, too many to draw on",
                            p.name
                        )));
                    }
                    if slugs.is_empty() {
                        terms.push("0".into());
                    } else {
                        terms.push(format!("t.slug IN ({})", vec!["?"; slugs.len()].join(",")));
                        args.extend(slugs.into_iter().map(Value::Text));
                    }
                }
            }
        }
        Ok((format!(" AND ({})", terms.join(" OR ")), args))
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
