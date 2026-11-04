//! The engine behind the UI: the library database, the queue and the transport.
//!
//! The views address tracks by their index in `data::Library`; the engine by slug. `Backend`
//! translates, and hands the UI a [`Snapshot`] of the session after every call so the UI's own
//! queue state (`main::Player`) is always a mirror of the engine's, never a second source of
//! truth. The session lives in the database: the embedded sound player (feature `audio`) and any
//! other handle (the CLI) follow it, and `snapshot` picks their changes up.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use weave_player_lib::core::{PlayStatus, RepeatMode};
use weave_player_lib::{Library as Engine, SessionView};

use crate::data::Library;

/// What the UI shows of the session.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    /// The whole queue as library indexes; `current` indexes it.
    pub queue: Vec<usize>,
    pub current: Option<usize>,
    pub playing: bool,
    /// Seconds into the current track.
    pub pos: f32,
    pub shuffle: bool,
    pub repeat: bool,
    /// Counts the tracks started (the engine's `start_seq`): it changes when a track starts or
    /// restarts, which is when the listening history grows.
    pub started: u64,
}

pub struct Backend {
    engine: Engine,
    /// The database file (none for the in-memory library of the tests).
    db: Option<PathBuf>,
    index: HashMap<String, usize>,
    slugs: Vec<String>,
    /// The embedded sound player. Dropping it stops the sound and frees the library's player lock.
    #[cfg(feature = "audio")]
    sound: Option<weave_player_lib::Player>,
}

/// Which library to open.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    /// A database file: the library, or one given with `--db`.
    Db(PathBuf),
    /// `--mock-db [seeded|empty]`: a separate dev database, seeded with mock data or empty. Debug
    /// builds.
    #[cfg(debug_assertions)]
    Mock(weave_player_lib::mock::Mock),
}

/// What the command line asks for: `--mock-db`, else `--db <path>`, else `$WEAVE_DB`, else the
/// default library (the CLI's).
pub fn source_from_args() -> Source {
    let mut db = None;
    let mut args = std::env::args_os().skip(1);
    while let Some(a) = args.next() {
        #[cfg(debug_assertions)]
        {
            use weave_player_lib::mock::Mock;
            let kind = |v: &str| {
                (v == "empty")
                    .then_some(Mock::Empty)
                    .unwrap_or(Mock::Seeded)
            };
            if a == "--mock-db" {
                // `--mock-db` or `--mock-db empty` (the kind is optional).
                let next = args.next();
                return Source::Mock(kind(next.as_deref().and_then(|v| v.to_str()).unwrap_or("")));
            } else if let Some(v) = a.to_str().and_then(|a| a.strip_prefix("--mock-db=")) {
                return Source::Mock(kind(v));
            }
        }
        if a == "--db" {
            db = args.next().map(PathBuf::from);
        } else if let Some(p) = a.to_str().and_then(|a| a.strip_prefix("--db=")) {
            db = Some(PathBuf::from(p));
        }
    }
    Source::Db(
        db.or_else(|| std::env::var_os("WEAVE_DB").map(PathBuf::from))
            .unwrap_or_else(weave_player_lib::paths::default_db),
    )
}

/// The engine for a source, with the path of its database.
fn open_engine(source: &Source) -> Result<(PathBuf, Engine), String> {
    match source {
        Source::Db(db) => {
            if let Some(dir) = db.parent() {
                std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            }
            let engine = Engine::open(db).map_err(|e| format!("{}: {e}", db.display()))?;
            Ok((db.clone(), engine))
        }
        #[cfg(debug_assertions)]
        Source::Mock(kind) => weave_player_lib::mock::open_kind(*kind).map_err(|e| e.to_string()),
    }
}

impl Backend {
    /// Opens (creating, if need be) the library and starts the sound player for it.
    pub fn open(source: &Source) -> Result<(Self, Library), String> {
        let (db, engine) = open_engine(source)?;
        let db = db.as_path();

        let mut b = Self::with(engine);
        b.db = Some(db.to_path_buf());
        #[cfg(feature = "audio")]
        {
            use weave_player_lib::{Player, PlayerOptions};
            match Player::start(db, PlayerOptions::default()) {
                Ok(p) => b.sound = Some(p),
                // Another player owns the library (or there is no output): the UI still drives
                // the session, and whoever plays follows it.
                Err(e) => eprintln!("weave: no sound from this window: {e}"),
            }
        }
        let lib = b.load()?;
        Ok((b, lib))
    }

