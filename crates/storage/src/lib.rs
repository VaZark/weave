//! SQLite persistence: migrations, the canonical library, playlists, annotations and the
//! persisted playback session.

mod annotations;
mod migrations;
mod playlists;
mod query;
mod session;

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use std::collections::HashMap;

use weave_core::{
    Error, FilterSpec, Metadata, Page, Result, ScannedTrack, SearchResult, Track, TrackId,
    normalize_tag, slugify, track_slug,
};

fn db_err(e: rusqlite::Error) -> Error {
    Error::Storage(e.to_string())
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImportStats {
    pub added: usize,
    pub skipped: usize,
}

/// Oldest SQLite this crate supports: `rusqlite`'s own floor. The SQL used here
/// needs nothing newer, but only the bundled version is tested in CI-style runs.
pub const MIN_SQLITE_VERSION: i32 = 3_014_000;

pub struct Storage {
    pub(crate) conn: Connection,
}

impl Storage {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| Error::Storage(e.to_string()))?;
        }
        Self::init(Connection::open(path).map_err(db_err)?)
            .map_err(|e| Error::Storage(format!("{e}: {}", path.display())))
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory().map_err(db_err)?)
    }

    fn init(mut conn: Connection) -> Result<Self> {
        let found = rusqlite::version_number();
        if found < MIN_SQLITE_VERSION {
            return Err(Error::Storage(format!(
                "SQLite {} is too old; Weave needs 3.14.0 or newer",
                rusqlite::version()
            )));
        }
        conn.pragma_update(None, "foreign_keys", true)
            .map_err(db_err)?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(db_err)?;
        migrations::run(&mut conn)?;
        Ok(Self { conn })
    }

    /// Insert tracks not yet known by `(backend_id, external_id)`; all-or-nothing.
    pub fn import_tracks(
        &mut self,
        backend_id: &str,
        tracks: &[ScannedTrack],
    ) -> Result<ImportStats> {
        let tx = self.conn.transaction().map_err(db_err)?;
        let mut stats = ImportStats::default();
        for t in tracks {
            let exists: bool = tx
                .query_row(
                    "SELECT 1 FROM sources WHERE backend_id = ?1 AND external_id = ?2",
                    params![backend_id, t.external_id],
                    |_| Ok(true),
                )
                .optional()
                .map_err(db_err)?
                .unwrap_or(false);
            if exists {
                stats.skipped += 1;
                continue;
            }
            let id = uuid::Uuid::new_v4().simple().to_string();
            let m = &t.metadata;
            let slug = unique_slug(
                &tx,
                "tracks",
                "slug",
                &track_slug(
                    m.artist.as_deref(),
                    m.album.as_deref(),
                    &m.title,
                    backend_id,
                ),
            )?;
            tx.execute(
                "INSERT INTO tracks (id, slug, added_at) VALUES (?1, ?2, CAST(strftime('%s', 'now') AS INTEGER))",
                [&id, &slug],
            )
            .map_err(db_err)?;
            insert_metadata(&tx, &id, &t.metadata)?;
            tx.execute(
                "INSERT INTO sources (id, track_id, backend_id, external_id) VALUES (?1, ?2, ?3, ?4)",
                params![uuid::Uuid::new_v4().simple().to_string(), id, backend_id, t.external_id],
            )
            .map_err(db_err)?;
            stats.added += 1;
        }
        tx.commit().map_err(db_err)?;
        Ok(stats)
    }

    /// Matching tracks in the filter's sort order (default: artist, album, title), one
    /// page at a time. `filter.limit` caps the matches before paging.
    pub fn search(&self, filter: &FilterSpec, page: Page) -> Result<SearchResult> {
        let filter = filter.clone().normalized();
        filter.validate()?;
        let (where_sql, mut args) = query::where_clause(&filter);

        let counted: i64 = self
            .conn
            .query_row(
                &format!("SELECT COUNT(*){}{where_sql}", query::FROM),
                rusqlite::params_from_iter(args.iter()),
                |r| r.get(0),
            )
            .map_err(db_err)?;
        let total = filter.limit.map_or(counted, |l| counted.min(l as i64)) as usize;

        let (offset, limit) = window(total, page);
        if limit == 0 {
            return Ok(SearchResult {
                total,
                tracks: Vec::new(),
            });
        }
        let sql = format!(
            "SELECT {}{}{where_sql}{} LIMIT ? OFFSET ?",
            query::COLUMNS,
            query::FROM,
            query::order_by(filter.sort)
        );
        args.push((limit as i64).into());
        args.push((offset as i64).into());
        let tracks = self.fetch(&sql, args)?;
        Ok(SearchResult { total, tracks })
    }

    /// Resolve a slug (or internal key) to a track.
    pub fn track(&self, id: &str) -> Result<Track> {
        let id = self.resolve_id(id)?;
        let sql = format!(
            "SELECT {}{} WHERE m.track_id = ?",
            query::COLUMNS,
            query::FROM
        );
        self.fetch(&sql, vec![id.into()])?
            .pop()
            .ok_or_else(|| Error::NotFound("track".into()))
    }

    /// Tracks for exact slugs, in the order given (duplicates repeat). Unknown slugs are skipped.
    pub fn tracks_by_slugs(&self, slugs: &[String]) -> Result<Vec<Track>> {
        let mut found: HashMap<String, Track> = HashMap::new();
        for chunk in slugs.chunks(400) {
            let marks = vec!["?"; chunk.len()].join(",");
            let sql = format!(
                "SELECT {}{} WHERE t.slug IN ({marks})",
                query::COLUMNS,
                query::FROM
            );
            let args = chunk.iter().map(|s| s.clone().into()).collect();
            for t in self.fetch(&sql, args)? {
                found.insert(t.id.0.clone(), t);
            }
        }
        Ok(slugs.iter().filter_map(|s| found.get(s).cloned()).collect())
    }

    /// Accepts a slug (normalized, the public ID), or an internal key or unique prefix of one ( with `slugify`,
    /// so `Romeo Santos-Song-local` finds `romeo-santos-song-local`).
    /// Returns the internal key.
    pub(crate) fn resolve_id(&self, key: &str) -> Result<String> {
        let key = key.trim();
        if key.is_empty() {
            return Err(Error::InvalidInput("track id cannot be empty".into()));
        }
        let by_slug: Option<String> = self
            .conn
            .query_row(
                "SELECT id FROM tracks WHERE slug = ?1",
                [slugify(key)],
                |r| r.get(0),
            )
            .optional()
            .map_err(db_err)?;
        if let Some(id) = by_slug {
            return Ok(id);
        }
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM tracks WHERE substr(id, 1, length(?1)) = ?1 LIMIT 2")
            .map_err(db_err)?;
        let ids: Vec<String> = stmt
            .query_map([key], |r| r.get(0))
            .map_err(db_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(db_err)?;
        match ids.len() {
            0 => Err(Error::NotFound(format!("track {key}"))),
            1 => Ok(ids.into_iter().next().unwrap()),
            _ => Err(Error::InvalidInput(format!(
                "track id prefix \"{key}\" is ambiguous"
            ))),
        }
    }

    /// Run a `COLUMNS` query and attach tags with one extra query for the whole page.
    pub(crate) fn fetch(&self, sql: &str, args: Vec<rusqlite::types::Value>) -> Result<Vec<Track>> {
        let mut stmt = self.conn.prepare(sql).map_err(db_err)?;
        let mut rows: Vec<(String, Track)> = stmt
            .query_map(rusqlite::params_from_iter(args), |r| {
                // Column 0 is the internal key; the public ID is the slug (column 12).
                let internal: String = r.get(0)?;
                Ok((
                    internal,
                    Track {
                        id: TrackId(r.get(12)?),
                        metadata: Metadata {
                            title: r.get(1)?,
                            artist: r.get(2)?,
                            album: r.get(3)?,
                            album_artist: r.get(4)?,
                            genre: r.get(5)?,
                            composer: r.get(6)?,
                            year: r.get(7)?,
                            track_number: r.get(8)?,
                            disc_number: r.get(9)?,
                            duration_ms: r.get(10)?,
                            bpm: r.get(11)?,
                        },
                        tags: Vec::new(),
                        favorite: r.get::<_, i64>(13)? != 0,
                        rating: r.get(14)?,
                    },
                ))
            })
            .map_err(db_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(db_err)?;
        if rows.is_empty() {
            return Ok(Vec::new());
        }

        let marks = vec!["?"; rows.len()].join(",");
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT tt.track_id, g.name FROM track_tags tt JOIN tags g ON g.id = tt.tag_id
                 WHERE tt.track_id IN ({marks}) ORDER BY g.normalized_name"
            ))
            .map_err(db_err)?;
        let mut by_track: HashMap<String, Vec<String>> = HashMap::new();
        let tag_rows = stmt
            .query_map(
                rusqlite::params_from_iter(rows.iter().map(|(internal, _)| internal)),
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .map_err(db_err)?;
        for row in tag_rows {
            let (id, name) = row.map_err(db_err)?;
            by_track.entry(id).or_default().push(name);
        }
        Ok(rows
            .drain(..)
            .map(|(internal, mut t)| {
                t.tags = by_track.remove(&internal).unwrap_or_default();
                t
            })
            .collect())
    }

    /// Every source of a track as `(backend_id, external_id)`, oldest first.
    pub fn sources(&self, track_id: &str) -> Result<Vec<(String, String)>> {
        let track_id = self.resolve_id(track_id)?;
        let mut stmt = self
            .conn
            .prepare(
                "SELECT backend_id, external_id FROM sources WHERE track_id = ?1 ORDER BY rowid",
            )
            .map_err(db_err)?;
        stmt.query_map([track_id], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(db_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(db_err)
    }

    /// External reference of a track's source for `backend_id`.
    pub fn source_ref(&self, track_id: &str, backend_id: &str) -> Result<Option<String>> {
        let track_id = self.resolve_id(track_id)?;
        self.conn
            .query_row(
                "SELECT external_id FROM sources WHERE track_id = ?1 AND backend_id = ?2",
                params![track_id, backend_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(db_err)
    }

    /// Add tags (deduplicated, case-insensitive). Returns how many were newly applied.
    pub fn add_tags(&mut self, track_id: &str, names: &[String]) -> Result<usize> {
        let track_id = self.resolve_id(track_id)?;
        let track_id = track_id.as_str();
        let tx = self.conn.transaction().map_err(db_err)?;
        let mut added = 0;
        for name in names {
            let display = name.trim();
            let norm = normalize_tag(display);
            if norm.is_empty() {
                return Err(Error::InvalidInput("tag names cannot be empty".into()));
            }
            let tag_id = ensure_tag(&tx, display, &norm)?;
            added += tx
                .execute(
                    "INSERT OR IGNORE INTO track_tags (track_id, tag_id) VALUES (?1, ?2)",
                    params![track_id, tag_id],
                )
                .map_err(db_err)?;
        }
        tx.commit().map_err(db_err)?;
        Ok(added)
    }

    /// Remove tag associations. Returns how many were removed.
    pub fn remove_tags(&mut self, track_id: &str, names: &[String]) -> Result<usize> {
        let track_id = self.resolve_id(track_id)?;
        let track_id = track_id.as_str();
        let tx = self.conn.transaction().map_err(db_err)?;
        let mut removed = 0;
        for name in names {
            removed += tx
                .execute(
                    "DELETE FROM track_tags WHERE track_id = ?1 AND tag_id IN
                       (SELECT id FROM tags WHERE normalized_name = ?2)",
                    params![track_id, normalize_tag(name)],
                )
                .map_err(db_err)?;
        }
        tx.commit().map_err(db_err)?;
        Ok(removed)
    }

    pub fn tags(&self, track_id: &str) -> Result<Vec<String>> {
        let track_id = self.resolve_id(track_id)?;
        let track_id = track_id.as_str();
        let mut stmt = self
            .conn
            .prepare(
                "SELECT g.name FROM track_tags tt JOIN tags g ON g.id = tt.tag_id
                 WHERE tt.track_id = ?1 ORDER BY g.normalized_name",
            )
            .map_err(db_err)?;
        stmt.query_map([track_id], |r| r.get(0))
            .map_err(db_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(db_err)
    }
}

/// `base`, or `base-2`, `base-3`… if taken.
pub(crate) fn unique_slug(
    tx: &Transaction,
    table: &str,
    column: &str,
    base: &str,
) -> Result<String> {
    let sql = format!("SELECT 1 FROM {table} WHERE {column} = ?1");
    let taken = |s: &str| -> Result<bool> {
        tx.query_row(&sql, [s], |_| Ok(()))
            .optional()
            .map(|r| r.is_some())
            .map_err(db_err)
    };
    if !taken(base)? {
        return Ok(base.to_string());
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find_map(|c| match taken(&c) {
            Ok(false) => Some(Ok(c)),
            Ok(true) => None,
            Err(e) => Some(Err(e)),
        })
        .expect("unbounded range")
}

fn insert_metadata(tx: &Transaction, id: &str, m: &Metadata) -> Result<()> {
    let norm = |v: &Option<String>| v.as_ref().map(|s| s.to_lowercase());
    tx.execute(
        "INSERT INTO track_metadata
           (track_id, title, title_norm, artist, artist_norm, album, album_norm, album_artist,
            genre, genre_norm, composer, year, track_number, disc_number, duration_ms, bpm)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        params![
            id,
            m.title,
            m.title.to_lowercase(),
            m.artist,
            norm(&m.artist),
            m.album,
            norm(&m.album),
            m.album_artist,
            m.genre,
            norm(&m.genre),
            m.composer,
            m.year,
            m.track_number,
            m.disc_number,
            m.duration_ms.map(|d| d as i64),
            m.bpm,
        ],
    )
    .map_err(db_err)?;
    Ok(())
}

fn ensure_tag(tx: &Transaction, display: &str, norm: &str) -> Result<i64> {
    tx.execute(
        "INSERT OR IGNORE INTO tags (name, normalized_name) VALUES (?1, ?2)",
        params![display, norm],
    )
    .map_err(db_err)?;
    tx.query_row(
        "SELECT id FROM tags WHERE normalized_name = ?1",
        [norm],
        |r| r.get(0),
    )
    .map_err(db_err)
}

/// Offset and size of the window to read, given the total and the requested page.
fn window(total: usize, page: Page) -> (usize, usize) {
    if page.offset >= total {
        return (page.offset, 0);
    }
    let remaining = total - page.offset;
    (
        page.offset,
        page.limit.map_or(remaining, |l| l.min(remaining)),
    )
}

#[cfg(test)]
mod tests;
