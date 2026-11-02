//! Duplicates, one mechanism for artists, albums and tracks: find things that look alike, let
//! the user merge them or say they are different, and remember that. Nothing is ever merged
//! on its own; similarity only produces questions.

use std::collections::{HashMap, HashSet};

use rusqlite::{Connection, params};
use weave_core::{
    DedupMark, DuplicateCandidate, EntityKind, Error, MergeStats, Result, SimilarPair, entity_norm,
    loose_key,
};

use crate::{Storage, db_err};

/// Tracks whose lengths are within this many milliseconds count as the same recording.
const SAME_LENGTH_MS: u64 = 3000;

/// One artist, album or track as a candidate for being someone's duplicate.
struct Item {
    rowid: i64,
    /// Internal key.
    key: String,
    id: String,
    name: String,
    detail: String,
    /// Only items with the same group can look alike.
    group: String,
    /// Which to keep first: more tracks, then the older.
    weight: usize,
    /// Albums and tracks also need their artists to look alike (`None` is unknown).
    artist: Option<String>,
    duration_ms: Option<u64>,
}

fn ordered(a: &str, b: &str) -> (String, String) {
    if a < b {
        (a.into(), b.into())
    } else {
        (b.into(), a.into())
    }
}

/// Record that two things are not duplicates. Takes a connection or a transaction.
pub(crate) fn mark_different(conn: &Connection, kind: EntityKind, a: &str, b: &str) -> Result<()> {
    let (lo, hi) = ordered(a, b);
    conn.execute(
        "INSERT OR IGNORE INTO not_duplicates (kind, a_id, b_id) VALUES (?1, ?2, ?3)",
        params![kind.as_str(), lo, hi],
    )
    .map_err(db_err)?;
    Ok(())
}

fn forget(conn: &Connection, kind: EntityKind, key: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM not_duplicates WHERE kind = ?1 AND (a_id = ?2 OR b_id = ?2)",
        params![kind.as_str(), key],
    )
    .map_err(db_err)?;
    Ok(())
}

fn length(ms: Option<u64>) -> String {
    ms.map_or_else(
        || "unknown length".into(),
        |ms| format!("{}:{:02}", ms / 60_000, ms / 1000 % 60),
    )
}

impl Storage {
    /// Where the library stands now: take it before an import to tell afterwards what it added.
    pub fn dedup_mark(&self) -> Result<DedupMark> {
        let max = |table: &str| -> Result<i64> {
            self.conn
                .query_row(
                    &format!("SELECT COALESCE(MAX(rowid), 0) FROM {table}"),
                    [],
                    |r| r.get(0),
                )
                .map_err(db_err)
        };
        Ok(DedupMark {
            artists: max("artists")?,
            albums: max("albums")?,
            tracks: max("tracks")?,
        })
    }

