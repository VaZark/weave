use rusqlite::Connection;
use weave_core::{Error, Result};

/// Until 0.1 the schema is one squashed step that is edited in place, and pre-0.1
/// databases are not migrated: delete and re-import. `PRAGMA user_version` therefore holds
/// a fingerprint of this text, so a database made from an older edit is recognized (and
/// reported clearly) instead of failing later with "no such column".
/// At 0.1 this becomes a list of append-only numbered migrations.
const MIGRATIONS: &[&str] = &["CREATE TABLE tracks (
         id       TEXT PRIMARY KEY,       -- opaque internal key, never shown to users
         slug     TEXT NOT NULL UNIQUE,   -- public ID, see weave_core::track_slug
         added_at INTEGER NOT NULL,       -- unix seconds
         artist_id TEXT REFERENCES artists(id) ON DELETE SET NULL,
         album_id  TEXT REFERENCES albums(id) ON DELETE SET NULL
     );
     CREATE INDEX tracks_artist ON tracks(artist_id);
     CREATE INDEX tracks_album  ON tracks(album_id);
     -- Artists and albums are entities of their own: profiles, a slug ID, tags. The metadata
     -- columns keep the text; these link tracks that spell a name the same way.
     -- Names are not unique: different people share them. Import makes a new artist rather than
     -- guess, and the user merges (or separates) afterwards.
     CREATE TABLE artists (
         id        TEXT PRIMARY KEY,       -- opaque internal key
         slug      TEXT NOT NULL UNIQUE,   -- public ID; the user may change it
         name      TEXT NOT NULL,
         name_norm TEXT NOT NULL           -- weave_core::entity_norm
     );
     CREATE INDEX artists_name ON artists(name_norm);
     -- Featured artists of a track (`Song feat. Guest`); the primary artist is tracks.artist_id.
     CREATE TABLE track_featured (
         track_id  TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
         artist_id TEXT NOT NULL REFERENCES artists(id) ON DELETE CASCADE,
         position  INTEGER NOT NULL,
         PRIMARY KEY (track_id, artist_id)
     );
     CREATE INDEX track_featured_artist ON track_featured(artist_id);
     -- Pairs of artists, albums or tracks the user said are different, so they are never
     -- suggested as duplicates again. Internal keys; rows for deleted things are cleaned up.
     CREATE TABLE not_duplicates (
         kind TEXT NOT NULL,
         a_id TEXT NOT NULL,
         b_id TEXT NOT NULL,
         PRIMARY KEY (kind, a_id, b_id),
         CHECK (a_id < b_id)
     );
     CREATE TABLE albums (
         id         TEXT PRIMARY KEY,
         slug       TEXT NOT NULL UNIQUE,
         title      TEXT NOT NULL,
         title_norm TEXT NOT NULL,
         artist_id  TEXT REFERENCES artists(id) ON DELETE SET NULL   -- the artist profile it is linked to
     );
     CREATE INDEX albums_artist   ON albums(artist_id);
     CREATE INDEX albums_identity ON albums(title_norm, artist_id);
     -- User annotations live apart from source-derived metadata so syncs never touch them.
     CREATE TABLE track_annotations (
         track_id TEXT PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
         rating   INTEGER CHECK (rating BETWEEN 1 AND 5)
     );
     CREATE INDEX annotations_rating ON track_annotations(rating);
     CREATE TABLE track_metadata (
         track_id     TEXT PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
         title        TEXT NOT NULL,
         title_norm   TEXT NOT NULL,       -- *_norm: lowercased copies used by filters and ordering
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
     CREATE INDEX meta_sort  ON track_metadata(artist_norm, album_norm, title_norm, track_id);
     CREATE INDEX meta_bpm   ON track_metadata(bpm);
     CREATE INDEX meta_year  ON track_metadata(year);
     CREATE INDEX meta_genre ON track_metadata(genre_norm);
     -- The files behind a track, kept apart from the track so identity never depends on a path.
     -- Normally one; merging duplicate tracks keeps the other files here. The oldest row plays.
     CREATE TABLE sources (
         track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
         path     TEXT PRIMARY KEY
     );
     CREATE INDEX sources_track ON sources(track_id);
     -- This instance: the one row names it on the network and says what it may do.
     CREATE TABLE instance (
         id       INTEGER PRIMARY KEY CHECK (id = 1),
         name     TEXT,                             -- NULL until chosen: the host name is used
         playback INTEGER NOT NULL DEFAULT 1        -- 0 = remote-only, never plays audio
     );
     INSERT INTO instance (id) VALUES (1);
     -- Tags: an optional key and a value (`genre:bachata`, `practice`), at most one parent.
     -- Identity is independent of the parent; assignments are explicit and flat, and the tree
     -- only widens queries. Cycles are rejected in code, in the same transaction as the change.
     CREATE TABLE tags (
         id              INTEGER PRIMARY KEY,
         name            TEXT NOT NULL,            -- display spelling, `key:value`
         normalized_name TEXT NOT NULL UNIQUE,     -- lookup identity
         key             TEXT,                     -- lowercase; NULL for an unkeyed tag
         value           TEXT NOT NULL,
         parent_id       INTEGER REFERENCES tags(id)
     );
     CREATE INDEX tags_parent  ON tags(parent_id);
     CREATE INDEX tags_key ON tags(key);
     CREATE TABLE track_tags (
         track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
         tag_id   INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
         PRIMARY KEY (track_id, tag_id)
     );
     CREATE INDEX track_tags_tag ON track_tags(tag_id, track_id);
     -- The same tags on artists, albums and playlists.
     CREATE TABLE artist_tags (
         artist_id TEXT NOT NULL REFERENCES artists(id) ON DELETE CASCADE,
         tag_id    INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
         PRIMARY KEY (artist_id, tag_id)
     );
     CREATE INDEX artist_tags_tag ON artist_tags(tag_id, artist_id);
     CREATE TABLE album_tags (
         album_id TEXT NOT NULL REFERENCES albums(id) ON DELETE CASCADE,
         tag_id   INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
         PRIMARY KEY (album_id, tag_id)
     );
     CREATE INDEX album_tags_tag ON album_tags(tag_id, album_id);
     CREATE TABLE playlists (
         id              TEXT PRIMARY KEY,   -- slug, the public ID
         name            TEXT NOT NULL,
         normalized_name TEXT NOT NULL UNIQUE,
         description     TEXT,
         kind            TEXT NOT NULL CHECK (kind IN ('dynamic', 'manual')),
         filter_spec     TEXT NOT NULL       -- JSON FilterSpec; empty for manual playlists
     );
     CREATE TABLE playlist_tracks (
         playlist_id TEXT NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
         track_id    TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
         play_order  INTEGER NOT NULL,   -- canonical order: 0..n-1, contiguous, like a queue's
         PRIMARY KEY (playlist_id, track_id)
     );
     CREATE INDEX playlist_tracks_order ON playlist_tracks(playlist_id, play_order);
     CREATE TABLE playlist_tags (
         playlist_id TEXT NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
         tag_id      INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
         PRIMARY KEY (playlist_id, tag_id)
     );
     CREATE INDEX playlist_tags_tag ON playlist_tags(tag_id, playlist_id);
     -- Playback session: a single queue and its state (see weave_core::Queue).
     CREATE TABLE queue_items (
         position  INTEGER PRIMARY KEY,                 -- list order, 0-based
         track_id  TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
         play_rank INTEGER NOT NULL UNIQUE              -- play order (shuffle)
     );
     CREATE TABLE player_state (
         id               INTEGER PRIMARY KEY CHECK (id = 1),
         status           TEXT NOT NULL,
         repeat           TEXT NOT NULL,
         shuffle          INTEGER NOT NULL,
         end_of_queue     TEXT NOT NULL,
         sleep_until      INTEGER,                      -- unix milliseconds
         sleep_soft       INTEGER NOT NULL,             -- finish the playing track first
         stop_after_track INTEGER NOT NULL,             -- stop scheduled for the end of this track
         current_position INTEGER,
         volume           REAL NOT NULL,                -- 0.0 to 1.0
         position_ms      INTEGER NOT NULL,             -- position anchor ...
         position_at      INTEGER NOT NULL,             -- ... taken at this unix ms
         start_seq        INTEGER NOT NULL,             -- bumped when a track starts
         seek_seq         INTEGER NOT NULL,             -- bumped on a seek request
         host_quit        INTEGER NOT NULL DEFAULT 0    -- asks the playback host to exit
     );
     INSERT INTO player_state
         (id, status, repeat, shuffle, end_of_queue, sleep_until, sleep_soft, stop_after_track,
          current_position, volume, position_ms, position_at, start_seq, seek_seq)
         VALUES (1, 'stopped', 'off', 0, 'stop', NULL, 0, 0, NULL, 1.0, 0, 0, 0, 0);"];

/// FNV-1a over the schema text, kept positive and non-zero (zero means "empty database").
fn fingerprint() -> i64 {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in MIGRATIONS[0].bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    i64::from((hash & 0x7fff_ffff).max(1))
}

pub fn run(conn: &mut Connection) -> Result<()> {
    let db = |e: rusqlite::Error| Error::Storage(e.to_string());
    let found: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(db)?;
    let tables: i64 = conn
        .query_row("SELECT COUNT(*) FROM sqlite_master", [], |r| r.get(0))
        .map_err(db)?;
    let expected = fingerprint();

    if found == expected {
        return Ok(());
    }
    if found != 0 || tables != 0 {
        return Err(Error::Storage(format!(
            "this library was made by a different pre-0.1 build (schema marker {found}, this build \
             expects {expected}); pre-0.1 libraries are not migrated, so delete the library file \
             and re-import"
        )));
    }
    let tx = conn.transaction().map_err(db)?;
    tx.execute_batch(MIGRATIONS[0]).map_err(db)?;
    tx.pragma_update(None, "user_version", expected)
        .map_err(db)?;
    tx.commit().map_err(db)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_database_gets_the_schema_and_its_fingerprint() {
        let mut conn = Connection::open_in_memory().unwrap();
        run(&mut conn).unwrap();
        let v: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, fingerprint());
        run(&mut conn).unwrap(); // reopening is a no-op
        let has_table: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name = 'track_annotations'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(has_table, 1);
    }

    #[test]
    fn a_database_from_an_older_edit_of_the_schema_is_rejected_clearly() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE tracks (id TEXT); PRAGMA user_version = 1;")
            .unwrap();
        let msg = run(&mut conn).unwrap_err().to_string();
        assert!(
            msg.contains("delete the library file") && msg.contains("marker 1"),
            "{msg}"
        );
    }

    #[test]
    fn tables_without_a_marker_are_not_adopted() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE something_else (x INTEGER);")
            .unwrap();
        assert!(run(&mut conn).is_err());
    }

    #[test]
    fn the_fingerprint_changes_with_the_schema_text() {
        assert!(fingerprint() > 0);
        let mut h = 0x811c_9dc5u32;
        for b in format!("{}x", MIGRATIONS[0]).bytes() {
            h ^= u32::from(b);
            h = h.wrapping_mul(0x0100_0193);
        }
        assert_ne!(i64::from((h & 0x7fff_ffff).max(1)), fingerprint());
    }
}
