//! Artists and albums as entities: linking at import, listing, profiles, merging and
//! duplicate suggestions. Names are grouped only when spelled the same (case and spacing
//! aside); anything merely similar stays separate until the user says otherwise.

use std::collections::HashMap;

use rusqlite::{OptionalExtension, Transaction, params, types::Value};
use weave_core::{
    AlbumEntry, AlbumProfile, ArtistEntry, ArtistProfile, Count, Credits, EntityKind, EntityRef,
    Error, FilterSpec, MergeStats, Result, Stats, album_slug, artist_slug, entity_norm, slugify,
};

use crate::{Storage, db_err, query, unique_slug};

/// Collapse whitespace; the display spelling of a stored name.
fn clean(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn new_key() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// Artists made during one import, by spelling, so that tracks of one run share an artist.
pub(crate) type RunArtists = HashMap<String, String>;

/// The artist for this name in the current import run. An artist already in the library is
/// never reused: a new one is made (the same name can be a different person) and the user
/// merges afterwards, so nothing is ever collapsed by guess.
pub(crate) fn run_artist(tx: &Transaction, run: &mut RunArtists, name: &str) -> Result<String> {
    let norm = entity_norm(name);
    if let Some(id) = run.get(&norm) {
        return Ok(id.clone());
    }
    let id = new_key();
    let slug = unique_slug(tx, "artists", "slug", &artist_slug(name))?;
    tx.execute(
        "INSERT INTO artists (id, slug, name, name_norm) VALUES (?1, ?2, ?3, ?4)",
        params![id, slug, clean(name), norm],
    )
    .map_err(db_err)?;
    run.insert(norm, id.clone());
    Ok(id)
}

/// The album with this title under this artist, created when missing.
pub(crate) fn ensure_album(
    tx: &Transaction,
    title: &str,
    artist_id: Option<&str>,
    artist_name: Option<&str>,
) -> Result<String> {
    let norm = entity_norm(title);
    let found: Option<String> = tx
        .query_row(
            "SELECT id FROM albums WHERE title_norm = ?1 AND artist_id IS ?2",
            params![norm, artist_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(db_err)?;
    if let Some(id) = found {
        return Ok(id);
    }
    let id = new_key();
    let slug = unique_slug(tx, "albums", "slug", &album_slug(artist_name, title))?;
    tx.execute(
        "INSERT INTO albums (id, slug, title, title_norm, artist_id) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, slug, clean(title), norm, artist_id],
    )
    .map_err(db_err)?;
    Ok(id)
}

/// Link a freshly inserted track to its artist, its featured artists and its album.
pub(crate) fn link_track(
    tx: &Transaction,
    run: &mut RunArtists,
    track: &str,
    credits: &Credits,
    album: Option<&str>,
    album_artist: Option<&str>,
) -> Result<()> {
    let blank = |s: Option<&str>| s.map(str::trim).filter(|s| !s.is_empty()).map(String::from);
    let (album, album_artist) = (blank(album), blank(album_artist));
    let artist_id = credits
        .primary
        .as_deref()
        .map(|n| run_artist(tx, run, n))
        .transpose()?;
    for (position, name) in credits.featured.iter().enumerate() {
        let id = run_artist(tx, run, name)?;
        tx.execute(
            "INSERT OR IGNORE INTO track_featured (track_id, artist_id, position) VALUES (?1, ?2, ?3)",
            params![track, id, position as i64],
        )
        .map_err(db_err)?;
    }
    let album_id = match &album {
        Some(title) => {
            // An album belongs to its album artist, else to the track's artist.
            let owner = album_artist.as_ref().or(credits.primary.as_ref());
            let owner_id = match owner {
                Some(n) if Some(n) == credits.primary.as_ref() => artist_id.clone(),
                Some(n) => Some(run_artist(tx, run, n)?),
                None => None,
            };
            Some(ensure_album(
                tx,
                title,
                owner_id.as_deref(),
                owner.map(String::as_str),
            )?)
        }
        None => None,
    };
    tx.execute(
        "UPDATE tracks SET artist_id = ?1, album_id = ?2 WHERE id = ?3",
        params![artist_id, album_id, track],
    )
    .map_err(db_err)?;
    Ok(())
}

const STATS_SQL: &str =
    "SELECT COUNT(*), COALESCE(SUM(m.duration_ms), 0), MIN(m.year), MAX(m.year),
        MIN(m.bpm), MAX(m.bpm), AVG(m.bpm), COUNT(a.rating), AVG(a.rating),
        MIN(t.added_at), MAX(t.added_at)";

impl Storage {
    /// Internal key of an artist given its slug, else its name. A slug wins over a name that
    /// happens to slugify the same way; a name shared by several artists needs the slug.
    pub(crate) fn resolve_artist(&self, key: &str) -> Result<String> {
        let key = key.trim();
        let by_slug: Option<String> = self
            .conn
            .query_row(
                "SELECT id FROM artists WHERE slug = ?1",
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
            .prepare("SELECT id FROM artists WHERE name_norm = ?1 LIMIT 2")
            .map_err(db_err)?;
        let ids: Vec<String> = stmt
            .query_map([entity_norm(key)], |r| r.get(0))
            .map_err(db_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(db_err)?;
        match ids.len() {
            0 => Err(Error::NotFound(format!("artist \"{key}\""))),
            1 => Ok(ids.into_iter().next().unwrap()),
            _ => Err(Error::InvalidInput(format!(
                "several artists are called \"{key}\"; use the artist's ID (see `artist list`)"
            ))),
        }
    }

    /// Internal key of an album given its slug or, if only one album has it, its title.
    pub(crate) fn resolve_album(&self, key: &str) -> Result<String> {
        let key = key.trim();
        let by_slug: Option<String> = self
            .conn
            .query_row(
                "SELECT id FROM albums WHERE slug = ?1",
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
            .prepare("SELECT id FROM albums WHERE title_norm = ?1 LIMIT 2")
            .map_err(db_err)?;
        let ids: Vec<String> = stmt
            .query_map([entity_norm(key)], |r| r.get(0))
            .map_err(db_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(db_err)?;
        match ids.len() {
            0 => Err(Error::NotFound(format!("album \"{key}\""))),
            1 => Ok(ids.into_iter().next().unwrap()),
            _ => Err(Error::InvalidInput(format!(
                "several albums are called \"{key}\"; use the album's ID (see `album list`)"
            ))),
        }
    }

    /// Tag names per owner, for the owners given (internal keys).
    fn tag_names(
        &self,
        table: &str,
        col: &str,
        ids: &[String],
    ) -> Result<HashMap<String, Vec<String>>> {
        let mut out: HashMap<String, Vec<String>> = HashMap::new();
        for chunk in ids.chunks(400) {
            let marks = vec!["?"; chunk.len()].join(",");
            let mut stmt = self
                .conn
                .prepare(&format!(
                    "SELECT tt.{col}, g.name FROM {table} tt JOIN tags g ON g.id = tt.tag_id
                     WHERE tt.{col} IN ({marks}) ORDER BY g.normalized_name"
                ))
                .map_err(db_err)?;
            let rows = stmt
                .query_map(rusqlite::params_from_iter(chunk), |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })
                .map_err(db_err)?;
            for row in rows {
                let (id, name) = row.map_err(db_err)?;
                out.entry(id).or_default().push(name);
            }
        }
        Ok(out)
    }

    /// Artists, by name. Only the tag conditions of `filter` apply (all/any/exclude, clauses
    /// and ranges over the tags put on artists themselves).
    pub fn artists(&self, filter: &FilterSpec) -> Result<Vec<ArtistEntry>> {
        let f = filter.clone().normalized();
        f.validate()?;
        let (tag_sql, args) = query::tag_conditions(&f, query::ARTIST_TAGS);
        let sql = format!(
            "SELECT ar.id, ar.slug, ar.name,
                    (SELECT COUNT(*) FROM tracks t WHERE t.artist_id = ar.id),
                    (SELECT COUNT(*) FROM albums al WHERE al.artist_id = ar.id),
                    (SELECT COUNT(*) FROM track_featured f WHERE f.artist_id = ar.id)
             FROM artists ar WHERE 1=1{tag_sql} ORDER BY ar.name_norm, ar.slug"
        );
        let mut stmt = self.conn.prepare(&sql).map_err(db_err)?;
        let rows: Vec<(String, ArtistEntry)> = stmt
            .query_map(rusqlite::params_from_iter(args), |r| {
                Ok((
                    r.get(0)?,
                    ArtistEntry {
                        id: r.get(1)?,
                        name: r.get(2)?,
                        tracks: r.get::<_, i64>(3)? as usize,
                        albums: r.get::<_, i64>(4)? as usize,
                        featured: r.get::<_, i64>(5)? as usize,
                        tags: Vec::new(),
                    },
                ))
            })
            .map_err(db_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(db_err)?;
        let keys: Vec<String> = rows.iter().map(|(k, _)| k.clone()).collect();
        let mut tags = self.tag_names("artist_tags", "artist_id", &keys)?;
        Ok(rows
            .into_iter()
            .map(|(k, mut e)| {
                e.tags = tags.remove(&k).unwrap_or_default();
                e
            })
            .collect())
    }

    /// Albums by artist and title, optionally only an artist's. The tag conditions of `filter`
    /// apply to the tags put on albums themselves.
    pub fn albums(&self, filter: &FilterSpec, artist: Option<&str>) -> Result<Vec<AlbumEntry>> {
        let f = filter.clone().normalized();
        f.validate()?;
        let (tag_sql, mut args) = query::tag_conditions(&f, query::ALBUM_TAGS);
        let mut owner_sql = String::new();
        if let Some(a) = artist {
            owner_sql = " AND al.artist_id = ?".into();
            args.push(Value::Text(self.resolve_artist(a)?));
        }
        let sql = format!(
            "SELECT al.id, al.slug, al.title, ar.slug, ar.name,
                    (SELECT MIN(m.year) FROM tracks t JOIN track_metadata m ON m.track_id = t.id
                     WHERE t.album_id = al.id),
                    (SELECT COUNT(*) FROM tracks t WHERE t.album_id = al.id)
             FROM albums al LEFT JOIN artists ar ON ar.id = al.artist_id
             WHERE 1=1{tag_sql}{owner_sql}
             ORDER BY ar.name_norm IS NULL, ar.name_norm, al.title_norm, al.slug"
        );
        let mut stmt = self.conn.prepare(&sql).map_err(db_err)?;
        let rows: Vec<(String, AlbumEntry)> = stmt
            .query_map(rusqlite::params_from_iter(args), |r| {
                let artist = match (
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<String>>(4)?,
                ) {
                    (Some(id), Some(name)) => Some(EntityRef { id, name }),
                    _ => None,
                };
                Ok((
                    r.get(0)?,
                    AlbumEntry {
                        id: r.get(1)?,
                        title: r.get(2)?,
                        artist,
                        year: r.get(5)?,
                        tracks: r.get::<_, i64>(6)? as usize,
                        tags: Vec::new(),
                    },
                ))
            })
            .map_err(db_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(db_err)?;
        let keys: Vec<String> = rows.iter().map(|(k, _)| k.clone()).collect();
        let mut tags = self.tag_names("album_tags", "album_id", &keys)?;
        Ok(rows
            .into_iter()
            .map(|(k, mut e)| {
                e.tags = tags.remove(&k).unwrap_or_default();
                e
            })
            .collect())
    }

    /// Figures over the tracks matching `cond` (a condition over `t`, `m`, `a` with one `?1`).
    fn stats(&self, cond: &str, key: &str) -> Result<Stats> {
        self.conn
            .query_row(
                &format!("{STATS_SQL}{} WHERE {cond}", query::FROM),
                [key],
                |r| {
                    Ok(Stats {
                        tracks: r.get::<_, i64>(0)? as usize,
                        duration_ms: r.get::<_, i64>(1)? as u64,
                        year_min: r.get(2)?,
                        year_max: r.get(3)?,
                        bpm_min: r.get(4)?,
                        bpm_max: r.get(5)?,
                        bpm_avg: r.get(6)?,
                        rated: r.get::<_, i64>(7)? as usize,
                        rating_avg: r.get(8)?,
                        first_added: r.get(9)?,
                        last_added: r.get(10)?,
                    })
                },
            )
            .map_err(db_err)
    }

    /// Tag counts over the tracks matching `cond`: the `genre:` tags, or all the others
    /// (numbers like bpm and year left out), most common first.
    fn tag_counts(&self, cond: &str, key: &str, genres: bool) -> Result<Vec<Count>> {
        let which = if genres {
            "g.key = 'genre'"
        } else {
            "(g.key IS NULL OR g.key NOT IN ('genre', 'bpm', 'year'))"
        };
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT g.name, COUNT(*) FROM track_tags tt
                 JOIN tags g ON g.id = tt.tag_id JOIN tracks t ON t.id = tt.track_id
                 WHERE {cond} AND {which}
                 GROUP BY g.id ORDER BY 2 DESC, g.normalized_name LIMIT 12"
            ))
            .map_err(db_err)?;
        stmt.query_map([key], |r| {
            Ok(Count {
                name: r.get(0)?,
                count: r.get::<_, i64>(1)? as usize,
            })
        })
        .map_err(db_err)?
        .collect::<std::result::Result<_, _>>()
        .map_err(db_err)
    }

    pub fn artist(&self, key: &str) -> Result<ArtistProfile> {
        let id = self.resolve_artist(key)?;
        let (slug, name): (String, String) = self
            .conn
            .query_row("SELECT slug, name FROM artists WHERE id = ?1", [&id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .map_err(db_err)?;
        let mut stmt = self
            .conn
            .prepare(
                "SELECT t.slug, m.title FROM track_featured f
                 JOIN tracks t ON t.id = f.track_id JOIN track_metadata m ON m.track_id = t.id
                 WHERE f.artist_id = ?1 ORDER BY m.title_norm, t.slug",
            )
            .map_err(db_err)?;
        let featured_on = stmt
            .query_map([&id], |r| {
                Ok(EntityRef {
                    id: r.get(0)?,
                    name: r.get(1)?,
                })
            })
            .map_err(db_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(db_err)?;
        let loose_tracks: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM tracks WHERE artist_id = ?1 AND album_id IS NULL",
                [&id],
                |r| r.get(0),
            )
            .map_err(db_err)?;
        let cond = "t.artist_id = ?1";
        Ok(ArtistProfile {
            tags: self.tags_of(EntityKind::Artist, &slug)?,
            stats: self.stats(cond, &id)?,
            albums: self.albums(&FilterSpec::default(), Some(&slug))?,
            loose_tracks: loose_tracks as usize,
            genres: self.tag_counts(cond, &id, true)?,
            track_tags: self.tag_counts(cond, &id, false)?,
            id: slug,
            name,
            featured_on,
        })
    }

    pub fn album(&self, key: &str) -> Result<AlbumProfile> {
        let id = self.resolve_album(key)?;
        let (slug, title, artist): (String, String, Option<EntityRef>) = self
            .conn
            .query_row(
                "SELECT al.slug, al.title, ar.slug, ar.name FROM albums al
                 LEFT JOIN artists ar ON ar.id = al.artist_id WHERE al.id = ?1",
                [&id],
                |r| {
                    let artist = match (
                        r.get::<_, Option<String>>(2)?,
                        r.get::<_, Option<String>>(3)?,
                    ) {
                        (Some(id), Some(name)) => Some(EntityRef { id, name }),
                        _ => None,
                    };
                    Ok((r.get(0)?, r.get(1)?, artist))
                },
            )
            .map_err(db_err)?;
        let cond = "t.album_id = ?1";
        let tracks = self.fetch(
            &format!(
                "SELECT {}{} WHERE t.album_id = ?
                 ORDER BY m.disc_number IS NULL, m.disc_number, m.track_number IS NULL,
                          m.track_number, m.title_norm, m.track_id",
                query::COLUMNS,
                query::FROM
            ),
            vec![Value::Text(id.clone())],
        )?;
        Ok(AlbumProfile {
            tags: self.tags_of(EntityKind::Album, &slug)?,
            stats: self.stats(cond, &id)?,
            genres: self.tag_counts(cond, &id, true)?,
            track_tags: self.tag_counts(cond, &id, false)?,
            tracks,
            id: slug,
            title,
            artist,
        })
    }

    /// Fold `from` into `into`: its tracks, featured credits, albums and tags move over, and track
    /// metadata now spells the surviving name.
    pub fn merge_artists(&mut self, from: &str, into: &str) -> Result<MergeStats> {
        let (from_id, into_id) = (self.resolve_artist(from)?, self.resolve_artist(into)?);
        if from_id == into_id {
            return Err(Error::InvalidInput(
                "cannot merge an artist into itself".into(),
            ));
        }
        let name_of = |tx: &Transaction, id: &str| -> Result<(String, String)> {
            tx.query_row(
                "SELECT name, name_norm FROM artists WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(db_err)
        };
        let tx = self.conn.transaction().map_err(db_err)?;
        let (_, from_norm) = name_of(&tx, &from_id)?;
        let (into_name, _) = name_of(&tx, &into_id)?;

        // Albums: one the survivor already has (same title) absorbs the other's tracks.
        let mut moved_albums = 0;
        let albums: Vec<(String, String)> = {
            let mut stmt = tx
                .prepare("SELECT id, title_norm FROM albums WHERE artist_id = ?1")
                .map_err(db_err)?;
            stmt.query_map([&from_id], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(db_err)?
                .collect::<std::result::Result<_, _>>()
                .map_err(db_err)?
        };
        for (album, title_norm) in albums {
            let twin: Option<String> = tx
                .query_row(
                    "SELECT id FROM albums WHERE artist_id = ?1 AND title_norm = ?2",
                    params![into_id, title_norm],
                    |r| r.get(0),
                )
                .optional()
                .map_err(db_err)?;
            match twin {
                Some(twin) => {
                    tx.execute(
                        "UPDATE tracks SET album_id = ?1 WHERE album_id = ?2",
                        params![twin, album],
                    )
                    .map_err(db_err)?;
                    tx.execute(
                        "INSERT OR IGNORE INTO album_tags (album_id, tag_id)
                         SELECT ?1, tag_id FROM album_tags WHERE album_id = ?2",
                        params![twin, album],
                    )
                    .map_err(db_err)?;
                    tx.execute("DELETE FROM albums WHERE id = ?1", [&album])
                        .map_err(db_err)?;
                }
                None => {
                    tx.execute(
                        "UPDATE albums SET artist_id = ?1 WHERE id = ?2",
                        params![into_id, album],
                    )
                    .map_err(db_err)?;
                }
            }
            moved_albums += 1;
        }

        // Tracks: the metadata text follows the surviving name.
        tx.execute(
            "UPDATE track_metadata SET artist = ?1, artist_norm = ?2
             WHERE track_id IN (SELECT id FROM tracks WHERE artist_id = ?3)",
            params![into_name, into_name.to_lowercase(), from_id],
        )
        .map_err(db_err)?;
        tx.execute(
            "UPDATE track_metadata SET album_artist = ?1
             WHERE album_artist IS NOT NULL AND lower(trim(album_artist)) = ?2",
            params![into_name, from_norm],
        )
        .map_err(db_err)?;
        let moved_tracks = tx
            .execute(
                "UPDATE tracks SET artist_id = ?1 WHERE artist_id = ?2",
                params![into_id, from_id],
            )
            .map_err(db_err)?;

        tx.execute(
            "INSERT OR IGNORE INTO artist_tags (artist_id, tag_id)
             SELECT ?1, tag_id FROM artist_tags WHERE artist_id = ?2",
            params![into_id, from_id],
        )
        .map_err(db_err)?;
        // Featured credits move too; a track can't feature its own primary artist.
        tx.execute(
            "UPDATE OR IGNORE track_featured SET artist_id = ?1 WHERE artist_id = ?2",
            params![into_id, from_id],
        )
        .map_err(db_err)?;
        tx.execute(
            "DELETE FROM track_featured WHERE artist_id = ?1",
            [&from_id],
        )
        .map_err(db_err)?;
        tx.execute(
            "DELETE FROM track_featured WHERE artist_id =
               (SELECT t.artist_id FROM tracks t WHERE t.id = track_featured.track_id)",
            [],
        )
        .map_err(db_err)?;
        tx.execute("DELETE FROM artists WHERE id = ?1", [&from_id])
            .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(MergeStats {
            tracks: moved_tracks,
            albums: moved_albums,
        })
    }

    /// Link an album to an artist profile, or unlink it (`None`).
    pub fn link_album(&mut self, album: &str, artist: Option<&str>) -> Result<AlbumProfile> {
        let id = self.resolve_album(album)?;
        let artist_id = artist.map(|a| self.resolve_artist(a)).transpose()?;
        let taken: bool = self
            .conn
            .query_row(
                "SELECT 1 FROM albums o JOIN albums al ON al.id = ?1
                 WHERE o.id <> al.id AND o.title_norm = al.title_norm AND o.artist_id IS ?2",
                params![id, artist_id],
                |_| Ok(true),
            )
            .optional()
            .map_err(db_err)?
            .unwrap_or(false);
        if taken {
            return Err(Error::AlreadyExists(
                "that artist already has an album with this title".into(),
            ));
        }
        self.conn
            .execute(
                "UPDATE albums SET artist_id = ?1 WHERE id = ?2",
                params![artist_id, id],
            )
            .map_err(db_err)?;
        self.album(album)
    }

    /// Change an artist's public ID. The new slug is normalized like any slug and must be free.
    /// Album and track IDs were assigned from the old name and do not change.
    pub fn set_artist_slug(&mut self, artist: &str, slug: &str) -> Result<ArtistProfile> {
        let id = self.resolve_artist(artist)?;
        let slug = slugify(slug);
        if slug.is_empty() {
            return Err(Error::InvalidInput(
                "an ID needs at least one letter or digit".into(),
            ));
        }
        let taken: bool = self
            .conn
            .query_row(
                "SELECT 1 FROM artists WHERE slug = ?1 AND id <> ?2",
                params![slug, id],
                |_| Ok(true),
            )
            .optional()
            .map_err(db_err)?
            .unwrap_or(false);
        if taken {
            return Err(Error::AlreadyExists(format!("artist ID \"{slug}\"")));
        }
        self.conn
            .execute(
                "UPDATE artists SET slug = ?1 WHERE id = ?2",
                params![slug, id],
            )
            .map_err(db_err)?;
        self.artist(&slug)
    }

    /// Split an artist in two: a new artist (the same name unless `name` says otherwise, with
    /// the given slug or a fresh one) takes the listed tracks and albums. For people who share
    /// a name. The two are recorded as different people. Returns the new artist.
    pub fn split_artist(
        &mut self,
        artist: &str,
        tracks: &[String],
        albums: &[String],
        name: Option<&str>,
        slug: Option<&str>,
    ) -> Result<ArtistProfile> {
        if tracks.is_empty() && albums.is_empty() {
            return Err(Error::InvalidInput(
                "say what moves to the new artist: tracks and/or albums".into(),
            ));
        }
        let from_id = self.resolve_artist(artist)?;
        let from_name: String = self
            .conn
            .query_row("SELECT name FROM artists WHERE id = ?1", [&from_id], |r| {
                r.get(0)
            })
            .map_err(db_err)?;
        let name = name.map_or_else(|| from_name.clone(), clean);
        if name.is_empty() {
            return Err(Error::InvalidInput("an artist needs a name".into()));
        }
        let track_ids = tracks
            .iter()
            .map(|t| {
                let id = self.resolve_id(t)?;
                let owner: Option<String> = self
                    .conn
                    .query_row("SELECT artist_id FROM tracks WHERE id = ?1", [&id], |r| {
                        r.get(0)
                    })
                    .map_err(db_err)?;
                if owner.as_deref() == Some(from_id.as_str()) {
                    Ok(id)
                } else {
                    Err(Error::InvalidInput(format!(
                        "track \"{t}\" is not by {artist}"
                    )))
                }
            })
            .collect::<Result<Vec<_>>>()?;
        let album_ids = albums
            .iter()
            .map(|a| {
                let id = self.resolve_album(a)?;
                let owner: Option<String> = self
                    .conn
                    .query_row("SELECT artist_id FROM albums WHERE id = ?1", [&id], |r| {
                        r.get(0)
                    })
                    .map_err(db_err)?;
                if owner.as_deref() == Some(from_id.as_str()) {
                    Ok(id)
                } else {
                    Err(Error::InvalidInput(format!(
                        "album \"{a}\" is not {artist}'s"
                    )))
                }
            })
            .collect::<Result<Vec<_>>>()?;

        let tx = self.conn.transaction().map_err(db_err)?;
        let new_id = new_key();
        let new_slug = match slug {
            Some(s) => {
                let s = slugify(s);
                if s.is_empty() {
                    return Err(Error::InvalidInput(
                        "an ID needs at least one letter or digit".into(),
                    ));
                }
                let taken: bool = tx
                    .query_row("SELECT 1 FROM artists WHERE slug = ?1", [&s], |_| Ok(true))
                    .optional()
                    .map_err(db_err)?
                    .unwrap_or(false);
                if taken {
                    return Err(Error::AlreadyExists(format!("artist ID \"{s}\"")));
                }
                s
            }
            None => unique_slug(&tx, "artists", "slug", &artist_slug(&name))?,
        };
        tx.execute(
            "INSERT INTO artists (id, slug, name, name_norm) VALUES (?1, ?2, ?3, ?4)",
            params![new_id, new_slug, name, entity_norm(&name)],
        )
        .map_err(db_err)?;
        for album in &album_ids {
            tx.execute(
                "UPDATE albums SET artist_id = ?1 WHERE id = ?2",
                params![new_id, album],
            )
            .map_err(db_err)?;
            tx.execute(
                "UPDATE tracks SET artist_id = ?1 WHERE album_id = ?2 AND artist_id = ?3",
                params![new_id, album, from_id],
            )
            .map_err(db_err)?;
        }
        for track in &track_ids {
            tx.execute(
                "UPDATE tracks SET artist_id = ?1 WHERE id = ?2",
                params![new_id, track],
            )
            .map_err(db_err)?;
        }
        tx.execute(
            "UPDATE track_metadata SET artist = ?1, artist_norm = ?2
             WHERE track_id IN (SELECT id FROM tracks WHERE artist_id = ?3)",
            params![name, name.to_lowercase(), new_id],
        )
        .map_err(db_err)?;
        crate::dedup::mark_different(&tx, EntityKind::Artist, &from_id, &new_id)?;
        tx.commit().map_err(db_err)?;
        self.artist(&new_slug)
    }

    /// Featured artists of a track, in credit order.
    pub fn track_featured(&self, track_id: &str) -> Result<Vec<EntityRef>> {
        let id = self.resolve_id(track_id)?;
        let mut stmt = self
            .conn
            .prepare(
                "SELECT ar.slug, ar.name FROM track_featured f JOIN artists ar ON ar.id = f.artist_id
                 WHERE f.track_id = ?1 ORDER BY f.position",
            )
            .map_err(db_err)?;
        stmt.query_map([id], |r| {
            Ok(EntityRef {
                id: r.get(0)?,
                name: r.get(1)?,
            })
        })
        .map_err(db_err)?
        .collect::<std::result::Result<_, _>>()
        .map_err(db_err)
    }

    /// When a track was first indexed and the artist and album it is linked to.
    pub fn track_links(
        &self,
        track_id: &str,
    ) -> Result<(i64, Option<EntityRef>, Option<EntityRef>)> {
        let id = self.resolve_id(track_id)?;
        self.conn
            .query_row(
                "SELECT t.added_at, ar.slug, ar.name, al.slug, al.title FROM tracks t
                 LEFT JOIN artists ar ON ar.id = t.artist_id
                 LEFT JOIN albums al ON al.id = t.album_id WHERE t.id = ?1",
                [id],
                |r| {
                    let pair = |a: Option<String>, b: Option<String>| match (a, b) {
                        (Some(id), Some(name)) => Some(EntityRef { id, name }),
                        _ => None,
                    };
                    Ok((
                        r.get(0)?,
                        pair(r.get(1)?, r.get(2)?),
                        pair(r.get(3)?, r.get(4)?),
                    ))
                },
            )
            .map_err(db_err)
    }
}
