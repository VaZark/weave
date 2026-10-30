use rusqlite::Connection;
use weave_core::{Error, Result};

/// Append-only. `PRAGMA user_version` records how many have been applied.
const MIGRATIONS: &[&str] = &[
    "CREATE TABLE tracks (
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
     );",
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
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(applied) {
        let tx = conn.transaction().map_err(db)?;
        tx.execute_batch(sql).map_err(db)?;
        tx.pragma_update(None, "user_version", i + 1).map_err(db)?;
        tx.commit().map_err(db)?;
    }
    Ok(())
}