    fn with(engine: Engine) -> Self {
        Self {
            engine,
            db: None,
            index: HashMap::new(),
            slugs: Vec::new(),
            #[cfg(feature = "audio")]
            sound: None,
        }
    }

    /// The database file, to open another handle on it.
    pub fn db(&self) -> Option<&Path> {
        self.db.as_deref()
    }

    /// Reads the library again, and the slugs the UI's indexes mean.
    pub fn load(&mut self) -> Result<Library, String> {
        let lib = Library::load(&self.engine).map_err(|e| e.to_string())?;
        self.slugs = lib.tracks.iter().map(|t| t.slug.clone()).collect();
        self.index = self
            .slugs
            .iter()
            .enumerate()
            .map(|(i, s)| (s.clone(), i))
            .collect();
        Ok(lib)
    }

    fn slugs_of(&self, ids: &[usize]) -> Vec<String> {
        ids.iter()
            .filter_map(|i| self.slugs.get(*i).cloned())
            .collect()
    }

    fn snapshot_of(&self, v: &SessionView) -> Snapshot {
        // Tracks the loaded library does not know (added since) are left out; `current` is
        // recounted over the ones kept.
        let mut queue = Vec::with_capacity(v.tracks.len());
        let mut current = None;
        for (at, t) in v.tracks.iter().enumerate() {
            if let Some(i) = self.index.get(&t.id.0) {
                if v.state.current == Some(at) {
                    current = Some(queue.len());
                }
                queue.push(*i);
            }
        }
        Snapshot {
            queue,
            current,
            playing: v.state.status == PlayStatus::Playing,
            pos: v.position.as_secs_f32(),
            shuffle: v.state.shuffle,
            repeat: v.state.repeat != RepeatMode::Off,
            started: v.state.start_seq,
        }
    }

    /// The session as it is now (also what another handle changed).
    pub fn snapshot(&mut self) -> Snapshot {
        #[cfg(feature = "audio")]
        if let Some(sound) = &self.sound {
            // The player reports what it does; the UI reads the session, so just keep the
            // channel from filling up.
            while sound.events().try_recv().is_ok() {}
        }
        match self.engine.session() {
            Ok(v) => self.snapshot_of(&v),
            Err(e) => {
                eprintln!("weave: {e}");
                Snapshot::default()
            }
        }
    }

    /// Runs a session change and returns the session after it. A refusal (nothing to resume, a
    /// position out of range) leaves the session as it was.
    fn run(
        &mut self,
        f: impl FnOnce(&mut Engine) -> weave_player_lib::core::Result<SessionView>,
    ) -> Snapshot {
        match f(&mut self.engine) {
            Ok(v) => self.snapshot_of(&v),
            Err(e) => {
                eprintln!("weave: {e}");
                self.snapshot()
            }
        }
    }

    /// Plays `ids` as the queue, starting at `start`.
    pub fn play(&mut self, ids: &[usize], start: usize) -> Snapshot {
        let slugs = self.slugs_of(ids);
        self.run(|e| {
            e.queue_clear()?;
            e.queue_add(&slugs, None, false)?;
            e.queue_jump(start + 1)
        })
    }

    /// Adds a track right after the playing one, or at the end. With nothing playing it starts.
    pub fn enqueue(&mut self, id: usize, next: bool) -> Snapshot {
        let Some(slug) = self.slugs.get(id).cloned() else {
            return self.snapshot();
        };
        let idle = self.snapshot().current.is_none();
        if idle {
            return self.play(&[id], 0);
        }
        self.run(|e| {
            e.queue_add(&[slug], None, next)?;
            e.session()
        })
    }

    /// Pauses what plays, plays what is paused.
    pub fn toggle(&mut self, playing: bool) -> Snapshot {
        self.run(|e| if playing { e.pause() } else { e.resume() })
    }