    fn artist_items(&self) -> Result<Vec<Item>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT ar.rowid, ar.id, ar.slug, ar.name,
                        (SELECT COUNT(*) FROM tracks t WHERE t.artist_id = ar.id)
                 FROM artists ar",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |r| {
                let name: String = r.get(3)?;
                let tracks = r.get::<_, i64>(4)? as usize;
                Ok(Item {
                    rowid: r.get(0)?,
                    key: r.get(1)?,
                    id: r.get(2)?,
                    detail: format!("{tracks} tracks"),
                    group: loose_key(&name),
                    name,
                    weight: tracks,
                    artist: None,
                    duration_ms: None,
                })
            })
            .map_err(db_err)?;
        rows.collect::<std::result::Result<_, _>>().map_err(db_err)
    }

    fn album_items(&self) -> Result<Vec<Item>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT al.rowid, al.id, al.slug, al.title, ar.name,
                        (SELECT COUNT(*) FROM tracks t WHERE t.album_id = al.id)
                 FROM albums al LEFT JOIN artists ar ON ar.id = al.artist_id",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |r| {
                let title: String = r.get(3)?;
                let artist: Option<String> = r.get(4)?;
                let tracks = r.get::<_, i64>(5)? as usize;
                Ok(Item {
                    rowid: r.get(0)?,
                    key: r.get(1)?,
                    id: r.get(2)?,
                    detail: match &artist {
                        Some(a) => format!("by {a}, {tracks} tracks"),
                        None => format!("no artist, {tracks} tracks"),
                    },
                    group: loose_key(&title),
                    name: title,
                    weight: tracks,
                    artist: artist.map(|a| loose_key(&a)),
                    duration_ms: None,
                })
            })
            .map_err(db_err)?;
        rows.collect::<std::result::Result<_, _>>().map_err(db_err)
    }

    fn track_items(&self) -> Result<Vec<Item>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT t.rowid, t.id, t.slug, m.title, ar.name, m.duration_ms,
                        (SELECT path FROM sources s WHERE s.track_id = t.id ORDER BY s.rowid LIMIT 1)
                 FROM tracks t JOIN track_metadata m ON m.track_id = t.id
                 LEFT JOIN artists ar ON ar.id = t.artist_id",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |r| {
                let title: String = r.get(3)?;
                let artist: Option<String> = r.get(4)?;
                let duration: Option<i64> = r.get(5)?;
                let path: Option<String> = r.get(6)?;
                let duration = duration.map(|d| d as u64);
                let who = artist.clone().unwrap_or_else(|| "unknown artist".into());
                // Tracks need the same title and artist; the length is checked pair by pair.
                let group = format!(
                    "{}|{}",
                    loose_key(&title),
                    artist.as_deref().map(loose_key).unwrap_or_default()
                );
                Ok(Item {
                    rowid: r.get(0)?,
                    key: r.get(1)?,
                    id: r.get(2)?,
                    detail: format!(
                        "{who}, {}, {}",
                        length(duration),
                        path.unwrap_or_else(|| "no file".into())
                    ),
                    group,
                    name: title,
                    weight: 0,
                    artist: None,
                    duration_ms: duration,
                })
            })
            .map_err(db_err)?;
        rows.collect::<std::result::Result<_, _>>().map_err(db_err)
    }

    fn not_duplicates(&self, kind: EntityKind) -> Result<HashSet<(String, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT a_id, b_id FROM not_duplicates WHERE kind = ?1")
            .map_err(db_err)?;
        let rows = stmt
            .query_map([kind.as_str()], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(db_err)?;
        rows.collect::<std::result::Result<_, _>>().map_err(db_err)
    }

    /// Things of `kind` that look alike, minus the pairs the user said are different; with
    /// `since`, only pairs involving something added after that mark. Suggestions only.
    ///
    /// - Artists look alike when their names are the same apart from case, spacing,
    ///   punctuation and a leading "the", which includes the very same name (people share names).
    /// - Albums, when their titles do and their artists do (or one is unknown).
    /// - Tracks, when title and artist do and the lengths are within three seconds (or unknown).
    pub fn duplicates(
        &self,
        kind: EntityKind,
        since: Option<&DedupMark>,
    ) -> Result<Vec<SimilarPair>> {
        let (items, newer_than) = match kind {
            EntityKind::Artist => (self.artist_items()?, since.map(|m| m.artists)),
            EntityKind::Album => (self.album_items()?, since.map(|m| m.albums)),
            EntityKind::Track => (self.track_items()?, since.map(|m| m.tracks)),
            EntityKind::Playlist => {
                return Err(Error::InvalidInput(
                    "playlists are not checked for duplicates".into(),
                ));
            }
        };
        let different = self.not_duplicates(kind)?;
        let mut groups: HashMap<String, Vec<Item>> = HashMap::new();
        for item in items
            .into_iter()
            .filter(|i| !i.group.trim_matches('|').is_empty())
        {
            groups.entry(item.group.clone()).or_default().push(item);
        }
        let candidate = |i: &Item| DuplicateCandidate {
            id: i.id.clone(),
            name: i.name.clone(),
            detail: i.detail.clone(),
        };
        let mut out = Vec::new();
        for mut group in groups.into_values().filter(|g| g.len() > 1) {
            group.sort_by(|x, y| y.weight.cmp(&x.weight).then(x.rowid.cmp(&y.rowid)));
            for (i, a) in group.iter().enumerate() {
                for b in &group[i + 1..] {
                    if newer_than.is_some_and(|m| a.rowid <= m && b.rowid <= m) {
                        continue;
                    }
                    if different.contains(&ordered(&a.key, &b.key)) {
                        continue;
                    }
                    let alike_artist = match (&a.artist, &b.artist) {
                        (Some(x), Some(y)) => x == y,
                        _ => true,
                    };
                    let alike_length = match (a.duration_ms, b.duration_ms) {
                        (Some(x), Some(y)) => x.abs_diff(y) <= SAME_LENGTH_MS,
                        _ => true,
                    };
                    if !alike_artist || !alike_length {
                        continue;
                    }
                    let same = entity_norm(&a.name) == entity_norm(&b.name);
                    let reason = match (kind, same) {
                        (EntityKind::Track, true) => "same title and artist, about the same length",
                        (EntityKind::Track, false) => {
                            "titles differ only in case or punctuation, about the same length"
                        }
                        (_, true) => "same name",
                        (_, false) => "names differ only in case, spacing or punctuation",
                    };
                    out.push(SimilarPair {
                        kind,
                        a: candidate(a),
                        b: candidate(b),
                        reason: reason.into(),
                    });
                }
            }
        }
        out.sort_by(|x, y| (&x.a.name, &x.a.id, &x.b.id).cmp(&(&y.a.name, &y.a.id, &y.b.id)));
        Ok(out)
    }

    /// Fold `from` into `into` (the thing to keep). Artists: tracks, featured credits, albums
    /// and tags move. Albums: tracks and tags move. Tracks: files, tags, a missing rating,
    /// featured credits, queue spots and playlist places move.
    pub fn merge_duplicates(
        &mut self,
        kind: EntityKind,
        from: &str,
        into: &str,
    ) -> Result<MergeStats> {
        let (from_key, into_key) = (
            self.resolve_entity(kind, from)?,
            self.resolve_entity(kind, into)?,
        );
        if from_key == into_key {
            return Err(Error::InvalidInput(format!(
                "cannot merge a {kind} into itself"
            )));
        }
        let stats = match kind {
            EntityKind::Artist => self.merge_artists(from, into)?,
            EntityKind::Album => self.merge_albums(&from_key, &into_key)?,
            EntityKind::Track => self.merge_tracks(&from_key, &into_key)?,
            EntityKind::Playlist => {
                return Err(Error::InvalidInput("playlists cannot be merged".into()));
            }
        };
        forget(&self.conn, kind, &from_key)?;
        Ok(stats)
    }

    /// Record that two things are different, so they are not suggested as duplicates again.
    pub fn separate_duplicates(&mut self, kind: EntityKind, a: &str, b: &str) -> Result<()> {
        if kind == EntityKind::Playlist {
            return Err(Error::InvalidInput(
                "playlists are not checked for duplicates".into(),
            ));
        }
        let (a, b) = (self.resolve_entity(kind, a)?, self.resolve_entity(kind, b)?);
        if a == b {
            return Err(Error::InvalidInput(format!("that is one {kind}, not two")));
        }
        mark_different(&self.conn, kind, &a, &b)
    }

    fn merge_albums(&mut self, from: &str, into: &str) -> Result<MergeStats> {
        let into_title: String = self
            .conn
            .query_row("SELECT title FROM albums WHERE id = ?1", [into], |r| {
                r.get(0)
            })
            .map_err(db_err)?;
        let tx = self.conn.transaction().map_err(db_err)?;
        tx.execute(
            "UPDATE track_metadata SET album = ?1, album_norm = ?2
             WHERE track_id IN (SELECT id FROM tracks WHERE album_id = ?3)",
            params![into_title, into_title.to_lowercase(), from],
        )
        .map_err(db_err)?;
        let moved = tx
            .execute(
                "UPDATE tracks SET album_id = ?1 WHERE album_id = ?2",
                params![into, from],
            )
            .map_err(db_err)?;
        tx.execute(
            "INSERT OR IGNORE INTO album_tags (album_id, tag_id)
             SELECT ?1, tag_id FROM album_tags WHERE album_id = ?2",
            params![into, from],
        )
        .map_err(db_err)?;
        tx.execute("DELETE FROM albums WHERE id = ?1", [from])
            .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(MergeStats {
            tracks: moved,
            albums: 1,
        })
    }

    fn merge_tracks(&mut self, from: &str, into: &str) -> Result<MergeStats> {
        let tx = self.conn.transaction().map_err(db_err)?;
        // The other file stays attached, after the ones the kept track already has.
        tx.execute(
            "UPDATE sources SET track_id = ?1 WHERE track_id = ?2",
            params![into, from],
        )
        .map_err(db_err)?;
        tx.execute(
            "INSERT OR IGNORE INTO track_tags (track_id, tag_id)
             SELECT ?1, tag_id FROM track_tags WHERE track_id = ?2",
            params![into, from],
        )
        .map_err(db_err)?;
        tx.execute(
            "INSERT OR IGNORE INTO track_annotations (track_id) VALUES (?1)",
            [into],
        )
        .map_err(db_err)?;
        tx.execute(
            "UPDATE track_annotations SET rating =
               (SELECT rating FROM track_annotations WHERE track_id = ?2)
             WHERE track_id = ?1 AND rating IS NULL",
            params![into, from],
        )
        .map_err(db_err)?;
        tx.execute(
            "UPDATE OR IGNORE track_featured SET track_id = ?1 WHERE track_id = ?2",
            params![into, from],
        )
        .map_err(db_err)?;
        tx.execute(
            "UPDATE queue_items SET track_id = ?1 WHERE track_id = ?2",
            params![into, from],
        )
        .map_err(db_err)?;
        // Playlists: a place held by the removed track becomes the kept one's, unless the
        // playlist already has it; orders are renumbered afterwards.
        let playlists: Vec<String> = {
            let mut stmt = tx
                .prepare("SELECT DISTINCT playlist_id FROM playlist_tracks WHERE track_id = ?1")
                .map_err(db_err)?;
            stmt.query_map([from], |r| r.get(0))
                .map_err(db_err)?
                .collect::<std::result::Result<_, _>>()
                .map_err(db_err)?
        };
        tx.execute(
            "DELETE FROM playlist_tracks WHERE track_id = ?1 AND playlist_id IN
               (SELECT playlist_id FROM playlist_tracks WHERE track_id = ?2)",
            params![from, into],
        )
        .map_err(db_err)?;
        tx.execute(
            "UPDATE playlist_tracks SET track_id = ?1 WHERE track_id = ?2",
            params![into, from],
        )
        .map_err(db_err)?;
        for playlist in &playlists {
            crate::playlists::renumber(&tx, playlist)?;
        }
        tx.execute("DELETE FROM tracks WHERE id = ?1", [from])
            .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(MergeStats {
            tracks: 1,
            albums: 0,
        })
    }
}
