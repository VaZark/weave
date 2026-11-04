//! SQLite persistence: migrations, the canonical library, playlists, annotations and the
//! persisted playback session.

mod annotations;
mod dedup;
mod entities;
mod history;
mod migrations;
mod playlists;
mod prefs;
mod query;
mod session;
mod tags;

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use std::collections::HashMap;

use weave_player_core::{
    EntityKind, Error, FilterSpec, Instance, Metadata, Page, Result, ScannedTrack, SearchResult,
    Track, TrackId, default_instance_name, metadata_tags, parse_credits, parse_instance_name,
    slugify, track_slug,
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
        history::ensure(&conn)?;
        prefs::ensure(&conn)?;
        Ok(Self { conn })
    }

    /// Insert tracks whose file path is not yet known; all-or-nothing.
    pub fn import_tracks(&mut self, tracks: &[ScannedTrack]) -> Result<ImportStats> {
        let tx = self.conn.transaction().map_err(db_err)?;
        let mut stats = ImportStats::default();
        let mut run = entities::RunArtists::new();
        for t in tracks {
            let exists: bool = tx
                .query_row("SELECT 1 FROM sources WHERE path = ?1", [&t.path], |_| {
                    Ok(true)
                })
                .optional()
                .map_err(db_err)?
                .unwrap_or(false);
            if exists {
                stats.skipped += 1;
                continue;
            }
            let id = uuid::Uuid::new_v4().simple().to_string();
            // Featured credits become entries of their own; the track keeps its primary artist.
            let credits = parse_credits(t.metadata.artist.as_deref(), &t.metadata.title);
            let meta = Metadata {
                artist: credits.primary.clone(),
                ..t.metadata.clone()
            };
            let m = &meta;
            let slug = unique_slug(
                &tx,
                "tracks",
                "slug",
                &track_slug(m.artist.as_deref(), m.album.as_deref(), &m.title),
            )?;
            tx.execute(
                "INSERT INTO tracks (id, slug, added_at) VALUES (?1, ?2, CAST(strftime('%s', 'now') AS INTEGER))",
                [&id, &slug],
            )
            .map_err(db_err)?;
            insert_metadata(&tx, &id, m)?;
            entities::link_track(
                &tx,
                &mut run,
                &id,
                &credits,
                m.album.as_deref(),
                m.album_artist.as_deref(),
            )?;
            for name in metadata_tags(m) {
                tags::assign(&tx, EntityKind::Track, &id, &name)?;
            }
            tx.execute(
                "INSERT INTO sources (track_id, path) VALUES (?1, ?2)",
                params![id, t.path],
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
        let (conditions, mut args) = query::conditions(&filter);
        let (from_sources, source_args) = self.source_terms(&filter, 0)?;
        args.extend(source_args);
        let where_sql = format!(" WHERE 1=1{conditions}{from_sources}");

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
    /// so `Romeo Santos-Song` finds `romeo-santos-song`).
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
                        rating: r.get(13)?,
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

    /// The file a track plays: its oldest one.
    pub fn source_path(&self, track_id: &str) -> Result<Option<String>> {
        Ok(self.source_paths(track_id)?.into_iter().next())
    }

    /// Every file behind a track, oldest first. More than one after duplicate tracks are merged.
    pub fn source_paths(&self, track_id: &str) -> Result<Vec<String>> {
        let track_id = self.resolve_id(track_id)?;
        let mut stmt = self
            .conn
            .prepare("SELECT path FROM sources WHERE track_id = ?1 ORDER BY rowid")
            .map_err(db_err)?;
        stmt.query_map([track_id], |r| r.get(0))
            .map_err(db_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(db_err)
    }

    /// This instance's name and capabilities.
    pub fn instance(&self) -> Result<Instance> {
        let (name, playback): (Option<String>, i64) = self
            .conn
            .query_row(
                "SELECT name, playback FROM instance WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(db_err)?;
        Ok(Instance {
            name: name.unwrap_or_else(default_instance_name),
            playback: playback != 0,
        })
    }

    /// Name the instance (normalized to a slug).
    pub fn set_instance_name(&mut self, name: &str) -> Result<Instance> {
        let name = parse_instance_name(name)?;
        self.conn
            .execute("UPDATE instance SET name = ?1 WHERE id = 1", [name])
            .map_err(db_err)?;
        self.instance()
    }

    /// Allow or forbid this instance to play audio; off makes it remote-only.
    pub fn set_playback_allowed(&mut self, allowed: bool) -> Result<Instance> {
        self.conn
            .execute("UPDATE instance SET playback = ?1 WHERE id = 1", [allowed])
            .map_err(db_err)?;
        self.instance()
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
#[cfg(test)]
mod tests_entities;
#[cfg(test)]
mod tests_sources;
#[cfg(test)]
mod tests_tags;