    pub fn next(&mut self) -> Snapshot {
        self.run(|e| e.next())
    }

    pub fn previous(&mut self) -> Snapshot {
        self.run(|e| e.previous())
    }

    pub fn seek(&mut self, secs: f32) -> Snapshot {
        self.run(|e| e.seek(Duration::from_secs_f32(secs.max(0.0))))
    }

    pub fn shuffle(&mut self, on: bool) -> Snapshot {
        self.run(|e| e.set_shuffle(on))
    }

    pub fn repeat(&mut self, on: bool) -> Snapshot {
        let mode = if on {
            RepeatMode::Playlist
        } else {
            RepeatMode::Off
        };
        self.run(|e| e.set_repeat(mode))
    }

    /// Removes the track at `at` (0-based) of the queue.
    pub fn remove(&mut self, at: usize) -> Snapshot {
        self.run(|e| e.queue_remove(&[at + 1]))
    }

    /// Moves the track at `from` to `to` (0-based) of the queue.
    pub fn move_item(&mut self, from: usize, to: usize) -> Snapshot {
        self.run(|e| e.queue_move(from + 1, to + 1))
    }

    /// Saves a playlist's description (empty clears it). The filter stays as it is.
    pub fn set_playlist_description(&mut self, name: &str, text: &str) -> Result<(), String> {
        use weave_player_lib::core::FilterSpec;
        self.engine
            .update_playlist(name, Some(text), FilterSpec::default())
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// Renames a playlist. Its ID stays; another playlist's name is refused.
    pub fn rename_playlist(&mut self, name: &str, new_name: &str) -> Result<(), String> {
        self.engine
            .rename_playlist(name, new_name)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// The listening history, to put on the library again after a track started.
    pub fn history(&self) -> Vec<weave_player_lib::core::PlayStat> {
        self.engine.play_history().unwrap_or_default()
    }

    /// The sleep timer ran out: stop where it is.
    pub fn pause(&mut self) -> Snapshot {
        self.run(|e| e.pause())
    }
}

/// Indexes files and folders into the library at `db`, on a handle of its own so the window stays
/// responsive. Nothing is moved or rewritten; what is already indexed is skipped. Returns the
/// sentence to tell the user.
pub fn import(db: &Path, paths: &[PathBuf]) -> Result<String, String> {
    let mut engine = Engine::open(db).map_err(|e| e.to_string())?;
    let (mut added, mut known) = (0, 0);
    for p in paths {
        let stats = engine
            .import(p)
            .map_err(|e| format!("{}: {e}", p.display()))?;
        added += stats.added;
        known += stats.skipped;
    }
    Ok(match (added, known) {
        (0, 0) => "No audio found".to_string(),
        (0, k) => format!("Already in the library ({k})"),
        (1, _) => "Imported 1 track".to_string(),
        (n, _) => format!("Imported {n} tracks"),
    })
}

/// A small library to run the UI's tests against: the engine in memory, with deterministic tracks,
/// tags, ratings and playlists, and a listening history on the loaded copy.
#[cfg(test)]
pub mod sample {
    use super::*;
    use weave_player_lib::core::{FilterSpec, Metadata, ScannedTrack};

    /// Adds a manual playlist to the engine behind `b` (the UI's copy is read again with `load`).
    pub fn playlist(b: &mut Backend, name: &str, description: &str) {
        b.engine
            .create_manual_playlist(name, Some(description))
            .unwrap();
    }

    /// A library with nothing in it.
    pub fn empty() -> (Backend, Library) {
        let mut b = Backend::with(Engine::open_in_memory().unwrap());
        let lib = b.load().unwrap();
        (b, lib)
    }

    pub fn backend() -> (Backend, Library) {
        const ARTISTS: [&str; 8] = [
            "Aventura",
            "Kaysha",
            "Romeo Santos",
            "Celia Cruz",
            "Dadju",
            "Prince Royce",
            "Grupo Extra",
            "Sael",
        ];
        const WORDS: [&str; 12] = [
            "Obsesión",
            "Noche",
            "Corazón",
            "Bailar",
            "Sombras",
            "Lluvia",
            "Fuego",
            "Luna",
            "Dime",
            "Volver",
            "Cielo",
            "Besos",
        ];
        const TAGS: [&[&str]; 8] = [
            &["genre:bachata-sensual", "mood:romantic", "lang:spanish"],
            &["genre:zouk", "mood:calm", "lang:french"],
            &["genre:bachata-traditional", "lang:spanish"],
            &["genre:salsa", "mood:energetic", "lang:spanish"],
            &["genre:zouk", "mood:late-night", "lang:french"],
            &["genre:bachata-sensual", "mood:calm"],
            &["genre:bachata", "practice"],
            &["genre:zouk", "mood:romantic"],
        ];
        let mut engine = Engine::open_in_memory().unwrap();
        let scanned: Vec<ScannedTrack> = (0..60)
            .map(|i| {
                let a = i % ARTISTS.len();
                ScannedTrack {
                    path: format!("/music/{i:02}.flac"),
                    metadata: Metadata {
                        title: format!(
                            "{} {}",
                            WORDS[(i * 5) % WORDS.len()],
                            WORDS[(i * 7 + 3) % WORDS.len()]
                        ),
                        artist: Some(ARTISTS[a].into()),
                        album: Some(format!(
                            "{} Vol. {}",
                            WORDS[(a * 3) % WORDS.len()],
                            i / 16 + 1
                        )),
                        year: Some(2000 + (a * 3 + i / 16) as i32),
                        duration_ms: Some((170 + ((i * 53) % 130) as u64) * 1000),
                        bpm: Some(78.0 + ((i * 37) % 90) as f64),
                        ..Metadata::default()
                    },
                }
            })
            .collect();
        engine.import_scanned(&scanned).unwrap();
        // Tags: the vocabulary first (children under their parents), then the assignments.
        for (name, parent) in [
            ("genre:bachata", None),
            ("genre:bachata-sensual", Some("genre:bachata")),
            ("genre:bachata-traditional", Some("genre:bachata")),
            ("genre:zouk", None),
            ("genre:salsa", None),
            ("mood:calm", None),
            ("mood:late-night", Some("mood:calm")),
            ("mood:romantic", None),
            ("mood:energetic", None),
            ("lang:spanish", None),
            ("lang:french", None),
            ("practice", None),
        ] {
            engine.create_tag(name, parent).unwrap();
        }
        // Tag and rate by the order the tracks were imported in (their path number).
        let by_path = engine_paths(&engine);
        for (slug, i) in by_path {
            let a = i % ARTISTS.len();
            let tags: Vec<String> = TAGS[(i * 3 + a) % TAGS.len()]
                .iter()
                .map(|t| t.to_string())
                .collect();
            engine.add_tags(&slug, &tags).unwrap();
            engine
                .set_rating(&slug, Some(((i * 11) % 5) as u8 + 1))
                .unwrap();
        }
        engine
            .set_tag_style("mood:romantic", Some("heart"), Some("#ff6b9d"))
            .unwrap();
        let mut b = Backend::with(engine);
        let mut lib = b.load().unwrap();
        // A listening history, as the UI's history sections expect it.
        for t in &mut lib.tracks {
            let i = t.id;
            t.plays = ((i * 7919) % 41) as u32;
            t.last_played_days = ((i * 53) % 400) as u32;
            // Newer plays have a later time; the index breaks ties, as the days do.
            t.played_at = 1_800_000_000 - i64::from(t.last_played_days) * 86_400 - i as i64;
        }
        (b, lib)
    }

    /// Slug and import number of each track, by the number in its path.
    fn engine_paths(engine: &Engine) -> Vec<(String, usize)> {
        let found = engine
            .search(&FilterSpec::default(), weave_player_lib::core::Page::ALL)
            .unwrap();
        found
            .tracks
            .iter()
            .filter_map(|t| {
                let d = engine.track_detail(&t.id.0).ok()?;
                let n = d
                    .paths
                    .first()?
                    .rsplit('/')
                    .next()?
                    .split('.')
                    .next()?
                    .parse()
                    .ok()?;
                Some((t.id.0.clone(), n))
            })
            .collect()
    }
}
