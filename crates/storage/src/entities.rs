//! Artists and albums as entities: linking at import, listing, profiles, merging and
//! duplicate suggestions. Names are grouped only when spelled the same (case and spacing
//! aside); anything merely similar stays separate until the user says otherwise.

use std::collections::{HashMap, HashSet};

use rusqlite::{OptionalExtension, Transaction, params, types::Value};
use weave_core::{
    AlbumEntry, AlbumProfile, ArtistEntry, ArtistProfile, Count, EntityKind, EntityRef, Error,
    FilterSpec, MergeStats, Result, SimilarArtists, Stats, album_slug, artist_slug, entity_norm,
    loose_key, slugify,
};

use crate::{Storage, db_err, query, unique_slug};

/// Collapse whitespace; the display spelling of a stored name.
fn clean(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn new_key() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// The artist spelled this way, created when missing. Aliases the user created by merging
/// count as the same spelling.
pub(crate) fn ensure_artist(tx: &Transaction, name: &str) -> Result<String> {
    let norm = entity_norm(name);
    let found: Option<String> = tx
        .query_row(
            "SELECT id FROM artists WHERE name_norm = ?1
             UNION SELECT artist_id FROM artist_aliases WHERE alias_norm = ?1",
            [&norm],
            |r| r.get(0),
        )
        .optional()
        .map_err(db_err)?;
    if let Some(id) = found {
        return Ok(id);
    }
    let id = new_key();
    let slug = unique_slug(tx, "artists", "slug", &artist_slug(name))?;
    tx.execute(
        "INSERT INTO artists (id, slug, name, name_norm) VALUES (?1, ?2, ?3, ?4)",
        params![id, slug, clean(name), norm],
    )
    .map_err(db_err)?;
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

/// Link a freshly inserted track to its artist and album.
pub(crate) fn link_track(
    tx: &Transaction,
    track: &str,
    artist: Option<&str>,
    album: Option<&str>,
    album_artist: Option<&str>,
) -> Result<()> {
    let blank = |s: Option<&str>| s.map(str::trim).filter(|s| !s.is_empty()).map(String::from);
    let (artist, album, album_artist) = (blank(artist), blank(album), blank(album_artist));
    let artist_id = artist
        .as_deref()
        .map(|n| ensure_artist(tx, n))
        .transpose()?;
    let album_id = match &album {
        Some(title) => {
            // An album belongs to its album artist, else to the track's artist.
            let owner = album_artist.as_ref().or(artist.as_ref());
            let owner_id = match owner {
                Some(n) if Some(n) == artist.as_ref() => artist_id.clone(),
                Some(n) => Some(ensure_artist(tx, n)?),
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
    /// Internal key of an artist given its slug, else its name, else a name merged into it.
    /// A slug wins over a name that happens to slugify the same way.
    pub(crate) fn resolve_artist(&self, key: &str) -> Result<String> {
        let key = key.trim();
        for (sql, arg) in [
            ("SELECT id FROM artists WHERE slug = ?1", slugify(key)),
            (
                "SELECT id FROM artists WHERE name_norm = ?1",
                entity_norm(key),
            ),
            (
                "SELECT artist_id FROM artist_aliases WHERE alias_norm = ?1",
                entity_norm(key),
            ),
        ] {
            let found: Option<String> = self
                .conn
                .query_row(sql, [arg], |r| r.get(0))
                .optional()
                .map_err(db_err)?;
            if let Some(id) = found {
                return Ok(id);
            }
        }
        Err(Error::NotFound(format!("artist \"{key}\"")))
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
                    (SELECT COUNT(*) FROM albums al WHERE al.artist_id = ar.id)
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
            .prepare("SELECT alias FROM artist_aliases WHERE artist_id = ?1 ORDER BY alias_norm")
            .map_err(db_err)?;
        let aliases = stmt
            .query_map([&id], |r| r.get(0))
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
            aliases,
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

    /// Artists whose names look alike (same letters and digits ignoring punctuation, spacing
    /// and a leading "the"), minus the pairs the user marked as different people. Suggestions
    /// only: nothing is merged until the user says so.
    pub fn similar_artists(&self) -> Result<Vec<SimilarArtists>> {
        let separate = self.separate_pairs()?;
        let mut groups: HashMap<String, Vec<ArtistEntry>> = HashMap::new();
        for a in self.artists(&FilterSpec::default())? {
            let key = loose_key(&a.name);
            if !key.is_empty() {
                groups.entry(key).or_default().push(a);
            }
        }
        let mut out = Vec::new();
        for mut group in groups.into_values().filter(|g| g.len() > 1) {
            // The artist with the most tracks first: the natural one to merge the others into.
            group.sort_by(|x, y| y.tracks.cmp(&x.tracks).then(x.name.cmp(&y.name)));
            for (i, a) in group.iter().enumerate() {
                for b in &group[i + 1..] {
                    let pair = if a.id < b.id {
                        (a.id.clone(), b.id.clone())
                    } else {
                        (b.id.clone(), a.id.clone())
                    };
                    if !separate.contains(&pair) {
                        out.push(SimilarArtists {
                            a: a.clone(),
                            b: b.clone(),
                        });
                    }
                }
            }
        }
        out.sort_by(|x, y| (&x.a.name, &x.b.name).cmp(&(&y.a.name, &y.b.name)));
        Ok(out)
    }

    /// Pairs of artist slugs marked as different people, smaller slug first.
    fn separate_pairs(&self) -> Result<HashSet<(String, String)>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT a.slug, b.slug FROM artist_separate s
                 JOIN artists a ON a.id = s.a_id JOIN artists b ON b.id = s.b_id",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(db_err)?;
        let mut out = HashSet::new();
        for row in rows {
            let (a, b) = row.map_err(db_err)?;
            out.insert(if a < b { (a, b) } else { (b, a) });
        }
        Ok(out)
    }

    /// Record that two artists are different people, so they are not suggested as duplicates.
    pub fn separate_artists(&mut self, a: &str, b: &str) -> Result<()> {
        let (a, b) = (self.resolve_artist(a)?, self.resolve_artist(b)?);
        if a == b {
            return Err(Error::InvalidInput("that is one artist, not two".into()));
        }
        let (lo, hi) = if a < b { (a, b) } else { (b, a) };
        self.conn
            .execute(
                "INSERT OR IGNORE INTO artist_separate (a_id, b_id) VALUES (?1, ?2)",
                params![lo, hi],
            )
            .map_err(db_err)?;
        Ok(())
    }

    /// Fold `from` into `into`: its tracks, albums and tags move over, its name stays as an
    /// alias so a re-import lands on `into`, and track metadata now spells the surviving name.
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
        let (from_name, from_norm) = name_of(&tx, &from_id)?;
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
        tx.execute(
            "UPDATE artist_aliases SET artist_id = ?1 WHERE artist_id = ?2",
            params![into_id, from_id],
        )
        .map_err(db_err)?;
        tx.execute(
            "INSERT OR REPLACE INTO artist_aliases (alias_norm, alias, artist_id)
             VALUES (?1, ?2, ?3)",
            params![from_norm, from_name, into_id],
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
