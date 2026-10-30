use rusqlite::{Connection, Transaction};
use weave_core::{Error, Result};

/// Append-only. `PRAGMA user_version` records how many have been applied.
enum Step {
    Sql(&'static str),
    Code(fn(&Transaction) -> Result<()>),
}

const MIGRATIONS: &[Step] = &[
    Step::Sql("CREATE TABLE tracks (
         id     TEXT PRIMARY KEY,
         title  TEXT NOT NULL,
         artist TEXT,
         album  TEXT,
         bpm    REAL
     );
     CREATE TABLE sources (
         id          TEXT PRIMARY KEY,
         track_id    TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
         backend_id  TEXT NOT NULL,
         external_id TEXT NOT NULL,
         UNIQUE (backend_id, external_id)
     );
     CREATE INDEX sources_track ON sources(track_id);
     CREATE TABLE tags (
         id              INTEGER PRIMARY KEY,
         name            TEXT NOT NULL,
         normalized_name TEXT NOT NULL UNIQUE
     );
     CREATE TABLE track_tags (
         track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
         tag_id   INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
         PRIMARY KEY (track_id, tag_id)
     );
     CREATE INDEX track_tags_tag ON track_tags(tag_id);
     CREATE TABLE smart_playlists (
         id              TEXT PRIMARY KEY,
         name            TEXT NOT NULL,
         normalized_name TEXT NOT NULL UNIQUE,
         filter_spec     TEXT NOT NULL
     );"),
    // Descriptive metadata moves to its own indexed table. `*_norm` columns hold
    // the lowercased value so filters and ordering never compute per row.
    Step::Sql("CREATE TABLE track_metadata (
         track_id     TEXT PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
         title        TEXT NOT NULL,
         title_norm   TEXT NOT NULL,
         artist       TEXT,
         artist_norm  TEXT,
         album        TEXT,
         album_norm   TEXT,
         album_artist TEXT,
         genre        TEXT,
         genre_norm   TEXT,
         composer     TEXT,
         year         INTEGER,
         track_number INTEGER,
         disc_number  INTEGER,
         duration_ms  INTEGER,
         bpm          REAL
     );
     INSERT INTO track_metadata (track_id, title, title_norm, artist, artist_norm, album, album_norm, bpm)
         SELECT id, title, ulower(title), artist, ulower(artist), album, ulower(album), bpm FROM tracks;
     ALTER TABLE tracks DROP COLUMN title;
     ALTER TABLE tracks DROP COLUMN artist;
     ALTER TABLE tracks DROP COLUMN album;
     ALTER TABLE tracks DROP COLUMN bpm;
     CREATE INDEX meta_sort   ON track_metadata(artist_norm, album_norm, title_norm, track_id);
     CREATE INDEX meta_bpm    ON track_metadata(bpm);
     CREATE INDEX meta_year   ON track_metadata(year);
     CREATE INDEX meta_genre  ON track_metadata(genre_norm);"),
    // Readable alternative IDs; unique index is created after the backfill.
    Step::Code(add_slugs),
];

pub fn run(conn: &mut Connection) -> Result<()> {
    let db = |e: rusqlite::Error| Error::Storage(e.to_string());
    let applied: usize = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).map_err(db)?;
    if applied > MIGRATIONS.len() {
        return Err(Error::Storage(format!(
            "database schema v{applied} is newer than this build (v{})",
            MIGRATIONS.len()
        )));
    }
    for (i, step) in MIGRATIONS.iter().enumerate().skip(applied) {
        let tx = conn.transaction().map_err(db)?;
        match step {
            Step::Sql(sql) => tx.execute_batch(sql).map_err(db)?,
            Step::Code(f) => f(&tx)?,
        }
        tx.pragma_update(None, "user_version", i + 1).map_err(db)?;
        tx.commit().map_err(db)?;
    }
    Ok(())
}

fn add_slugs(tx: &Transaction) -> Result<()> {
    let db = |e: rusqlite::Error| Error::Storage(e.to_string());
    tx.execute_batch("ALTER TABLE tracks ADD COLUMN slug TEXT").map_err(db)?;
    let rows: Vec<(String, Option<String>, Option<String>, String, String)> = {
        let mut stmt = tx
            .prepare(
                "SELECT t.id, m.artist, m.album, m.title,
                        COALESCE((SELECT s.backend_id FROM sources s WHERE s.track_id = t.id
                                  ORDER BY s.rowid LIMIT 1), 'unknown')
                 FROM tracks t JOIN track_metadata m ON m.track_id = t.id ORDER BY t.rowid",
            )
            .map_err(db)?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
            .map_err(db)?
            .collect::<std::result::Result<_, _>>()
            .map_err(db)?
    };
    for (id, artist, album, title, source) in rows {
        let slug = crate::unique_slug(tx, &weave_core::track_slug(artist.as_deref(), album.as_deref(), &title, &source))?;
        tx.execute("UPDATE tracks SET slug = ?1 WHERE id = ?2", [&slug, &id]).map_err(db)?;
    }
    tx.execute_batch("CREATE UNIQUE INDEX tracks_slug ON tracks(slug)").map_err(db)
}
